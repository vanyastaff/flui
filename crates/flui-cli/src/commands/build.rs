use crate::BuildTarget;
use crate::build::platform::BuildUnit as CargoBuildUnit;
use crate::build::{
    AndroidBuilder, AppBundle, BuilderContextBuilder, DesktopBuilder, IosBuilder, Platform,
    Profile, WebBuilder,
};
use crate::error::{CliError, CliResult, ResultExt};
use crate::ui;
use console::style;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Build options collected into a struct to avoid excessive bool parameters.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct BuildOptions {
    /// Build in release mode.
    pub(crate) release: bool,
    /// iOS: Build device and simulator libraries (XCFramework without an Xcode project).
    pub(crate) universal: bool,
    /// Select iOS static-library/XCFramework delivery instead of an application.
    pub(crate) library: bool,
    /// Exact iOS simulator UDID (resolved before compilation).
    pub(crate) simulator: Option<String>,
    /// Build a named example rather than the current package's binary.
    pub(crate) example: Option<String>,
    /// Build a named workspace package's binary.
    pub(crate) package: Option<String>,
}

impl BuildOptions {
    /// Resolve the cargo unit this build selects.
    ///
    /// `--example` and `--package` are mutually exclusive (rejected by
    /// `validate_options`); neither given is the generated-project default:
    /// the current directory's own binary.
    fn cargo_target(&self) -> CargoBuildUnit {
        if self.library {
            return CargoBuildUnit::Library {
                package: self.package.clone(),
            };
        }
        match (&self.example, &self.package) {
            (Some(example), _) => CargoBuildUnit::Example(example.clone()),
            (None, Some(package)) => CargoBuildUnit::Package(package.clone()),
            (None, None) => CargoBuildUnit::DefaultBinary,
        }
    }

    /// The cargo unit's kind and name, for the `build.start` JSON event.
    fn unit_descriptor(&self) -> (&'static str, Option<String>) {
        if self.library {
            return ("lib", self.package.clone());
        }
        match (&self.example, &self.package) {
            (Some(example), _) => ("example", Some(example.clone())),
            (None, Some(package)) => ("package", Some(package.clone())),
            (None, None) => ("bin", None),
        }
    }
}

/// One artifact a build produced: reported in human output (path + size) and
/// in the `build.done` JSON event.
struct Artifact {
    /// `"binary"` | `"app-bundle"` | `"apk"` | `"xcframework"` | `"wasm"` | `"dir"`.
    ///
    /// This is the stable, machine-facing tag: it never changes shape based
    /// on how the build was invoked. See `label` for the human-facing text.
    kind: &'static str,
    /// Override for the human success line when `kind` alone would be
    /// misleading — e.g. `macos --universal` reports the same `"binary"`
    /// kind as a plain non-macOS desktop build, but here it means "fused,
    /// not bundled", which is worth spelling out. `None` shows `kind` as-is.
    label: Option<&'static str>,
    /// Where the artifact was delivered.
    path: PathBuf,
    /// Size on disk (the sum of contained files for a bundle/directory).
    size_bytes: u64,
}

impl Artifact {
    fn new(kind: &'static str, path: PathBuf, size_bytes: u64) -> Self {
        Self {
            kind,
            label: None,
            path,
            size_bytes,
        }
    }

    /// Override the human-facing success line's description without
    /// changing the machine-facing JSON `kind`.
    fn with_label(mut self, label: &'static str) -> Self {
        self.label = Some(label);
        self
    }

    /// What the human success line calls this artifact.
    fn display_kind(&self) -> &'static str {
        self.label.unwrap_or(self.kind)
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "kind": self.kind,
            "path": self.path.display().to_string(),
            "size_bytes": self.size_bytes,
        })
    }
}

/// Drive one of the async builders to completion on the build runtime
/// `execute` entered; a plain executor cannot, because the builders spawn
/// `tokio::process` children that need the runtime's reactor.
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Handle::current().block_on(future)
}

/// Render a byte count the way a person reads it: KB below one MB, MB above.
fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= MB {
        format!("{:.2} MB", bytes / MB)
    } else {
        format!("{:.2} KB", bytes / KB)
    }
}

/// Refuse to do any build work outside a FLUI/Cargo project.
///
/// Every build backend eventually shells out to `cargo`, so the one thing
/// they all need is a `Cargo.toml` at the workspace root. Checking it up
/// front turns "some tool 40 seconds in reported a confusing cargo error"
/// into an immediate, exact diagnosis.
fn ensure_flui_project(root: &Path) -> CliResult<()> {
    if root.join("Cargo.toml").is_file() {
        Ok(())
    } else {
        Err(CliError::NotFluiProject {
            reason: "Cargo.toml not found".into(),
        })
    }
}

