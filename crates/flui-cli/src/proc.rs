//! Bounded execution of external tools.
//!
//! Every probe the CLI makes into the environment (`adb`, `xcrun`, a
//! browser's `--version`, `rustup target list`) runs through
//! [`output_with_timeout`]. An unbounded `Command::output()` is how `flui
//! devices` once hung forever on macOS: `Safari -v` does not print a version,
//! it *launches Safari*, and the CLI sat waiting for a GUI process to exit.
//! A probe that has not answered within its budget is killed and reported
//! as a timeout, so one misbehaving tool can never freeze the command.

use std::io::{self, Read};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

/// The budget for a quick version/list probe of a well-behaved tool.
pub(crate) const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Run `command` to completion, capturing both streams, or kill it once
/// `timeout` elapses.
///
/// # Errors
///
/// - the spawn error when the tool cannot be started (typically
///   `ErrorKind::NotFound`: not on `PATH`);
/// - `ErrorKind::TimedOut` when the deadline passes; the child has been
///   killed and reaped by then.
pub(crate) fn output_with_timeout(command: &mut Command, timeout: Duration) -> io::Result<Output> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn()?;

    // Drain both pipes on their own threads: a child that fills one pipe
    // while we block on the other would deadlock a single-threaded reader.
    let deadline = Instant::now() + timeout;
    let stdout = child.stdout.take().map(spawn_reader);
    let stderr = child.stderr.take().map(spawn_reader);

    let status = wait_with_deadline(&mut child, deadline)?;

    // The drain is bounded by the same deadline. A child that exited
    // normally can still leave the pipe open through a grandchild that
    // inherited it (`adb devices` starting the adb server daemon is the
    // classic case); whatever was read by the deadline is the output, and
    // the reader thread is detached rather than joined.
    let stdout = stdout.map_or_else(Vec::new, |reader| reader.finish(deadline));
    let stderr = stderr.map_or_else(Vec::new, |reader| reader.finish(deadline));

    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

/// Run `command` and return its stdout as trimmed text when it exited
/// successfully within `timeout`; `None` otherwise (not found, failed,
/// or timed out — the distinction is logged at debug level).
///
/// This is the shape most environment probes want: "what version is it,
/// if it is there at all".
pub(crate) fn probe_stdout(command: &mut Command, timeout: Duration) -> Option<String> {
    match output_with_timeout(command, timeout) {
        Ok(output) if output.status.success() => {
            Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
        }
        Ok(output) => {
            crate::ui::debug(format!(
                "probe {} exited with {}: {}",
                command.get_program().to_string_lossy(),
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
            None
        }
        Err(error) => {
            crate::ui::debug(format!(
                "probe {} failed: {error}",
                command.get_program().to_string_lossy()
            ));
            None
        }
    }
}

/// A pipe being drained on its own thread, readable up to a deadline.
struct PipeReader {
    buffer: Arc<Mutex<Vec<u8>>>,
    done: mpsc::Receiver<()>,
}

impl PipeReader {
    /// Everything read by `deadline`; the thread is left to finish on its
    /// own if the pipe is still open then.
    fn finish(self, deadline: Instant) -> Vec<u8> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let _ = self.done.recv_timeout(remaining);
        // A poisoned lock means the reader panicked; keep what it appended.
        let guard = self
            .buffer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.clone()
    }
}

fn spawn_reader<R: Read + Send + 'static>(mut reader: R) -> PipeReader {
    let buffer = Arc::new(Mutex::new(Vec::new()));
    let (tx, done) = mpsc::channel();
    let sink = Arc::clone(&buffer);
    thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let mut guard = sink
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    guard.extend_from_slice(&chunk[..n]);
                }
            }
        }
        let _ = tx.send(());
    });
    PipeReader { buffer, done }
}

fn wait_with_deadline(
    child: &mut Child,
    deadline: Instant,
) -> io::Result<std::process::ExitStatus> {
    // 20 ms polling keeps a fast probe fast (one or two polls) without
    // burning CPU while a slow one runs; the deadline bounds the total.
    const POLL: Duration = Duration::from_millis(20);
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "process did not finish before its deadline",
            ));
        }
        thread::sleep(POLL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn fast_probe_returns_output() {
        let output = output_with_timeout(
            Command::new("sh").args(["-c", "echo out; echo err >&2"]),
            Duration::from_secs(5),
        )
        .expect("sh runs");
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "out");
        assert_eq!(String::from_utf8_lossy(&output.stderr).trim(), "err");
    }

    #[cfg(unix)]
    #[test]
    fn slow_probe_is_killed_and_reported_as_timeout() {
        let started = Instant::now();
        let error = output_with_timeout(
            Command::new("sh").args(["-c", "sleep 30"]),
            Duration::from_millis(200),
        )
        .expect_err("must time out");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the wait must end at the deadline, not when the child would have exited"
        );
    }

    /// A child that exits at once but leaves a grandchild holding its stdout
    /// (a daemon it started) must not hang the probe: the drain is bounded.
    #[cfg(unix)]
    #[test]
    fn grandchild_holding_the_pipe_does_not_hang_the_probe() {
        let started = Instant::now();
        let output = output_with_timeout(
            Command::new("sh").args(["-c", "echo early; (sleep 30) & exit 0"]),
            Duration::from_millis(500),
        )
        .expect("the child itself exited normally");
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "early");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "drain must be bounded"
        );
    }

    #[test]
    fn missing_program_is_not_found() {
        let error = output_with_timeout(
            &mut Command::new("flui-definitely-not-a-real-tool-xyz"),
            Duration::from_secs(1),
        )
        .expect_err("cannot spawn");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[cfg(unix)]
    #[test]
    fn probe_stdout_is_none_on_failure() {
        assert_eq!(
            probe_stdout(
                Command::new("sh").args(["-c", "exit 3"]),
                Duration::from_secs(1)
            ),
            None
        );
        assert_eq!(
            probe_stdout(
                Command::new("sh").args(["-c", "echo v1"]),
                Duration::from_secs(1)
            ),
            Some("v1".into())
        );
    }
}
