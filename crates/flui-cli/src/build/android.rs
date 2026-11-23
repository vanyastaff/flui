use std::ffi::OsString;
use std::path::{Path, PathBuf};
use tokio::process::Command;

use crate::app_id::AppId;
use crate::build::android_signing::SigningKey;
use crate::build::error::{BuildError, BuildResult};
use crate::build::platform::{BuildArtifacts, BuilderContext, FinalArtifacts, Profile};
use crate::build::util::{check_command_exists, environment, process};

fn rust_target(abi: &str) -> &str {
    match abi {
        "arm64-v8a" => "aarch64-linux-android",
        "armeabi-v7a" => "armv7-linux-androideabi",
        "x86_64" => "x86_64-linux-android",
        "x86" => "i686-linux-android",
        other => other,
    }
}

/// Builder for Android platform (APK builds via Gradle and cargo-ndk)
#[derive(Debug)]
pub(crate) struct AndroidBuilder {
    workspace_root: PathBuf,
    android_home: PathBuf,
    /// The NDK this build resolved, which every `cargo ndk` call links with
    /// and a Gradle project strips with.
    ndk_home: PathBuf,
    /// `JAVA_HOME` when set. Gradle needs it; the build-tools packager only
    /// needs a `java` for `apksigner`, from here or from `PATH`.
    java_home: Option<PathBuf>,
}

/// What a package states besides its code: its version and the key it is
/// signed with, both from `flui.toml`.
struct PackageFacts {
    config: crate::config::FluiConfig,
    version_code: u32,
    version_name: String,
    signing: SigningKey,
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

    /// A `cargo ndk` command in the project, linking with the NDK this build
    /// resolved.
    fn cargo_ndk(&self) -> Command {
        let mut command = Command::new("cargo");
        command
            .arg("ndk")
            .envs(environment::ndk_env(&self.android_home, &self.ndk_home))
            .current_dir(&self.workspace_root);
        command
    }

    /// The version and the signing key a package built with `profile`
    /// states, read from `flui.toml`.
    ///
    /// # Errors
    ///
    /// When `flui.toml` cannot be read, a release build of a pre-release
    /// declares no `[app] build`, or the key cannot sign
    /// ([`SigningKey::for_build`]).
    fn package_facts(&self, profile: Profile) -> BuildResult<PackageFacts> {
        // flui.toml names the package either way, so a changed `[app] id`
        // reaches the APK whichever packages it.
        let config = crate::config::FluiConfig::load_from(&self.workspace_root.join("flui.toml"))
            .map_err(|error| {
            BuildError::Other(format!("flui.toml names the package: {error}"))
        })?;
        let numbers = config
            .app
            .version
            .package_numbers(config.app.build, profile)?;
        let version_code = numbers.android_version_code;
        let version_name = numbers.as_written;
        let signing = SigningKey::for_build(
            profile,
            config.signing.android.as_ref(),
            &self.workspace_root,
            &|name| std::env::var_os(name),
            &crate::build::android_signing::path_kind,
        )?;
        Ok(PackageFacts {
            config,
            version_code,
            version_name,
            signing,
        })
    }
}

