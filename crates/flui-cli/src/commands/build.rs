//! Build command for cross-platform compilation.

use crate::BuildTarget;
use crate::error::{CliResult, ResultExt};
use console::style;
use flui_build::platform::BuildUnit as CargoBuildUnit;
use flui_build::{
    AndroidBuilder, AppBundle, BuildPhase, BuilderContextBuilder, DesktopBuilder, IOSBuilder,
    Platform, PlatformBuilder, Profile, ProgressManager, WebBuilder,
};
use std::path::PathBuf;

/// Build options collected into a struct to avoid excessive bool parameters.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BuildOptions {
    /// Build in release mode.
    pub release: bool,
    /// Android: Create separate APKs per ABI.
    pub split_per_abi: bool,
    /// Web: Optimize WASM size.
    pub optimize_wasm: bool,
    /// iOS: Build universal binary.
    pub universal: bool,
    /// Use verbose output with progress bars.
    pub verbose: bool,
    /// Build a named example rather than the current package's binary.
    pub example: Option<String>,
    /// Build a named workspace package's binary.
    pub package: Option<String>,
}

impl BuildOptions {
    /// Resolve the cargo unit this build selects.
    ///
    /// `--example` wins over `--package` when both are given (an example is the
    /// more specific instruction), and neither given is the generated-project
    /// default: the current directory's own binary.
    fn cargo_target(&self) -> CargoBuildUnit {
        match (&self.example, &self.package) {
            (Some(example), _) => CargoBuildUnit::Example(example.clone()),
            (None, Some(package)) => CargoBuildUnit::Package(package.clone()),
            (None, None) => CargoBuildUnit::DefaultBinary,
        }
    }
}

/// Execute the build command.
///
/// # Errors
///
/// Returns an error if the build fails for the target platform.
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "mirrors clap argument structure"
)]
#[expect(clippy::too_many_arguments, reason = "mirrors clap argument structure")]
pub fn execute(
    target: BuildTarget,
    release: bool,
    output: Option<PathBuf>,
    split_per_abi: bool,
    optimize_wasm: bool,
    universal: bool,
    example: Option<String>,
    package: Option<String>,
) -> CliResult<()> {
    let options = BuildOptions {
        release,
        split_per_abi,
        optimize_wasm,
        universal,
        verbose: false,
        example,
        package,
    };

    let mode = if release { "release" } else { "debug" };
    cliclack::intro(style(format!(" flui build {target} ")).on_cyan().black())?;
    cliclack::log::info(format!("Mode: {}", style(mode).cyan()))?;

    // `flui-build`'s builders shell out through `tokio::process`, whose
    // `Command::status()` needs a Tokio reactor in scope on this thread.
    // `pollster` only drives the future; it installs no reactor, so every
    // build path panicked with "there is no reactor running" until this guard
    // existed. A multi-thread runtime is what keeps the reactor driven while
    // the main thread is parked inside `pollster::block_on` — the worker
    // threads service the IO/signal driver that child-process reaping needs,
    // which the current-thread flavour would leave undriven. Entering (not
    // `block_on`) keeps the `pollster::block_on` call sites unchanged.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("Failed to start the build runtime")?;
    let _reactor = runtime.enter();

    // Use flui_build for cross-platform builds
    let result = match target {
        BuildTarget::Android => build_android(options, output.as_ref()),
        BuildTarget::Ios => build_ios(options, output.as_ref()),
        BuildTarget::Web => build_web(options, output.as_ref()),
        BuildTarget::Desktop => build_desktop(options, output.as_ref()),
        BuildTarget::Windows | BuildTarget::Linux | BuildTarget::Macos => {
            build_specific_platform(target, options, output.as_ref())
        }
    };

    if let Err(e) = result {
        cliclack::outro_cancel(format!("Build failed: {e}"))?;
        return Err(e);
    }

    cliclack::outro(style("Build completed successfully").green())?;

    Ok(())
}

