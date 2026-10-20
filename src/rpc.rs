//! A JSON-RPC client for the Pi coding agent.
//!
//! Pi is driven as a long-lived subprocess speaking strict JSONL on stdin/stdout
//! (`pi --mode rpc`). Everything in this module is transport: process management,
//! framing, and command ids. Record *semantics* live in [`crate::state`].

use anyhow::{Context as _, Result, anyhow};
use async_channel::{Receiver, Sender};
use serde_json::{Value, json};
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

/// How to launch Pi.
#[derive(Clone, Debug)]
pub struct PiConfig {
    /// Executable to run. Defaults to `pi` from `PATH`.
    pub program: String,
    /// Extra command line arguments.
    pub args: Vec<String>,
    /// Folder Pi treats as the project root.
    pub cwd: PathBuf,
}


impl PiConfig {
    pub fn new(cwd: impl Into<PathBuf>) -> Self {
        let cwd = cwd.into();
        let args = vec!["--mode".to_string(), "rpc".to_string()];
        Self {
            program: crate::installation::program(),
            args,
            cwd,
        }
    }

    /// Append extra Pi flags, e.g. `--continue` or `--model`.
    pub fn with_args(mut self, extra: Vec<String>) -> Self {
        self.args.extend(extra);
        self
    }
}

/// Kills the child process when the last clone of the client goes away.
struct ChildGuard(Mutex<Option<Child>>);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let mut guard = match self.0.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(mut child) = guard.take() {
            // Pi's SIGTERM handler cleans up tracked tool subprocesses. The
            // whole process group receives it, so tools started by Pi do not
            // survive when the client goes away. Reaping must not block
            // navigation; force termination only after a grace period.
            #[cfg(unix)]
            unsafe {
                libc::kill(-(child.id() as libc::pid_t), libc::SIGTERM);
            }
            #[cfg(not(unix))]
            let _ = child.kill();
            thread::spawn(move || {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
                loop {
                    if child.try_wait().ok().flatten().is_some() { break; }
                    if std::time::Instant::now() >= deadline {
                        crate::process::terminate_tree(&mut child);
                        break;
                    }
                    thread::sleep(std::time::Duration::from_millis(20));
                }
            });
        }
    }
}

/// A safe summary of one Pi stderr line.
///
/// The raw line is deliberately not retained or sent to the UI: stderr can
/// contain prompt text, tool arguments, paths or provider credentials.  Only
/// the error classification and byte count cross the transport boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PiDiagnostic {
    pub is_error: bool,
    pub bytes: usize,
}

impl PiDiagnostic {
    fn from_stderr(line: &str) -> Self {
        Self {
            is_error: line
                .as_bytes()
                .windows(5)
                .any(|window| window.eq_ignore_ascii_case(b"error")),
            bytes: line.len(),
        }
    }

    fn oversized(bytes: usize) -> Self {
        Self {
            is_error: false,
            bytes,
        }
    }

    pub fn log_message(&self) -> String {
        let category = if self.is_error { "error" } else { "diagnostic" };
        format!(
            "dish: pi stderr: {category} ({} bytes, content omitted)",
            self.bytes
        )
    }

    pub fn toast_message(&self) -> &'static str {
        "O Pi registrou um erro interno. Reinicie o Dish se o problema continuar."
    }
}

/// Why a command did not enter the Pi writer queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RpcSendError {
    QueueFull,
    Closed,
    TooLarge,
}

impl std::fmt::Display for RpcSendError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::QueueFull => "A fila de comandos do Pi está cheia; aguarde e tente novamente.",
            Self::Closed => "O canal de entrada do Pi está fechado.",
            Self::TooLarge => "O comando excede o limite de tamanho permitido.",
        })
    }
}

pub type SendResult<T> = std::result::Result<T, RpcSendError>;

/// Serializes a command into a bounded buffer so an oversized prompt is
/// rejected while it is being encoded, not after a second full copy exists.
struct LimitedBuffer {
    bytes: Vec<u8>,
    limit: usize,
}

