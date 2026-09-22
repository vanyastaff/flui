//! Web builds: the project's `fn main` binary compiled for
//! `wasm32-unknown-unknown`, then `wasm-bindgen --target web` turning the
//! `.wasm` into `pkg/app.js` + `pkg/app_bg.wasm`, the pair the web template's
//! `index.html` imports. `run_app` dispatches to the web runner on wasm32,
//! so a generated project needs no web-specific entry point.
//!
//! `wasm-bindgen` on `PATH` must match the `wasm-bindgen` crate the project
//! resolves to; the mismatch error the tool prints otherwise names neither
//! side, so the check runs here, before anything compiles.

use std::path::{Path, PathBuf};
use tokio::process::Command;

use crate::build::error::{BuildError, BuildResult};
use crate::build::platform::{BuildArtifacts, BuilderContext, FinalArtifacts};
use crate::build::util::{cargo, process};
use crate::proc::{PROBE_TIMEOUT, probe_stdout};

const WASM_TARGET: &str = "wasm32-unknown-unknown";
/// `index.html` imports `./pkg/app.js`; wasm-bindgen names the module after
/// `--out-name`.
const PKG_DIR: &str = "pkg";
const OUT_NAME: &str = "app";

/// Builder for the web platform.
#[derive(Debug)]
pub(crate) struct WebBuilder {
    workspace_root: PathBuf,
}

impl WebBuilder {
    #[must_use]
    pub(crate) fn new(workspace_root: &Path) -> Self {
        Self {
            workspace_root: workspace_root.to_path_buf(),
        }
    }

    /// The wasm target, `wasm-bindgen`, and its version against the project's.
    pub(crate) fn validate_environment(&self) -> BuildResult<()> {
        let output = std::process::Command::new("rustup")
            .args(["target", "list", "--installed"])
            .output()?;
        if !String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(|line| line.trim() == WASM_TARGET)
        {
            return Err(BuildError::TargetNotInstalled {
                target: WASM_TARGET.to_string(),
                install_cmd: format!("rustup target add {WASM_TARGET}"),
            });
        }

        let wanted = resolved_wasm_bindgen_version(&self.workspace_root)?;
        let installed = installed_wasm_bindgen_version();
        match check_wasm_bindgen(wanted.as_deref(), installed.as_deref()) {
            Ok(()) => Ok(()),
            Err(hint) => Err(BuildError::ToolNotFound {
                tool: match installed {
                    Some(version) => {
                        format!("wasm-bindgen matching the project (installed: {version})")
                    }
                    None => "wasm-bindgen".to_string(),
                },
                install_hint: hint,
            }),
        }
    }

    /// Compile the project's binary for wasm32; the artifact is the `.wasm`.
    pub(crate) async fn build_rust(&self, ctx: &BuilderContext) -> BuildResult<BuildArtifacts> {
        if matches!(
            ctx.target,
            crate::build::platform::BuildUnit::Library { .. }
        ) {
            return Err(BuildError::invalid_config(
                "build unit",
                "explicit static-library delivery is supported only on iOS",
            ));
        }
        if !matches!(ctx.platform, crate::build::platform::Platform::Web { .. }) {
            return Err(BuildError::InvalidPlatform {
                reason: "expected the web platform".to_string(),
            });
        }
        let dir = &ctx.workspace_root;
        let target = cargo::select_target(dir, &ctx.target).await?;
        let mut args = vec!["build".to_string(), "--target".into(), WASM_TARGET.into()];
        if let Some(flag) = ctx.profile.cargo_flag() {
            args.push(flag.into());
        }
        args.extend(target.cargo_args());
        crate::ui::debug(format!("building {} for {WASM_TARGET}", target.metadata()));
        let wasm = cargo::build_artifact(dir, &args, &target).await?;
        Ok(BuildArtifacts {
            rust_libs: vec![wasm],
            executable: None,
            metadata: target.metadata(),
        })
    }

