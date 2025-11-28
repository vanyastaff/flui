use crate::error::{CliError, CliResult, ResultExt};
use console::style;
use std::process::Command;

pub fn execute(
    device: Option<String>,
    release: bool,
    hot_reload: bool,
    profile: Option<String>,
    verbose: bool,
) -> CliResult<()> {
    let mode = if release { "release" } else { "debug" };
    println!(
        "{}",
        style(format!("Running FLUI app ({} mode)...", mode))
            .green()
            .bold()
    );
    println!();

    // Check if in FLUI project
    ensure_flui_project()?;

    // Select device
    let target_device = if let Some(device) = device {
        device
    } else {
        select_default_device()?
    };

    println!(
        "  {} Target device: {}",
        style("✓").green(),
        style(&target_device).cyan()
    );

    // Build cargo command
    let mut cmd = Command::new("cargo");
    cmd.arg("run");

    // Add mode flags
    if release {
        cmd.arg("--release");
    } else if let Some(profile) = profile {
        cmd.arg(format!("--profile={}", profile));
    }

    // Add hot reload flag (via environment variable)
    if hot_reload && !release {
        cmd.env("FLUI_HOT_RELOAD", "1");
        println!("  {} Hot reload enabled", style("✓").green());
    }

    // Add verbose flag
    if verbose {
        cmd.arg("--verbose");
    }

    println!();
    println!("{}", style("Building and running...").dim());
    println!();

    // Run command
    let status = cmd.status().context("Failed to run cargo")?;

    if !status.success() {
        return Err(CliError::RunFailed);
    }

    Ok(())
}

fn ensure_flui_project() -> CliResult<()> {
    let cargo_toml = std::path::Path::new("Cargo.toml");
    if !cargo_toml.exists() {
        return Err(CliError::NotFluiProject {
            reason: "Cargo.toml not found".to_string(),
        });
    }

    // Check for FLUI dependency
    let content = std::fs::read_to_string(cargo_toml)?;
    if !content.contains("flui_app") && !content.contains("flui_widgets") {
        return Err(CliError::NotFluiProject {
            reason: "flui_app or flui_widgets dependency not found in Cargo.toml".to_string(),
        });
    }

    Ok(())
}

fn select_default_device() -> CliResult<String> {
    // For desktop, return current OS
    #[cfg(target_os = "windows")]
    return Ok("Windows Desktop".to_string());

    #[cfg(target_os = "linux")]
    return Ok("Linux Desktop".to_string());

    #[cfg(target_os = "macos")]
    return Ok("macOS Desktop".to_string());

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    Err(CliError::NoDefaultDevice)
}