impl std::io::Write for LimitedBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.bytes.len().saturating_add(bytes.len()) > self.limit {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "command exceeds the RPC limit",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[derive(Clone)]
pub struct PiClient {
    tx: Sender<String>,
    /// Parsed protocol records read from Pi's stdout.
    pub records: Receiver<Value>,
    /// Safe summaries of diagnostic lines read from Pi's stderr.
    pub logs: Receiver<PiDiagnostic>,
    /// A later writer failure, after a command was accepted into the queue.
    pub failures: Receiver<String>,
    next_id: Arc<AtomicU64>,
    failed: Arc<AtomicBool>,
    _guard: Arc<ChildGuard>,
}

impl PiClient {
    /// Start `pi --mode rpc` and wire up its pipes.
    pub fn spawn(config: &PiConfig) -> Result<Self> {
        let mut command = Command::new(&config.program);
        command
            .args(&config.args)
            .current_dir(&config.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        crate::platform::hide_console(&mut command);
        crate::process::configure_group(&mut command);

        let mut child = command
            .spawn()
            .with_context(|| format!("could not start `{}` — is Pi installed and on PATH?", config.program))?;

        let stdin = child.stdin.take().ok_or_else(|| anyhow!("pi stdin unavailable"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("pi stdout unavailable"))?;
        let stderr = child.stderr.take().ok_or_else(|| anyhow!("pi stderr unavailable"))?;

        // Bounded queues provide backpressure; `send` uses try_send so a full
        // UI queue becomes an explicit error instead of a blocked UI thread.
        let (line_tx, line_rx) = async_channel::bounded::<String>(8);
        let (record_tx, record_rx) = async_channel::bounded::<Value>(8);
        let (log_tx, log_rx) = async_channel::bounded::<PiDiagnostic>(256);
        let (failure_tx, failure_rx) = async_channel::bounded::<String>(1);
        let failed = Arc::new(AtomicBool::new(false));

        // Writer: one JSON object per line, terminated by LF. Closing stdin
        // (when this thread exits) is how Pi is asked to shut down.
        thread::Builder::new()
            .name("dish-pi-stdin".into())
            .spawn({
                let failed = failed.clone();
                move || {
                    let mut stdin = stdin;
                    while let Ok(line) = line_rx.recv_blocking() {
                        if stdin.write_all(line.as_bytes()).is_err()
                            || stdin.write_all(b"\n").is_err()
                            || stdin.flush().is_err()
                        {
                            failed.store(true, Ordering::Release);
                            let _ = failure_tx.try_send(
                                "O Dish perdeu a entrada do Pi; reinicie a sessão.".into(),
                            );
                            break;
                        }
                    }
                }
            })
            .expect("spawn pi stdin thread");

        // Reader: split strictly on LF. Never use a text line reader here —
        // Unicode line separators are legal inside JSON strings.
        thread::Builder::new()
            .name("dish-pi-stdout".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                loop {
                    match crate::limits::read_line_limited(
                        &mut reader,
                        crate::limits::MAX_RPC_FRAME_BYTES,
                        false,
                    ) {
                        Ok(crate::limits::BoundedLine::Line(mut line)) => {
                            while line.ends_with(b"\n") || line.ends_with(b"\r") {
                                line.pop();
                            }
                            if line.is_empty() {
                                continue;
                            }
                            match serde_json::from_slice::<Value>(&line) {
                                Ok(value) => {
                                    if record_tx.send_blocking(value).is_err() {
                                        break;
                                    }
                                }
                                Err(error) => {
                                    crate::logging::warn(format_args!(
                                        "{}",
                                        unparsable_record_summary(&error, line.len())
                                    ));
                                }
                            }
                        }
                        Ok(crate::limits::BoundedLine::TooLong { bytes }) => {
                            crate::logging::warn(format_args!(
                                "dish: pi stdout frame exceeded {} bytes ({} bytes); closing stream",
                                crate::limits::MAX_RPC_FRAME_BYTES,
                                bytes
                            ));
                            break;
                        }
                        Ok(crate::limits::BoundedLine::Eof) => break,
                        Err(error) => {
                            crate::logging::warn(format_args!("dish: pi stdout closed: {error}"));
                            break;
                        }
                    }
                }
            })
            .expect("spawn pi stdout thread");

        thread::Builder::new()
            .name("dish-pi-stderr".into())
            .spawn(move || {
                let mut reader = BufReader::new(stderr);
                loop {
                    match crate::limits::read_line_limited(
                        &mut reader,
                        crate::limits::MAX_RPC_STDERR_BYTES,
                        true,
                    ) {
                        Ok(crate::limits::BoundedLine::Line(line)) => {
                            let line = String::from_utf8_lossy(&line);
                            if line.trim().is_empty() {
                                continue;
                            }
                            if log_tx.send_blocking(PiDiagnostic::from_stderr(&line)).is_err() {
                                break;
                            }
                        }
                        Ok(crate::limits::BoundedLine::TooLong { bytes }) => {
                            if log_tx
                                .send_blocking(PiDiagnostic::oversized(bytes))
                                .is_err()
                            {
                                break;
                            }
                        }
                        Ok(crate::limits::BoundedLine::Eof) => break,
                        Err(_) => break,
                    }
                }
            })
            .expect("spawn pi stderr thread");

        Ok(Self {
            tx: line_tx,
            records: record_rx,
            logs: log_rx,
            failures: failure_rx,
            next_id: Arc::new(AtomicU64::new(1)),
            failed,
            _guard: Arc::new(ChildGuard(Mutex::new(Some(child)))),
        })
    }

    /// Send an arbitrary command, assigning it a fresh correlating id.
    ///
    /// `Err` means the command was not accepted into the writer queue.  A
    /// failure that happens after acceptance is delivered through `failures`.
    pub fn send(&self, mut command: Value) -> SendResult<String> {
        if self.failed.load(Ordering::Acquire) {
            return Err(RpcSendError::Closed);
        }
        let id = format!("d{}", self.next_id.fetch_add(1, Ordering::Relaxed));
        if let Value::Object(map) = &mut command {
            map.insert("id".into(), Value::String(id.clone()));
        }
        let mut encoded = LimitedBuffer {
            bytes: Vec::new(),
            limit: crate::limits::MAX_RPC_COMMAND_BYTES,
        };
        if serde_json::to_writer(&mut encoded, &command).is_err() {
            return Err(RpcSendError::TooLarge);
        }
        let Ok(encoded) = String::from_utf8(encoded.bytes) else {
            return Err(RpcSendError::TooLarge);
        };
        match self.tx.try_send(encoded) {
            Ok(()) => Ok(id),
            Err(async_channel::TrySendError::Full(_)) => Err(RpcSendError::QueueFull),
            Err(async_channel::TrySendError::Closed(_)) => {
                self.failed.store(true, Ordering::Release);
                Err(RpcSendError::Closed)
            }
        }
    }

    /// Send a command of the given `type` with `extra` fields merged in.
    pub fn call(&self, command_type: &str, extra: Value) -> SendResult<String> {
        let mut command = match extra {
            Value::Object(map) => Value::Object(map),
            Value::Null => json!({}),
            other => json!({ "value": other }),
        };
        if let Value::Object(map) = &mut command {
            map.insert("type".into(), Value::String(command_type.to_string()));
        }
        self.send(command)
    }

    /// Typed convenience methods used by the UI.
    pub fn prompt(&self, message: &str, streaming: Option<&str>) -> SendResult<String> {
        self.prompt_with_images(message, streaming, &[])
    }

    pub fn prompt_with_images(
        &self,
        message: &str,
        streaming: Option<&str>,
        images: &[Value],
    ) -> SendResult<String> {
        let mut command = json!({ "type": "prompt", "message": message });
        if !images.is_empty() {
            command["images"] = json!(images);
        }
        if let Some(behavior) = streaming {
            command["streamingBehavior"] = json!(behavior);
        }
        self.send(command)
    }

    pub fn bash(&self, command: &str) -> SendResult<String> {
        self.call("bash", json!({ "command": command }))
    }
}

/// A parsed `response` record, keyed back to the command that produced it.
#[derive(Clone, Debug)]
pub struct RpcResponse {
    pub id: Option<String>,
    pub command: String,
    pub success: bool,
    pub error: Option<String>,
    pub data: Value,
}

impl RpcResponse {
    pub fn parse(record: &Value) -> Option<Self> {
        if record.get("type").and_then(Value::as_str) != Some("response") {
            return None;
        }
        Some(Self {
            id: record.get("id").and_then(Value::as_str).map(str::to_string),
            command: record
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            success: record.get("success").and_then(Value::as_bool).unwrap_or(false),
            error: record.get("error").and_then(Value::as_str).map(str::to_string),
            data: record.get("data").cloned().unwrap_or(Value::Null),
        })
    }
}

/// Describes an invalid JSONL frame without copying its contents into logs.
fn unparsable_record_summary(error: &serde_json::Error, bytes: usize) -> String {
    format!(
        "dish: unparsable record: category={:?} line={} column={} bytes={} content omitted",
        error.classify(),
        error.line(),
        error.column(),
        bytes
    )
}

/// Locate a usable `pi` binary, preferring the caller's PATH.
/// `DISH_PI_BIN` overrides the lookup.
#[allow(dead_code)]
pub fn find_pi_program() -> Option<String> {
    if let Ok(explicit) = std::env::var("DISH_PI_BIN") {
        if !explicit.is_empty() {
            return Some(explicit);
        }
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join("pi");
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    None
}

/// Best-effort human label for the project folder.
pub fn folder_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn fake(cwd: &Path) -> PiClient {
        PiClient::spawn(&PiConfig {
            program: "python3".into(),
            args: vec![format!("{}/tests/fixtures/fake_pi.py", env!("CARGO_MANIFEST_DIR"))],
            cwd: cwd.to_owned(),
        }).unwrap()
    }

    fn receive(client: &PiClient, kind: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match client.records.try_recv() {
                Ok(value) if value["type"] == kind => return value,
                Ok(_) => {}
                Err(async_channel::TryRecvError::Empty) => std::thread::sleep(Duration::from_millis(5)),
                Err(error) => panic!("Process exited before {kind}: {error}"),
            }
            assert!(Instant::now() < deadline, "Timed out waiting for {kind}");
        }
    }

