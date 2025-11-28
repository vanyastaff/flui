use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};

use crate::platform::{private, BuildArtifacts, BuilderContext, FinalArtifacts, PlatformBuilder};
use crate::util::{check_command_exists, environment, process};

pub struct AndroidBuilder {
    workspace_root: PathBuf,
    android_home: PathBuf,
    ndk_home: PathBuf,
    _java_home: Option<PathBuf>,
}

impl AndroidBuilder {
    pub fn new(workspace_root: &Path) -> Result<Self> {
        let android_home = environment::resolve_android_home()?;
        let ndk_home = environment::resolve_ndk_home(&android_home)?;

        // Java is optional - only needed for Gradle APK build
        let java_home = environment::resolve_java_home().ok();

        if java_home.is_none() {
            tracing::warn!("JAVA_HOME not set - APK build will be skipped");
        }

        Ok(Self {
            workspace_root: workspace_root.to_path_buf(),
            android_home,
            ndk_home,
            _java_home: java_home,
        })
    }
}

impl private::Sealed for AndroidBuilder {}

impl PlatformBuilder for AndroidBuilder {
    fn platform_name(&self) -> &str {
        "android"
    }

    fn validate_environment(&self) -> Result<()> {
        // Check cargo-ndk
        check_command_exists("cargo").context("cargo not found - Rust toolchain required")?;

        // Try to find cargo-ndk
        let cargo_ndk_result = std::process::Command::new("cargo")
            .args(["ndk", "--version"])
            .output();

        if cargo_ndk_result.is_err() || !cargo_ndk_result.as_ref().unwrap().status.success() {
            return Err(anyhow!(
                "cargo-ndk not installed. Install with: cargo install cargo-ndk"
            ));
        }

        // Check Gradle (optional - warn if not found)
        let gradle_wrapper = self.workspace_root.join("platforms").join("android").join(
            if cfg!(target_os = "windows") {
                "gradlew.bat"
            } else {
                "gradlew"
            },
        );

        if !gradle_wrapper.exists() {
            tracing::warn!("Gradle wrapper not found - will build native libraries only");
            tracing::warn!("To build APK, ensure Gradle is set up in platforms/android/");
        }

        // Check Android targets are installed
        let output = std::process::Command::new("rustup")
            .args(["target", "list", "--installed"])
            .output()?;

        let installed_targets = String::from_utf8_lossy(&output.stdout);

        // Check for at least one Android target
        if !installed_targets.contains("android") {
            return Err(anyhow!(
                "No Android Rust targets installed. Install with: rustup target add aarch64-linux-android"
            ));
        }

        tracing::debug!("Android environment validation passed");
        tracing::debug!("  ANDROID_HOME: {:?}", self.android_home);
        tracing::debug!("  NDK: {:?}", self.ndk_home);

        Ok(())
    }

    fn build_rust(&self, ctx: &BuilderContext) -> Result<BuildArtifacts> {
        let targets = match &ctx.platform {
            crate::platform::Platform::Android { targets } => targets,
            _ => return Err(anyhow!("Invalid platform for Android builder")),
        };

        let jni_libs_dir = self
            .workspace_root
            .join("platforms")
            .join("android")
            .join("app")
            .join("src")
            .join("main")
            .join("jniLibs");

        // Clean jniLibs directory
        if jni_libs_dir.exists() {
            tracing::debug!("Cleaning jniLibs directory: {:?}", jni_libs_dir);
            std::fs::remove_dir_all(&jni_libs_dir)?;
        }
        std::fs::create_dir_all(&jni_libs_dir)?;

        let mut rust_libs = Vec::new();

        for target in targets {
            tracing::info!("Building for Android target: {}", target);

            let mut args = vec![
                "ndk",
                "-t",
                target.as_str(),
                "-o",
                jni_libs_dir.to_str().unwrap(),
                "--manifest-path",
                "crates/flui_app/Cargo.toml",
                "build",
                "--lib",
            ];

            if let Some(profile_flag) = ctx.profile.cargo_flag() {
                args.push(profile_flag);
            }

            pollster::block_on(process::run_command("cargo", &args))?;

            // Find the .so file
            let abi_dir = jni_libs_dir.join(target);
            if abi_dir.exists() {
                for entry in std::fs::read_dir(&abi_dir)? {
                    let entry = entry?;
                    let path = entry.path();
                    if path.extension().is_some_and(|ext| ext == "so") {
                        rust_libs.push(path);
                    }
                }
            }
        }

        if rust_libs.is_empty() {
            return Err(anyhow!("No .so files generated"));
        }

        tracing::info!("Generated {} native libraries", rust_libs.len());

        Ok(BuildArtifacts {
            rust_libs,
            metadata: serde_json::json!({}),
        })
    }

