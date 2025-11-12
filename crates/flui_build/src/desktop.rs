use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};

use crate::platform::{BuildArtifacts, BuilderContext, FinalArtifacts, PlatformBuilder};
use crate::util::process;

pub struct DesktopBuilder {
    workspace_root: PathBuf,
}

impl DesktopBuilder {
    pub fn new(workspace_root: &Path) -> Result<Self> {
        Ok(Self {
            workspace_root: workspace_root.to_path_buf(),
        })
    }

    fn detect_host_target() -> String {
        // Get host triple
        let output = std::process::Command::new("rustc")
            .args(["-vV"])
            .output()
            .expect("Failed to run rustc");

        let output_str = String::from_utf8_lossy(&output.stdout);

        for line in output_str.lines() {
            if line.starts_with("host:") {
                return line.split(':').nth(1).unwrap().trim().to_string();
            }
        }

        // Fallback to common targets
        if cfg!(target_os = "windows") {
            "x86_64-pc-windows-msvc".to_string()
        } else if cfg!(target_os = "macos") {
            if cfg!(target_arch = "aarch64") {
                "aarch64-apple-darwin".to_string()
            } else {
                "x86_64-apple-darwin".to_string()
            }
        } else {
            "x86_64-unknown-linux-gnu".to_string()
        }
    }
}

impl PlatformBuilder for DesktopBuilder {
    fn platform_name(&self) -> &str {
        "desktop"
    }

    fn validate_environment(&self) -> Result<()> {
        // Just need cargo
        crate::util::check_command_exists("cargo")?;
        Ok(())
    }

    fn build_rust(&self, ctx: &BuilderContext) -> Result<BuildArtifacts> {
        let target = match &ctx.platform {
            crate::platform::Platform::Desktop { target } => {
                target.clone().unwrap_or_else(Self::detect_host_target)
            }
            _ => return Err(anyhow!("Invalid platform for Desktop builder")),
        };

        tracing::info!("Building for desktop target: {}", target);

        let mut args = vec![
            "build",
            "--manifest-path",
            "crates/flui_app/Cargo.toml",
            "--target",
            &target,
        ];

        if let Some(profile_flag) = ctx.profile.cargo_flag() {
            args.push(profile_flag);
        }

        pollster::block_on(process::run_command("cargo", &args))?;

        // Find the library (flui_app builds as a library, not executable)
        let lib_name = if cfg!(target_os = "windows") {
            "flui_app.dll"
        } else if cfg!(target_os = "macos") {
            "libflui_app.dylib"
        } else {
            "libflui_app.so"
        };

        let lib_path = self
            .workspace_root
            .join("target")
            .join(&target)
            .join(ctx.profile.as_str())
            .join(lib_name);

        if !lib_path.exists() {
            return Err(anyhow!("Library not found at: {:?}", lib_path));
        }

        Ok(BuildArtifacts {
            rust_libs: vec![lib_path],
            metadata: serde_json::json!({
                "target": target,
            }),
        })
    }

    fn build_platform(&self, ctx: &BuilderContext, artifacts: &BuildArtifacts) -> Result<FinalArtifacts> {
        let lib_src = artifacts
            .rust_libs
            .first()
            .ok_or_else(|| anyhow!("No library found"))?;

        // Copy to output directory
        let lib_name = lib_src.file_name().unwrap();
        let output_lib = ctx.output_dir.join(lib_name);

        std::fs::create_dir_all(&ctx.output_dir)?;
        std::fs::copy(lib_src, &output_lib)?;

        let size_bytes = std::fs::metadata(&output_lib)?.len();

        tracing::info!("Desktop library copied to: {:?}", output_lib);

        Ok(FinalArtifacts {
            app_binary: output_lib,
            size_bytes,
        })
    }

    fn clean(&self, ctx: &BuilderContext) -> Result<()> {
        if ctx.output_dir.exists() {
            std::fs::remove_dir_all(&ctx.output_dir)?;
            tracing::info!("Cleaned output: {:?}", ctx.output_dir);
        }

        // Note: We don't clean cargo target/ directory as it's shared
        tracing::info!("To clean Cargo build artifacts, run: cargo clean");

        Ok(())
    }
}
