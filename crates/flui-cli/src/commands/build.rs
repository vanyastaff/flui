//! Build command for cross-platform compilation.

use crate::error::{CliResult, ResultExt};
use crate::BuildTarget;
use console::style;
use flui_build::{
    AndroidBuilder, BuildPhase, BuilderContextBuilder, DesktopBuilder, Platform, PlatformBuilder,
    Profile, ProgressManager, WebBuilder,
};
use std::path::PathBuf;

/// Build options collected into a struct to avoid excessive bool parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(dead_code)]
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
}

/// Execute the build command.
///
/// # Errors
///
/// Returns an error if the build fails for the target platform.
#[allow(clippy::fn_params_excessive_bools)]
pub fn execute(
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
        verbose: false,
    };

    let mode = if release { "release" } else { "debug" };
    cliclack::intro(style(format!(" flui build {} ", target)).on_cyan().black())?;
    cliclack::log::info(format!("Mode: {}", style(mode).cyan()))?;

    // Use flui_build for cross-platform builds
    let result = match target {
        BuildTarget::Android => build_android(&options, output.as_ref()),
        BuildTarget::Ios => build_ios(&options),
        BuildTarget::Web => build_web(&options, output.as_ref()),
        BuildTarget::Desktop => build_desktop(&options, output.as_ref()),
        BuildTarget::Windows | BuildTarget::Linux | BuildTarget::Macos => {
            build_specific_platform(target, &options, output.as_ref())
        }
    };

    if let Err(e) = result {
        cliclack::outro_cancel(format!("Build failed: {e}"))?;
        return Err(e);
    }

    cliclack::outro(style("Build completed successfully").green())?;

    Ok(())
}

/// Execute build with progress indicators from flui_build.
///
/// This function provides detailed progress tracking using indicatif progress bars.
///
/// # Errors
///
/// Returns an error if the build fails.
#[allow(dead_code)]
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
    };

    let progress_manager = ProgressManager::new();

    let result = match target {
        BuildTarget::Android => {
            build_android_with_progress(&options, output.as_ref(), &progress_manager)
        }
        BuildTarget::Ios => build_ios(&options),
        BuildTarget::Web => build_web_with_progress(&options, output.as_ref(), &progress_manager),
        BuildTarget::Desktop => {
            build_desktop_with_progress(&options, output.as_ref(), &progress_manager)
        }
        BuildTarget::Windows | BuildTarget::Linux | BuildTarget::Macos => {
            build_specific_platform_with_progress(
                target,
                &options,
                output.as_ref(),
                &progress_manager,
            )
        }
    };

    progress_manager.join();
    result
}

fn build_android(options: &BuildOptions, output: Option<&PathBuf>) -> CliResult<()> {
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
    let artifacts = android_builder
        .build_rust(&ctx)
        .context("Failed to build Rust libraries")?;

    spinner.set_message("Building APK...");
    let final_artifacts = android_builder
        .build_platform(&ctx, &artifacts)
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
    options: &BuildOptions,
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
    let artifacts = android_builder
        .build_rust(&ctx)
        .context("Failed to build Rust libraries")?;
    progress.finish_phase("Rust libraries compiled");

    // Build platform phase
    progress.start_phase(BuildPhase::BuildPlatform, Some("Building APK..."));
    progress.set_progress(70);
    let final_artifacts = android_builder
        .build_platform(&ctx, &artifacts)
        .context("Failed to build APK")?;

    progress.finish(format!(
        "APK built: {} ({:.2} MB)",
        final_artifacts.app_binary.display(),
        final_artifacts.size_bytes as f64 / 1_048_576.0
    ));

    Ok(())
}

fn build_ios(options: &BuildOptions) -> CliResult<()> {
    cliclack::log::warning("iOS builds not yet supported")?;

    let release_flag = if options.release { " --release" } else { "" };
    let workaround = format!(
        "{}\n  {}",
        style("Workaround:").bold(),
        style(format!(
            "cargo build --target aarch64-apple-ios{release_flag}"
        ))
        .dim(),
    );
    cliclack::note("iOS Support", workaround)?;

    Ok(())
}

