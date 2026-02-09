//! Platform management commands.
//!
//! Manages platform support for FLUI projects (add, remove, list).
//!
//! # Planned Features
//!
//! - Add platform scaffolding (android/, ios/, web/ directories)
//! - Remove platform support
//! - List currently enabled platforms

use crate::error::{CliError, CliResult};
use console::style;

/// Add platform support to the project.
///
/// # Arguments
///
/// * `platforms` - List of platforms to add (android, ios, web, etc.)
///
/// # Errors
///
/// Returns `CliError::NotImplemented` as this feature is not yet available.
pub fn add(platforms: &[String]) -> CliResult<()> {
    cliclack::intro(style(" flui platform add ").on_yellow().black())?;

    let workaround = format!(
        "{}\n  {}\n  {}\n  {}",
        style("Manually create platform directories:").bold(),
        "• platforms/android/",
        "• platforms/ios/",
        "• platforms/web/"
    );

    cliclack::note(format!("Adding: {}", platforms.join(", ")), workaround)?;

    cliclack::outro_cancel("Platform add not yet implemented")?;

    Err(CliError::not_implemented("Platform add"))
}

/// Remove platform support from the project.
///
/// # Arguments
///
/// * `platform` - Platform to remove
///
/// # Errors
///
/// Returns `CliError::NotImplemented` as this feature is not yet available.
pub fn remove(platform: &str) -> CliResult<()> {
    cliclack::intro(style(" flui platform remove ").on_red().black())?;

    cliclack::note(
        format!("Removing: {}", platform),
        style("This will delete the platform directory")
            .dim()
            .to_string(),
    )?;

    cliclack::outro_cancel("Platform remove not yet implemented")?;

    Err(CliError::not_implemented("Platform remove"))
}

/// List all supported platforms.
///
/// This function works and shows all platforms FLUI can target.
pub fn list() -> CliResult<()> {
    cliclack::intro(style(" flui platforms ").on_blue().black())?;

    let platforms = format!(
        "{} Android      {}\n\
         {} iOS          {}\n\
         {} Web          {}\n\
         {} Windows      {}\n\
         {} Linux        {}\n\
         {} macOS        {}",
        style("●").green(),
        style("Mobile").dim(),
        style("●").green(),
        style("Mobile (macOS only)").dim(),
        style("●").green(),
        style("WASM").dim(),
        style("●").green(),
        style("Desktop").dim(),
        style("●").green(),
        style("Desktop").dim(),
        style("●").green(),
        style("Desktop").dim(),
    );

    cliclack::note("Supported Platforms", platforms)?;

    cliclack::outro(format!("{} platforms available", style("6").cyan()))?;

    Ok(())
}
