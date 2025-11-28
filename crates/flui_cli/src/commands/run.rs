//! Run command for executing FLUI applications.
//!
//! Wraps `cargo run` with hot reload support and device selection.

use crate::error::{CliError, CliResult};
use crate::runner::{CargoCommand, OutputStyle};
use console::style;

/// Execute the run command.
///
/// # Arguments
///
/// * `device` - Target device (optional, defaults to desktop)
/// * `release` - Build in release mode
/// * `hot_reload` - Enable hot reload (development mode only)
/// * `profile` - Custom build profile
/// * `verbose` - Enable verbose output
///
/// # Errors
///
/// Returns `CliError::NotFluiProject` if not in a FLUI project.
/// Returns `CliError::NoDefaultDevice` if no device is available.
/// Returns `CliError::RunFailed` if the application fails to run.
pub fn execute(
    device: Option<String>,
    release: bool,
    hot_reload: bool,
    profile: Option<String>,
    verbose: bool,
) -> CliResult<()> {
    let mode = if release { "release" } else { "debug" };
    cliclack::intro(style(" flui run ").on_green().black())?;
    cliclack::log::info(format!("Mode: {}", style(mode).cyan()))?;

    // Check if in FLUI project
    ensure_flui_project()?;

    // Select device
    let target_device = device.map_or_else(select_default_device, Ok)?;
    cliclack::log::info(format!("Target device: {}", style(&target_device).cyan()))?;

    // Build command
    let mut cmd = CargoCommand::run_app();

    if release {
        cmd = cmd.release();
    } else if let Some(prof) = profile {
        cmd = cmd.profile(prof);
    }

    // Hot reload via environment variable
    if hot_reload && !release {
        cmd = cmd.env("FLUI_HOT_RELOAD", "1");
        cliclack::log::success("Hot reload enabled")?;
    }

    if verbose {
        cmd = cmd.verbose();
    }

    cliclack::log::step("Building and running...")?;

    cmd.output_style(OutputStyle::Streaming).run()?;

    cliclack::outro(style("Application finished").green())?;

    Ok(())
}

/// Ensure we're in a FLUI project directory.
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

/// Select the default device based on host OS.
fn select_default_device() -> CliResult<String> {
    #[cfg(target_os = "windows")]
    return Ok("Windows Desktop".to_string());

    #[cfg(target_os = "linux")]
    return Ok("Linux Desktop".to_string());

    #[cfg(target_os = "macos")]
    return Ok("macOS Desktop".to_string());

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    Err(CliError::NoDefaultDevice)
}
