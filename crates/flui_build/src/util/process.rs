use anyhow::{anyhow, Context, Result};
use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

/// Run a command and stream output to console
pub async fn run_command<S: AsRef<str>>(program: &str, args: &[S]) -> Result<()> {
    let args_str: Vec<&str> = args.iter().map(|s| s.as_ref()).collect();

    tracing::info!("Running: {} {}", program, args_str.join(" "));

    let status = Command::new(program)
        .args(&args_str)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .with_context(|| format!("Failed to execute: {}", program))?;

    if !status.success() {
        return Err(anyhow!(
            "Command failed with exit code: {:?}",
            status.code()
        ));
    }

    Ok(())
}

/// Run a command and capture output
pub async fn run_command_with_output<S: AsRef<str>>(program: &str, args: &[S]) -> Result<String> {
    let args_str: Vec<&str> = args.iter().map(|s| s.as_ref()).collect();

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
        .with_context(|| format!("Failed to execute: {}", program))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("Command failed: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(stdout)
}

/// Run a command in a specific directory
pub async fn run_command_in_dir<S: AsRef<str>>(
    program: &str,
    args: &[S],
    dir: &Path,
) -> Result<()> {
    let args_str: Vec<&str> = args.iter().map(|s| s.as_ref()).collect();

    tracing::info!("Running in {:?}: {} {}", dir, program, args_str.join(" "));

    let status = Command::new(program)
        .args(&args_str)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .with_context(|| format!("Failed to execute: {}", program))?;

    if !status.success() {
        return Err(anyhow!(
            "Command failed with exit code: {:?}",
            status.code()
        ));
    }

    Ok(())
}