/// Scene plugin build/deploy methods for hot-reload workflow.
///
/// These methods are separate from the build entry points because they
/// operate on a scene plugin crate (cdylib), not the host application.
impl AndroidBuilder {
    /// Build the scene plugin crate `scene_crate` (a `cdylib`) for the ABI
    /// `abi` and answer the library `cargo ndk` reports for it.
    ///
    /// `cargo ndk -o` copies the `cdylib` artifacts cargo reports for the
    /// selected package, and only them, into `<dir>/<abi>/`, so the library
    /// is that directory's one `.so` (beside `libc++_shared.so` when the
    /// crate links it), never a file picked by its name from the target
    /// directory.
    pub(crate) async fn build_scene_plugin(
        &self,
        abi: &str,
        scene_crate: &str,
        api_level: u32,
        release: bool,
    ) -> BuildResult<PathBuf> {
        crate::ui::debug(format!("Building scene plugin '{scene_crate}' for {abi}"));
        let out_dir =
            crate::build::util::cargo::target_directory(&self.workspace_root)?.join("flui-scene");
        if out_dir.exists() {
            std::fs::remove_dir_all(&out_dir)?;
        }
        std::fs::create_dir_all(&out_dir)?;

        process::run(self.cargo_ndk().args(scene_build_args(
            abi,
            api_level,
            &out_dir,
            scene_crate,
            release,
        )))
        .await?;

        let abi_dir = out_dir.join(abi);
        let libraries: Vec<PathBuf> = std::fs::read_dir(&abi_dir)
            .map_err(|_| BuildError::path_not_found(abi_dir.clone(), "cargo ndk's output"))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension().is_some_and(|ext| ext == "so")
                    && path
                        .file_name()
                        .is_some_and(|name| name != "libc++_shared.so")
            })
            .collect();
        let [so_path] = libraries.as_slice() else {
            return Err(BuildError::invalid_config(
                scene_crate,
                format!(
                    "cargo ndk reported {} libraries for the scene crate in {}, not one; its \
                     `[lib] crate-type` must include \"cdylib\"",
                    libraries.len(),
                    abi_dir.display()
                ),
            ));
        };

        // A 16 KB-page device cannot `dlopen` a 4 KB-aligned plugin either.
        if super::android_elf::needs_16_kb_pages(abi) {
            super::android_elf::check_page_alignment(so_path)?;
        }
        crate::ui::debug(format!("Scene plugin built: {}", so_path.display()));
        Ok(so_path.clone())
    }

    /// Push the scene plugin `so_path` to the device `serial` and copy it
    /// into the files of the application `package`, as `lib_name`.
    ///
    /// `adb push` to `/data/local/tmp/`, then `run-as` the application to
    /// copy it into its own directory, where SELinux lets the app map it
    /// ([`scene_push_commands`]).
    pub(crate) async fn push_scene_plugin(
        &self,
        adb: &Path,
        serial: &str,
        so_path: &Path,
        package: &AppId,
        lib_name: &str,
    ) -> BuildResult<()> {
        for args in scene_push_commands(serial, so_path, package, lib_name) {
            process::run(Command::new(adb).args(args)).await?;
        }
        crate::ui::debug(format!(
            "Scene plugin pushed to {serial}: files/{lib_name} of {}",
            package.android_package()
        ));
        Ok(())
    }
}

/// The refusal of a project whose own package has no library target.
fn no_library() -> BuildError {
    BuildError::invalid_config(
        "Cargo.toml",
        "the application has no library, which NativeActivity loads as a \
         `cdylib`; `flui create` puts the application in `src/lib.rs`",
    )
}

/// The `cargo ndk` arguments that build the library of `package` for `abi`
/// against `api_level` and copy it into `out_dir` as `<abi>/lib<name>.so`.
///
/// The library is compiled as a `cdylib`, the form `NativeActivity` loads,
/// with the `android_main` its `#[flui::main]` function writes. The crate
/// type is asked for here rather than declared in the manifest: a declared
/// `cdylib` is linked on every desktop build as well, a second link of the
/// whole framework per edit that no desktop run loads. A manifest that still
/// declares it builds the same library.
fn app_build_args<'a>(
    package: &'a str,
    abi: &'a str,
    api_level: &'a str,
    out_dir: &'a str,
    profile_flag: Option<&'a str>,
    features: &'a [String],
) -> Vec<&'a str> {
    let mut args = vec![
        "-t",
        abi,
        "--platform",
        api_level,
        "-o",
        out_dir,
        "rustc",
        "-p",
        package,
        "--lib",
        "--crate-type",
        "cdylib",
    ];
    args.extend(profile_flag);
    for feature in features {
        args.extend(["--features", feature.as_str()]);
    }
    args
}