    /// `wasm-bindgen` into `<output>/pkg`, then the page assets from
    /// `platforms/web/` beside it. The output directory is the deliverable.
    pub(crate) async fn build_platform(
        &self,
        ctx: &BuilderContext,
        artifacts: &BuildArtifacts,
    ) -> BuildResult<FinalArtifacts> {
        let wasm = artifacts
            .rust_libs
            .first()
            .ok_or_else(|| BuildError::Other("the wasm build produced no artifact".into()))?;
        let dist = &ctx.output_dir;
        let pkg = dist.join(PKG_DIR);
        std::fs::create_dir_all(&pkg)?;

        process::run(
            Command::new("wasm-bindgen")
                .args(["--target", "web", "--no-typescript", "--out-name", OUT_NAME])
                .arg("--out-dir")
                .arg(&pkg)
                .arg(wasm),
        )
        .await?;

        let web_dir = self.workspace_root.join("platforms").join("web");
        for name in ["index.html", "manifest.json"] {
            let source = web_dir.join(name);
            if source.is_file() {
                std::fs::copy(&source, dist.join(name))?;
            }
        }
        let icons = web_dir.join("icons");
        if icons.is_dir() {
            let dist_icons = dist.join("icons");
            std::fs::create_dir_all(&dist_icons)?;
            for entry in std::fs::read_dir(&icons)? {
                let entry = entry?;
                if entry.file_type()?.is_file() {
                    std::fs::copy(entry.path(), dist_icons.join(entry.file_name()))?;
                }
            }
        }
        let index = dist.join("index.html");
        if !index.is_file() {
            return Err(BuildError::path_not_found(
                web_dir.join("index.html"),
                "the web platform is not scaffolded; run `flui platform add web`",
            ));
        }

        let size_bytes = std::fs::read_dir(&pkg)?
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "wasm"))
            .filter_map(|entry| entry.metadata().ok())
            .map(|metadata| metadata.len())
            .sum();
        crate::ui::debug(format!("web build in {}", dist.display()));
        Ok(FinalArtifacts {
            app_binary: index,
            size_bytes,
        })
    }
}

/// The `wasm-bindgen` crate version the project resolves to for wasm32, or
/// `None` when the project does not depend on it at all.
fn resolved_wasm_bindgen_version(root: &Path) -> BuildResult<Option<String>> {
    let metadata = cargo_metadata::MetadataCommand::new()
        .current_dir(root)
        .other_options(vec![
            "--filter-platform".to_string(),
            WASM_TARGET.to_string(),
        ])
        .exec()
        .map_err(|error| BuildError::Other(format!("cargo metadata: {error}")))?;
    Ok(metadata
        .packages
        .iter()
        .find(|package| package.name.as_str() == "wasm-bindgen")
        .map(|package| package.version.to_string()))
}

/// `wasm-bindgen --version` is `wasm-bindgen 0.2.100`.
fn installed_wasm_bindgen_version() -> Option<String> {
    let output = probe_stdout(
        std::process::Command::new("wasm-bindgen").arg("--version"),
        PROBE_TIMEOUT,
    )?;
    output.split_whitespace().last().map(str::to_string)
}

/// `Ok` when the installed tool can process what the project compiles;
/// otherwise the exact `cargo install` that fixes it.
fn check_wasm_bindgen(wanted: Option<&str>, installed: Option<&str>) -> Result<(), String> {
    let install = |version: Option<&str>| match version {
        Some(version) => format!("cargo install wasm-bindgen-cli --version {version} --locked"),
        None => "cargo install wasm-bindgen-cli --locked".to_string(),
    };
    match (wanted, installed) {
        (_, None) => Err(install(wanted)),
        (Some(wanted), Some(installed)) if wanted != installed => Err(install(Some(wanted))),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wasm_bindgen_versions_must_match_exactly_and_the_hint_names_the_project_version() {
        assert!(check_wasm_bindgen(Some("0.2.100"), Some("0.2.100")).is_ok());
        assert_eq!(
            check_wasm_bindgen(Some("0.2.100"), Some("0.2.99")).unwrap_err(),
            "cargo install wasm-bindgen-cli --version 0.2.100 --locked"
        );
        assert_eq!(
            check_wasm_bindgen(Some("0.2.100"), None).unwrap_err(),
            "cargo install wasm-bindgen-cli --version 0.2.100 --locked"
        );
        assert_eq!(
            check_wasm_bindgen(None, None).unwrap_err(),
            "cargo install wasm-bindgen-cli --locked"
        );
        // A project without the crate cannot mismatch anything.
        assert!(check_wasm_bindgen(None, Some("0.2.99")).is_ok());
    }
}
