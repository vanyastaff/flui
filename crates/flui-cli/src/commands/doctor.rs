//! Doctor command for checking FLUI environment setup.
//!
//! Verifies that all required tools and SDKs are properly installed.

use crate::error::CliResult;
use console::style;
use std::fmt::Write;
use std::process::Command;

/// Execute the doctor command.
///
/// # Arguments
///
/// * `verbose` - Show detailed information
/// * `android` - Check only Android toolchain
/// * `ios` - Check only iOS toolchain
/// * `web` - Check only Web toolchain
#[expect(
    clippy::fn_params_excessive_bools,
    reason = "matches CLI arg structure"
)]
pub fn execute(verbose: bool, android: bool, ios: bool, web: bool) -> CliResult<()> {
    cliclack::intro(style(" flui doctor ").on_cyan().black())?;

    let mut results = Vec::with_capacity(10);
    let mut all_ok = true;

    // Core checks (always run)
    results.push(check_rust(verbose, &mut all_ok));
    results.push(check_cargo(&mut all_ok));
    results.push(check_flui());

    // Platform-specific checks
    let check_all = !android && !ios && !web;

    if android || check_all {
        results.push(check_java(verbose, &mut all_ok));
        results.push(check_android(verbose, &mut all_ok));
    }

    if ios || check_all {
        #[cfg(target_os = "macos")]
        {
            results.push(check_ios(verbose, &mut all_ok));
        }
        #[cfg(not(target_os = "macos"))]
        {
            if ios {
                results.push(format!(
                    "{} iOS: Not available on non-macOS",
                    style("⚠").yellow()
                ));
            }
        }
    }

    if web || check_all {
        results.push(check_web(&mut all_ok));
    }

    // Check wgpu support
    results.push(check_wgpu(verbose));

    // Display all results
    let output = results.join("\n");
    cliclack::note("Environment Check", output)?;

    if all_ok {
        cliclack::outro(style("All checks passed!").green())?;
    } else {
        cliclack::outro_cancel("Some checks failed. Please fix the issues above.")?;
    }

    Ok(())
}

fn check_rust(verbose: bool, all_ok: &mut bool) -> String {
    let Ok(output) = Command::new("rustc").arg("--version").output() else {
        *all_ok = false;
        return format!(
            "{} Rust: {}\n  {}",
            style("✗").red(),
            style("Not found").red(),
            style("Install from https://rustup.rs/").dim()
        );
    };

    let version = String::from_utf8_lossy(&output.stdout);
    let mut result = format!(
        "{} Rust: {}",
        style("✓").green(),
        style(version.trim()).cyan()
    );

    if verbose {
        if let Ok(path) = which::which("rustc") {
            write!(result, "\n  Path: {}", style(path.display()).dim()).ok();
        }
    }

    result
}

fn check_cargo(all_ok: &mut bool) -> String {
    let Ok(output) = Command::new("cargo").arg("--version").output() else {
        *all_ok = false;
        return format!("{} Cargo: {}", style("✗").red(), style("Not found").red());
    };

    let version = String::from_utf8_lossy(&output.stdout);
    format!(
        "{} Cargo: {}",
        style("✓").green(),
        style(version.trim()).cyan()
    )
}

fn check_flui() -> String {
    let version = env!("CARGO_PKG_VERSION");
    format!(
        "{} FLUI CLI: {}",
        style("✓").green(),
        style(format!("v{version}")).cyan()
    )
}

fn check_java(verbose: bool, all_ok: &mut bool) -> String {
    let Ok(output) = Command::new("java").arg("-version").output() else {
        *all_ok = false;
        return format!(
            "{} Java: {}\n  {}",
            style("✗").red(),
            style("Not found").red(),
            style("Download from https://adoptium.net/").dim()
        );
    };

    let version_output = String::from_utf8_lossy(&output.stderr);
    let Some(line) = version_output.lines().next() else {
        return format!(
            "{} Java: {}",
            style("⚠").yellow(),
            style("Version not detected").yellow()
        );
    };

    let mut result = format!("{} Java: {}", style("✓").green(), style(line.trim()).cyan());

    if verbose {
        if let Ok(java_home) = std::env::var("JAVA_HOME") {
            write!(result, "\n  JAVA_HOME: {}", style(java_home).dim()).ok();
        }
    }

    if version_output.contains("version \"1.8") {
        write!(result, "\n  {}", style("⚠ Java 11+ recommended").yellow()).ok();
    }

    result
}

fn check_android(verbose: bool, all_ok: &mut bool) -> String {
    let mut results = Vec::with_capacity(3);

    // Check ANDROID_HOME
    if let Ok(android_home) = std::env::var("ANDROID_HOME") {
        let sdk_path = std::path::Path::new(&android_home);
        if sdk_path.exists() {
            results.push(format!(
                "{} Android SDK: {}",
                style("✓").green(),
                style(&android_home).cyan()
            ));

            if verbose {
                let ndk_path = sdk_path.join("ndk");
                if ndk_path.exists() {
                    results.push(format!("  {} NDK installed", style("✓").green()));
                } else {
                    results.push(format!("  {} NDK not found", style("⚠").yellow()));
                }
            }
        } else {
            *all_ok = false;
            results.push(format!(
                "{} Android SDK: {}",
                style("⚠").yellow(),
                style("Path not found").yellow()
            ));
        }
    } else {
        *all_ok = false;
        results.push(format!(
            "{} Android SDK: {}\n  {}",
            style("⚠").yellow(),
            style("Not configured").yellow(),
            style("Set ANDROID_HOME environment variable").dim()
        ));
    }

    // Check Android Rust targets
    results.push(check_android_targets(all_ok));

    results.join("\n")
}

