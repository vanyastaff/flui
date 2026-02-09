//! Platform management commands.
//!
//! Manages platform support for FLUI projects: add, remove, and list platforms.
//! Platform scaffolding creates the `platforms/<name>/` directory structure
//! and updates `flui.toml` configuration.

use crate::config::FluiConfig;
use crate::error::{CliError, CliResult};
use console::style;
use flui_build::scaffold::{
    is_valid_platform, scaffold_platform, valid_platform_names, ScaffoldParams,
};

/// Add platform support to the project.
///
/// For each platform name, validates it, checks for duplicates, scaffolds the
/// platform directory, and updates `flui.toml`.
pub fn add(platforms: &[String]) -> CliResult<()> {
    cliclack::intro(style(" flui platform add ").on_yellow().black())?;

    if platforms.is_empty() {
        cliclack::outro(style("No platforms specified").dim())?;
        return Ok(());
    }

    // Load project config.
    let mut config = FluiConfig::load()?;
    let project_dir = std::env::current_dir()?;

    // Derive scaffold params from config.
    let lib_name = config.app.name.replace('-', "_");
    let package_name = config.app.app_id();
    let params = ScaffoldParams {
        app_name: &config.app.name,
        lib_name: &lib_name,
        package_name: &package_name,
    };

    let mut added_count = 0u32;

    for platform in platforms {
        let platform_lower = platform.to_lowercase();

        let is_duplicate = config
            .build
            .target_platforms
            .iter()
            .any(|p| p == &platform_lower);

        match platform_lower.as_str() {
            p if !is_valid_platform(p) => {
                cliclack::log::error(format!(
                    "Invalid platform '{}'. Valid: {}",
                    platform,
                    valid_platform_names().join(", ")
                ))?;
            }
            _ if is_duplicate => {
                cliclack::log::warning(format!("Platform '{platform_lower}' is already added"))?;
            }
            _ => {
                // Scaffold the platform directory.
                let spinner = cliclack::spinner();
                spinner.start(format!("Creating platforms/{platform_lower}/"));

                scaffold_platform(&platform_lower, &project_dir, &params)
                    .map_err(|e| CliError::build_failed(&platform_lower, e.to_string()))?;

                spinner.stop(format!("Created platforms/{platform_lower}/"));

                // Update config.
                config.build.target_platforms.push(platform_lower.clone());
                added_count += 1;

                cliclack::log::success(format!(
                    "Updated flui.toml: added \"{platform_lower}\" to target_platforms"
                ))?;
            }
        }
    }

    // Save updated config.
    if added_count > 0 {
        config.save(&project_dir)?;
    }

    cliclack::outro(format!(
        "{} platform{} added successfully",
        added_count,
        if added_count == 1 { "" } else { "s" }
    ))?;

    Ok(())
}

/// Remove platform support from the project.
///
/// Verifies the platform exists, prompts for confirmation, removes the
/// `platforms/<name>/` directory, and updates `flui.toml`.
pub fn remove(platform: &str) -> CliResult<()> {
    cliclack::intro(style(" flui platform remove ").on_red().black())?;

    let platform_lower = platform.to_lowercase();

    // Load project config.
    let mut config = FluiConfig::load()?;
    let project_dir = std::env::current_dir()?;

    // Verify platform is configured.
    let idx = config
        .build
        .target_platforms
        .iter()
        .position(|p| p == &platform_lower);

    let Some(idx) = idx else {
        cliclack::outro(
            style(format!(
                "Platform '{platform_lower}' is not configured in this project"
            ))
            .red(),
        )?;
        return Err(CliError::Missing(format!(
            "Platform '{platform_lower}' not in target_platforms"
        )));
    };

    // Warn if this is the last platform.
    if config.build.target_platforms.len() == 1 {
        cliclack::log::warning(
            "This is the last remaining platform. Removing it will leave no target platforms.",
        )?;
    }

    // Prompt for confirmation.
    let platform_dir = project_dir.join("platforms").join(&platform_lower);
    let confirm = cliclack::confirm(format!(
        "Remove {} platform? This will delete {}",
        platform_lower,
        platform_dir.display()
    ))
    .interact()?;

    if !confirm {
        cliclack::outro(style("Cancelled").dim())?;
        return Ok(());
    }

    // Remove the directory if it exists.
    if platform_dir.exists() {
        let spinner = cliclack::spinner();
        spinner.start(format!("Removing platforms/{platform_lower}/"));

        std::fs::remove_dir_all(&platform_dir).map_err(|e| {
            CliError::build_failed(&platform_lower, format!("Failed to remove directory: {e}"))
        })?;

        spinner.stop(format!("Removed platforms/{platform_lower}/"));
    }

    // Update config.
    config.build.target_platforms.remove(idx);
    config.save(&project_dir)?;

    cliclack::log::success(format!(
        "Updated flui.toml: removed \"{platform_lower}\" from target_platforms"
    ))?;

    cliclack::outro(format!(
        "Platform {} removed",
        style(&platform_lower).green()
    ))?;

    Ok(())
}

/// List all supported platforms.
///
/// Shows all platforms FLUI can target, with indicators for which ones
/// are currently configured in the project.
pub fn list() -> CliResult<()> {
    cliclack::intro(style(" flui platforms ").on_blue().black())?;

    // Try to load project config for status indicators.
    let configured = FluiConfig::load()
        .ok()
        .map(|c| c.build.target_platforms)
        .unwrap_or_default();

    let platform_info: &[(&str, &str)] = &[
        ("android", "Mobile"),
        ("ios", "Mobile (macOS only)"),
        ("web", "WASM"),
        ("windows", "Desktop"),
        ("linux", "Desktop"),
        ("macos", "Desktop"),
    ];

    let mut lines = Vec::new();
    for (name, category) in platform_info {
        let indicator = if configured.iter().any(|p| p == name) {
            style("●").green()
        } else {
            style("○").dim()
        };
        lines.push(format!(
            "{} {:<12} {}",
            indicator,
            name,
            style(category).dim()
        ));
    }

    cliclack::note("Supported Platforms", lines.join("\n"))?;

    if !configured.is_empty() {
        cliclack::log::info(format!("Active: {}", configured.join(", ")))?;
    }

    cliclack::outro(format!("{} platforms available", style("6").cyan()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_platform_check() {
        assert!(is_valid_platform("android"));
        assert!(is_valid_platform("Android"));
        assert!(is_valid_platform("ios"));
        assert!(!is_valid_platform("fuchsia"));
        assert!(!is_valid_platform(""));
    }

    #[test]
    fn test_valid_platform_names_complete() {
        let names = valid_platform_names();
        assert_eq!(names.len(), 6);
        assert!(names.contains(&"android"));
        assert!(names.contains(&"ios"));
        assert!(names.contains(&"web"));
        assert!(names.contains(&"windows"));
        assert!(names.contains(&"linux"));
        assert!(names.contains(&"macos"));
    }
}