/// Resolve application-bundle metadata for a macOS build, from `flui.toml`.
///
/// A `.app` needs a display name and a bundle identifier; `flui.toml`'s
/// `[app].name`/`[app].organization` are exactly those. When the file is absent
/// (e.g. building an in-repo example), the directory name stands in — a bundle
/// still gets staged, with a derived identifier, rather than the build silently
/// degrading to a bare binary.
fn macos_bundle() -> CliResult<AppBundle> {
    let root = std::env::current_dir().context("Failed to resolve application directory")?;
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
            format!("Failed to inspect {}", manifest.display()),
        )),
    }
}

/// Execute build with progress indicators from `flui_build`.
///
/// This function provides detailed progress tracking using indicatif progress bars.
///
/// # Errors
///
/// Returns an error if the build fails.
#[expect(dead_code, reason = "progress-based build for future verbose mode")]
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "mirrors clap argument structure"
)]
pub fn execute_with_progress(
    target: BuildTarget,
    release: bool,
    output: Option<PathBuf>,
    split_per_abi: bool,
    optimize_wasm: bool,
    universal: bool,
) -> CliResult<()> {
    let options = BuildOptions {
        release,
        split_per_abi,
        optimize_wasm,
        universal,
        verbose: true,
        example: None,
        package: None,
    };

    let progress_manager = ProgressManager::new();

    let result = match target {
        BuildTarget::Android => {
            build_android_with_progress(options, output.as_ref(), &progress_manager)
        }
        BuildTarget::Ios => build_ios(options, output.as_ref()),
        BuildTarget::Web => build_web_with_progress(options, output.as_ref(), &progress_manager),
        BuildTarget::Desktop => {
            build_desktop_with_progress(options, output.as_ref(), &progress_manager)
        }
        BuildTarget::Windows | BuildTarget::Linux | BuildTarget::Macos => {
            build_specific_platform_with_progress(
                target,
                options,
                output.as_ref(),
                &progress_manager,
            )
        }
    };

    progress_manager.join();
    result
}

fn build_android(options: BuildOptions, output: Option<&PathBuf>) -> CliResult<()> {
    let spinner = cliclack::spinner();
    spinner.start("Building Android APK...");

    let workspace_root = std::env::current_dir()?;
    let profile = if options.release {
        Profile::Release
    } else {
        Profile::Debug
    };

    let android_builder =
        AndroidBuilder::new(&workspace_root).context("Failed to initialize Android builder")?;

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

    spinner.set_message("Validating Android environment...");
    android_builder
        .validate_environment()
        .context("Android environment validation failed")?;

    spinner.set_message("Building Rust libraries...");
    let artifacts = pollster::block_on(android_builder.build_rust(&ctx))
        .context("Failed to build Rust libraries")?;

    spinner.set_message("Building APK...");
    let final_artifacts = pollster::block_on(android_builder.build_platform(&ctx, &artifacts))
        .context("Failed to build APK")?;

    spinner.stop(format!("{} Android APK built", style("✓").green()));

    cliclack::log::success(format!(
        "APK location: {}",
        final_artifacts.app_binary.display()
    ))?;
    cliclack::log::info(format!(
        "Size: {:.2} MB",
        final_artifacts.size_bytes as f64 / 1_048_576.0
    ))?;

    Ok(())
}

