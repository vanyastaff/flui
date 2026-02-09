use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::error::{BuildError, BuildResult};
use crate::output_parser::{get_parser, BuildEvent};
use crate::progress::BuildProgress;

/// Run a command and stream output to console
pub async fn run_command<S: AsRef<str>>(program: &str, args: &[S]) -> BuildResult<()> {
    let args_str: Vec<&str> = args.iter().map(std::convert::AsRef::as_ref).collect();

    tracing::info!("Running: {} {}", program, args_str.join(" "));

    let status = Command::new(program)
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
#[allow(dead_code)]
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

    let output = Command::new(program)
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

    let status = Command::new(program)
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
#[allow(dead_code)]
pub(crate) async fn run_command_with_progress<S: AsRef<str>>(
    program: &str,
    args: &[S],
    mut progress: Option<&mut BuildProgress>,
    verbose: bool,
) -> BuildResult<()> {
    let args_str: Vec<&str> = args.iter().map(std::convert::AsRef::as_ref).collect();

    tracing::debug!("Running: {} {}", program, args_str.join(" "));

    let mut child = Command::new(program)
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

            if let Some(event) = parser.parse_line(&line) {
                if let Some(ref mut prog) = progress {
                    match event {
                        BuildEvent::Started { task } => {
                            if !verbose {
                                prog.set_message(&task);
                            }
                        }
                        BuildEvent::Progress { current, total } => {
                            #[allow(clippy::cast_possible_truncation)]
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
