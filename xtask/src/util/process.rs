use anyhow::{anyhow, Context, Result};
use std::process::Stdio;
use tokio::process::Command;

/// Run a command and stream output to console
pub async fn run_command<S: AsRef<str>>(program: &str, args: &[S]) -> Result<()> {
    let args_str: Vec<&str> = args.iter().map(|s| s.as_ref()).collect();

    tracing::debug!("Running: {} {}", program, args_str.join(" "));

    let status = Command::new(program)
        .args(&args_str)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await
        .with_context(|| format!("Failed to execute: {}", program))?;

    if !status.success() {
        return Err(anyhow!("Command failed with exit code: {:?}", status.code()));
    }

    Ok(())
}