fn build_android_with_progress(
    options: BuildOptions,
    output: Option<&PathBuf>,
    manager: &ProgressManager,
) -> CliResult<()> {
    let mut progress = manager.create_build("Android");

    let workspace_root = std::env::current_dir()?;
    let profile = if options.release {
        Profile::Release
    } else {
        Profile::Debug
    };

    let android_builder =
        AndroidBuilder::new(&workspace_root).context("Failed to initialize Android builder")?;

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

    // Validate phase
    progress.start_phase(BuildPhase::Validate, Some("Checking Android SDK..."));
    progress.set_progress(10);
    android_builder
        .validate_environment()
        .context("Android environment validation failed")?;
    progress.finish_phase("Environment validated");

    // Build Rust phase
    progress.start_phase(BuildPhase::BuildRust, Some("Compiling Rust libraries..."));
    progress.set_progress(30);
    let artifacts = pollster::block_on(android_builder.build_rust(&ctx))
        .context("Failed to build Rust libraries")?;
    progress.finish_phase("Rust libraries compiled");

    // Build platform phase
    progress.start_phase(BuildPhase::BuildPlatform, Some("Building APK..."));
    progress.set_progress(70);
    let final_artifacts = pollster::block_on(android_builder.build_platform(&ctx, &artifacts))
        .context("Failed to build APK")?;

    progress.finish(format!(
        "APK built: {} ({:.2} MB)",
        final_artifacts.app_binary.display(),
        final_artifacts.size_bytes as f64 / 1_048_576.0
    ));

    Ok(())
}

fn build_ios(options: BuildOptions, output: Option<&PathBuf>) -> CliResult<()> {
    let spinner = cliclack::spinner();
    spinner.start("Building iOS libraries...");

    let workspace_root = std::env::current_dir()?;
    let profile = if options.release {
        Profile::Release
    } else {
        Profile::Debug
    };

    // Default is the device arm64 slice; `--universal` adds the simulator
    // slice so one artifact serves both. A caller that wants only one names
    // it here rather than the builder guessing.
    let targets = if options.universal {
        vec![
            "aarch64-apple-ios".to_string(),
            "aarch64-apple-ios-sim".to_string(),
        ]
    } else {
        vec!["aarch64-apple-ios".to_string()]
    };

    let ios_builder =
        IOSBuilder::new(&workspace_root).context("Failed to initialize iOS builder")?;

    let mut builder = BuilderContextBuilder::new(workspace_root)
        .with_platform(Platform::IOS { targets })
        .with_target(options.cargo_target())
        .with_profile(profile);

    if let Some(out) = output {
        builder = builder.with_output_dir(out.clone());
    }

    let ctx = builder.build();

    spinner.set_message("Validating iOS environment...");
    ios_builder
        .validate_environment()
        .context("iOS environment validation failed")?;

    spinner.set_message("Building iOS libraries...");
    let artifacts = pollster::block_on(ios_builder.build_rust(&ctx))
        .context("Failed to build iOS libraries")?;

    spinner.set_message("Building iOS app...");
    let final_artifacts = pollster::block_on(ios_builder.build_platform(&ctx, &artifacts))
        .context("Failed to build iOS app")?;

    spinner.stop(format!("{} iOS build finished", style("✓").green()));

    cliclack::log::success(format!(
        "Artifact: {}",
        final_artifacts.app_binary.display()
    ))?;
    cliclack::log::info(format!(
        "Size: {:.2} MB",
        final_artifacts.size_bytes as f64 / 1_048_576.0
    ))?;

    Ok(())
}

fn build_web(options: BuildOptions, output: Option<&PathBuf>) -> CliResult<()> {
    let spinner = cliclack::spinner();
    spinner.start("Building Web (WASM)...");

    let workspace_root = std::env::current_dir()?;
    let profile = if options.release {
        Profile::Release
    } else {
        Profile::Debug
    };

    let web_builder =
        WebBuilder::new(&workspace_root).context("Failed to initialize Web builder")?;

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

    spinner.set_message("Validating Web environment...");
    web_builder
        .validate_environment()
        .context("Web environment validation failed")?;

    spinner.set_message("Building WASM...");
    let artifacts =
        pollster::block_on(web_builder.build_rust(&ctx)).context("Failed to build WASM")?;

    spinner.set_message("Building web package...");
    let final_artifacts = pollster::block_on(web_builder.build_platform(&ctx, &artifacts))
        .context("Failed to build web package")?;

    spinner.stop(format!("{} Web package built", style("✓").green()));

    cliclack::log::success(format!("Build location: {}", ctx.output_dir.display()))?;
    cliclack::log::info(format!(
        "Size: {:.2} KB",
        final_artifacts.size_bytes as f64 / 1024.0
    ))?;

    Ok(())
}

