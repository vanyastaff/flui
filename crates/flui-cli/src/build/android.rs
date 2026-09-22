use std::path::{Path, PathBuf};
use tokio::process::Command;

use crate::build::error::{BuildError, BuildResult};
use crate::build::platform::{BuildArtifacts, BuilderContext, FinalArtifacts};
use crate::build::util::{check_command_exists, environment, process};

/// Builder for Android platform (APK builds via Gradle and cargo-ndk)
#[derive(Debug)]
pub(crate) struct AndroidBuilder {
    workspace_root: PathBuf,
    android_home: PathBuf,
    ndk_home: PathBuf,
    /// `JAVA_HOME` when set. Gradle needs it; the build-tools packager only
    /// needs a `java` for `apksigner`, from here or from `PATH`.
    java_home: Option<PathBuf>,
}

impl AndroidBuilder {
    /// Creates a new `AndroidBuilder`
    ///
    /// # Errors
    ///
    /// Returns error if `ANDROID_HOME` or NDK is not configured
    pub(crate) fn new(workspace_root: &Path) -> BuildResult<Self> {
        let android_home = environment::resolve_android_home()?;
        let ndk_home = environment::resolve_ndk_home(&android_home)?;

        let java_home = environment::resolve_java_home().ok();

        Ok(Self {
            workspace_root: workspace_root.to_path_buf(),
            android_home,
            ndk_home,
            java_home,
        })
    }
}

/// Scene plugin build/deploy methods for hot-reload workflow.
///
/// These methods are separate from the build entry points because they
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
    pub(crate) async fn build_scene_plugin(
        &self,
        target: &str,
        scene_crate: &str,
        release: bool,
    ) -> BuildResult<PathBuf> {
        crate::ui::debug(format!(
            "Building scene plugin '{scene_crate}' for {target}"
        ));

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

        process::run(Command::new("cargo").args(&args)).await?;

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

        crate::ui::debug(format!("Scene plugin built: {}", so_path.display()));
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
    pub(crate) async fn push_scene_plugin(
        &self,
        so_path: &Path,
        package: &str,
        lib_name: &str,
    ) -> BuildResult<()> {
        let so_str = so_path
            .to_str()
            .ok_or_else(|| BuildError::Other(format!("invalid path: {}", so_path.display())))?;

        let tmp_path = format!("/data/local/tmp/{lib_name}");
        let app_path = format!("/data/data/{package}/files/{lib_name}");

        // Push to /data/local/tmp/
        process::run(Command::new("adb").args(["push", so_str, &tmp_path])).await?;

        // Copy into app's data directory (SELinux requires app_data_file context)
        let cp_cmd = format!("cp {tmp_path} {app_path}");
        process::run(Command::new("adb").args(["shell", "run-as", package, "sh", "-c", &cp_cmd]))
            .await?;

        crate::ui::debug(format!("Scene plugin pushed to device: {app_path}"));
        Ok(())
    }
}