/// Execute the build command.
///
/// # Errors
///
/// Returns an error if the build fails for the target platform.
#[expect(clippy::too_many_arguments, reason = "mirrors clap argument structure")]
pub(crate) fn execute(
    target: BuildTarget,
    release: bool,
    output: Option<PathBuf>,
    universal: bool,
    example: Option<String>,
    package: Option<String>,
    library: bool,
    simulator: Option<String>,
) -> CliResult<()> {
    let options = BuildOptions {
        release,
        universal,
        library,
        simulator,
        example,
        package,
    };

    validate_options(target, &options)?;
    run(target, options, output)
}

/// Reject conflicting selectors before any environment probing or build
/// work starts. Each message names the exact flags in conflict.
///
/// `--lib`/`--example` and `--universal`/`--simulator` are *not* checked
/// here: clap's own `conflicts_with` on those arguments (see `main.rs`)
/// already rejects them, before this function ever runs. Duplicating that
/// check here would be dead code.
fn validate_options(target: BuildTarget, options: &BuildOptions) -> CliResult<()> {
    if options.library && target != BuildTarget::Ios {
        return Err(CliError::Usage(
            "--lib is an iOS-only option; pass it with `flui build ios --lib`".into(),
        ));
    }
    if options.simulator.is_some() && target != BuildTarget::Ios {
        return Err(CliError::Usage(
            "--simulator is an iOS-only option; pass it with `flui build ios --simulator <UDID>`"
                .into(),
        ));
    }
    if options.example.is_some() && options.package.is_some() {
        return Err(CliError::Usage(
            "--example and --package are mutually exclusive: choose one build unit".into(),
        ));
    }
    if options.universal && !options.library && target == BuildTarget::Ios {
        return Err(CliError::Usage(
            "--universal requires --lib on iOS: an application delivery cannot be built as a universal XCFramework"
                .into(),
        ));
    }
    Ok(())
}

/// Dispatch to the platform backend, time it, and report the outcome
/// uniformly (human text plus `build.start`/`build.done` JSON events).
fn run(target: BuildTarget, options: BuildOptions, output: Option<PathBuf>) -> CliResult<()> {
    let mode = if options.release { "release" } else { "debug" };
    let (unit_kind, unit_name) = options.unit_descriptor();

    ui::intro(style(format!(" flui build {target} ")).on_cyan().black())?;
    ui::info(format!("Mode: {}", style(mode).cyan()))?;
    ui::emit(
        "build.start",
        &serde_json::json!({
            "platform": target.to_string(),
            "profile": mode,
            "unit": { "kind": unit_kind, "name": unit_name },
        }),
    );

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to start the build runtime")?;
    let _reactor = runtime.enter();

    let started = Instant::now();
    let result = match target {
        BuildTarget::Android => build_android(&options, output.as_ref()),
        BuildTarget::Ios => build_ios(&options, output.as_ref()),
        BuildTarget::Web => build_web(&options, output.as_ref()),
        BuildTarget::Desktop => build_desktop(&options, output.as_ref()),
        BuildTarget::Windows | BuildTarget::Linux | BuildTarget::Macos => {
            build_specific_platform(target, &options, output.as_ref())
        }
    };
    let seconds = started.elapsed().as_secs_f64();

    match result {
        Ok(artifacts) => {
            ui::emit(
                "build.done",
                &serde_json::json!({
                    "ok": true,
                    "platform": target.to_string(),
                    "artifacts": artifacts.iter().map(Artifact::to_json).collect::<Vec<_>>(),
                    "seconds": seconds,
                }),
            );
            for artifact in &artifacts {
                ui::success(format!(
                    "{}: {} ({})",
                    artifact.display_kind(),
                    artifact.path.display(),
                    human_size(artifact.size_bytes)
                ))?;
            }
            ui::outro(format!("{} Built in {:.1}s", style("✓").green(), seconds))?;
            Ok(())
        }
        Err(error) => {
            ui::emit(
                "build.done",
                &serde_json::json!({
                    "ok": false,
                    "platform": target.to_string(),
                    "artifacts": [],
                    "seconds": seconds,
                }),
            );
            ui::outro_cancel(format!("Build failed: {error}"))?;
            Err(error)
        }
    }
}

