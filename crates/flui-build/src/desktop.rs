use std::path::{Path, PathBuf};

use crate::error::{BuildError, BuildResult};
use crate::platform::{private, BuildArtifacts, BuilderContext, FinalArtifacts, PlatformBuilder};
use crate::util::process;

/// Builder for desktop platforms (Windows, macOS, Linux)
#[derive(Debug)]
pub struct DesktopBuilder {
    workspace_root: PathBuf,
}

impl DesktopBuilder {
    /// Creates a new `DesktopBuilder`
    ///
    /// # Errors
    ///
    /// Currently infallible, but returns Result for consistency
    pub fn new(workspace_root: &Path) -> BuildResult<Self> {
        Ok(Self {
            workspace_root: workspace_root.to_path_buf(),
        })
    }

    fn detect_host_target() -> BuildResult<String> {
        // Get host triple from rustc
        let output = std::process::Command::new("rustc")
            .args(["-vV"])
            .output()
            .map_err(|e| BuildError::CommandFailed {
                command: "rustc -vV".to_string(),
                exit_code: -1,
                stderr: e.to_string(),
            })?;

        let output_str = String::from_utf8_lossy(&output.stdout);

        let host = output_str
            .lines()
            .find(|line| line.starts_with("host:"))
            .and_then(|line| line.split(':').nth(1))
            .map(|s| s.trim().to_string());

        if let Some(target) = host {
            return Ok(target);
        }

        // Fallback to common targets
        let fallback = if cfg!(target_os = "windows") {
            "x86_64-pc-windows-msvc"
        } else if cfg!(target_os = "macos") {
            if cfg!(target_arch = "aarch64") {
                "aarch64-apple-darwin"
            } else {
                "x86_64-apple-darwin"
            }
        } else {
            "x86_64-unknown-linux-gnu"
        };

        tracing::warn!("Could not detect host target from rustc, falling back to {fallback}");
        Ok(fallback.to_string())
    }
}

impl private::Sealed for DesktopBuilder {}

impl PlatformBuilder for DesktopBuilder {
    fn platform_name(&self) -> &'static str {
        "desktop"
    }

    fn validate_environment(&self) -> BuildResult<()> {
        // Just need cargo
        crate::util::check_command_exists("cargo")?;
        Ok(())
    }

    async fn build_rust(&self, ctx: &BuilderContext) -> BuildResult<BuildArtifacts> {
        let target = match &ctx.platform {
            crate::platform::Platform::Desktop { target } => match target {
                Some(t) => t.clone(),
                None => Self::detect_host_target()?,
            },
            _ => {
                return Err(BuildError::InvalidPlatform {
                    reason: "Expected Desktop platform".to_string(),
                })
            }
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

        process::run_command("cargo", &args).await?;

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
            return Err(BuildError::PathNotFound {
                path: lib_path.clone(),
                context: "Compiled library not found".to_string(),
            });
        }

        Ok(BuildArtifacts {
            rust_libs: vec![lib_path],
            metadata: serde_json::json!({
                "target": target,
            }),
        })
    }

    async fn build_platform(
        &self,
        ctx: &BuilderContext,
        artifacts: &BuildArtifacts,
    ) -> BuildResult<FinalArtifacts> {
        let lib_src = artifacts
            .rust_libs
            .first()
            .ok_or_else(|| BuildError::Other("No library found".to_string()))?;

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

    async fn clean(&self, ctx: &BuilderContext) -> BuildResult<()> {
        if ctx.output_dir.exists() {
            std::fs::remove_dir_all(&ctx.output_dir)?;
            tracing::info!("Cleaned output: {:?}", ctx.output_dir);
        }

        // Note: We don't clean cargo target/ directory as it's shared
        tracing::info!("To clean Cargo build artifacts, run: cargo clean");

        Ok(())
    }
}
