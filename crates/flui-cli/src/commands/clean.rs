use crate::error::{CliError, CliResult};
use crate::runner::{CargoCommand, OutputStyle};
use crate::ui;
use console::style;
use serde_json::json;
use std::path::{Path, PathBuf};

/// Platform names `flui clean --platform` accepts.
const VALID_PLATFORMS: &[&str] = &["android", "ios", "web"];

/// Execute the clean command.
///
/// # Arguments
///
/// * `deep` - Also clean platform-specific directories
/// * `platform` - Clean only a specific platform
///
/// # Errors
///
/// Returns `CliError::CleanFailed` if cargo clean fails, or `CliError::Missing`
/// if `platform` names something other than `android`, `ios` or `web`.
pub fn execute(deep: bool, platform: Option<String>) -> CliResult<()> {
    ui::intro(style(" flui clean ").on_red().white())?;

    if let Some(ref plat) = platform {
        let plat_lower = plat.to_lowercase();
        if !VALID_PLATFORMS.contains(&plat_lower.as_str()) {
            let message = format!(
                "Invalid platform '{plat}'. Valid values: {}",
                VALID_PLATFORMS.join(", ")
            );
            ui::outro_cancel(&message)?;
            return Err(CliError::Missing(message));
        }

        let spinner = ui::spinner();
        spinner.start(format!("Cleaning {plat_lower} artifacts..."));
        let removed = clean_platform(&plat_lower)?;
        spinner.stop(format!("{} {plat_lower} cleaned", style("✓").green()));
        report_removed(&removed)?;
    } else {
        let spinner = ui::spinner();
        spinner.start("Cleaning cargo artifacts...");
        let _ = CargoCommand::clean()
            .output_style(OutputStyle::Silent)
            .run()?;
        spinner.stop(format!("{} Cargo artifacts cleaned", style("✓").green()));

        if deep {
            let spinner = ui::spinner();
            spinner.start("Cleaning platform directories...");
            let mut removed = Vec::new();
            for platform in VALID_PLATFORMS {
                removed.extend(clean_platform(platform)?);
            }
            spinner.stop(format!(
                "{} Platform directories cleaned",
                style("✓").green()
            ));
            report_removed(&removed)?;
        }
    }

    let mode = if deep { "deep" } else { "standard" };
    ui::emit("clean.done", &json!({ "ok": true, "mode": mode }));
    ui::outro(format!("Clean completed ({})", style(mode).cyan()))?;

    Ok(())
}

/// Emit a `clean.removed` event per path and, in human mode, list them.
fn report_removed(removed: &[PathBuf]) -> CliResult<()> {
    for path in removed {
        ui::emit(
            "clean.removed",
            &json!({ "path": path.display().to_string() }),
        );
    }
    for path in removed {
        ui::info(format!("Removed {}", path.display()))?;
    }
    Ok(())
}

/// Clean build artifacts for a specific platform, returning the paths that
/// were actually removed.
fn clean_platform(platform: &str) -> CliResult<Vec<PathBuf>> {
    let platform_dir = Path::new("platforms").join(platform);

    if !platform_dir.exists() {
        return Ok(Vec::new());
    }

    let sub_dirs: &[&str] = match platform {
        "android" => &["app/build", ".gradle"],
        "web" => &["pkg"],
        "ios" => &["build"],
        _ => &[],
    };

    let mut removed = Vec::new();
    for sub_dir in sub_dirs {
        let dir = platform_dir.join(sub_dir);
        if remove_dir_if_exists(&dir)? {
            removed.push(dir);
        }
    }

    Ok(removed)
}

/// Remove a directory if it exists; returns whether it was removed.
fn remove_dir_if_exists(path: &Path) -> CliResult<bool> {
    if path.exists() {
        std::fs::remove_dir_all(path)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_platforms_are_exactly_the_documented_three() {
        assert_eq!(VALID_PLATFORMS, &["android", "ios", "web"]);
    }
}