/// Resolve application-bundle metadata for a macOS build, from `flui.toml`.
///
/// A `.app` needs a display name and a bundle identifier; `flui.toml`'s
/// `[app].name`/`[app].organization` are exactly those. When the file is absent
/// (e.g. building an in-repo example), the directory name stands in — a bundle
/// still gets staged, with a derived identifier, rather than the build silently
/// degrading to a bare binary.
fn macos_bundle() -> CliResult<AppBundle> {
    let root = std::env::current_dir().context("failed to resolve application directory")?;
    macos_bundle_at(&root)
}

fn macos_bundle_at(root: &std::path::Path) -> CliResult<AppBundle> {
    let manifest = root.join("flui.toml");
    match std::fs::symlink_metadata(&manifest) {
        Ok(_) => {
            let config = crate::config::FluiConfig::load_from(&manifest)?;
            Ok(AppBundle::new(&config.app.name, &config.app.organization))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let name = root
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| {
                    crate::error::CliError::Missing(
                        "Application directory has no usable bundle name".into(),
                    )
                })?;
            Ok(AppBundle::new(name, "dev.flui"))
        }
        Err(error) => Err(crate::error::CliError::context(
            error,
            format!("failed to inspect {}", manifest.display()),
        )),
    }
}

fn build_android(options: &BuildOptions, output: Option<&PathBuf>) -> CliResult<Vec<Artifact>> {
    let workspace_root = std::env::current_dir()?;
    ensure_flui_project(&workspace_root)?;

    let spinner = ui::spinner();
    spinner.start("Building Android APK...");

    let profile = if options.release {
        Profile::Release
    } else {
        Profile::Debug
    };

    let android_builder =
        AndroidBuilder::new(&workspace_root).context("failed to initialize Android builder")?;

    let mut builder = BuilderContextBuilder::new(workspace_root)
        .with_platform(Platform::Android {
            targets: vec!["arm64-v8a".to_string()],
        })
        .with_profile(profile);

    if let Some(out) = output {
        builder = builder.with_output_dir(out.clone());
    }

    let ctx = builder.build();

    std::fs::create_dir_all(&ctx.output_dir)?;

    ui::emit("build.phase", &serde_json::json!({ "name": "validate" }));
    spinner.start("Validating Android environment...");
    android_builder
        .validate_environment()
        .context("Android environment validation failed")?;

    ui::emit("build.phase", &serde_json::json!({ "name": "build_rust" }));
    spinner.start("Building Rust libraries...");
    let artifacts =
        block_on(android_builder.build_rust(&ctx)).context("failed to build Rust libraries")?;

    ui::emit(
        "build.phase",
        &serde_json::json!({ "name": "build_platform" }),
    );
    spinner.start("Building APK...");
    let final_artifacts = block_on(android_builder.build_platform(&ctx, &artifacts))
        .context("failed to build APK")?;

    spinner.stop(format!("{} Android APK built", style("✓").green()));

    Ok(vec![Artifact::new(
        "apk",
        final_artifacts.app_binary,
        final_artifacts.size_bytes,
    )])
}

fn build_ios(options: &BuildOptions, output: Option<&PathBuf>) -> CliResult<Vec<Artifact>> {
    let workspace_root = std::env::current_dir()?;
    ensure_flui_project(&workspace_root)?;

    let spinner = ui::spinner();
    spinner.start("Building iOS Rust target...");

    let profile = if options.release {
        Profile::Release
    } else {
        Profile::Debug
    };

    let simulator = options
        .simulator
        .as_deref()
        .map(super::ios::resolve_simulator)
        .transpose()?;
    let targets = if let Some(simulator) = &simulator {
        vec![simulator.triple.clone()]
    } else if options.universal {
        vec![
            "aarch64-apple-ios".to_string(),
            "aarch64-apple-ios-sim".to_string(),
        ]
    } else {
        vec!["aarch64-apple-ios".to_string()]
    };

    let ios_builder = IosBuilder::new();

    let bundle = super::ios::configured_bundle(&workspace_root)?;
    let mut builder = BuilderContextBuilder::new(workspace_root)
        .with_platform(Platform::Ios { targets })
        .with_target(options.cargo_target())
        .with_profile(profile);
    if !options.library
        && let Some(bundle) = bundle
    {
        builder = builder.with_bundle(bundle);
    }
    if let Some(out) = output {
        builder = builder.with_output_dir(out.clone());
    }
    let ctx = builder.build();

    ui::emit("build.phase", &serde_json::json!({ "name": "validate" }));
    spinner.start("Validating iOS environment...");
    IosBuilder::validate_environment().context("iOS environment validation failed")?;

    ui::emit("build.phase", &serde_json::json!({ "name": "build_rust" }));
    spinner.start("Building iOS Rust target...");
    let artifacts =
        block_on(ios_builder.build_rust(&ctx)).context("failed to build iOS Rust target")?;

    ui::emit(
        "build.phase",
        &serde_json::json!({ "name": "build_platform" }),
    );
    spinner.start("Building iOS app...");
    let final_artifacts = block_on(ios_builder.build_platform(&ctx, &artifacts))
        .context("failed to build iOS app")?;

    if let Some(simulator) = simulator {
        super::ios::check_runtime(&simulator, &final_artifacts.app_binary)?;
    }
    spinner.stop(format!("{} iOS build finished", style("✓").green()));

    let kind = if options.library {
        "xcframework"
    } else {
        "app-bundle"
    };
    Ok(vec![Artifact::new(
        kind,
        final_artifacts.app_binary,
        final_artifacts.size_bytes,
    )])
}

