use crate::build::context_builder::default_output_dir;
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
pub(crate) fn execute(deep: bool, platform: Option<String>) -> CliResult<()> {
    ui::intro(style(" flui clean ").on_red().white())?;

    if let Some(ref plat) = platform {
        let plat_lower = plat.to_lowercase();
        if !VALID_PLATFORMS.contains(&plat_lower.as_str()) {
            let message = format!(
                "invalid platform '{plat}'; valid values: {}",
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
/// were actually removed: the build's default output directory, and what the
/// platform's own build tool writes inside `platforms/<platform>/`.
fn clean_platform(platform: &str) -> CliResult<Vec<PathBuf>> {
    let platform_dir = Path::new("platforms").join(platform);
    let tool_outputs: &[&str] = match platform {
        "android" => &["app/build", "build", ".gradle", "app/src/main/jniLibs"],
        "ios" => &["build"],
        _ => &[],
    };

    let mut removed = Vec::new();
    let dirs = std::iter::once(default_output_dir(Path::new(""), platform)).chain(
        tool_outputs
            .iter()
            .map(|sub_dir| platform_dir.join(sub_dir)),
    );
    for dir in dirs {
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