    #[test]
    fn stderr_is_summarized_before_it_reaches_the_ui() {
        let root = std::env::temp_dir().join(format!("dish-rpc-privacy-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let script = root.join("emit-secret.py");
        std::fs::write(
            &script,
            "import sys, time\nsys.stderr.write('fatal Error: token=sk-live-SYNTHETIC\\n')\nsys.stderr.flush()\ntime.sleep(10)\n",
        )
        .unwrap();
        let client = PiClient::spawn(&PiConfig {
            program: "python3".into(),
            args: vec![script.to_string_lossy().into_owned()],
            cwd: root.clone(),
        })
        .unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        let diagnostic = loop {
            match client.logs.try_recv() {
                Ok(diagnostic) => break diagnostic,
                Err(async_channel::TryRecvError::Empty) => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("stderr channel closed: {error}"),
            }
            assert!(Instant::now() < deadline, "Timed out waiting for stderr");
        };
        assert!(diagnostic.is_error);
        let message = diagnostic.log_message();
        assert!(!message.contains("sk-live-SYNTHETIC"));
        assert!(!message.contains("token"));
        drop(client);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_writer_failure_after_acceptance_is_reported() {
        let root = std::env::temp_dir().join(format!("dish-rpc-writer-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let script = root.join("closed-pi.sh");
        std::fs::write(&script, "#!/bin/sh\nexec 1>&-\nexec 0<&-\nsleep 10\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
        let client = PiClient::spawn(&PiConfig {
            program: "/bin/sh".into(),
            args: vec![script.to_string_lossy().into_owned()],
            cwd: root.clone(),
        })
        .unwrap();

        // Let the shell close its stdin; an immediate write could still land in
        // the pipe before the child closes it. A later failure must not be
        // silent after the command was accepted.
        std::thread::sleep(Duration::from_millis(200));
        let _ = client.call("get_state", Value::Null);
        let deadline = Instant::now() + Duration::from_secs(5);
        let failure = loop {
            match client.failures.try_recv() {
                Ok(message) => break message,
                Err(async_channel::TryRecvError::Empty) => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("failure channel closed: {error}"),
            }
            assert!(Instant::now() < deadline, "Timed out waiting for failure");
        };
        assert!(failure.contains("entrada"));

        loop {
            match client.call("get_state", Value::Null) {
                Err(RpcSendError::Closed) => break,
                Ok(_) => std::thread::sleep(Duration::from_millis(5)),
                Err(error) => panic!("unexpected send error: {error:?}"),
            }
            assert!(Instant::now() < deadline, "writer never reported closed");
        }
        drop(client);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn independent_processes_keep_streaming_and_isolate_commands() {
        let root = std::env::temp_dir().join(format!("dish-rpc-{}", std::process::id()));
        let a_dir = root.join("a");
        let b_dir = root.join("b");
        std::fs::create_dir_all(&a_dir).unwrap();
        std::fs::create_dir_all(&b_dir).unwrap();
        let a = fake(&a_dir);
        let b = fake(&b_dir);
        a.call("get_state", Value::Null).unwrap();
        b.call("get_state", Value::Null).unwrap();
        let a_state = receive(&a, "response");
        let b_state = receive(&b, "response");
        assert_ne!(a_state["data"]["pid"], b_state["data"]["pid"]);
        assert_eq!(a_state["data"]["cwd"], a_dir.to_str().unwrap());
        assert_eq!(b_state["data"]["cwd"], b_dir.to_str().unwrap());
        a.prompt("task-a", None).unwrap();
        b.prompt("task-b", None).unwrap();
        // Simulate viewing B while A is left running without consuming its UI.
        receive(&b, "agent_end");
        let a_delta = loop {
            let record = receive(&a, "message_update");
            if record["assistantMessageEvent"]["type"] == "text_delta" {
                break record;
            }
        };
        assert!(a_delta["assistantMessageEvent"]["delta"].as_str().unwrap().contains("task-a / a"));
        receive(&a, "agent_end");
        // Losing A must not stop B or route commands to A.
        a.call("test_exit", Value::Null).unwrap();
        b.call("set_thinking_level", json!({"level": "max"})).unwrap();
        receive(&b, "response");
        b.call("get_state", Value::Null).unwrap();
        let state = receive(&b, "response");
        assert_eq!(state["data"]["thinkingLevel"], "max");
        drop(a);
        drop(b);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod privacy_tests {
    use super::*;

    const SECRET: &str = "sk-live-SYNTHETIC-DIAGNOSTIC-0123456789";

    #[test]
    fn stderr_diagnostics_never_retain_the_line() {
        let diagnostic = PiDiagnostic::from_stderr(&format!("fatal Error: token={SECRET}"));
        assert!(diagnostic.is_error);

        let log = diagnostic.log_message();
        assert!(!log.contains(SECRET));
        assert!(!log.contains("token"));
        assert!(log.contains("error"));
        assert!(log.contains("content omitted"));
        assert!(!diagnostic.toast_message().contains(SECRET));
    }

    #[test]
    fn malformed_record_summary_omits_the_payload() {
        let payload = format!(r#"{{"type":"message","token":"{SECRET}"#);
        let error = serde_json::from_str::<Value>(&payload).unwrap_err();
        let summary = unparsable_record_summary(&error, payload.len());

        assert!(!summary.contains(SECRET));
        assert!(!summary.contains("token"));
        assert!(summary.contains("bytes="));
    }

    #[test]
    fn command_encoding_stops_at_the_limit() {
        let mut buffer = LimitedBuffer {
            bytes: Vec::new(),
            limit: 8,
        };
        let result = serde_json::to_writer(&mut buffer, &json!({ "message": "0123456789" }));
        assert!(result.is_err());
        assert!(buffer.bytes.len() <= 8);
    }

    #[test]
    fn stderr_error_detection_is_case_insensitive() {
        assert!(PiDiagnostic::from_stderr("ERROR: failed").is_error);
        assert!(PiDiagnostic::from_stderr("warning: Error while running").is_error);
        assert!(!PiDiagnostic::from_stderr("progress 10%").is_error);
    }
}