fn build_web(options: &BuildOptions, output: Option<&PathBuf>) -> CliResult<Vec<Artifact>> {
    let workspace_root = std::env::current_dir()?;
    ensure_flui_project(&workspace_root)?;

    let spinner = ui::spinner();
    spinner.start("Building Web (WASM)...");

    let profile = if options.release {
        Profile::Release
    } else {
        Profile::Debug
    };

    let web_builder = WebBuilder::new(&workspace_root);

    let mut builder = BuilderContextBuilder::new(workspace_root)
        .with_platform(Platform::Web {
            target: "web".to_string(),
        })
        .with_profile(profile);

    if let Some(out) = output {
        builder = builder.with_output_dir(out.clone());
    }

    let ctx = builder.build();

    std::fs::create_dir_all(&ctx.output_dir)?;

    ui::emit("build.phase", &serde_json::json!({ "name": "validate" }));
    spinner.start("Validating Web environment...");
    web_builder
        .validate_environment()
        .context("Web environment validation failed")?;

    ui::emit("build.phase", &serde_json::json!({ "name": "build_rust" }));
    spinner.start("Building WASM...");
    let artifacts = block_on(web_builder.build_rust(&ctx)).context("failed to build WASM")?;

    ui::emit(
        "build.phase",
        &serde_json::json!({ "name": "build_platform" }),
    );
    spinner.start("Building web package...");
    let final_artifacts = block_on(web_builder.build_platform(&ctx, &artifacts))
        .context("failed to build web package")?;

    spinner.stop(format!("{} Web package built", style("✓").green()));

    Ok(vec![Artifact::new(
        "dir",
        ctx.output_dir.clone(),
        final_artifacts.size_bytes,
    )])
}

fn build_desktop(options: &BuildOptions, output: Option<&PathBuf>) -> CliResult<Vec<Artifact>> {
    let workspace_root = std::env::current_dir()?;

    let profile = if options.release {
        Profile::Release
    } else {
        Profile::Debug
    };

    let desktop_builder = DesktopBuilder::new();

    let mut builder = BuilderContextBuilder::new(workspace_root.clone())
        .with_platform(Platform::Desktop { target: None })
        .with_target(options.cargo_target())
        .with_profile(profile);

    #[cfg(target_os = "macos")]
    {
        builder = builder.with_bundle(macos_bundle()?);
    }

    // Placed after the macOS bundle-metadata resolution above (not before
    // it): an invalid `flui.toml` is a more specific diagnosis than a
    // missing `Cargo.toml`, and must still win when both are absent/invalid.
    ensure_flui_project(&workspace_root)?;

    if let Some(out) = output {
        builder = builder.with_output_dir(out.clone());
    }

    let ctx = builder.build();

    std::fs::create_dir_all(&ctx.output_dir)?;

    let spinner = ui::spinner();
    spinner.start("Building Desktop binary...");

    ui::emit("build.phase", &serde_json::json!({ "name": "validate" }));
    spinner.start("Validating Desktop environment...");
    DesktopBuilder::validate_environment().context("desktop environment validation failed")?;

    ui::emit("build.phase", &serde_json::json!({ "name": "build_rust" }));
    spinner.start("Building binary...");
    let artifacts =
        block_on(desktop_builder.build_rust(&ctx)).context("failed to build the binary")?;

    ui::emit(
        "build.phase",
        &serde_json::json!({ "name": "build_platform" }),
    );
    spinner.start("Copying binary...");
    let final_artifacts =
        DesktopBuilder::build_platform(&ctx, &artifacts).context("failed to copy binary")?;

    spinner.stop(format!("{} Desktop binary built", style("✓").green()));

    let kind = if cfg!(target_os = "macos") {
        "app-bundle"
    } else {
        "binary"
    };
    Ok(vec![Artifact::new(
        kind,
        final_artifacts.app_binary,
        final_artifacts.size_bytes,
    )])
}