impl AndroidBuilder {
    pub(crate) fn validate_environment(&self) -> BuildResult<()> {
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

        // Signing the APK runs `apksigner`, a Java program: a JDK from
        // `JAVA_HOME` or a `java` on PATH. Gradle projects need the former.
        let java_on_path = which::which("java").is_ok();
        if self.java_home.is_none() && !java_on_path {
            return Err(BuildError::ToolNotFound {
                tool: "java (a JDK 17+, for apksigner)".to_string(),
                install_hint: "install a JDK and set JAVA_HOME, or put `java` on PATH".to_string(),
            });
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

        crate::ui::debug("Android environment validation passed".to_string());
        crate::ui::debug(format!("  ANDROID_HOME: {}", self.android_home.display()));
        crate::ui::debug(format!("  NDK: {}", self.ndk_home.display()));

        Ok(())
    }

    pub(crate) async fn build_rust(&self, ctx: &BuilderContext) -> BuildResult<BuildArtifacts> {
        if matches!(
            ctx.target,
            crate::build::platform::BuildUnit::Library { .. }
        ) {
            return Err(BuildError::invalid_config(
                "build unit",
                "explicit static-library delivery is supported only on iOS",
            ));
        }
        let crate::build::platform::Platform::Android { targets } = &ctx.platform else {
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
            crate::ui::debug(format!(
                "Cleaning jniLibs directory: {}",
                jni_libs_dir.display()
            ));
            std::fs::remove_dir_all(&jni_libs_dir)?;
        }
        std::fs::create_dir_all(&jni_libs_dir)?;

        // cargo-ndk takes the output dir as a UTF-8 CLI argument; a non-UTF-8
        // workspace root is a caller-supplied environment problem, not a bug.
        let jni_libs_str = jni_libs_dir.to_str().ok_or_else(|| {
            BuildError::invalid_config(
                "workspace_root",
                format!("jniLibs path {} is not valid UTF-8", jni_libs_dir.display()),
            )
        })?;

        let mut rust_libs = Vec::new();

        for target in targets {
            crate::ui::debug(format!("Building for Android target: {target}"));

            // The project's own package: its `[lib]` is a `cdylib` with
            // `android_main` (what `flui create` writes), which `cargo ndk`
            // drops into `jniLibs/<abi>/lib<name>.so`.
            let mut args = vec![
                "ndk",
                "-t",
                target.as_str(),
                "-o",
                jni_libs_str,
                "build",
                "--lib",
            ];

            if let Some(profile_flag) = ctx.profile.cargo_flag() {
                args.push(profile_flag);
            }

            process::run(
                Command::new("cargo")
                    .args(&args)
                    .current_dir(&self.workspace_root),
            )
            .await?;

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

        crate::ui::debug(format!("Generated {} native libraries", rust_libs.len()));

        Ok(BuildArtifacts {
            rust_libs,
            executable: None,
            metadata: serde_json::json!({}),
        })
    }

    pub(crate) async fn build_platform(
        &self,
        ctx: &BuilderContext,
        artifacts: &BuildArtifacts,
    ) -> BuildResult<FinalArtifacts> {
        crate::ui::debug("Building APK with Gradle...".to_string());

        let android_dir = self.workspace_root.join("platforms").join("android");

        let gradle_wrapper_name = if cfg!(target_os = "windows") {
            "gradlew.bat"
        } else {
            "gradlew"
        };

        // A Gradle wrapper in the project means the app has grown a Java or
        // Kotlin side and Gradle owns the APK; without one, the SDK's own
        // build-tools package the NativeActivity app directly.
        let gradle_wrapper_path = android_dir.join(gradle_wrapper_name);
        if !gradle_wrapper_path.exists() {
            return self.package_with_build_tools(ctx, artifacts).await;
        }
        let java_home = self.java_home.as_ref().ok_or_else(|| BuildError::ToolNotFound {
            tool: "JAVA_HOME (Gradle needs a JDK)".to_string(),
            install_hint: "set JAVA_HOME to a JDK 17+, or remove platforms/android/gradlew to package without Gradle".to_string(),
        })?;

        let gradle_task = match ctx.profile {
            crate::build::platform::Profile::Debug => "assembleDebug",
            crate::build::platform::Profile::Release => "assembleRelease",
        };
        process::run(
            Command::new(&gradle_wrapper_path)
                .arg(gradle_task)
                .env("JAVA_HOME", java_home)
                .current_dir(&android_dir),
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

        crate::ui::debug(format!("APK copied to: {}", output_apk.display()));

        Ok(FinalArtifacts {
            app_binary: output_apk,
            size_bytes,
        })
    }
}

/// The Gradle-less delivery: `aapt2` + `zipalign` + `apksigner` from the
/// SDK's build-tools, see `android_package.rs`.
impl AndroidBuilder {
    async fn package_with_build_tools(
        &self,
        ctx: &BuilderContext,
        artifacts: &BuildArtifacts,
    ) -> BuildResult<FinalArtifacts> {
        let config = crate::config::FluiConfig::load_from(&self.workspace_root.join("flui.toml"))
            .map_err(|error| {
            BuildError::Other(format!("flui.toml names the package: {error}"))
        })?;
        let package = config.app.app_id();
        let manifest = self
            .workspace_root
            .join("platforms")
            .join("android")
            .join("app")
            .join("src")
            .join("main")
            .join("AndroidManifest.xml");
        if !manifest.is_file() {
            return Err(BuildError::path_not_found(
                manifest,
                "the Android platform is not scaffolded; run `flui platform add android`",
            ));
        }
        let native_libs: Vec<(String, PathBuf)> = artifacts
            .rust_libs
            .iter()
            .map(|lib| {
                let abi = lib
                    .parent()
                    .and_then(Path::file_name)
                    .and_then(|name| name.to_str())
                    .unwrap_or("arm64-v8a")
                    .to_string();
                (abi, lib.clone())
            })
            .collect();
        let output =
            ctx.output_dir
                .join(format!("{}-{}.apk", config.app.name, ctx.profile.as_str()));
        let tools =
            super::android_package::BuildTools::locate(&self.android_home, self.java_home.clone())?;
        tools
            .package(&super::android_package::ApkSpec {
                manifest: &manifest,
                package: &package,
                native_libs: &native_libs,
                work_dir: &ctx.output_dir.join("apk-work"),
                output: &output,
            })
            .await?;
        let size_bytes = std::fs::metadata(&output)?.len();
        crate::ui::debug(format!(
            "APK {} ({} entries)",
            output.display(),
            super::android_package::archive_entries(&output)?.len()
        ));
        Ok(FinalArtifacts {
            app_binary: output,
            size_bytes,
        })
    }
}
