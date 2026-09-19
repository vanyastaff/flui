use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::error::{BuildError, BuildResult};
use crate::output_parser::{BuildEvent, get_parser};
use crate::progress::BuildProgress;

fn cancellation_safe_command(program: &str) -> Command {
    let mut command = Command::new(program);
    command.kill_on_drop(true);
    command
}

/// Run a command and stream output to console
pub async fn run_command<S: AsRef<str>>(program: &str, args: &[S]) -> BuildResult<()> {
    let args_str: Vec<&str> = args.iter().map(std::convert::AsRef::as_ref).collect();

    tracing::info!("Running: {} {}", program, args_str.join(" "));

    let status = cancellation_safe_command(program)
        .args(&args_str)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .map_err(|e| BuildError::CommandFailed {
            command: format!("{} {}", program, args_str.join(" ")),
            exit_code: -1,
            stderr: e.to_string(),
        })?;

    if !status.success() {
        return Err(BuildError::CommandFailed {
            command: format!("{} {}", program, args_str.join(" ")),
            exit_code: status.code().unwrap_or(-1),
            stderr: format!("Command failed with exit code: {:?}", status.code()),
        });
    }

    Ok(())
}

/// Run a command and capture output
#[cfg_attr(not(test), expect(dead_code))]
pub(crate) async fn run_command_with_output<S: AsRef<str>>(
    program: &str,
    args: &[S],
) -> BuildResult<String> {
    let args_str: Vec<&str> = args.iter().map(std::convert::AsRef::as_ref).collect();

    tracing::debug!(
        "Running (capturing output): {} {}",
        program,
        args_str.join(" ")
    );

    let output = cancellation_safe_command(program)
        .args(&args_str)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|e| BuildError::CommandFailed {
            command: format!("{} {}", program, args_str.join(" ")),
            exit_code: -1,
            stderr: e.to_string(),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(BuildError::CommandFailed {
            command: format!("{} {}", program, args_str.join(" ")),
            exit_code: output.status.code().unwrap_or(-1),
            stderr: stderr.to_string(),
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(stdout)
}

/// Run a command in a specific directory
pub async fn run_command_in_dir<S: AsRef<str>>(
    program: &str,
    args: &[S],
    dir: &Path,
) -> BuildResult<()> {
    let args_str: Vec<&str> = args.iter().map(std::convert::AsRef::as_ref).collect();

    tracing::info!("Running in {:?}: {} {}", dir, program, args_str.join(" "));

    let status = cancellation_safe_command(program)
        .args(&args_str)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .map_err(|e| BuildError::CommandFailed {
            command: format!("{} {}", program, args_str.join(" ")),
            exit_code: -1,
            stderr: e.to_string(),
        })?;

    if !status.success() {
        return Err(BuildError::CommandFailed {
            command: format!("{} {}", program, args_str.join(" ")),
            exit_code: status.code().unwrap_or(-1),
            stderr: format!("Command failed with exit code: {:?}", status.code()),
        });
    }

    Ok(())
}

/// Run a command with progress reporting and output parsing
///
/// # Arguments
///
/// * `program` - Command to run
/// * `args` - Command arguments
/// * `progress` - Optional progress reporter
/// * `verbose` - If true, show all output; if false, only show parsed events
#[cfg_attr(not(test), expect(dead_code))]
pub(crate) async fn run_command_with_progress<S: AsRef<str>>(
    program: &str,
    args: &[S],
    mut progress: Option<&mut BuildProgress>,
    verbose: bool,
) -> BuildResult<()> {
    let args_str: Vec<&str> = args.iter().map(std::convert::AsRef::as_ref).collect();

    tracing::debug!("Running: {} {}", program, args_str.join(" "));

    let mut child = cancellation_safe_command(program)
        .args(&args_str)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| BuildError::CommandFailed {
            command: format!("{} {}", program, args_str.join(" ")),
            exit_code: -1,
            stderr: e.to_string(),
        })?;

    let parser = get_parser(program);

    if let Some(stdout) = child.stdout.take() {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            if verbose {
                tracing::info!("{}", line);
            }

            if let Some(event) = parser.parse_line(&line)
                && let Some(ref mut prog) = progress
            {
                match event {
                    BuildEvent::Started { task } => {
                        if !verbose {
                            prog.set_message(&task);
                        }
                    }
                    BuildEvent::Progress { current, total } => {
                        let percent = (current * 100 / total.max(1)) as u8;
                        prog.set_progress(percent);
                    }
                    BuildEvent::Completed { task, duration_ms } => {
                        let msg = if let Some(ms) = duration_ms {
                            format!("{} ({:.2}s)", task, ms as f64 / 1000.0)
                        } else {
                            task
                        };
                        if !verbose {
                            prog.finish_phase(msg);
                        }
                    }
                    BuildEvent::Warning { message } => {
                        if !verbose {
                            tracing::warn!("{}", message);
                        }
                    }
                    BuildEvent::Error { message } => {
                        if !verbose {
                            tracing::error!("{}", message);
                        }
                    }
                    BuildEvent::Info { message } => {
                        if !verbose {
                            prog.set_message(&message);
                        }
                    }
                }
            }
        }
    }

    let status = child.wait().await.map_err(|e| BuildError::CommandFailed {
        command: format!("{} {}", program, args_str.join(" ")),
        exit_code: -1,
        stderr: e.to_string(),
    })?;

    if !status.success() {
        return Err(BuildError::CommandFailed {
            command: format!("{} {}", program, args_str.join(" ")),
            exit_code: status.code().unwrap_or(-1),
            stderr: format!("Command failed with exit code: {:?}", status.code()),
        });
    }

    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use std::time::{Duration, Instant};
    use tokio::task::JoinHandle;

    const CHILD_PID_SCRIPT: &str = "printf '%s\\n' \"$$\" > \"$1\"; exec sleep 60";
    const POLL_INTERVAL: Duration = Duration::from_millis(10);
    const POLL_TIMEOUT: Duration = Duration::from_secs(2);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct ProcessSnapshot {
        starttime_ticks: u64,
        state: char,
    }

    #[tokio::test]
    async fn run_command_kills_child_when_cancelled() {
        let temp_dir = tempfile::tempdir().expect("failed to create temporary directory");
        let pid_file = temp_dir.path().join("run-command.pid");
        let pid_file_arg = pid_file.to_string_lossy().into_owned();
        let command_task = tokio::spawn(async move {
            run_command("sh", &["-c", CHILD_PID_SCRIPT, "sh", &pid_file_arg]).await
        });

        assert_cancellation_kills_child(command_task, &pid_file).await;
    }

    #[tokio::test]
    async fn run_command_with_output_kills_child_when_cancelled() {
        let temp_dir = tempfile::tempdir().expect("failed to create temporary directory");
        let pid_file = temp_dir.path().join("run-command-with-output.pid");
        let pid_file_arg = pid_file.to_string_lossy().into_owned();
        let command_task = tokio::spawn(async move {
            run_command_with_output("sh", &["-c", CHILD_PID_SCRIPT, "sh", &pid_file_arg]).await
        });

        assert_cancellation_kills_child(command_task, &pid_file).await;
    }

    #[tokio::test]
    async fn run_command_in_dir_kills_child_when_cancelled() {
        let temp_dir = tempfile::tempdir().expect("failed to create temporary directory");
        let pid_file = temp_dir.path().join("run-command-in-dir.pid");
        let pid_file_arg = pid_file.to_string_lossy().into_owned();
        let working_dir = temp_dir.path().to_owned();
        let command_task = tokio::spawn(async move {
            run_command_in_dir(
                "sh",
                &["-c", CHILD_PID_SCRIPT, "sh", &pid_file_arg],
                &working_dir,
            )
            .await
        });

        assert_cancellation_kills_child(command_task, &pid_file).await;
    }

    #[tokio::test]
    async fn run_command_with_progress_kills_child_when_cancelled() {
        let temp_dir = tempfile::tempdir().expect("failed to create temporary directory");
        let pid_file = temp_dir.path().join("run-command-with-progress.pid");
        let pid_file_arg = pid_file.to_string_lossy().into_owned();
        let command_task = tokio::spawn(async move {
            run_command_with_progress(
                "sh",
                &["-c", CHILD_PID_SCRIPT, "sh", &pid_file_arg],
                None,
                false,
            )
            .await
        });

        assert_cancellation_kills_child(command_task, &pid_file).await;
    }

    async fn assert_cancellation_kills_child<T>(mut command_task: JoinHandle<T>, pid_file: &Path)
    where
        T: Send + 'static,
    {
        let child_pid = wait_for_child_pid(pid_file, &mut command_task).await;
        let child_snapshot = process_snapshot(child_pid)
            .expect("child process disappeared before its cancellation could be observed");

        command_task.abort();
        let Err(join_error) = command_task.await else {
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
                Some(current_snapshot)
                    if current_snapshot.starttime_ticks != child_snapshot.starttime_ticks
                        || current_snapshot.state == 'Z' =>
                {
                    return;
                }
                Some(_) if Instant::now() >= deadline => {
                    cleanup_child(child_pid, child_snapshot.starttime_ticks);
                    panic!("child process {child_pid} remained live after task cancellation");
                }
                Some(_) => tokio::time::sleep(POLL_INTERVAL).await,
            }
        }
    }

    async fn wait_for_child_pid<T>(pid_file: &Path, command_task: &mut JoinHandle<T>) -> u32
    where
        T: Send + 'static,
    {
        let deadline = Instant::now() + POLL_TIMEOUT;
        loop {
            if let Ok(pid_contents) = fs::read_to_string(pid_file) {
                return pid_contents
                    .trim()
                    .parse()
                    .expect("child PID file must contain a valid PID");
            }

            if Instant::now() >= deadline {
                command_task.abort();
                let _ = command_task.await;
                panic!("child process did not write its PID before the timeout");
            }

            tokio::time::sleep(POLL_INTERVAL).await;
        }
    }

    fn process_snapshot(pid: u32) -> Option<ProcessSnapshot> {
        let stat_contents = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let (_, stat_fields) = stat_contents.rsplit_once(')')?;
        let mut stat_fields = stat_fields.split_whitespace();
        let state = stat_fields.next()?.chars().next()?;
        let starttime_ticks = stat_fields.nth(18)?.parse().ok()?;

        Some(ProcessSnapshot {
            starttime_ticks,
            state,
        })
    }

    fn cleanup_child(pid: u32, expected_starttime_ticks: u64) {
        if process_snapshot(pid)
            .is_some_and(|snapshot| snapshot.starttime_ticks == expected_starttime_ticks)
        {
            let _ = std::process::Command::new("kill")
                .args(["-KILL", &pid.to_string()])
                .status();
        }
    }
}