fn build_specific_platform(
    target: BuildTarget,
    options: &BuildOptions,
    output: Option<&PathBuf>,
) -> CliResult<Vec<Artifact>> {
    if target == BuildTarget::Macos && options.universal {
        return build_macos_universal(options, output);
    }

    let target_triple = target.target_triple();
    let workspace_root = std::env::current_dir()?;

    let profile = if options.release {
        Profile::Release
    } else {
        Profile::Debug
    };

    let desktop_builder = DesktopBuilder::new();

    let mut builder = BuilderContextBuilder::new(workspace_root.clone())
        .with_platform(Platform::Desktop {
            target: Some(target_triple.to_string()),
        })
        .with_target(options.cargo_target())
        .with_profile(profile);

    if target == BuildTarget::Macos {
        builder = builder.with_bundle(macos_bundle()?);
    }

    // See the matching comment in `build_desktop`: bundle-metadata parse
    // errors must still take priority over a missing `Cargo.toml`.
    ensure_flui_project(&workspace_root)?;

    if let Some(out) = output {
        builder = builder.with_output_dir(out.clone());
    }

    let ctx = builder.build();

    std::fs::create_dir_all(&ctx.output_dir)?;

    let spinner = ui::spinner();
    spinner.start(format!("Building for target: {target_triple}..."));

    ui::emit("build.phase", &serde_json::json!({ "name": "validate" }));
    spinner.start("Validating environment...");
    DesktopBuilder::validate_environment().context("environment validation failed")?;

    ui::emit("build.phase", &serde_json::json!({ "name": "build_rust" }));
    spinner.start("Building...");
    let artifacts = block_on(desktop_builder.build_rust(&ctx)).context("failed to build")?;

    ui::emit(
        "build.phase",
        &serde_json::json!({ "name": "build_platform" }),
    );
    spinner.start("Copying artifacts...");
    let final_artifacts =
        DesktopBuilder::build_platform(&ctx, &artifacts).context("failed to copy artifacts")?;

    spinner.stop(format!(
        "{} {} binary built",
        style("✓").green(),
        target_triple
    ));

    let kind = if target == BuildTarget::Macos {
        "app-bundle"
    } else {
        "binary"
    };
    Ok(vec![Artifact::new(
        kind,
        final_artifacts.app_binary,
        final_artifacts.size_bytes,
    )])
}

