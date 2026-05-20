use std::path::{Path, PathBuf};

use crate::error::{BuildError, BuildResult};
use crate::platform::{private, BuildArtifacts, BuilderContext, FinalArtifacts, PlatformBuilder};
use crate::util::{check_command_exists, environment, process};

/// Builder for Android platform (APK builds via Gradle and cargo-ndk)
#[derive(Debug)]
pub struct AndroidBuilder {
    workspace_root: PathBuf,
    android_home: PathBuf,
    ndk_home: PathBuf,
    _java_home: Option<PathBuf>,
}

impl AndroidBuilder {
    /// Creates a new `AndroidBuilder`
    ///
    /// # Errors
    ///
    /// Returns error if `ANDROID_HOME` or NDK is not configured
    pub fn new(workspace_root: &Path) -> BuildResult<Self> {
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

/// Scene plugin build/deploy methods for hot-reload workflow.
///
/// These methods are separate from the `PlatformBuilder` trait because they
/// operate on a scene plugin crate (cdylib), not the host application.
impl AndroidBuilder {
    /// Build a scene plugin crate as a cdylib `.so` for the given Android target.
    ///
    /// Returns the path to the compiled `.so` file in the target directory.
    ///
    /// # Arguments
    ///
    /// * `target` - Android target triple (e.g., "arm64-v8a")
    /// * `scene_crate` - Package name of the scene crate (e.g., "flui-android-scene")
    /// * `release` - Whether to build in release mode
    pub async fn build_scene_plugin(
        &self,
        target: &str,
        scene_crate: &str,
        release: bool,
    ) -> BuildResult<PathBuf> {
        tracing::info!("Building scene plugin '{}' for {}", scene_crate, target);

        let mut args = vec![
            "ndk".to_string(),
            "-t".to_string(),
            target.to_string(),
            "build".to_string(),
            "-p".to_string(),
            scene_crate.to_string(),
        ];

        if release {
            args.push("--release".to_string());
        }

        let args_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        process::run_command("cargo", &args_refs).await?;

        // Map cargo-ndk target name to Rust target triple
        let rust_target = match target {
            "arm64-v8a" => "aarch64-linux-android",
            "armeabi-v7a" => "armv7-linux-androideabi",
            "x86_64" => "x86_64-linux-android",
            "x86" => "i686-linux-android",
            other => other,
        };

        let profile_dir = if release { "release" } else { "debug" };

        // Find the .so file — scene crates produce lib{name}.so
        let target_dir = self
            .workspace_root
            .join("target")
            .join(rust_target)
            .join(profile_dir);

        // Look for any .so file matching the scene crate's lib name
        let so_path = std::fs::read_dir(&target_dir)
            .map_err(|_| BuildError::path_not_found(target_dir.clone(), "target output dir"))?
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.path())
            .find(|path| {
                path.extension().is_some_and(|ext| ext == "so")
                    && path.file_name().is_some_and(|name| {
                        let name = name.to_string_lossy();
                        name.starts_with("lib") && name.contains("scene")
                    })
            })
            .ok_or_else(|| {
                BuildError::path_not_found(target_dir, "scene plugin .so not found in target dir")
            })?;

        tracing::info!("Scene plugin built: {:?}", so_path);
        Ok(so_path)
    }

    /// Push a compiled scene plugin `.so` to a connected Android device.
    ///
    /// Uses `adb push` to `/data/local/tmp/` then `adb shell run-as` to copy
    /// into the app's internal data directory (required by SELinux).
    ///
    /// # Arguments
    ///
    /// * `so_path` - Local path to the `.so` file
    /// * `package` - Android package name (e.g., "com.vanya.flui.counter")
    /// * `lib_name` - Library filename on device (e.g., "libflui_scene.so")
    pub async fn push_scene_plugin(
        &self,
        so_path: &Path,
        package: &str,
        lib_name: &str,
    ) -> BuildResult<()> {
        let so_str = so_path
            .to_str()
            .ok_or_else(|| BuildError::Other(format!("Invalid path: {:?}", so_path)))?;

        let tmp_path = format!("/data/local/tmp/{lib_name}");
        let app_path = format!("/data/data/{package}/files/{lib_name}");

        // Push to /data/local/tmp/
        process::run_command("adb", &["push", so_str, &tmp_path]).await?;

        // Copy into app's data directory (SELinux requires app_data_file context)
        let cp_cmd = format!("cp {tmp_path} {app_path}");
        process::run_command("adb", &["shell", "run-as", package, "sh", "-c", &cp_cmd]).await?;

        tracing::info!("Scene plugin pushed to device: {}", app_path);
        Ok(())
    }
}

impl private::Sealed for AndroidBuilder {}

impl PlatformBuilder for AndroidBuilder {
    fn platform_name(&self) -> &'static str {
        "android"
    }

    fn validate_environment(&self) -> BuildResult<()> {
        // Check cargo-ndk
        check_command_exists("cargo")?;

        // Try to find cargo-ndk
        let cargo_ndk_ok = std::process::Command::new("cargo")
            .args(["ndk", "--version"])
            .output()
            .is_ok_and(|output| output.status.success());

        if !cargo_ndk_ok {
            return Err(BuildError::ToolNotFound {
                tool: "cargo-ndk".to_string(),
                install_hint: "cargo install cargo-ndk".to_string(),
            });
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
            return Err(BuildError::TargetNotInstalled {
                target: "aarch64-linux-android".to_string(),
                install_cmd: "rustup target add aarch64-linux-android".to_string(),
            });
        }

        tracing::debug!("Android environment validation passed");
        tracing::debug!("  ANDROID_HOME: {:?}", self.android_home);
        tracing::debug!("  NDK: {:?}", self.ndk_home);

        Ok(())
    }

    async fn build_rust(&self, ctx: &BuilderContext) -> BuildResult<BuildArtifacts> {
        let crate::platform::Platform::Android { targets } = &ctx.platform else {
            return Err(BuildError::InvalidPlatform {
                reason: "Expected Android platform".to_string(),
            });
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

            process::run_command("cargo", &args).await?;

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
            return Err(BuildError::Other("No .so files generated".to_string()));
        }

        tracing::info!("Generated {} native libraries", rust_libs.len());

        Ok(BuildArtifacts {
            rust_libs,
            metadata: serde_json::json!({}),
        })
    }

    async fn build_platform(
        &self,
        ctx: &BuilderContext,
        artifacts: &BuildArtifacts,
    ) -> BuildResult<FinalArtifacts> {
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
                .ok_or_else(|| BuildError::Other("No native libraries found".to_string()))?;
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
        process::run_command_in_dir(
            gradle_wrapper_path.to_str().unwrap(),
            &[gradle_task],
            &android_dir,
        )
        .await?;

        // Find the APK
        let apk_dir = android_dir
            .join("app")
            .join("build")
            .join("outputs")
            .join("apk")
            .join(ctx.profile.as_str());

        let apk_path = std::fs::read_dir(&apk_dir)?
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.path())
            .find(|path| path.extension().is_some_and(|ext| ext == "apk"))
            .ok_or_else(|| BuildError::PathNotFound {
                path: apk_dir.clone(),
                context: "APK file not found in build output".to_string(),
            })?;

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

    async fn clean(&self, ctx: &BuilderContext) -> BuildResult<()> {
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

        process::run_command_in_dir(gradle_wrapper, &["clean"], &android_dir).await?;

        // Clean output directory
        if ctx.output_dir.exists() {
            std::fs::remove_dir_all(&ctx.output_dir)?;
            tracing::info!("Cleaned output: {:?}", ctx.output_dir);
        }

        Ok(())
    }
}