/// The `cargo ndk` arguments that build the scene crate `scene_crate` for
/// `abi` against `api_level` and copy its library into `out_dir`. The crate
/// is named by its package name, which is how cargo-ndk finds its manifest.
fn scene_build_args(
    abi: &str,
    api_level: u32,
    out_dir: &Path,
    scene_crate: &str,
    release: bool,
) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        "-t".into(),
        abi.into(),
        "--platform".into(),
        api_level.to_string().into(),
        "-o".into(),
        out_dir.as_os_str().to_owned(),
        "build".into(),
        "-p".into(),
        scene_crate.into(),
        "--lib".into(),
    ];
    if release {
        args.push("--release".into());
    }
    args
}

/// The two `adb` commands that put the scene plugin on the device `serial`.
///
/// `adb shell` joins the words after it with spaces and hands them to the
/// device's shell, so each word is one the shell takes as it is: the paths
/// are FLUI's own (`/data/local/tmp/<lib_name>`, `files/<lib_name>` under the
/// directory `run-as` starts in) and the package is an [`AppId`], whose
/// grammar holds no character a shell reads.
fn scene_push_commands(
    serial: &str,
    so_path: &Path,
    package: &AppId,
    lib_name: &str,
) -> [Vec<OsString>; 2] {
    let staged = format!("/data/local/tmp/{lib_name}");
    [
        vec![
            "-s".into(),
            serial.into(),
            "push".into(),
            so_path.as_os_str().to_owned(),
            staged.clone().into(),
        ],
        vec![
            "-s".into(),
            serial.into(),
            "shell".into(),
            "run-as".into(),
            package.android_package().into(),
            "cp".into(),
            staged.into(),
            format!("files/{lib_name}").into(),
        ],
    ]
}

