//! Streaming execution of a platform build tool.
//!
//! Gradle, `wasm-pack`, `xcodebuild`, `cargo ndk` and `adb` print their own
//! progress; the builders let that output through to the terminal and only
//! care whether the tool succeeded. Contrast [`crate::proc`], which captures
//! the output of short probes under a deadline, and [`super::cargo`], which
//! reads cargo's JSON protocol line by line.

use std::process::Stdio;
use tokio::process::Command;

use crate::build::error::{BuildError, BuildResult};

/// Run `command` to completion with the terminal as its stdout and stderr.
///
/// The child is killed if the future is dropped before the tool exits, so a
/// cancelled build never leaves a Gradle daemon or an `xcodebuild` behind.
///
/// # Errors
///
/// `BuildError::CommandFailed` when the tool cannot be started or exits with
/// a non-zero status.
pub(crate) async fn run(command: &mut Command) -> BuildResult<()> {
    let std = command.as_std();
    let rendered = std::iter::once(std.get_program())
        .chain(std.get_args())
        .map(|part| part.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    match std.get_current_dir() {
        Some(dir) => crate::ui::debug(format!("running in {}: {rendered}", dir.display())),
        None => crate::ui::debug(format!("running: {rendered}")),
    }

    let status = command
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .status()
        .await
        .map_err(|error| BuildError::CommandFailed {
            command: rendered.clone(),
            exit_code: -1,
            stderr: error.to_string(),
        })?;

    if status.success() {
        Ok(())
    } else {
        Err(BuildError::CommandFailed {
            command: rendered,
            exit_code: status.code().unwrap_or(-1),
            stderr: "see the tool's output above".to_string(),
        })
    }
}

// Ported from the former flui-build crate (#1224). `/proc` makes this Linux
// only; the `kill_on_drop` it proves is set unconditionally above.
#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use std::time::{Duration, Instant};
    use tokio::task::JoinHandle;

    /// Writes its own PID to `$1`, then becomes a `sleep` the test can watch.
    const CHILD_PID_SCRIPT: &str = "printf '%s\\n' \"$$\" > \"$1\"; exec sleep 60";
    const POLL_INTERVAL: Duration = Duration::from_millis(10);
    const POLL_TIMEOUT: Duration = Duration::from_secs(2);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct ProcessSnapshot {
        starttime_ticks: u64,
        state: char,
    }

    #[tokio::test]
    async fn dropping_the_run_future_kills_the_child() {
        let temp_dir = tempfile::tempdir().expect("temporary directory");
        let pid_file = temp_dir.path().join("run.pid");
        let pid_file_arg = pid_file.to_string_lossy().into_owned();
        let mut task = tokio::spawn(async move {
            run(Command::new("sh").args(["-c", CHILD_PID_SCRIPT, "sh", &pid_file_arg])).await
        });

        let child_pid = wait_for_child_pid(&pid_file, &mut task).await;
        let before = process_snapshot(child_pid)
            .expect("child process disappeared before its cancellation could be observed");

        task.abort();
        let Err(join_error) = task.await else {
            panic!("the long-running command unexpectedly completed");
        };
        assert!(
            join_error.is_cancelled(),
            "cancelling the task must abort it"
        );

        let deadline = Instant::now() + POLL_TIMEOUT;
        loop {
            match process_snapshot(child_pid) {
                None => return,
                Some(now) if now.starttime_ticks != before.starttime_ticks || now.state == 'Z' => {
                    return;
                }
                Some(_) if Instant::now() >= deadline => {
                    cleanup_child(child_pid, before.starttime_ticks);
                    panic!("child process {child_pid} remained live after task cancellation");
                }
                Some(_) => tokio::time::sleep(POLL_INTERVAL).await,
            }
        }
    }

    async fn wait_for_child_pid<T: Send + 'static>(
        pid_file: &Path,
        task: &mut JoinHandle<T>,
    ) -> u32 {
        let deadline = Instant::now() + POLL_TIMEOUT;
        loop {
            if let Ok(contents) = fs::read_to_string(pid_file) {
                return contents
                    .trim()
                    .parse()
                    .expect("child PID file must contain a valid PID");
            }
            if Instant::now() >= deadline {
                task.abort();
                let _ = task.await;
                panic!("child process did not write its PID before the timeout");
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    fn process_snapshot(pid: u32) -> Option<ProcessSnapshot> {
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let (_, fields) = stat.rsplit_once(')')?;
        let mut fields = fields.split_whitespace();
        let state = fields.next()?.chars().next()?;
        let starttime_ticks = fields.nth(18)?.parse().ok()?;
        Some(ProcessSnapshot {
            starttime_ticks,
            state,
        })
    }

    fn cleanup_child(pid: u32, expected_starttime_ticks: u64) {
        if process_snapshot(pid).is_some_and(|s| s.starttime_ticks == expected_starttime_ticks) {
            let _ = std::process::Command::new("kill")
                .args(["-KILL", &pid.to_string()])
                .status();
        }
    }
}
