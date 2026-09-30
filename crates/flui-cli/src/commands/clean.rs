use crate::build::output::{
    Cleaned, RemoveDir, clean_output_dirs, project_output_root, remove_dir_all,
};
use crate::error::{CliError, CliResult};
use crate::runner::{CargoCommand, OutputStyle};
use crate::ui;
use console::style;
use serde_json::json;
use std::path::{Path, PathBuf};

/// Platform names `flui clean --platform` accepts.
/// `desktop` is one output directory for every desktop target, the way
/// `flui build` names it.
const VALID_PLATFORMS: &[&str] = &["android", "ios", "web", "desktop"];

/// Execute the clean command.
///
/// # Arguments
///
/// * `deep` - Also clean what platform build tools write in `platforms/`
/// * `platform` - Clean only a specific platform
///
/// # Errors
///
/// Returns `CliError::CleanFailed` if cargo clean fails, or `CliError::Missing`
/// if `platform` names something other than `android`, `ios`, `web` or
/// `desktop`.
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
        let root = std::env::current_dir()?;
        let cleaned = clean_platform(
            &root,
            &project_output_root(&root),
            &plat_lower,
            remove_dir_all,
        );
        spinner.stop(format!("{} {plat_lower} cleaned", style("✓").green()));
        report_removed(&cleaned.removed)?;
        if let Some(failure) = cleaned.failure {
            return Err(failure.into());
        }
    } else {
        // Build outputs first: the `--output` directories builds claimed are
        // recorded under `target/`, which `cargo clean` removes.
        let spinner = ui::spinner();
        spinner.start("Cleaning build outputs...");
        let cleaned = clean_build_outputs(&std::env::current_dir()?, deep, remove_dir_all);
        spinner.stop(format!("{} Build outputs cleaned", style("✓").green()));
        report_removed(&cleaned.removed)?;

        let spinner = ui::spinner();
        spinner.start("Cleaning cargo artifacts...");
        let _ = CargoCommand::clean()
            .output_style(OutputStyle::Silent)
            .run()?;
        spinner.stop(format!("{} Cargo artifacts cleaned", style("✓").green()));
        // A directory that could not be removed fails the clean, but only
        // after everything else, cargo's artifacts included, was cleaned.
        if let Some(failure) = cleaned.failure {
            return Err(failure.into());
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

/// The build outputs of every platform: each one's output directories and,
/// with `deep`, what its build tool writes in `platforms/`. A platform whose
/// removal fails does not stop the others.
fn clean_build_outputs(root: &Path, deep: bool, remove: RemoveDir) -> Cleaned {
    let output_root = project_output_root(root);
    let mut cleaned = Cleaned::default();
    for platform in VALID_PLATFORMS {
        cleaned.merge(if deep {
            clean_platform(root, &output_root, platform, remove)
        } else {
            clean_output_dirs(root, &output_root, platform, remove)
        });
    }
    cleaned
}

/// Clean build artifacts for a specific platform: the build's output
/// directories (the default one and each `--output` directory a build
/// claimed, see [`clean_output_dirs`]), and what the platform's own build
/// tool writes inside `platforms/<platform>/`.
fn clean_platform(root: &Path, output_root: &Path, platform: &str, remove: RemoveDir) -> Cleaned {
    let mut cleaned = clean_output_dirs(root, output_root, platform, remove);

    let platform_dir = root.join("platforms").join(platform);
    let tool_outputs: &[&str] = match platform {
        "android" => &["app/build", "build", ".gradle", "app/src/main/jniLibs"],
        "ios" => &["build"],
        _ => &[],
    };
    for sub_dir in tool_outputs {
        let dir = platform_dir.join(sub_dir);
        if dir.exists() {
            let result = remove(&dir);
            cleaned.record(dir, result);
        }
    }
    cleaned
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build::output::prepare_output_dir;
    use crate::build::{BuilderContextBuilder, Platform, Profile};

    /// A project with a web build claiming `dist-web/` and a desktop build
    /// claiming `dist-desktop/` beside it, and Gradle output in
    /// `platforms/android/app/build/`. Returns the temp dir, the project
    /// root, the claimed directories and the Gradle output.
    fn built_project() -> (tempfile::TempDir, PathBuf, Vec<PathBuf>, PathBuf) {
        let tmp = tempfile::tempdir().expect("temp dir");
        let root = tmp.path().join("app");
        std::fs::create_dir_all(&root).expect("project dir");
        let mut claimed = Vec::new();
        for platform in [
            Platform::Web {
                target: "web".to_string(),
            },
            Platform::Desktop { target: None },
        ] {
            let out = tmp.path().join(format!("dist-{}", platform.name()));
            let ctx = BuilderContextBuilder::new(root.clone())
                .with_platform(platform)
                .with_profile(Profile::Debug)
                .with_output_dir(out.clone())
                .build();
            prepare_output_dir(&ctx).expect("claim --output");
            claimed.push(out);
        }
        let gradle = root.join("platforms/android/app/build");
        std::fs::create_dir_all(&gradle).expect("gradle output");
        (tmp, root, claimed, gradle)
    }

    fn a_plain_clean_removes_claimed_out_dirs() {
        let (_tmp, root, claimed, gradle) = built_project();
        let cleaned = clean_build_outputs(&root, false, remove_dir_all);
        assert!(cleaned.failure.is_none(), "clean failed");
        for out in &claimed {
            assert!(!out.exists(), "{} survived a plain clean", out.display());
        }
        assert!(gradle.is_dir(), "a plain clean removed Gradle's output");
    }

    fn a_deep_clean_also_removes_build_tool_output() {
        let (_tmp, root, claimed, gradle) = built_project();
        let cleaned = clean_build_outputs(&root, true, remove_dir_all);
        assert!(cleaned.failure.is_none(), "clean failed");
        for out in &claimed {
            assert!(!out.exists(), "{} survived a deep clean", out.display());
        }
        assert!(!gradle.exists(), "a deep clean left Gradle's output");
    }

    /// A remover that fails on the web build's claimed directory.
    fn remove_all_but_web(dir: &Path) -> std::io::Result<()> {
        if dir.ends_with("dist-web") {
            return Err(std::io::Error::other("locked"));
        }
        remove_dir_all(dir)
    }

    fn one_platforms_failure_does_not_stop_the_others() {
        let (_tmp, root, claimed, _gradle) = built_project();
        let cleaned = clean_build_outputs(&root, false, remove_all_but_web);
        assert!(
            cleaned.failure.is_some(),
            "the web failure was not reported"
        );
        assert!(claimed[0].is_dir(), "the locked web output was removed");
        assert!(
            !claimed[1].exists(),
            "the desktop output was skipped after the web failure"
        );
    }

    #[test]
    fn clean_without_a_platform_removes_every_platforms_outputs() {
        crate::test_cases::run_cases(&[
            (
                "a_plain_clean_removes_claimed_out_dirs",
                a_plain_clean_removes_claimed_out_dirs,
            ),
            (
                "a_deep_clean_also_removes_build_tool_output",
                a_deep_clean_also_removes_build_tool_output,
            ),
            (
                "one_platforms_failure_does_not_stop_the_others",
                one_platforms_failure_does_not_stop_the_others,
            ),
        ]);
    }
}
