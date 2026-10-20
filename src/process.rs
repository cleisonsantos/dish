//! Bounded child-process execution.
//!
//! Every utility process that Dish starts for Git, providers, installation or
//! title generation must have a deadline that also covers its pipes.  A child
//! may exit while a descendant keeps stdout open, so waiting only on the direct
//! child is not enough: this module polls the reader threads, kills the whole
//! process group on Unix (or the tree with `taskkill` on Windows), and never
//! joins a reader without a bound.

use std::io::{self, Read, Write};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_millis(10);
const DRAIN_AFTER_KILL: Duration = Duration::from_millis(500);

#[derive(Debug)]
pub struct Output {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

/// Put the child in its own process group so a timeout can terminate the tools
/// it started, not just the direct child.
pub fn configure_group(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: `setpgid` is async-signal-safe and only touches the forked
        // child between fork and exec. The parent stays in its own group.
        unsafe {
            command.pre_exec(|| {
                if libc::setpgid(0, 0) == -1 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
    }
    #[cfg(not(unix))]
    {
        let _ = command;
    }
}

/// Run a command with bounded output and a deadline covering spawn, pipe
/// draining and exit.  `stderr_limit` of `None` discards stderr.
pub fn run(
    command: Command,
    timeout: Duration,
    stdout_limit: usize,
    stderr_limit: Option<usize>,
    input: Option<&[u8]>,
) -> io::Result<Output> {
    run_with_spawner(
        command,
        timeout,
        stdout_limit,
        stderr_limit,
        input,
        Command::spawn,
    )
}

pub fn run_with_spawner<F>(
    mut command: Command,
    timeout: Duration,
    stdout_limit: usize,
    stderr_limit: Option<usize>,
    input: Option<&[u8]>,
    mut spawn: F,
) -> io::Result<Output>
where
    F: FnMut(&mut Command) -> io::Result<Child>,
{
    configure_group(&mut command);
    command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped());
    if stderr_limit.is_some() {
        command.stderr(Stdio::piped());
    } else {
        command.stderr(Stdio::null());
    }
    crate::platform::hide_console(&mut command);

    let mut child = spawn(&mut command)?;
    if let Some(body) = input {
        let Some(mut stdin) = child.stdin.take() else {
            terminate_tree(&mut child);
            return Err(io::Error::other("stdin unavailable"));
        };
        let body = body.to_vec();
        // A child that never reads stdin must not block the caller.  Closing
        // the pipe (by dropping it) makes a blocked write return.
        std::thread::spawn(move || {
            let _ = stdin.write_all(&body);
        });
    }

    let (sender, receiver) = mpsc::channel::<ReadEvent>();
    let Some(stdout) = child.stdout.take() else {
        terminate_tree(&mut child);
        return Err(io::Error::other("stdout unavailable"));
    };
    let stdout_reader = spawn_reader(Stream::Stdout, stdout, stdout_limit, sender.clone());
    let stderr_reader = if let Some(limit) = stderr_limit {
        let Some(stderr) = child.stderr.take() else {
            terminate_tree(&mut child);
            return Err(io::Error::other("stderr unavailable"));
        };
        Some(spawn_reader(Stream::Stderr, stderr, limit, sender.clone()))
    } else {
        None
    };
    drop(sender);

    let deadline = Instant::now() + timeout;
    let mut stdout: Option<io::Result<Vec<u8>>> = None;
    let mut stderr: Option<io::Result<Vec<u8>>> = if stderr_limit.is_some() {
        None
    } else {
        Some(Ok(Vec::new()))
    };
    let mut status: Option<ExitStatus> = None;
    let mut exited_at: Option<Instant> = None;
    let mut failure: Option<io::Error> = None;

    loop {
        drain_events(&receiver, &mut stdout, &mut stderr, &mut failure);
        if failure.is_some() {
            break;
        }
        if status.is_none() {
            match child.try_wait() {
                Ok(value) => {
                    if value.is_some() {
                        status = value;
                        exited_at = Some(Instant::now());
                    }
                }
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
        }
        if stdout.is_some() && stderr.is_some() && status.is_some() {
            break;
        }
        let now = Instant::now();
        if status.is_some() {
            if exited_at.is_some_and(|at| now.duration_since(at) >= DRAIN_AFTER_KILL) {
                failure = Some(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "descendant kept output open after the command exited",
                ));
                break;
            }
        } else if now >= deadline {
            failure = Some(io::Error::new(
                io::ErrorKind::TimedOut,
                "command exceeded its deadline",
            ));
            break;
        }
        std::thread::sleep(POLL);
    }

    if failure.is_none() {
        let stdout = stdout
            .take()
            .ok_or_else(|| io::Error::other("stdout reader stopped"))??;
        let stderr = stderr
            .take()
            .ok_or_else(|| io::Error::other("stderr reader stopped"))??;
        join_if_finished(Some(stdout_reader));
        join_if_finished(stderr_reader);
        let status = status.ok_or_else(|| io::Error::other("command status missing"))?;
        return Ok(Output {
            status,
            stdout,
            stderr,
        });
    }

    terminate_tree(&mut child);
    let wait_until = Instant::now() + DRAIN_AFTER_KILL;
    while Instant::now() < wait_until {
        drain_events(&receiver, &mut stdout, &mut stderr, &mut failure);
        if stdout.is_some() && stderr.is_some() {
            break;
        }
        std::thread::sleep(POLL);
    }
    join_if_finished(Some(stdout_reader));
    join_if_finished(stderr_reader);
    Err(failure.unwrap_or_else(|| io::Error::other("command failed")))
}

#[derive(Clone, Copy)]
enum Stream {
    Stdout,
    Stderr,
}

struct ReadEvent {
    stream: Stream,
    result: io::Result<Vec<u8>>,
}

fn spawn_reader(
    stream: Stream,
    mut pipe: impl Read + Send + 'static,
    limit: usize,
    sender: mpsc::Sender<ReadEvent>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let result = read_limited(&mut pipe, limit);
        let _ = sender.send(ReadEvent { stream, result });
    })
}

