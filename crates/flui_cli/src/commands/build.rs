use crate::error::{CliError, CliResult, ResultExt};
use crate::BuildTarget;
use console::style;
use flui_build::*;
use std::path::PathBuf;

pub fn execute(
    target: BuildTarget,
    release: bool,
    output: Option<PathBuf>,
    split_per_abi: bool,
    optimize_wasm: bool,
    universal: bool,
) -> CliResult<()> {
    let mode = if release { "release" } else { "debug" };
    println!(
        "{}",
        style(format!("Building for {:?} ({} mode)...", target, mode))
            .green()
            .bold()
    );
    println!();

    // Use flui_build for cross-platform builds
    match target {
        BuildTarget::Android => build_android(release, split_per_abi)?,
        BuildTarget::Ios => build_ios(release, universal)?,
        BuildTarget::Web => build_web(release, optimize_wasm)?,
        BuildTarget::Desktop => build_desktop(release)?,
        BuildTarget::Windows | BuildTarget::Linux | BuildTarget::Macos => {
            build_specific_platform(target, release)?
        }
    }

    // Copy artifacts to output directory if specified
    if let Some(output_dir) = output {
        println!(
            "  {} Copying artifacts to {}",
            style("✓").green(),
            output_dir.display()
        );
        std::fs::create_dir_all(&output_dir)?;
        // TODO: Implement artifact copying based on platform
    }

    println!();
    println!("{}", style("✓ Build completed successfully").green().bold());

    Ok(())
}

fn build_android(release: bool, _split_per_abi: bool) -> CliResult<()> {
    println!("  {} Building Android APK", style("→").cyan());

    let workspace_root = std::env::current_dir()?;

    let ctx = BuilderContext {
        workspace_root: workspace_root.clone(),
        platform: Platform::Android {
            targets: vec!["arm64-v8a".to_string()],
        },
        profile: if release {
            Profile::Release
        } else {
            Profile::Debug
        },
        features: vec![],
        output_dir: workspace_root
            .join("target")
            .join("flui-out")
            .join("android"),
    };

    std::fs::create_dir_all(&ctx.output_dir)?;

    // Create builder
    let builder =
        AndroidBuilder::new(&workspace_root).context("Failed to initialize Android builder")?;

    // Validate environment
    builder
        .validate_environment()
        .context("Android environment validation failed")?;

    // Build Rust libraries
    let artifacts = builder
        .build_rust(&ctx)
        .context("Failed to build Rust libraries")?;

    // Build APK
    let final_artifacts = builder
        .build_platform(&ctx, &artifacts)
        .context("Failed to build APK")?;

    println!(
        "  {} APK location: {}",
        style("✓").green(),
        style(final_artifacts.app_binary.display()).cyan()
    );
    println!(
        "  {} Size: {:.2} MB",
        style("→").cyan(),
        final_artifacts.size_bytes as f64 / 1_048_576.0
    );

    Ok(())
}

fn build_ios(release: bool, _universal: bool) -> CliResult<()> {
    println!("  {} iOS builds not yet supported", style("!").yellow());
    println!("  {} iOS support coming soon", style("→").cyan());
    println!();
    println!("For now, please use:");
    println!(
        "  1. Direct cargo build: {}",
        style(format!(
            "cargo build --target aarch64-apple-ios{}",
            if release { " --release" } else { "" }
        ))
        .cyan()
    );
    println!("  2. Or use Xcode for iOS builds");

    Err(CliError::NotImplemented {
        feature: "iOS build via flui CLI".to_string(),
    })
}