    fn build_platform(
        &self,
        ctx: &BuilderContext,
        artifacts: &BuildArtifacts,
    ) -> Result<FinalArtifacts> {
        tracing::info!("Building APK with Gradle...");

        let android_dir = self.workspace_root.join("platforms").join("android");

        let gradle_wrapper_name = if cfg!(target_os = "windows") {
            "gradlew.bat"
        } else {
            "gradlew"
        };

        // Check if gradle wrapper exists
        let gradle_wrapper_path = android_dir.join(gradle_wrapper_name);
        if !gradle_wrapper_path.exists() {
            tracing::warn!("Gradle wrapper not found, skipping APK build");
            tracing::info!(
                "Native libraries built successfully at: platforms/android/app/src/main/jniLibs/"
            );

            // Return the .so file as the artifact
            let so_file = artifacts
                .rust_libs
                .first()
                .ok_or_else(|| anyhow!("No native libraries found"))?;
            let size_bytes = std::fs::metadata(so_file)?.len();

            return Ok(FinalArtifacts {
                app_binary: so_file.clone(),
                size_bytes,
            });
        }

        let gradle_task = match ctx.profile {
            crate::platform::Profile::Debug => "assembleDebug",
            crate::platform::Profile::Release => "assembleRelease",
        };

        // Use absolute path for gradle wrapper
        pollster::block_on(process::run_command_in_dir(
            gradle_wrapper_path.to_str().unwrap(),
            &[gradle_task],
            &android_dir,
        ))?;

        // Find the APK
        let apk_dir = android_dir
            .join("app")
            .join("build")
            .join("outputs")
            .join("apk")
            .join(ctx.profile.as_str());

        let apk_path = std::fs::read_dir(&apk_dir)?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .find(|path| path.extension().is_some_and(|ext| ext == "apk"))
            .ok_or_else(|| anyhow!("APK not found in {:?}", apk_dir))?;

        let size_bytes = std::fs::metadata(&apk_path)?.len();

        // Copy to output directory
        let output_apk = ctx
            .output_dir
            .join(format!("flui-{}.apk", ctx.profile.as_str()));
        std::fs::copy(&apk_path, &output_apk)?;

        tracing::info!("APK copied to: {:?}", output_apk);

        Ok(FinalArtifacts {
            app_binary: output_apk,
            size_bytes,
        })
    }

    fn clean(&self, ctx: &BuilderContext) -> Result<()> {
        let jni_libs_dir = self
            .workspace_root
            .join("platforms")
            .join("android")
            .join("app")
            .join("src")
            .join("main")
            .join("jniLibs");

        if jni_libs_dir.exists() {
            std::fs::remove_dir_all(&jni_libs_dir)?;
            tracing::info!("Cleaned jniLibs: {:?}", jni_libs_dir);
        }

        // Clean Gradle build
        let android_dir = self.workspace_root.join("platforms").join("android");
        let gradle_wrapper = if cfg!(target_os = "windows") {
            "gradlew.bat"
        } else {
            "./gradlew"
        };

        pollster::block_on(process::run_command_in_dir(
            gradle_wrapper,
            &["clean"],
            &android_dir,
        ))?;

        // Clean output directory
        if ctx.output_dir.exists() {
            std::fs::remove_dir_all(&ctx.output_dir)?;
            tracing::info!("Cleaned output: {:?}", ctx.output_dir);
        }

        Ok(())
    }
}