fn read_limited(pipe: &mut impl Read, limit: usize) -> io::Result<Vec<u8>> {
    let mut kept = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        let count = pipe.read(&mut buffer)?;
        if count == 0 {
            return Ok(kept);
        }
        if kept.len().saturating_add(count) > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("output exceeded {limit} bytes"),
            ));
        }
        kept.extend_from_slice(&buffer[..count]);
    }
}

fn drain_events(
    receiver: &mpsc::Receiver<ReadEvent>,
    stdout: &mut Option<io::Result<Vec<u8>>>,
    stderr: &mut Option<io::Result<Vec<u8>>>,
    failure: &mut Option<io::Error>,
) {
    loop {
        match receiver.try_recv() {
            Ok(event) => {
                if let Err(error) = &event.result {
                    if failure.is_none() {
                        *failure = Some(io::Error::new(error.kind(), error.to_string()));
                    }
                }
                match event.stream {
                    Stream::Stdout => *stdout = Some(event.result),
                    Stream::Stderr => *stderr = Some(event.result),
                }
            }
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                if (stdout.is_none() || stderr.is_none()) && failure.is_none() {
                    *failure = Some(io::Error::other("output reader stopped"));
                }
                return;
            }
        }
    }
}

fn join_if_finished(handle: Option<std::thread::JoinHandle<()>>) {
    if let Some(handle) = handle {
        if handle.is_finished() {
            let _ = handle.join();
        }
    }
}

/// Kill the child and any descendants that remained in its process group.
pub fn terminate_tree(child: &mut Child) {
    #[cfg(unix)]
    {
        let pid = child.id() as libc::pid_t;
        // SAFETY: a negative pid targets the process group created in
        // `configure_group`; the signal number is a valid libc constant.
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
    #[cfg(windows)]
    {
        let pid = child.id().to_string();
        let mut command = Command::new("taskkill");
        command
            .args(["/T", "/F", "/PID", pid.as_str()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        crate::platform::hide_console(&mut command);
        let _ = command.status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn script(root: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
        let path = root.join(name);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }

    fn command(path: &std::path::Path) -> Command {
        let mut command = Command::new("/bin/sh");
        command.arg(path);
        command
    }

    fn root(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "dish-process-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn captures_bounded_output_and_status() {
        let dir = root("normal");
        let path = script(&dir, "emit", "printf 'ok'; printf 'erro' >&2; exit 3");
        let output = run(
            command(&path),
            Duration::from_secs(2),
            1024,
            Some(1024),
            None,
        )
        .unwrap();
        assert_eq!(output.stdout, b"ok");
        assert_eq!(output.stderr, b"erro");
        assert_eq!(output.status.code(), Some(3));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn output_over_the_limit_is_an_error() {
        let dir = root("limit");
        let path = script(&dir, "large", "printf '0123456789'");
        let error = run(command(&path), Duration::from_secs(2), 4, None, None).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn timeout_covers_a_child_that_keeps_running() {
        let dir = root("timeout");
        let path = script(&dir, "sleep", "exec sleep 10");
        let started = Instant::now();
        let error = run(command(&path), Duration::from_millis(100), 1024, None, None).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(2));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn timeout_covers_a_descendant_that_keeps_the_pipe_open() {
        let dir = root("descendant");
        let path = script(&dir, "spawn", "(sleep 10) & exit 0");
        let started = Instant::now();
        let error = run(command(&path), Duration::from_secs(2), 1024, None, None).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(3));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn closing_stdout_does_not_hide_a_running_child() {
        let dir = root("closed-stdout");
        let path = script(&dir, "closed", "exec 1>&-; exec sleep 10");
        let started = Instant::now();
        let error = run(command(&path), Duration::from_millis(100), 1024, None, None).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(started.elapsed() < Duration::from_secs(2));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