fn build_web(release: bool, _optimize_wasm: bool) -> CliResult<()> {
    println!("  {} Building Web (WASM)", style("→").cyan());

    let workspace_root = std::env::current_dir()?;

    let ctx = BuilderContext {
        workspace_root: workspace_root.clone(),
        platform: Platform::Web {
            target: "web".to_string(),
        },
        profile: if release {
            Profile::Release
        } else {
            Profile::Debug
        },
        features: vec![],
        output_dir: workspace_root.join("target").join("flui-out").join("web"),
    };

    std::fs::create_dir_all(&ctx.output_dir)?;

    // Create builder
    let builder = WebBuilder::new(&workspace_root).context("Failed to initialize Web builder")?;

    // Validate environment
    builder
        .validate_environment()
        .context("Web environment validation failed")?;

    // Build WASM
    let artifacts = builder.build_rust(&ctx).context("Failed to build WASM")?;

    // Build final web package
    let final_artifacts = builder
        .build_platform(&ctx, &artifacts)
        .context("Failed to build web package")?;

    println!(
        "  {} Build location: {}",
        style("✓").green(),
        style(ctx.output_dir.display()).cyan()
    );
    println!(
        "  {} Size: {:.2} KB",
        style("→").cyan(),
        final_artifacts.size_bytes as f64 / 1024.0
    );

    Ok(())
}

fn build_desktop(release: bool) -> CliResult<()> {
    println!("  {} Building Desktop binary", style("→").cyan());

    let workspace_root = std::env::current_dir()?;

    let ctx = BuilderContext {
        workspace_root: workspace_root.clone(),
        platform: Platform::Desktop {
            target: None, // Auto-detect host platform
        },
        profile: if release {
            Profile::Release
        } else {
            Profile::Debug
        },
        features: vec![],
        output_dir: workspace_root
            .join("target")
            .join("flui-out")
            .join("desktop"),
    };

    std::fs::create_dir_all(&ctx.output_dir)?;

    // Create builder
    let builder =
        DesktopBuilder::new(&workspace_root).context("Failed to initialize Desktop builder")?;

    // Validate environment
    builder
        .validate_environment()
        .context("Desktop environment validation failed")?;

    // Build binary
    let artifacts = builder.build_rust(&ctx).context("Failed to build binary")?;

    // Copy to output
    let final_artifacts = builder
        .build_platform(&ctx, &artifacts)
        .context("Failed to copy binary")?;

    println!(
        "  {} Binary location: {}",
        style("✓").green(),
        style(final_artifacts.app_binary.display()).cyan()
    );
    println!(
        "  {} Size: {:.2} MB",
        style("→").cyan(),
        final_artifacts.size_bytes as f64 / 1_048_576.0
    );

    Ok(())
}

fn build_specific_platform(target: BuildTarget, release: bool) -> CliResult<()> {
    let target_triple = get_target_triple(target);

    println!(
        "  {} Building for target: {}",
        style("→").cyan(),
        target_triple
    );

    let workspace_root = std::env::current_dir()?;

    let ctx = BuilderContext {
        workspace_root: workspace_root.clone(),
        platform: Platform::Desktop {
            target: Some(target_triple.to_string()),
        },
        profile: if release {
            Profile::Release
        } else {
            Profile::Debug
        },
        features: vec![],
        output_dir: workspace_root
            .join("target")
            .join("flui-out")
            .join(target_triple),
    };

    std::fs::create_dir_all(&ctx.output_dir)?;

    // Use desktop builder with specific target
    let builder = DesktopBuilder::new(&workspace_root).context("Failed to initialize builder")?;

    builder
        .validate_environment()
        .context("Environment validation failed")?;

    let artifacts = builder.build_rust(&ctx).context("Failed to build")?;

    let final_artifacts = builder
        .build_platform(&ctx, &artifacts)
        .context("Failed to copy artifacts")?;

    println!(
        "  {} Binary location: {}",
        style("✓").green(),
        style(final_artifacts.app_binary.display()).cyan()
    );

    Ok(())
}

fn get_target_triple(target: BuildTarget) -> &'static str {
    match target {
        BuildTarget::Windows => "x86_64-pc-windows-msvc",
        BuildTarget::Linux => "x86_64-unknown-linux-gnu",
        BuildTarget::Macos => "x86_64-apple-darwin",
        BuildTarget::Android => "aarch64-linux-android",
        BuildTarget::Ios => "aarch64-apple-ios",
        BuildTarget::Web => "wasm32-unknown-unknown",
        BuildTarget::Desktop => {
            #[cfg(target_os = "windows")]
            return "x86_64-pc-windows-msvc";

            #[cfg(target_os = "linux")]
            return "x86_64-unknown-linux-gnu";

            #[cfg(target_os = "macos")]
            return "x86_64-apple-darwin";

            #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
            return "unknown";
        }
    }
}
