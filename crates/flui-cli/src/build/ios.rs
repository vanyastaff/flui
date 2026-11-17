use std::path::Path;

use crate::build::error::{BuildError, BuildResult};
use crate::build::platform::{BuildArtifacts, BuildUnit, BuilderContext, FinalArtifacts};
use crate::build::util::{check_command_exists, environment};

/// Builder for iOS: an application `.app` or a library XCFramework, each
/// packaged with Xcode's command-line tools and no Xcode project.
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
    /// Refuse an iOS build of the project in `workspace_root` that cannot run
    /// here: no `xcodebuild`, or no iOS target the project's toolchain builds.
    pub(crate) fn validate_environment(workspace_root: &Path) -> BuildResult<()> {
        // Check xcodebuild
        check_command_exists("xcodebuild")?;

        // At least one iOS target: a device's or a simulator's.
        let mut any_buildable = false;
        for triple in ["aarch64-apple-ios-sim", "x86_64-apple-ios"] {
            if matches!(
                environment::rust_target_support(workspace_root, triple)?,
                environment::TargetSupport::Installed | environment::TargetSupport::FromSource
            ) {
                any_buildable = true;
                break;
            }
        }
        if !any_buildable {
            // Refuses, naming the toolchain, unless the device target builds.
            environment::require_rust_target(workspace_root, "aarch64-apple-ios")?;
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
                crate::build::util::cargo::build_artifact(
                    &ctx.workspace_root,
                    &args,
                    &selected,
                    &ctx.floors.cargo_env(target),
                )
                .await?,
            );
        }
        if application {
            Ok(BuildArtifacts {
                rust_libs: Vec::new(),
                executable: outputs.pop(),
                package: Some(selected.built_package()),
            })
        } else {
            Ok(BuildArtifacts {
                rust_libs: outputs,
                executable: None,
                package: Some(selected.built_package()),
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

        // A library build delivers the archives it built as an XCFramework,
        // whatever Xcode project the application keeps: that project consumes
        // the framework, it does not build in its place.
        crate::build::ios_package::package(ctx, artifacts).await
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

#[cfg(test)]
mod tests {
    use super::IosBuilder;
    use crate::build::{BuildArtifacts, BuildUnit, BuilderContextBuilder, Platform, Profile};

    /// A library build is delivered by the XCFramework packager even when
    /// the application keeps an Xcode project in `platforms/ios`: the
    /// packager, not `xcodebuild -project`, answers, here by refusing a
    /// build whose targets have no archive each. No tool runs before that
    /// refusal, so the case holds on every host.
    #[test]
    fn a_library_build_beside_an_xcode_project_is_delivered_as_a_framework() {
        let project = tempfile::tempdir().expect("fixture");
        std::fs::create_dir_all(project.path().join("platforms/ios/flui.xcodeproj"))
            .expect("an Xcode project");
        let ctx = BuilderContextBuilder::new(project.path().to_path_buf())
            .platform(Platform::Ios {
                targets: vec!["aarch64-apple-ios".to_owned()],
            })
            .target(BuildUnit::Library { package: None })
            .profile(Profile::Release)
            .output_dir(project.path().join("out"))
            .build();
        let artifacts = BuildArtifacts {
            rust_libs: Vec::new(),
            executable: None,
            package: None,
        };
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let refusal = runtime
            .block_on(IosBuilder::new().build_platform(&ctx, &artifacts))
            .expect_err("no archive to deliver");
        assert!(
            refusal
                .to_string()
                .contains("requested triples must have exactly one archive each"),
            "{refusal}"
        );
    }
}
