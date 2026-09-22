use std::path::Path;
use tokio::process::Command;

use crate::build::error::{BuildError, BuildResult};
use crate::build::platform::{BuildArtifacts, BuildUnit, BuilderContext, FinalArtifacts};
use crate::build::util::{check_command_exists, process};

/// Builder for iOS platform (.app bundles via Xcode).
#[derive(Debug, Default)]
pub(crate) struct IosBuilder;

impl IosBuilder {
    /// Create a stateless builder; operations use their `BuilderContext`.
    #[must_use]
    pub(crate) const fn new() -> Self {
        Self
    }
}

impl IosBuilder {
    pub(crate) fn validate_environment() -> BuildResult<()> {
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

        crate::ui::debug("iOS environment validation passed".to_string());

        Ok(())
    }

    pub(crate) async fn build_rust(&self, ctx: &BuilderContext) -> BuildResult<BuildArtifacts> {
        let crate::build::platform::Platform::Ios { targets } = &ctx.platform else {
            return Err(BuildError::InvalidPlatform {
                reason: "Expected iOS platform".to_string(),
            });
        };

        if targets.is_empty() {
            return Err(BuildError::invalid_config(
                "iOS targets",
                "at least one target triple is required",
            ));
        }
        let application = !matches!(ctx.target, BuildUnit::Library { .. });
        if application && targets.len() != 1 {
            return Err(BuildError::invalid_config(
                "iOS targets",
                "iOS applications require exactly one target; use explicit library delivery for multiple slices",
            ));
        }
        let selected = if application {
            crate::build::util::cargo::select_target(&ctx.workspace_root, &ctx.target).await?
        } else {
            crate::build::util::cargo::select_static_library(&ctx.workspace_root, &ctx.target)
                .await?
        };
        let mut outputs = Vec::new();
        for target in targets {
            let mut args = cargo_args_for(ctx, target);
            args.extend(selected.cargo_args());
            outputs.push(
                crate::build::util::cargo::build_artifact(&ctx.workspace_root, &args, &selected)
                    .await?,
            );
        }
        if application {
            Ok(BuildArtifacts {
                rust_libs: Vec::new(),
                executable: outputs.pop(),
                metadata: selected.metadata(),
            })
        } else {
            Ok(BuildArtifacts {
                rust_libs: outputs,
                executable: None,
                metadata: selected.metadata(),
            })
        }
    }

    pub(crate) async fn build_platform(
        &self,
        ctx: &BuilderContext,
        artifacts: &BuildArtifacts,
    ) -> BuildResult<FinalArtifacts> {
        if let Some(executable) = &artifacts.executable {
            if matches!(ctx.target, BuildUnit::Library { .. }) || !artifacts.rust_libs.is_empty() {
                return Err(BuildError::invalid_config(
                    "iOS artifacts",
                    "executable delivery conflicts with library selection or archives",
                ));
            }
            return crate::build::ios_package::package_application(ctx, artifacts, executable)
                .await;
        }
        if !matches!(ctx.target, BuildUnit::Library { .. }) {
            return Err(BuildError::invalid_config(
                "iOS application",
                "missing executable artifact",
            ));
        }

        crate::ui::debug("Building iOS app with Xcode...".to_string());

        let ios_dir = ctx.workspace_root.join("platforms").join("ios");

        // Check if Xcode project exists
        let xcodeproj = ios_dir.join("flui.xcodeproj");
        if !xcodeproj.exists() {
            return crate::build::ios_package::package(ctx, artifacts).await;
        }

        // Determine scheme and configuration
        let configuration = match ctx.profile {
            crate::build::platform::Profile::Debug => "Debug",
            crate::build::platform::Profile::Release => "Release",
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

        process::run(Command::new("xcodebuild").args(&args).current_dir(&ios_dir)).await?;

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

        crate::ui::debug(format!("iOS app copied to: {}", output_app.display()));

        Ok(FinalArtifacts {
            app_binary: output_app,
            size_bytes,
        })
    }
}

/// The cargo invocation that builds `ctx.target` for one iOS `target` triple.
///
/// `--lib` is added only for the library path: `--lib` and `--example` are
/// mutually exclusive in cargo, and an example must be built with
/// `--example NAME` alone.
fn cargo_args_for(ctx: &BuilderContext, target: &str) -> Vec<String> {
    let mut args = vec![
        "build".to_string(),
        "--target".to_string(),
        target.to_string(),
    ];
    if let Some(profile_flag) = ctx.profile.cargo_flag() {
        args.push(profile_flag.to_string());
    }
    args
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
