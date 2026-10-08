//! A JSON-RPC client for the Pi coding agent.
//!
//! Pi is driven as a long-lived subprocess speaking strict JSONL on stdin/stdout
//! (`pi --mode rpc`). Everything in this module is transport: process management,
//! framing, and command ids. Record *semantics* live in [`crate::state`].

use anyhow::{Context as _, Result, anyhow};
use async_channel::{Receiver, Sender};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
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
            // Pi's SIGTERM handler cleans up tracked tool subprocesses. Reaping
            // must not block navigation; force termination only after a grace period.
            #[cfg(unix)]
            unsafe {
                libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
            }
            #[cfg(not(unix))]
            let _ = child.kill();
            thread::spawn(move || {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
                loop {
                    if child.try_wait().ok().flatten().is_some() { break; }
                    if std::time::Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                    thread::sleep(std::time::Duration::from_millis(20));
                }
            });
        }
    }
}

#[derive(Clone)]
pub struct PiClient {
    tx: Sender<String>,
    /// Parsed protocol records read from Pi's stdout.
    pub records: Receiver<Value>,
    /// Diagnostic lines read from Pi's stderr.
    pub logs: Receiver<String>,
    next_id: Arc<AtomicU64>,
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

        let mut child = command
            .spawn()
            .with_context(|| format!("could not start `{}` — is Pi installed and on PATH?", config.program))?;

        let stdin = child.stdin.take().ok_or_else(|| anyhow!("pi stdin unavailable"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("pi stdout unavailable"))?;
        let stderr = child.stderr.take().ok_or_else(|| anyhow!("pi stderr unavailable"))?;

        let (line_tx, line_rx) = async_channel::unbounded::<String>();
        let (record_tx, record_rx) = async_channel::bounded::<Value>(2048);
        let (log_tx, log_rx) = async_channel::unbounded::<String>();

        // Writer: one JSON object per line, terminated by LF. Closing stdin
        // (when this thread exits) is how Pi is asked to shut down.
        thread::Builder::new()
            .name("dish-pi-stdin".into())
            .spawn(move || {
                let mut stdin = stdin;
                while let Ok(line) = line_rx.recv_blocking() {
                    if stdin.write_all(line.as_bytes()).is_err()
                        || stdin.write_all(b"\n").is_err()
                        || stdin.flush().is_err()
                    {
                        break;
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
                let mut buffer = Vec::<u8>::new();
                loop {
                    buffer.clear();
                    match reader.read_until(b'\n', &mut buffer) {
                        Ok(0) => break,
                        Ok(_) => {
                            let mut line = String::from_utf8_lossy(&buffer).into_owned();
                            while line.ends_with('\n') || line.ends_with('\r') {
                                line.pop();
                            }
                            if line.is_empty() {
                                continue;
                            }
                            match serde_json::from_str::<Value>(&line) {
                                Ok(value) => {
                                    if record_tx.send_blocking(value).is_err() {
                                        break;
                                    }
                                }
                                Err(error) => {
                                    eprintln!("dish: unparsable record: {error}: {line}");
                                }
                            }
                        }
                        Err(error) => {
                            eprintln!("dish: pi stdout closed: {error}");
                            break;
                        }
                    }
                }
            })
            .expect("spawn pi stdout thread");

        thread::Builder::new()
            .name("dish-pi-stderr".into())
            .spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if log_tx.send_blocking(line).is_err() {
                        break;
                    }
                }
            })
            .expect("spawn pi stderr thread");

        Ok(Self {
            tx: line_tx,
            records: record_rx,
            logs: log_rx,
            next_id: Arc::new(AtomicU64::new(1)),
            _guard: Arc::new(ChildGuard(Mutex::new(Some(child)))),
        })
    }

    /// Send an arbitrary command, assigning it a fresh correlating id.
    pub fn send(&self, mut command: Value) -> String {
        let id = format!("d{}", self.next_id.fetch_add(1, Ordering::Relaxed));
        if let Value::Object(map) = &mut command {
            map.insert("id".into(), Value::String(id.clone()));
        }
        let encoded = command.to_string();
        if self.tx.send_blocking(encoded).is_err() {
            eprintln!("dish: pi stdin is closed; dropping command");
        }
        id
    }

    /// Send a command of the given `type` with `extra` fields merged in.
    pub fn call(&self, command_type: &str, extra: Value) -> String {
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
    pub fn prompt(&self, message: &str, streaming: Option<&str>) -> String {
        self.prompt_with_images(message, streaming, &[])
    }

    pub fn prompt_with_images(&self, message: &str, streaming: Option<&str>, images: &[Value]) -> String {
        let mut command = json!({ "type": "prompt", "message": message });
        if !images.is_empty() {
            command["images"] = json!(images);
        }
        if let Some(behavior) = streaming {
            command["streamingBehavior"] = json!(behavior);
        }
        self.send(command)
    }

    pub fn bash(&self, command: &str) -> String {
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
    fn independent_processes_keep_streaming_and_isolate_commands() {
        let root = std::env::temp_dir().join(format!("dish-rpc-{}", std::process::id()));
        let a_dir = root.join("a");
        let b_dir = root.join("b");
        std::fs::create_dir_all(&a_dir).unwrap();
        std::fs::create_dir_all(&b_dir).unwrap();
        let a = fake(&a_dir);
        let b = fake(&b_dir);
        a.call("get_state", Value::Null);
        b.call("get_state", Value::Null);
        let a_state = receive(&a, "response");
        let b_state = receive(&b, "response");
        assert_ne!(a_state["data"]["pid"], b_state["data"]["pid"]);
        assert_eq!(a_state["data"]["cwd"], a_dir.to_str().unwrap());
        assert_eq!(b_state["data"]["cwd"], b_dir.to_str().unwrap());
        a.prompt("task-a", None);
        b.prompt("task-b", None);
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
        a.call("test_exit", Value::Null);
        b.call("set_thinking_level", json!({"level": "max"}));
        receive(&b, "response");
        b.call("get_state", Value::Null);
        let state = receive(&b, "response");
        assert_eq!(state["data"]["thinkingLevel"], "max");
        drop(a);
        drop(b);
        std::fs::remove_dir_all(root).unwrap();
    }
}