fn build_web(options: &BuildOptions, output: Option<&PathBuf>) -> CliResult<()> {
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
    let artifacts = web_builder
        .build_rust(&ctx)
        .context("Failed to build WASM")?;

    spinner.set_message("Building web package...");
    let final_artifacts = web_builder
        .build_platform(&ctx, &artifacts)
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
    options: &BuildOptions,
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
    let artifacts = web_builder
        .build_rust(&ctx)
        .context("Failed to build WASM")?;
    progress.finish_phase("WASM compiled");

    // Build platform phase
    progress.start_phase(BuildPhase::BuildPlatform, Some("Packaging web assets..."));
    progress.set_progress(70);
    let final_artifacts = web_builder
        .build_platform(&ctx, &artifacts)
        .context("Failed to build web package")?;

    progress.finish(format!(
        "Web package built: {} ({:.2} KB)",
        ctx.output_dir.display(),
        final_artifacts.size_bytes as f64 / 1024.0
    ));

    Ok(())
}

fn build_desktop(options: &BuildOptions, output: Option<&PathBuf>) -> CliResult<()> {
    let spinner = cliclack::spinner();
    spinner.start("Building Desktop binary...");

    let workspace_root = std::env::current_dir()?;
    let profile = if options.release {
        Profile::Release
    } else {
        Profile::Debug
    };

    let desktop_builder =
        DesktopBuilder::new(&workspace_root).context("Failed to initialize Desktop builder")?;

    let mut builder = BuilderContextBuilder::new(workspace_root)
        .with_platform(Platform::Desktop { target: None })
        .with_profile(profile);

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
    let artifacts = desktop_builder
        .build_rust(&ctx)
        .context("Failed to build binary")?;

    spinner.set_message("Copying binary...");
    let final_artifacts = desktop_builder
        .build_platform(&ctx, &artifacts)
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
    options: &BuildOptions,
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

    let desktop_builder =
        DesktopBuilder::new(&workspace_root).context("Failed to initialize Desktop builder")?;

    let mut builder = BuilderContextBuilder::new(workspace_root)
        .with_platform(Platform::Desktop { target: None })
        .with_profile(profile);

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
    let artifacts = desktop_builder
        .build_rust(&ctx)
        .context("Failed to build binary")?;
    progress.finish_phase("Binary compiled");

    // Build platform phase
    progress.start_phase(BuildPhase::BuildPlatform, Some("Copying artifacts..."));
    progress.set_progress(70);
    let final_artifacts = desktop_builder
        .build_platform(&ctx, &artifacts)
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
    options: &BuildOptions,
    output: Option<&PathBuf>,
) -> CliResult<()> {
    let target_triple = target.target_triple();

    let spinner = cliclack::spinner();
    spinner.start(format!("Building for target: {}...", target_triple));

    let workspace_root = std::env::current_dir()?;
    let profile = if options.release {
        Profile::Release
    } else {
        Profile::Debug
    };

    let desktop_builder =
        DesktopBuilder::new(&workspace_root).context("Failed to initialize builder")?;

    let mut builder = BuilderContextBuilder::new(workspace_root)
        .with_platform(Platform::Desktop {
            target: Some(target_triple.to_string()),
        })
        .with_profile(profile);

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
    let artifacts = desktop_builder
        .build_rust(&ctx)
        .context("Failed to build")?;

    spinner.set_message("Copying artifacts...");
    let final_artifacts = desktop_builder
        .build_platform(&ctx, &artifacts)
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

fn build_specific_platform_with_progress(
    target: BuildTarget,
    options: &BuildOptions,
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

    let desktop_builder =
        DesktopBuilder::new(&workspace_root).context("Failed to initialize builder")?;

    let mut builder = BuilderContextBuilder::new(workspace_root)
        .with_platform(Platform::Desktop {
            target: Some(target_triple.to_string()),
        })
        .with_profile(profile);

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
    let artifacts = desktop_builder
        .build_rust(&ctx)
        .context("Failed to build")?;
    progress.finish_phase("Compiled");

    // Build platform phase
    progress.start_phase(BuildPhase::BuildPlatform, Some("Copying artifacts..."));
    progress.set_progress(70);
    let final_artifacts = desktop_builder
        .build_platform(&ctx, &artifacts)
        .context("Failed to copy artifacts")?;

    progress.finish(format!(
        "{} binary built: {}",
        target_triple,
        final_artifacts.app_binary.display()
    ));

    Ok(())
}