fn build_web_with_progress(
    options: BuildOptions,
    output: Option<&PathBuf>,
    manager: &ProgressManager,
) -> CliResult<()> {
    let mut progress = manager.create_build("Web");

    let workspace_root = std::env::current_dir()?;
    let profile = if options.release {
        Profile::Release
    } else {
        Profile::Debug
    };

    let web_builder =
        WebBuilder::new(&workspace_root).context("Failed to initialize Web builder")?;

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

    // Validate phase
    progress.start_phase(BuildPhase::Validate, Some("Checking wasm-pack..."));
    progress.set_progress(10);
    web_builder
        .validate_environment()
        .context("Web environment validation failed")?;
    progress.finish_phase("Environment validated");

    // Build Rust phase
    progress.start_phase(BuildPhase::BuildRust, Some("Compiling to WASM..."));
    progress.set_progress(30);
    let artifacts =
        pollster::block_on(web_builder.build_rust(&ctx)).context("Failed to build WASM")?;
    progress.finish_phase("WASM compiled");

    // Build platform phase
    progress.start_phase(BuildPhase::BuildPlatform, Some("Packaging web assets..."));
    progress.set_progress(70);
    let final_artifacts = pollster::block_on(web_builder.build_platform(&ctx, &artifacts))
        .context("Failed to build web package")?;

    progress.finish(format!(
        "Web package built: {} ({:.2} KB)",
        ctx.output_dir.display(),
        final_artifacts.size_bytes as f64 / 1024.0
    ));

    Ok(())
}

fn build_desktop(options: BuildOptions, output: Option<&PathBuf>) -> CliResult<()> {
    let spinner = cliclack::spinner();
    spinner.start("Building Desktop binary...");

    let workspace_root = std::env::current_dir()?;
    let profile = if options.release {
        Profile::Release
    } else {
        Profile::Debug
    };

    let desktop_builder = DesktopBuilder::new();

    let mut builder = BuilderContextBuilder::new(workspace_root)
        .with_platform(Platform::Desktop { target: None })
        .with_target(options.cargo_target())
        .with_profile(profile);

    // `Desktop` builds for the host; on macOS that means staging a `.app`.
    #[cfg(target_os = "macos")]
    {
        builder = builder.with_bundle(macos_bundle()?);
    }

    if let Some(out) = output {
        builder = builder.with_output_dir(out.clone());
    }

    let ctx = builder.build();

    std::fs::create_dir_all(&ctx.output_dir)?;

    spinner.set_message("Validating Desktop environment...");
    desktop_builder
        .validate_environment()
        .context("Desktop environment validation failed")?;

    spinner.set_message("Building binary...");
    let artifacts =
        pollster::block_on(desktop_builder.build_rust(&ctx)).context("Failed to build binary")?;

    spinner.set_message("Copying binary...");
    let final_artifacts = pollster::block_on(desktop_builder.build_platform(&ctx, &artifacts))
        .context("Failed to copy binary")?;

    spinner.stop(format!("{} Desktop binary built", style("✓").green()));

    cliclack::log::success(format!(
        "Binary location: {}",
        final_artifacts.app_binary.display()
    ))?;
    cliclack::log::info(format!(
        "Size: {:.2} MB",
        final_artifacts.size_bytes as f64 / 1_048_576.0
    ))?;

    Ok(())
}

