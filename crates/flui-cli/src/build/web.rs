use std::path::{Path, PathBuf};

use crate::build::error::{BuildError, BuildResult};
use crate::build::platform::{
    BuildArtifacts, BuilderContext, FinalArtifacts, PlatformBuilder, private,
};
use crate::build::util::{check_command_exists, process};

/// Builder for Web/WASM platform (via wasm-pack)
#[derive(Debug)]
pub struct WebBuilder {
    workspace_root: PathBuf,
}

impl WebBuilder {
    /// Creates a new `WebBuilder`
    #[must_use]
    pub fn new(workspace_root: &Path) -> Self {
        Self {
            workspace_root: workspace_root.to_path_buf(),
        }
    }
}

impl private::Sealed for WebBuilder {}

impl PlatformBuilder for WebBuilder {
    fn validate_environment(&self) -> BuildResult<()> {
        // Check wasm-pack
        check_command_exists("wasm-pack")?;

        // Check WASM target
        let output = std::process::Command::new("rustup")
            .args(["target", "list", "--installed"])
            .output()?;

        let installed_targets = String::from_utf8_lossy(&output.stdout);

        if !installed_targets.contains("wasm32-unknown-unknown") {
            return Err(BuildError::TargetNotInstalled {
                target: "wasm32-unknown-unknown".to_string(),
                install_cmd: "rustup target add wasm32-unknown-unknown".to_string(),
            });
        }

        Ok(())
    }

    async fn build_rust(&self, ctx: &BuilderContext) -> BuildResult<BuildArtifacts> {
        if matches!(
            ctx.target,
            crate::build::platform::BuildUnit::Library { .. }
        ) {
            return Err(BuildError::invalid_config(
                "build unit",
                "explicit static-library delivery is supported only on iOS",
            ));
        }
        let crate::build::platform::Platform::Web { target } = &ctx.platform else {
            return Err(BuildError::InvalidPlatform {
                reason: "Expected Web platform".to_string(),
            });
        };

        crate::ui::debug(format!("Building WASM for target: {target}"));

        let web_dist_dir = self
            .workspace_root
            .join("platforms")
            .join("web")
            .join("dist");

        // Create dist directory
        std::fs::create_dir_all(&web_dist_dir)?;

        // wasm-pack takes the out-dir as a UTF-8 CLI argument; a non-UTF-8
        // workspace root is a caller-supplied environment problem, not a bug.
        let web_dist_str = web_dist_dir.to_str().ok_or_else(|| {
            BuildError::invalid_config(
                "workspace_root",
                format!(
                    "web dist path {} is not valid UTF-8",
                    web_dist_dir.display()
                ),
            )
        })?;

        let mut args = vec![
            "build",
            "--target",
            target.as_str(),
            "--out-dir",
            web_dist_str,
        ];

        if matches!(ctx.profile, crate::build::platform::Profile::Release) {
            args.push("--release");
        } else {
            args.push("--dev");
        }

        process::run_command_in_dir(
            "wasm-pack",
            &args,
            &self.workspace_root.join("crates").join("flui_app"),
        )
        .await?;

        // Find generated WASM files
        let mut rust_libs = Vec::new();
        for entry in std::fs::read_dir(&web_dist_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "wasm") {
                rust_libs.push(path);
            }
        }

        if rust_libs.is_empty() {
            return Err(BuildError::Other("No WASM files generated".to_string()));
        }

        crate::ui::debug(format!("Generated {} WASM files", rust_libs.len()));

        Ok(BuildArtifacts {
            rust_libs,
            executable: None,
            metadata: serde_json::json!({}),
        })
    }

    async fn build_platform(
        &self,
        ctx: &BuilderContext,
        artifacts: &BuildArtifacts,
    ) -> BuildResult<FinalArtifacts> {
        // Copy HTML and other web assets to dist
        let web_dir = self.workspace_root.join("platforms").join("web");
        let dist_dir = web_dir.join("dist");

        // Copy index.html
        let index_html = web_dir.join("index.html");
        if index_html.exists() {
            std::fs::copy(&index_html, dist_dir.join("index.html"))?;
            crate::ui::debug("Copied index.html".to_string());
        }

        // Copy manifest.json if exists
        let manifest = web_dir.join("manifest.json");
        if manifest.exists() {
            std::fs::copy(&manifest, dist_dir.join("manifest.json"))?;
            crate::ui::debug("Copied manifest.json".to_string());
        }

        // Copy icons directory if exists
        let icons_dir = web_dir.join("icons");
        if icons_dir.exists() {
            let dist_icons = dist_dir.join("icons");
            std::fs::create_dir_all(&dist_icons)?;
            for entry in std::fs::read_dir(&icons_dir)? {
                let entry = entry?;
                let dest = dist_icons.join(entry.file_name());
                std::fs::copy(entry.path(), dest)?;
            }
            crate::ui::debug("Copied icons directory".to_string());
        }

        // Copy dist to output directory
        let output_dir = &ctx.output_dir;
        if output_dir.exists() {
            std::fs::remove_dir_all(output_dir)?;
        }
        copy_dir_recursive(&dist_dir, output_dir)?;

        // Calculate total size
        let size_bytes: u64 = artifacts
            .rust_libs
            .iter()
            .filter_map(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .sum();

        crate::ui::debug(format!("Web build copied to: {}", output_dir.display()));

        Ok(FinalArtifacts {
            app_binary: output_dir.join("index.html"),
            size_bytes,
        })
    }
}

/// Recursively copy directory
fn copy_dir_recursive(src: &Path, dst: &Path) -> BuildResult<()> {
    std::fs::create_dir_all(dst)?;

    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }

    Ok(())
}
