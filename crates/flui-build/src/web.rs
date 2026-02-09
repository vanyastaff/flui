use std::path::{Path, PathBuf};

use crate::error::{BuildError, BuildResult};
use crate::platform::{private, BuildArtifacts, BuilderContext, FinalArtifacts, PlatformBuilder};
use crate::util::{check_command_exists, process};

/// Builder for Web/WASM platform (via wasm-pack)
#[derive(Debug)]
pub struct WebBuilder {
    workspace_root: PathBuf,
}

impl WebBuilder {
    /// Creates a new `WebBuilder`
    ///
    /// # Errors
    ///
    /// Currently infallible, but returns Result for consistency
    pub fn new(workspace_root: &Path) -> BuildResult<Self> {
        Ok(Self {
            workspace_root: workspace_root.to_path_buf(),
        })
    }
}

impl private::Sealed for WebBuilder {}

impl PlatformBuilder for WebBuilder {
    fn platform_name(&self) -> &'static str {
        "web"
    }

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

    fn build_rust(&self, ctx: &BuilderContext) -> BuildResult<BuildArtifacts> {
        let crate::platform::Platform::Web { target } = &ctx.platform else {
            return Err(BuildError::InvalidPlatform {
                reason: "Expected Web platform".to_string(),
            });
        };

        tracing::info!("Building WASM for target: {}", target);

        let web_dist_dir = self
            .workspace_root
            .join("platforms")
            .join("web")
            .join("dist");

        // Create dist directory
        std::fs::create_dir_all(&web_dist_dir)?;

        let mut args = vec![
            "build",
            "--target",
            target.as_str(),
            "--out-dir",
            web_dist_dir.to_str().unwrap(),
        ];

        if matches!(ctx.profile, crate::platform::Profile::Release) {
            args.push("--release");
        } else {
            args.push("--dev");
        }

        pollster::block_on(process::run_command_in_dir(
            "wasm-pack",
            &args,
            &self.workspace_root.join("crates").join("flui_app"),
        ))?;

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

        tracing::info!("Generated {} WASM files", rust_libs.len());

        Ok(BuildArtifacts {
            rust_libs,
            metadata: serde_json::json!({}),
        })
    }

    fn build_platform(
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
            tracing::debug!("Copied index.html");
        }

        // Copy manifest.json if exists
        let manifest = web_dir.join("manifest.json");
        if manifest.exists() {
            std::fs::copy(&manifest, dist_dir.join("manifest.json"))?;
            tracing::debug!("Copied manifest.json");
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
            tracing::debug!("Copied icons directory");
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

        tracing::info!("Web build copied to: {:?}", output_dir);

        Ok(FinalArtifacts {
            app_binary: output_dir.join("index.html"),
            size_bytes,
        })
    }

    fn clean(&self, ctx: &BuilderContext) -> BuildResult<()> {
        let dist_dir = self
            .workspace_root
            .join("platforms")
            .join("web")
            .join("dist");

        if dist_dir.exists() {
            std::fs::remove_dir_all(&dist_dir)?;
            tracing::info!("Cleaned dist: {:?}", dist_dir);
        }

        if ctx.output_dir.exists() {
            std::fs::remove_dir_all(&ctx.output_dir)?;
            tracing::info!("Cleaned output: {:?}", ctx.output_dir);
        }

        Ok(())
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