fn check_android_targets(all_ok: &mut bool) -> String {
    if let Ok(output) = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
    {
        let targets = String::from_utf8_lossy(&output.stdout);
        let android_targets = [
            "aarch64-linux-android",
            "armv7-linux-androideabi",
            "i686-linux-android",
            "x86_64-linux-android",
        ];

        let missing: Vec<&str> = android_targets
            .iter()
            .filter(|t| !targets.contains(*t))
            .copied()
            .collect();

        if missing.is_empty() {
            format!(
                "{} Android targets: {}",
                style("✓").green(),
                style("All installed").cyan()
            )
        } else {
            *all_ok = false;
            let mut result = format!(
                "{} Android targets: {}",
                style("⚠").yellow(),
                style("Missing").yellow()
            );
            for target in &missing {
                write!(result, "\n  {} {}", style("⚠").yellow(), target).ok();
            }
            write!(
                result,
                "\n  {}",
                style(format!("rustup target add {}", missing.join(" "))).dim()
            )
            .ok();
            result
        }
    } else {
        String::new()
    }
}

#[cfg(target_os = "macos")]
fn check_ios(verbose: bool, all_ok: &mut bool) -> String {
    let mut results = Vec::with_capacity(3);

    match Command::new("xcode-select").arg("-p").output() {
        Ok(output) => {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout);
                results.push(format!(
                    "{} Xcode: {}",
                    style("✓").green(),
                    style("Installed").cyan()
                ));

                if verbose {
                    results.push(format!("  Path: {}", style(path.trim()).dim()));
                }
            } else {
                *all_ok = false;
                results.push(format!(
                    "{} Xcode: {}\n  {}",
                    style("✗").red(),
                    style("Not found").red(),
                    style("xcode-select --install").dim()
                ));
            }
        }
        Err(_) => {
            *all_ok = false;
            results.push(format!(
                "{} Xcode: {}",
                style("✗").red(),
                style("Not found").red()
            ));
        }
    }

    results.push(check_ios_targets(all_ok));
    results.join("\n")
}

#[cfg(target_os = "macos")]
fn check_ios_targets(all_ok: &mut bool) -> String {
    if let Ok(output) = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
    {
        let targets = String::from_utf8_lossy(&output.stdout);
        let ios_targets = [
            "aarch64-apple-ios",
            "aarch64-apple-ios-sim",
            "x86_64-apple-ios",
        ];

        let missing: Vec<&str> = ios_targets
            .iter()
            .filter(|t| !targets.contains(*t))
            .copied()
            .collect();

        if missing.is_empty() {
            format!(
                "{} iOS targets: {}",
                style("✓").green(),
                style("All installed").cyan()
            )
        } else {
            *all_ok = false;
            let mut result = format!(
                "{} iOS targets: {}",
                style("⚠").yellow(),
                style("Missing").yellow()
            );
            for target in &missing {
                result.push_str(&format!("\n  {} {}", style("⚠").yellow(), target));
            }
            result
        }
    } else {
        String::new()
    }
}

#[cfg(not(target_os = "macos"))]
#[expect(dead_code, reason = "cross-platform stub, called only on macOS")]
fn check_ios(_verbose: bool, _all_ok: &mut bool) -> String {
    String::new()
}

fn check_web(all_ok: &mut bool) -> String {
    let mut results = Vec::with_capacity(2);

    // Check wasm-pack
    match Command::new("wasm-pack").arg("--version").output() {
        Ok(output) => {
            let version = String::from_utf8_lossy(&output.stdout);
            results.push(format!(
                "{} wasm-pack: {}",
                style("✓").green(),
                style(version.trim()).cyan()
            ));
        }
        Err(_) => {
            results.push(format!(
                "{} wasm-pack: {}\n  {}",
                style("⚠").yellow(),
                style("Not found (optional)").yellow(),
                style("cargo install wasm-pack").dim()
            ));
        }
    }

    // Check wasm32 target
    if let Ok(output) = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
    {
        let targets = String::from_utf8_lossy(&output.stdout);
        if targets.contains("wasm32-unknown-unknown") {
            results.push(format!(
                "{} WASM target: {}",
                style("✓").green(),
                style("Installed").cyan()
            ));
        } else {
            *all_ok = false;
            results.push(format!(
                "{} WASM target: {}\n  {}",
                style("⚠").yellow(),
                style("Not installed").yellow(),
                style("rustup target add wasm32-unknown-unknown").dim()
            ));
        }
    }

    results.join("\n")
}

fn check_wgpu(verbose: bool) -> String {
    let mut result = format!("{} wgpu: {}", style("✓").green(), style("Available").cyan());

    if verbose {
        write!(
            result,
            "\n  {}",
            style("Provided via Rust dependencies").dim()
        )
        .ok();
    }

    result
}