fn build_desktop_with_progress(
    options: BuildOptions,
    output: Option<&PathBuf>,
    manager: &ProgressManager,
) -> CliResult<()> {
    let mut progress = manager.create_build("Desktop");

    let workspace_root = std::env::current_dir()?;
    let profile = if options.release {
        Profile::Release
    } else {
        Profile::Debug
    };

    let desktop_builder = DesktopBuilder::new();

    let mut builder = BuilderContextBuilder::new(workspace_root)
        .with_platform(Platform::Desktop { target: None })
        .with_target(options.cargo_target())
        .with_profile(profile);

    #[cfg(target_os = "macos")]
    {
        builder = builder.with_bundle(macos_bundle()?);
    }

    if let Some(out) = output {
        builder = builder.with_output_dir(out.clone());
    }

    let ctx = builder.build();
    std::fs::create_dir_all(&ctx.output_dir)?;

    // Validate phase
    progress.start_phase(BuildPhase::Validate, Some("Checking build tools..."));
    progress.set_progress(10);
    desktop_builder
        .validate_environment()
        .context("Desktop environment validation failed")?;
    progress.finish_phase("Environment validated");

    // Build Rust phase
    progress.start_phase(BuildPhase::BuildRust, Some("Compiling binary..."));
    progress.set_progress(30);
    let artifacts =
        pollster::block_on(desktop_builder.build_rust(&ctx)).context("Failed to build binary")?;
    progress.finish_phase("Binary compiled");

    // Build platform phase
    progress.start_phase(BuildPhase::BuildPlatform, Some("Copying artifacts..."));
    progress.set_progress(70);
    let final_artifacts = pollster::block_on(desktop_builder.build_platform(&ctx, &artifacts))
        .context("Failed to copy binary")?;

    progress.finish(format!(
        "Desktop binary built: {} ({:.2} MB)",
        final_artifacts.app_binary.display(),
        final_artifacts.size_bytes as f64 / 1_048_576.0
    ));

    Ok(())
}

fn build_specific_platform(
    target: BuildTarget,
    options: BuildOptions,
    output: Option<&PathBuf>,
) -> CliResult<()> {
    // A macOS universal binary is two single-arch builds combined with `lipo`;
    // it is not a distinct cargo target, so it takes a dedicated path rather
    // than being forced through the single-triple flow below.
    if target == BuildTarget::Macos && options.universal {
        return build_macos_universal(options, output);
    }

    let target_triple = target.target_triple();

    let spinner = cliclack::spinner();
    spinner.start(format!("Building for target: {target_triple}..."));

    let workspace_root = std::env::current_dir()?;
    let profile = if options.release {
        Profile::Release
    } else {
        Profile::Debug
    };

    let desktop_builder = DesktopBuilder::new();

    let mut builder = BuilderContextBuilder::new(workspace_root)
        .with_platform(Platform::Desktop {
            target: Some(target_triple.to_string()),
        })
        .with_target(options.cargo_target())
        .with_profile(profile);

    // `flui build macos` stages a `.app`; other triples keep the bare binary.
    if target == BuildTarget::Macos {
        builder = builder.with_bundle(macos_bundle()?);
    }

    if let Some(out) = output {
        builder = builder.with_output_dir(out.clone());
    }

    let ctx = builder.build();

    std::fs::create_dir_all(&ctx.output_dir)?;

    spinner.set_message("Validating environment...");
    desktop_builder
        .validate_environment()
        .context("Environment validation failed")?;

    spinner.set_message("Building...");
    let artifacts =
        pollster::block_on(desktop_builder.build_rust(&ctx)).context("Failed to build")?;

    spinner.set_message("Copying artifacts...");
    let final_artifacts = pollster::block_on(desktop_builder.build_platform(&ctx, &artifacts))
        .context("Failed to copy artifacts")?;

    spinner.stop(format!(
        "{} {} binary built",
        style("✓").green(),
        target_triple
    ));

    cliclack::log::success(format!(
        "Binary location: {}",
        final_artifacts.app_binary.display()
    ))?;

    Ok(())
}

