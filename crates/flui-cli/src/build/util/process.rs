use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

use crate::build::error::{BuildError, BuildResult};

/// Run a command and stream output to console
pub(crate) async fn run_command<S: AsRef<str>>(program: &str, args: &[S]) -> BuildResult<()> {
    let args_str: Vec<&str> = args.iter().map(std::convert::AsRef::as_ref).collect();

    crate::ui::debug(format!("Running: {} {}", program, args_str.join(" ")));

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

/// Run a command in a specific directory
pub(crate) async fn run_command_in_dir<S: AsRef<str>>(
    program: &str,
    args: &[S],
    dir: &Path,
) -> BuildResult<()> {
    let args_str: Vec<&str> = args.iter().map(std::convert::AsRef::as_ref).collect();

    crate::ui::debug(format!(
        "Running in {}: {} {}",
        dir.display(),
        program,
        args_str.join(" ")
    ));

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
