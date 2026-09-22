//! Platform management commands.
//!
//! Manages platform support for FLUI projects: add, remove, and list platforms.
//! Platform scaffolding creates the `platforms/<name>/` directory structure
//! and updates `flui.toml` configuration.

use crate::build::scaffold::{
    ScaffoldParams, is_valid_platform, scaffold_platform, valid_platform_names,
};
use crate::config::FluiConfig;
use crate::error::{CliError, CliResult};
use crate::ui;
use console::style;
use serde_json::json;

/// Add platform support to the project.
///
/// For each platform name, validates it, checks for duplicates, scaffolds the
/// platform directory, and updates `flui.toml`. Invalid names are reported
/// per-name and, unlike valid duplicates, fail the command overall: a typo
/// must not exit 0.
pub(crate) fn add(platforms: &[String]) -> CliResult<()> {
    ui::intro(style(" flui platform add ").on_yellow().black())?;

    if platforms.is_empty() {
        ui::outro(style("No platforms specified").dim())?;
        return Ok(());
    }

    // Load project config.
    let mut config = FluiConfig::load()?;
    let project_dir = std::env::current_dir()?;

    // Derive scaffold params from config.
    let lib_name = config.app.name.replace('-', "_");
    let package_name = config.app.app_id();
    let params = ScaffoldParams {
        app: &config.app.name,
        lib: &lib_name,
        package: &package_name,
    };

    let mut added_count = 0u32;
    let mut invalid = Vec::new();

    for platform in platforms {
        let platform_lower = platform.to_lowercase();

        if !is_valid_platform(&platform_lower) {
            ui::error(format!(
                "invalid platform '{}'; valid values: {}",
                platform,
                valid_platform_names().join(", ")
            ))?;
            invalid.push(platform.clone());
            continue;
        }

        let is_duplicate = config
            .build
            .target_platforms
            .iter()
            .any(|p| p == &platform_lower);

        if is_duplicate {
            ui::warning(format!("Platform '{platform_lower}' is already added"))?;
            continue;
        }

        // Scaffold the platform directory.
        let spinner = ui::spinner();
        spinner.start(format!("Creating platforms/{platform_lower}/"));

        scaffold_platform(&platform_lower, &project_dir, &params)
            .map_err(|e| CliError::build_failed(&platform_lower, e.to_string()))?;

        spinner.stop(format!("Created platforms/{platform_lower}/"));

        // Update config.
        config.build.target_platforms.push(platform_lower.clone());
        added_count += 1;

        ui::success(format!(
            "Updated flui.toml: added \"{platform_lower}\" to target_platforms"
        ))?;
        ui::emit("platform.added", &json!({ "platform": platform_lower }));
    }

    // Save updated config.
    if added_count > 0 {
        config.save(&project_dir)?;
    }

    ui::outro(format!(
        "{} platform{} added successfully",
        added_count,
        if added_count == 1 { "" } else { "s" }
    ))?;

    if !invalid.is_empty() {
        return Err(CliError::Usage(format!(
            "invalid platform(s): {}; valid values: {}",
            invalid.join(", "),
            valid_platform_names().join(", ")
        )));
    }

    Ok(())
}

/// Remove platform support from the project.
///
/// Verifies the platform exists, prompts for confirmation (unless `yes` is
/// set), removes the `platforms/<name>/` directory, and updates
/// `flui.toml`.
///
/// # Errors
///
/// Returns `CliError::NonInteractive` when `yes` is `false` and the
/// session cannot prompt (`--yes` is the way out).
pub(crate) fn remove(platform: &str, yes: bool) -> CliResult<()> {
    ui::intro(style(" flui platform remove ").on_red().black())?;

    let platform_lower = platform.to_lowercase();

    if !is_valid_platform(&platform_lower) {
        let message = format!(
            "invalid platform '{}'; valid values: {}",
            platform,
            valid_platform_names().join(", ")
        );
        ui::outro_cancel(&message)?;
        return Err(CliError::Usage(message));
    }

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
        ui::outro(
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
        ui::warning(
            "This is the last remaining platform. Removing it will leave no target platforms.",
        )?;
    }

    let platform_dir = project_dir.join("platforms").join(&platform_lower);

    // `--yes` skips the prompt entirely; otherwise the confirmation needs a
    // real terminal, or a CI job/piped input would hang or fail with an
    // opaque I/O error.
    if !yes {
        if !ui::is_interactive() {
            return Err(CliError::NonInteractive {
                what: "confirming platform removal".into(),
                hint: "pass --yes".into(),
            });
        }

        // Prompt for confirmation.
        let confirm = ui::prompt::confirm(&format!(
            "Remove {} platform? This will delete {}",
            platform_lower,
            platform_dir.display()
        ))?
        .unwrap_or(false);

        if !confirm {
            ui::outro(style("Cancelled").dim())?;
            return Ok(());
        }
    }

    // Remove the directory if it exists.
    if platform_dir.exists() {
        let spinner = ui::spinner();
        spinner.start(format!("Removing platforms/{platform_lower}/"));

        std::fs::remove_dir_all(&platform_dir).map_err(|e| {
            CliError::build_failed(&platform_lower, format!("failed to remove directory: {e}"))
        })?;

        spinner.stop(format!("Removed platforms/{platform_lower}/"));
    }

    // Update config.
    config.build.target_platforms.remove(idx);
    config.save(&project_dir)?;

    ui::success(format!(
        "Updated flui.toml: removed \"{platform_lower}\" from target_platforms"
    ))?;
    ui::emit("platform.removed", &json!({ "platform": platform_lower }));

    ui::outro(format!(
        "Platform {} removed",
        style(&platform_lower).green()
    ))?;

    Ok(())
}

/// List all supported platforms.
///
/// Shows all platforms FLUI can target, with indicators for which ones
/// are currently configured in the project.
pub(crate) fn list() -> CliResult<()> {
    ui::intro(style(" flui platforms ").on_blue().black())?;

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

    ui::note("Supported Platforms", lines.join("\n"))?;

    if !configured.is_empty() {
        ui::info(format!("Active: {}", configured.join(", ")))?;
    }

    ui::emit(
        "platform.list",
        &json!({
            "platforms": platform_info.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
            "active": configured,
        }),
    );

    ui::outro(format!(
        "{} platforms available",
        style(platform_info.len()).cyan()
    ))?;

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