/// Build one macOS executable carrying both `arm64` and `x86_64` slices.
///
/// Each slice is a separate cargo invocation (there is no "fat" Rust target);
/// `lipo -create` then fuses the two Mach-O files into a single universal
/// binary. Both slices land in arch-specific scratch subdirectories, and only
/// the fused artifact is promoted to the caller's output directory.
fn build_macos_universal(options: BuildOptions, output: Option<&PathBuf>) -> CliResult<()> {
    const SLICES: [&str; 2] = ["aarch64-apple-darwin", "x86_64-apple-darwin"];

    let spinner = cliclack::spinner();
    spinner.start("Building universal macOS binary (arm64 + x86_64)...");

    let workspace_root = std::env::current_dir()?;
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

        spinner.set_message(format!("Building {triple}..."));

        let builder_inst = DesktopBuilder::new();
        let slice_ctx = BuilderContextBuilder::new(workspace_root.clone())
            .with_platform(Platform::Desktop {
                target: Some(triple.to_string()),
            })
            .with_target(options.cargo_target())
            .with_profile(profile)
            .with_output_dir(slice_dir)
            .build();

        builder_inst
            .validate_environment()
            .context("Environment validation failed")?;
        let artifacts = pollster::block_on(builder_inst.build_rust(&slice_ctx))
            .with_context(|| format!("Failed to build {triple}"))?;
        let final_artifacts =
            pollster::block_on(builder_inst.build_platform(&slice_ctx, &artifacts))
                .with_context(|| format!("Failed to stage {triple}"))?;
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

    spinner.set_message("Fusing slices with lipo...");
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

    spinner.stop(format!("{} Universal binary built", style("✓").green()));
    cliclack::log::success(format!("Binary location: {}", fused.display()))?;

    Ok(())
}

fn build_specific_platform_with_progress(
    target: BuildTarget,
    options: BuildOptions,
    output: Option<&PathBuf>,
    manager: &ProgressManager,
) -> CliResult<()> {
    let target_triple = target.target_triple();
    let mut progress = manager.create_build(target_triple);

    let workspace_root = std::env::current_dir()?;
    let profile = if options.release {
        Profile::Release
    } else {
        Profile::Debug
    };

    let desktop_builder = DesktopBuilder::new();

    let mut builder = BuilderContextBuilder::new(workspace_root)
        .with_platform(Platform::Desktop {
            target: Some(target_triple.to_string()),
        })
        .with_target(options.cargo_target())
        .with_profile(profile);

    if target == BuildTarget::Macos {
        builder = builder.with_bundle(macos_bundle()?);
    }

    if let Some(out) = output {
        builder = builder.with_output_dir(out.clone());
    }

    let ctx = builder.build();
    std::fs::create_dir_all(&ctx.output_dir)?;

    // Validate phase
    progress.start_phase(BuildPhase::Validate, Some("Checking build tools..."));
    progress.set_progress(10);
    desktop_builder
        .validate_environment()
        .context("Environment validation failed")?;
    progress.finish_phase("Environment validated");

    // Build Rust phase
    progress.start_phase(BuildPhase::BuildRust, Some("Compiling..."));
    progress.set_progress(30);
    let artifacts =
        pollster::block_on(desktop_builder.build_rust(&ctx)).context("Failed to build")?;
    progress.finish_phase("Compiled");

    // Build platform phase
    progress.start_phase(BuildPhase::BuildPlatform, Some("Copying artifacts..."));
    progress.set_progress(70);
    let final_artifacts = pollster::block_on(desktop_builder.build_platform(&ctx, &artifacts))
        .context("Failed to copy artifacts")?;

    progress.finish(format!(
        "{} binary built: {}",
        target_triple,
        final_artifacts.app_binary.display()
    ));

    Ok(())
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
            assert!(error.to_string().contains("Failed to parse"));
        }
    }
}
