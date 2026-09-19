use std::path::{Path, PathBuf};

use crate::error::{BuildError, BuildResult};
use crate::platform::{
    BuildArtifacts, BuildUnit, BuilderContext, FinalArtifacts, PlatformBuilder, private,
};
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

    /// The static library name cargo emits for `target`.
    ///
    /// A named package/example contributes its own name; the generated-project
    /// default reads the manifest at the workspace root.
    fn static_lib_name(&self, target: &BuildUnit) -> BuildResult<String> {
        let crate_name = match target {
            BuildUnit::Package(name) => name.clone(),
            // An example builds as part of its package, so the library is the
            // package's; there is no per-example lib name.
            BuildUnit::Example(_) | BuildUnit::DefaultBinary => {
                Self::package_name_at(&self.workspace_root)?
            }
        };
        Ok(format!("lib{}.a", crate_name.replace('-', "_")))
    }

    fn package_name_at(dir: &Path) -> BuildResult<String> {
        let manifest = dir.join("Cargo.toml");
        let text = std::fs::read_to_string(&manifest).map_err(|e| {
            BuildError::path_not_found(dir.to_path_buf(), format!("reading Cargo.toml: {e}"))
        })?;
        let value = text.parse::<toml::Value>().map_err(|e| {
            BuildError::invalid_config("Cargo.toml", format!("could not parse manifest: {e}"))
        })?;
        value
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(toml::Value::as_str)
            .map(str::to_string)
            .or_else(|| dir.file_name().and_then(|n| n.to_str()).map(str::to_string))
            .ok_or_else(|| {
                BuildError::invalid_config(
                    "workspace_root",
                    format!("{} has no usable package name", dir.display()),
                )
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

    async fn build_rust(&self, ctx: &BuilderContext) -> BuildResult<BuildArtifacts> {
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
        let lib_name = self.static_lib_name(&ctx.target)?;

        for target in targets {
            tracing::info!("Building for iOS target: {}", target);

            let mut args = vec!["build".to_string(), "--target".to_string(), target.clone()];
            args.extend(ctx.target.cargo_args());
            // `--lib` and `--example` are mutually exclusive in cargo; a library
            // is what an iOS static-link build needs, so it is only added when
            // the target is not an explicit example.
            if !matches!(ctx.target, BuildUnit::Example(_)) {
                args.push("--lib".to_string());
            }
            if let Some(profile_flag) = ctx.profile.cargo_flag() {
                args.push(profile_flag.to_string());
            }
            if !ctx.features.is_empty() {
                args.push("--features".to_string());
                args.push(ctx.features.join(","));
            }

            process::run_command_in_dir("cargo", &args, &self.workspace_root).await?;

            // Find the .a static library
            let lib_path = self
                .workspace_root
                .join("target")
                .join(target)
                .join(ctx.profile.as_str())
                .join(&lib_name);

            if !lib_path.exists() {
                return Err(BuildError::PathNotFound {
                    path: lib_path.clone(),
                    context: format!("Static library '{lib_name}' not found"),
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
            executable: None,
            metadata: serde_json::json!({}),
        })
    }

    async fn build_platform(
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
            tracing::info!("Native libraries built successfully at: platforms/ios/Frameworks/");

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

        // xcodebuild takes the project path as a UTF-8 CLI argument; a
        // non-UTF-8 workspace root is a caller-supplied environment problem.
        let xcodeproj_str = xcodeproj.to_str().ok_or_else(|| {
            BuildError::invalid_config(
                "workspace_root",
                format!(
                    "Xcode project path {} is not valid UTF-8",
                    xcodeproj.display()
                ),
            )
        })?;

        let args = vec![
            "-project",
            xcodeproj_str,
            "-scheme",
            "flui",
            "-configuration",
            configuration,
            "-sdk",
            "iphoneos",
            "build",
        ];

        process::run_command_in_dir("xcodebuild", &args, &ios_dir).await?;

        // Find the .app bundle
        let build_dir = ios_dir.join("build").join(configuration).join("iphoneos");

        let app_path = std::fs::read_dir(&build_dir)?
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.path())
            .find(|path| path.extension().is_some_and(|ext| ext == "app"))
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

    async fn clean(&self, ctx: &BuilderContext) -> BuildResult<()> {
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
            process::run_command_in_dir("xcodebuild", &["clean"], &ios_dir).await?;
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
