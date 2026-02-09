//! Clean command for removing build artifacts.
//!
//! Supports cleaning cargo build artifacts and platform-specific
//! build directories (Android, iOS, Web).

use crate::error::CliResult;
use crate::runner::{CargoCommand, OutputStyle};
use console::style;
use std::path::Path;

/// Execute the clean command.
///
/// # Arguments
///
/// * `deep` - Also clean platform-specific directories
/// * `platform` - Clean only a specific platform
///
/// # Errors
///
/// Returns `CliError::CleanFailed` if cargo clean fails.
pub fn execute(deep: bool, platform: Option<String>) -> CliResult<()> {
    cliclack::intro(style(" flui clean ").on_red().white())?;

    let spinner = cliclack::spinner();

    if let Some(ref plat) = platform {
        spinner.start(format!("Cleaning {} artifacts...", plat));
        clean_platform(plat)?;
        spinner.stop(format!("{} {} cleaned", style("✓").green(), plat));
    } else {
        spinner.start("Cleaning cargo artifacts...");
        CargoCommand::clean()
            .output_style(OutputStyle::Silent)
            .run()?;
        spinner.stop(format!("{} Cargo artifacts cleaned", style("✓").green()));

        if deep {
            let spinner = cliclack::spinner();
            spinner.start("Cleaning platform directories...");
            clean_platform_dirs()?;
            spinner.stop(format!(
                "{} Platform directories cleaned",
                style("✓").green()
            ));
        }
    }

    let mode = if deep { "deep" } else { "standard" };
    cliclack::outro(format!("Clean completed ({})", style(mode).cyan()))?;

    Ok(())
}

/// Clean build artifacts for a specific platform.
fn clean_platform(platform: &str) -> CliResult<()> {
    let platform_dir = Path::new("platforms").join(platform);

    if !platform_dir.exists() {
        return Ok(());
    }

    match platform {
        "android" => {
            remove_dir_if_exists(&platform_dir.join("app").join("build"))?;
            remove_dir_if_exists(&platform_dir.join(".gradle"))?;
        }
        "web" => {
            remove_dir_if_exists(&platform_dir.join("pkg"))?;
        }
        "ios" => {
            remove_dir_if_exists(&platform_dir.join("build"))?;
        }
        _ => {}
    }

    Ok(())
}

/// Clean all platform-specific build directories.
fn clean_platform_dirs() -> CliResult<()> {
    for platform in &["android", "ios", "web"] {
        let _ = clean_platform(platform);
    }
    Ok(())
}

/// Remove a directory if it exists.
fn remove_dir_if_exists(path: &Path) -> CliResult<()> {
    if path.exists() {
        std::fs::remove_dir_all(path)?;
    }
    Ok(())
}