impl AndroidBuilder {
    /// Refuse a build with `profile` for `abi` that cannot run here, before
    /// anything compiles: no `cargo ndk`, no JDK for `apksigner`, no Rust
    /// target for the ABI, a version with no `versionCode`, a key that cannot
    /// sign.
    ///
    /// # Errors
    ///
    /// The first of those refusals.
    pub(crate) fn validate_environment(&self, abi: &str, profile: Profile) -> BuildResult<()> {
        check_command_exists("cargo")?;

        let cargo_ndk_ok = crate::proc::probe_stdout(
            std::process::Command::new("cargo")
                .args(["ndk", "--version"])
                .envs(environment::ndk_env(&self.android_home, &self.ndk_home))
                .current_dir(&self.workspace_root),
            crate::proc::PROBE_TIMEOUT,
        )
        .is_some();
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

        // The compiler target cargo-ndk will actually use.
        environment::require_rust_target(&self.workspace_root, rust_target(abi))?;

        // The packager without Gradle needs build-tools and an Android
        // platform at FLUI's targetSdk: a missing one is named now, not after
        // the native build. Gradle resolves its own.
        let gradle_wrapper = if cfg!(target_os = "windows") {
            "gradlew.bat"
        } else {
            "gradlew"
        };
        if !self
            .workspace_root
            .join("platforms")
            .join("android")
            .join(gradle_wrapper)
            .exists()
        {
            super::android_package::BuildTools::locate(&self.android_home, None)?;
        }

        if self.workspace_root.join("flui.toml").is_file() {
            self.package_facts(profile)?;
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

        // Refused before anything is built or cleaned.
        let package = self.application_package()?;

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

        // The API level the NDK links against is the floor the APK declares,
        // so the library calls nothing newer than the manifest promises.
        let platform = ctx.floors.android_api().to_string();
        let mut rust_libs = Vec::new();

        for target in targets {
            crate::ui::debug(format!("Building for Android target: {target}"));

            let args = app_build_args(
                &package,
                target,
                &platform,
                jni_libs_str,
                ctx.profile.cargo_flag(),
                &ctx.features,
            );

            process::run(
                self.cargo_ndk()
                    .args(&args)
                    .envs(ctx.floors.cargo_env(rust_target(target))),
            )
            .await?;

            // Find the .so file
            let abi_dir = jni_libs_dir.join(target);
            if abi_dir.exists() {
                for entry in std::fs::read_dir(&abi_dir)? {
                    let entry = entry?;
                    let path = entry.path();
                    if path.extension().is_some_and(|ext| ext == "so") {
                        if super::android_elf::needs_16_kb_pages(target) {
                            super::android_elf::check_page_alignment(&path)?;
                        }
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
            package: None,
        })
    }

    /// Package the APK of `artifacts`, and say how it was signed.
    pub(crate) async fn build_platform(
        &self,
        ctx: &BuilderContext,
        artifacts: &BuildArtifacts,
    ) -> BuildResult<AndroidPackage> {
        let android_dir = self.workspace_root.join("platforms").join("android");

        let gradle_wrapper_name = if cfg!(target_os = "windows") {
            "gradlew.bat"
        } else {
            "gradlew"
        };

        if !android_dir.is_dir() {
            return Err(BuildError::path_not_found(
                android_dir,
                "the Android platform is not scaffolded; run `flui platform add android`",
            ));
        }
        let facts = self.package_facts(ctx.profile)?;
        // Refused before anything is packaged: assets no APK can carry.
        let assets = crate::build::assets::of_project(&self.workspace_root)?;
        if facts.signing.is_debug_for_release() {
            let _ = crate::ui::warning(format!(
                "this release APK is signed with the debug key ({}), which a store refuses; \
                 name the release key in flui.toml under [signing.android]",
                facts.signing.keystore().display()
            ));
        }
        // A Gradle wrapper in the project means the app has grown a Java or
        // Kotlin side and Gradle owns the APK; without one, the SDK's own
        // build-tools package the NativeActivity app directly.
        let gradle_wrapper_path = android_dir.join(gradle_wrapper_name);
        let by_gradle = gradle_wrapper_path.exists();
        let declarations = self.declarations(ctx)?;
        self.write_manifest(&facts.config, &declarations, by_gradle)?;
        // Both packagers write the same file, so a Gradle wrapper added later
        // replaces the APK rather than leaving a second one beside it.
        let output = ctx.output_dir.join(format!(
            "{}-{}.apk",
            facts.config.app.name,
            ctx.profile.as_str()
        ));

        if !by_gradle {
            let apk = self
                .package_with_build_tools(ctx, artifacts, &facts, &declarations, &assets, &output)
                .await?;
            return Ok(AndroidPackage {
                apk,
                signing: facts.signing.package_signing(),
            });
        }
        crate::ui::debug("Building APK with Gradle...".to_string());
        // Gradle adds this directory to the main source set's assets, so the APK
        // carries `assets/flui_assets/<key>` (ADR-0384 §3).
        // Absolute: Gradle resolves a relative source directory against the
        // module, not the directory `flui` runs in.
        let gradle_assets = std::path::absolute(ctx.output_dir.join("gradle-assets"))?;
        crate::build::assets::stage(&assets, &gradle_assets)?;
        let java_home = self.java_home.as_ref().ok_or_else(|| BuildError::ToolNotFound {
            tool: "JAVA_HOME (Gradle needs a JDK)".to_string(),
            install_hint: "set JAVA_HOME to a JDK 17+, or remove platforms/android/gradlew to package without Gradle".to_string(),
        })?;

        let command_properties = crate::build::gradle_project::state_project(
            &android_dir,
            &crate::build::gradle_project::GradleFacts {
                min_sdk: ctx.floors.android_api(),
                target_sdk: crate::build::platform_floor::ANDROID_TARGET_SDK,
                application_id: facts.config.app.id.android_package(),
                version_code: facts.version_code,
                version_name: &facts.version_name,
                ndk_path: &self.ndk_home,
                profile: ctx.profile,
                release_key: facts.signing.release_key(),
                java_sources: &declarations
                    .android_java()
                    .map(|(_, path)| path.to_path_buf())
                    .collect::<Vec<_>>(),
                proguard_files: &declarations
                    .android_proguard()
                    .map(|(_, path)| path.to_path_buf())
                    .collect::<Vec<_>>(),
                assets_dir: (!assets.is_empty()).then_some(gradle_assets.as_path()),
            },
        )?;
        let gradle_task = match ctx.profile {
            Profile::Debug => "assembleDebug",
            Profile::Release => "assembleRelease",
        };
        process::run(
            Command::new(&gradle_wrapper_path)
                .args(&command_properties)
                .arg(gradle_task)
                .env("JAVA_HOME", java_home)
                .current_dir(&android_dir),
        )
        .await?;

        let apk_path = gradle_apk(
            &android_dir
                .join("app")
                .join("build")
                .join("outputs")
                .join("apk")
                .join(ctx.profile.as_str()),
            ctx.profile,
        )?;
        std::fs::copy(&apk_path, &output)?;
        let size_bytes = std::fs::metadata(&output)?.len();
        crate::ui::debug(format!("APK copied to: {}", output.display()));

        Ok(AndroidPackage {
            apk: FinalArtifacts {
                app_binary: output,
                size_bytes,
            },
            signing: facts.signing.package_signing(),
        })
    }
}

/// An APK the build delivered, and how it was signed.
#[derive(Debug)]
pub(crate) struct AndroidPackage {
    /// The APK and its size.
    pub(crate) apk: FinalArtifacts,
    /// The key it was signed with.
    pub(crate) signing: crate::build::android_signing::PackageSigning,
}

/// The APK the Gradle build of `profile` wrote in `apk_dir`: the module's
/// `app-<profile>.apk`, which the Android Gradle plugin names
/// `app-<profile>-unsigned.apk` when no signing config signs it.
///
/// Gradle leaves the outputs of earlier builds in place, so of the two names
/// the newer file is this build's.
///
/// # Errors
///
/// [`BuildError::UnsignedApk`] when this build's APK is the unsigned one,
/// [`BuildError::PathNotFound`] when there is neither.
fn gradle_apk(apk_dir: &Path, profile: Profile) -> BuildResult<PathBuf> {
    let signed = apk_dir.join(format!("app-{}.apk", profile.as_str()));
    let unsigned = apk_dir.join(format!("app-{}-unsigned.apk", profile.as_str()));
    let modified = |path: &Path| {
        std::fs::metadata(path)
            .ok()
            .filter(std::fs::Metadata::is_file)
            .and_then(|metadata| metadata.modified().ok())
    };
    match (modified(&signed), modified(&unsigned)) {
        (Some(signed_at), Some(unsigned_at)) if unsigned_at > signed_at => {
            Err(BuildError::UnsignedApk { apk: unsigned })
        }
        (Some(_), _) => Ok(signed),
        (None, Some(_)) => Err(BuildError::UnsignedApk { apk: unsigned }),
        (None, None) => Err(BuildError::PathNotFound {
            path: signed,
            context: "the APK Gradle should have written".to_string(),
        }),
    }
}

/// The Gradle-less delivery: `aapt2` + `zipalign` + `apksigner` from the
/// SDK's build-tools, see `android_package.rs`.
impl AndroidBuilder {
    /// The package whose library the Android build compiles: the one whose
    /// manifest is in the project's directory, refused when it has no
    /// library. `cargo rustc` compiles exactly one target, so the build names
    /// it, by name, which is how cargo-ndk looks up the manifest whose
    /// artifacts it copies (it panics on a package id). A dependency that
    /// shares the application's name makes that name ambiguous to cargo,
    /// which then refuses the build and names both.
    fn application_package(&self) -> BuildResult<String> {
        let project = super::util::cargo::cargo_project(&self.workspace_root)?;
        let (Some(package), Some(_)) = (project.package, project.library) else {
            return Err(no_library());
        };
        Ok(package)
    }

    /// What the crates linked for the ABIs built, with the features the build
    /// turns on, declare (ADR-0307): the build of the package
    /// [`Self::application_package`] names, which `cargo ndk rustc -p`
    /// compiled.
    fn declarations(&self, ctx: &BuilderContext) -> BuildResult<crate::declarations::Declarations> {
        let crate::build::platform::Platform::Android { targets } = &ctx.platform else {
            return Err(BuildError::InvalidPlatform {
                reason: "Expected Android platform".to_string(),
            });
        };
        let package = self.application_package()?;
        let triples: Vec<&str> = targets.iter().map(|abi| rust_target(abi)).collect();
        crate::declarations::Declarations::collect(
            crate::declarations::Build::at(&self.workspace_root)
                .selecting(crate::declarations::Selection::Package(&package))
                .features(&ctx.features),
            &triples,
        )
    }

    /// Writes `platforms/android/app/src/main/AndroidManifest.xml`, which
    /// both packagers read: FLUI's base, what the crates linked for the ABIs
    /// built declare, the label from `flui.toml` and the library `cargo ndk`
    /// built (ADR-0307). A provider's authority leads with Gradle's
    /// `${applicationId}` when Gradle packs the APK (`by_gradle`), so a
    /// variant's `applicationIdSuffix` reaches it, and with `flui.toml`'s id,
    /// the package `aapt2` links, otherwise.
    fn write_manifest(
        &self,
        config: &crate::config::FluiConfig,
        declarations: &crate::declarations::Declarations,
        by_gradle: bool,
    ) -> BuildResult<()> {
        let lib_name = super::util::cargo::cargo_project(&self.workspace_root)?
            .library
            .ok_or_else(no_library)?;
        super::android_manifest::write(
            &super::android_manifest::path_in(&self.workspace_root),
            &super::android_manifest::ManifestFacts {
                label: config.app.shown_name(),
                lib_name: &lib_name,
                application_id: if by_gradle {
                    super::android_manifest::ApplicationId::Gradle
                } else {
                    super::android_manifest::ApplicationId::Written(config.app.id.android_package())
                },
            },
            declarations,
        )
    }

    async fn package_with_build_tools(
        &self,
        ctx: &BuilderContext,
        artifacts: &BuildArtifacts,
        facts: &PackageFacts,
        declarations: &crate::declarations::Declarations,
        assets: &[crate::build::assets::PackagedAsset],
        output: &Path,
    ) -> BuildResult<FinalArtifacts> {
        let manifest = super::android_manifest::path_in(&self.workspace_root);
        let dex: Vec<PathBuf> = declarations
            .android_dex()
            .map(|(_, path)| path.to_path_buf())
            .collect();
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
        let tools =
            super::android_package::BuildTools::locate(&self.android_home, self.java_home.clone())?;
        tools
            .package(&super::android_package::ApkSpec {
                manifest: &manifest,
                package: facts.config.app.id.android_package(),
                native_libs: &native_libs,
                dex: &dex,
                work_dir: &ctx.output_dir.join("apk-work"),
                output,
                min_sdk: ctx.floors.android_api(),
                version_code: facts.version_code,
                version_name: &facts.version_name,
                signing: &facts.signing,
                assets,
            })
            .await?;
        let size_bytes = std::fs::metadata(output)?.len();
        crate::ui::debug(format!(
            "APK {} ({} entries)",
            output.display(),
            super::android_package::archive_entries(output)?.len()
        ));
        Ok(FinalArtifacts {
            app_binary: output.to_path_buf(),
            size_bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::path::Path;

    use super::{app_build_args, gradle_apk, scene_build_args, scene_push_commands};
    use crate::app_id::AppId;
    use crate::build::error::BuildError;
    use crate::build::platform::Profile;

    fn strings(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    /// The scene plugin reaches the device a run names, and the application
    /// directory by a `cp` whose every word is its own argument: `adb shell`
    /// joins its words for the device's shell, so `sh -c "cp …"` arrived there
    /// as `sh -c cp` with no operands.
    #[test]
    fn the_scene_plugin_is_copied_into_the_application_on_the_named_device() {
        let package = AppId::try_new("com.example.notes").expect("an identifier");
        let [push, copy] = scene_push_commands(
            "emulator-5554",
            Path::new("out/arm64-v8a/libnotes_scene.so"),
            &package,
            "libflui_scene.so",
        );
        assert_eq!(
            strings(&push),
            [
                "-s",
                "emulator-5554",
                "push",
                Path::new("out/arm64-v8a/libnotes_scene.so")
                    .to_string_lossy()
                    .as_ref(),
                "/data/local/tmp/libflui_scene.so"
            ]
        );
        assert_eq!(
            strings(&copy),
            [
                "-s",
                "emulator-5554",
                "shell",
                "run-as",
                "com.example.notes",
                "cp",
                "/data/local/tmp/libflui_scene.so",
                "files/libflui_scene.so"
            ]
        );
    }

    /// The application's library is compiled as the `cdylib` `NativeActivity`
    /// loads, whatever crate types its manifest declares, and its package is
    /// named, since `cargo rustc` compiles exactly one target.
    #[test]
    fn the_application_library_is_compiled_as_the_cdylib_android_loads() {
        let features = ["driver".to_owned()];
        assert_eq!(
            app_build_args(
                "my-app",
                "arm64-v8a",
                "26",
                "jniLibs",
                Some("--release"),
                &features
            ),
            [
                "-t",
                "arm64-v8a",
                "--platform",
                "26",
                "-o",
                "jniLibs",
                "rustc",
                "-p",
                "my-app",
                "--lib",
                "--crate-type",
                "cdylib",
                "--release",
                "--features",
                "driver"
            ]
        );
        assert_eq!(
            app_build_args("my-app", "x86_64", "26", "jniLibs", None, &[]),
            [
                "-t",
                "x86_64",
                "--platform",
                "26",
                "-o",
                "jniLibs",
                "rustc",
                "-p",
                "my-app",
                "--lib",
                "--crate-type",
                "cdylib"
            ]
        );
    }

    /// The scene crate is built against the application's floor and its
    /// library copied where the build reads it, the crate named as
    /// cargo-ndk finds it.
    #[test]
    fn the_scene_plugin_is_built_at_the_floor_into_its_own_directory() {
        assert_eq!(
            strings(&scene_build_args(
                "x86_64",
                26,
                Path::new("scene-out"),
                "notes-scene",
                true
            )),
            [
                "-t",
                "x86_64",
                "--platform",
                "26",
                "-o",
                "scene-out",
                "build",
                "-p",
                "notes-scene",
                "--lib",
                "--release"
            ]
        );
    }

    /// Gradle's APK is the signed one this build wrote: an unsigned APK, the
    /// one the release build type writes without a signing config, is
    /// refused, also when an older signed one is still there.
    #[test]
    fn a_gradle_apk_that_is_not_signed_is_refused() {
        let dir = tempfile::tempdir().expect("apk dir");
        let signed = dir.path().join("app-release.apk");
        let unsigned = dir.path().join("app-release-unsigned.apk");
        let write = |path: &Path, age_secs: u64| {
            std::fs::write(path, b"PK").expect("apk");
            let at = std::time::SystemTime::now() - std::time::Duration::from_secs(age_secs);
            std::fs::File::options()
                .write(true)
                .open(path)
                .and_then(|file| file.set_modified(at))
                .expect("modified time");
        };
        let mut failures = Vec::new();

        if !matches!(
            gradle_apk(dir.path(), Profile::Release),
            Err(BuildError::PathNotFound { .. })
        ) {
            failures.push("no APK at all".to_owned());
        }
        write(&unsigned, 0);
        match gradle_apk(dir.path(), Profile::Release) {
            Err(BuildError::UnsignedApk { apk }) if apk == unsigned => {}
            other => failures.push(format!("only the unsigned APK: {other:?}")),
        }
        write(&signed, 60);
        match gradle_apk(dir.path(), Profile::Release) {
            Err(BuildError::UnsignedApk { apk }) if apk == unsigned => {}
            other => failures.push(format!("an older signed APK beside it: {other:?}")),
        }
        write(&signed, 0);
        write(&unsigned, 60);
        match gradle_apk(dir.path(), Profile::Release) {
            Ok(apk) if apk == signed => {}
            other => failures.push(format!("a newer signed APK: {other:?}")),
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }
}
