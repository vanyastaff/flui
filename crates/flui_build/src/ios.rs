use std::path::{Path, PathBuf};

use crate::error::{BuildError, BuildResult};
use crate::platform::{private, BuildArtifacts, BuilderContext, FinalArtifacts, PlatformBuilder};
use crate::util::{check_command_exists, process};

/// Builder for iOS platform (.app bundles via Xcode)
#[derive(Debug)]
pub struct IOSBuilder {
    workspace_root: PathBuf,
}

impl IOSBuilder {
    /// Creates a new `IOSBuilder`
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

impl private::Sealed for IOSBuilder {}

impl PlatformBuilder for IOSBuilder {
    fn platform_name(&self) -> &'static str {
        "ios"
    }

    fn validate_environment(&self) -> BuildResult<()> {
        // Check xcodebuild
        check_command_exists("xcodebuild")?;

        // Check for iOS targets
        let output = std::process::Command::new("rustup")
            .args(["target", "list", "--installed"])
            .output()?;

        let installed_targets = String::from_utf8_lossy(&output.stdout);

        // Check for at least one iOS target
        if !installed_targets.contains("aarch64-apple-ios")
            && !installed_targets.contains("x86_64-apple-ios")
        {
            return Err(BuildError::TargetNotInstalled {
                target: "aarch64-apple-ios".to_string(),
                install_cmd: "rustup target add aarch64-apple-ios".to_string(),
            });
        }

        tracing::debug!("iOS environment validation passed");

        Ok(())
    }

    fn build_rust(&self, ctx: &BuilderContext) -> BuildResult<BuildArtifacts> {
        let crate::platform::Platform::IOS { targets } = &ctx.platform else {
            return Err(BuildError::InvalidPlatform {
                reason: "Expected iOS platform".to_string(),
            });
        };

        let ios_frameworks_dir = self
            .workspace_root
            .join("platforms")
            .join("ios")
            .join("Frameworks");

        // Clean frameworks directory
        if ios_frameworks_dir.exists() {
            tracing::debug!("Cleaning Frameworks directory: {:?}", ios_frameworks_dir);
            std::fs::remove_dir_all(&ios_frameworks_dir)?;
        }
        std::fs::create_dir_all(&ios_frameworks_dir)?;

        let mut rust_libs = Vec::new();

        for target in targets {
            tracing::info!("Building for iOS target: {}", target);

            let mut args = vec![
                "build",
                "--manifest-path",
                "crates/flui_app/Cargo.toml",
                "--target",
                target.as_str(),
                "--lib",
            ];

            if let Some(profile_flag) = ctx.profile.cargo_flag() {
                args.push(profile_flag);
            }

            pollster::block_on(process::run_command("cargo", &args))?;

            // Find the .a static library
            let lib_path = self
                .workspace_root
                .join("target")
                .join(target)
                .join(ctx.profile.as_str())
                .join("libflui_app.a");

            if !lib_path.exists() {
                return Err(BuildError::PathNotFound {
                    path: lib_path.clone(),
                    context: "Static library not found".to_string(),
                });
            }

            rust_libs.push(lib_path);
        }

        if rust_libs.is_empty() {
            return Err(BuildError::Other(
                "No static libraries generated".to_string(),
            ));
        }

        tracing::info!("Generated {} iOS libraries", rust_libs.len());

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
        tracing::info!("Building iOS app with Xcode...");

        let ios_dir = self.workspace_root.join("platforms").join("ios");

        // Check if Xcode project exists
        let xcodeproj = ios_dir.join("flui.xcodeproj");
        if !xcodeproj.exists() {
            tracing::warn!("Xcode project not found, skipping app build");
            tracing::info!(
                "Native libraries built successfully at: platforms/ios/Frameworks/"
            );

            // Return the .a file as the artifact
            let lib_file = artifacts
                .rust_libs
                .first()
                .ok_or_else(|| BuildError::Other("No native libraries found".to_string()))?;
            let size_bytes = std::fs::metadata(lib_file)?.len();

            return Ok(FinalArtifacts {
                app_binary: lib_file.clone(),
                size_bytes,
            });
        }

        // Determine scheme and configuration
        let configuration = match ctx.profile {
            crate::platform::Profile::Debug => "Debug",
            crate::platform::Profile::Release => "Release",
        };

        let args = vec![
            "-project",
            xcodeproj.to_str().unwrap(),
            "-scheme",
            "flui",
            "-configuration",
            configuration,
            "-sdk",
            "iphoneos",
            "build",
        ];

        pollster::block_on(process::run_command_in_dir(
            "xcodebuild",
            &args,
            &ios_dir,
        ))?;

        // Find the .app bundle
        let build_dir = ios_dir
            .join("build")
            .join(configuration)
            .join("iphoneos");

        let app_path = std::fs::read_dir(&build_dir)?
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.extension()
                    .is_some_and(|ext| ext == "app")
            })
            .ok_or_else(|| BuildError::PathNotFound {
                path: build_dir.clone(),
                context: ".app bundle not found in build output".to_string(),
            })?;

        // Calculate size of .app bundle (recursive)
        let size_bytes = calculate_dir_size(&app_path)?;

        // Copy to output directory
        let output_app = ctx.output_dir.join("flui.app");
        if output_app.exists() {
            std::fs::remove_dir_all(&output_app)?;
        }
        copy_dir_recursive(&app_path, &output_app)?;

        tracing::info!("iOS app copied to: {:?}", output_app);

        Ok(FinalArtifacts {
            app_binary: output_app,
            size_bytes,
        })
    }

    fn clean(&self, ctx: &BuilderContext) -> BuildResult<()> {
        let ios_frameworks_dir = self
            .workspace_root
            .join("platforms")
            .join("ios")
            .join("Frameworks");

        if ios_frameworks_dir.exists() {
            std::fs::remove_dir_all(&ios_frameworks_dir)?;
            tracing::info!("Cleaned Frameworks: {:?}", ios_frameworks_dir);
        }

        // Clean Xcode build
        let ios_dir = self.workspace_root.join("platforms").join("ios");
        let xcodeproj = ios_dir.join("flui.xcodeproj");

        if xcodeproj.exists() {
            pollster::block_on(process::run_command_in_dir(
                "xcodebuild",
                &["clean"],
                &ios_dir,
            ))?;
        }

        // Clean output directory
        if ctx.output_dir.exists() {
            std::fs::remove_dir_all(&ctx.output_dir)?;
            tracing::info!("Cleaned output: {:?}", ctx.output_dir);
        }

        Ok(())
    }
}

/// Calculate total size of a directory recursively
fn calculate_dir_size(dir: &Path) -> BuildResult<u64> {
    let mut total_size = 0u64;

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            total_size += calculate_dir_size(&path)?;
        } else {
            total_size += std::fs::metadata(&path)?.len();
        }
    }

    Ok(total_size)
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
