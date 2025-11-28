use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

use crate::error::{BuildError, BuildResult};

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
pub async fn run_command_with_output<S: AsRef<str>>(
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