/// Build one macOS executable carrying both `arm64` and `x86_64` slices.
///
/// Each slice is a separate cargo invocation (there is no "fat" Rust target);
/// `lipo -create` then fuses the two Mach-O files into a single universal
/// binary. Both slices land in arch-specific scratch subdirectories, and only
/// the fused artifact is promoted to the caller's output directory.
fn build_macos_universal(
    options: &BuildOptions,
    output: Option<&PathBuf>,
) -> CliResult<Vec<Artifact>> {
    const SLICES: [&str; 2] = ["aarch64-apple-darwin", "x86_64-apple-darwin"];

    let workspace_root = std::env::current_dir()?;
    ensure_flui_project(&workspace_root)?;

    let spinner = ui::spinner();
    spinner.start("Building universal macOS binary (arm64 + x86_64)...");
    ui::remark(
        "This fuses both architecture slices into one executable with `lipo`; \
         the result is not an .app bundle, unlike a plain `flui build macos`.",
    )?;

    let profile = if options.release {
        Profile::Release
    } else {
        Profile::Debug
    };

    let mut builder = BuilderContextBuilder::new(workspace_root.clone())
        .with_platform(Platform::Desktop {
            target: Some(SLICES[0].to_string()),
        })
        .with_target(options.cargo_target())
        .with_profile(profile);

    if let Some(out) = output {
        builder = builder.with_output_dir(out.clone());
    }
    let ctx = builder.build();
    let output_dir = ctx.output_dir.clone();
    std::fs::create_dir_all(&output_dir)?;
    let scratch = output_dir.join(".slices");

    let mut slice_paths = Vec::new();
    for triple in SLICES {
        let slice_dir = scratch.join(triple);
        std::fs::create_dir_all(&slice_dir)?;

        spinner.start(format!("Building {triple}..."));
        ui::emit(
            "build.phase",
            &serde_json::json!({ "name": "build_rust", "slice": triple }),
        );

        let builder_inst = DesktopBuilder::new();
        let slice_ctx = BuilderContextBuilder::new(workspace_root.clone())
            .with_platform(Platform::Desktop {
                target: Some(triple.to_string()),
            })
            .with_target(options.cargo_target())
            .with_profile(profile)
            .with_output_dir(slice_dir)
            .build();

        DesktopBuilder::validate_environment().context("environment validation failed")?;
        let artifacts = block_on(builder_inst.build_rust(&slice_ctx))
            .with_context(|| format!("failed to build {triple}"))?;
        let final_artifacts = DesktopBuilder::build_platform(&slice_ctx, &artifacts)
            .with_context(|| format!("failed to stage {triple}"))?;
        slice_paths.push(final_artifacts.app_binary);
    }

    let fused_name = slice_paths
        .first()
        .and_then(|p| p.file_name())
        .ok_or_else(|| crate::error::CliError::BuildFailed {
            platform: "macos".to_string(),
            details: "a slice has no file name".to_string(),
        })?;
    let fused = output_dir.join(fused_name);

    ui::emit(
        "build.phase",
        &serde_json::json!({ "name": "build_platform" }),
    );
    spinner.start("Fusing slices with lipo...");
    let mut args: Vec<String> = vec!["-create".to_string()];
    args.extend(slice_paths.iter().map(|p| p.display().to_string()));
    args.push("-output".to_string());
    args.push(fused.display().to_string());
    let status = std::process::Command::new("lipo")
        .args(&args)
        .status()
        .map_err(|e| crate::error::CliError::BuildFailed {
            platform: "macos".to_string(),
            details: format!("failed to run lipo: {e}"),
        })?;
    if !status.success() {
        return Err(crate::error::CliError::BuildFailed {
            platform: "macos".to_string(),
            details: "lipo failed to fuse the arch slices".to_string(),
        });
    }

    let _ = std::fs::remove_dir_all(&scratch);
    let size_bytes = std::fs::metadata(&fused)?.len();

    spinner.stop(format!("{} Universal binary built", style("✓").green()));

    Ok(vec![
        Artifact::new("binary", fused, size_bytes).with_label("universal binary (not bundled)"),
    ])
}

#[cfg(test)]
mod bundle_tests {
    #[test]
    fn app_document_controls_display_name_and_identifier() {
        let root = tempfile::tempdir().expect("fixture");
        std::fs::write(
            root.path().join("flui.toml"),
            "[app]\nname = \"Named App\"\nversion = \"0.1.0\"\norganization = \"org.example\"\n",
        )
        .expect("config");
        let bundle = super::macos_bundle_at(root.path()).expect("valid app document");
        assert_eq!(bundle.name, "Named App");
        assert_eq!(bundle.identifier, "org.example.named-app");
    }

    #[test]
    fn only_absent_app_config_uses_directory_identity() {
        let root = tempfile::tempdir().expect("fixture");
        let bundle = super::macos_bundle_at(root.path()).expect("missing config fallback");
        assert_eq!(
            bundle.name,
            root.path().file_name().expect("name").to_string_lossy()
        );
        for text in [
            "[app",
            "[app]\nname = 42\nversion = \"0.1.0\"\norganization = \"org.example\"\n",
        ] {
            std::fs::write(root.path().join("flui.toml"), text).expect("config");
            let error =
                super::macos_bundle_at(root.path()).expect_err("present invalid config must fail");
            assert!(error.to_string().contains("failed to parse"));
        }
    }

    #[test]
    fn missing_cargo_toml_is_rejected_before_any_build_work() {
        let root = tempfile::tempdir().expect("fixture");
        let error =
            super::ensure_flui_project(root.path()).expect_err("no Cargo.toml must be rejected");
        assert!(matches!(
            error,
            crate::error::CliError::NotFluiProject { .. }
        ));
        assert!(error.to_string().contains("Cargo.toml not found"));
        std::fs::write(root.path().join("Cargo.toml"), "[package]\n").expect("manifest");
        super::ensure_flui_project(root.path()).expect("present Cargo.toml is accepted");
    }
}
