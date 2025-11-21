use anyhow::Result;
use console::style;
use std::process::Command;

pub fn execute(verbose: bool, android: bool, ios: bool, web: bool) -> Result<()> {
    println!("{}", style("Checking FLUI environment...").green().bold());
    println!();

    let mut all_ok = true;

    // Check Rust installation
    all_ok &= check_rust(verbose);

    // Check Cargo
    all_ok &= check_cargo(verbose);

    // Check FLUI installation
    all_ok &= check_flui(verbose);

    // Platform-specific checks
    if android || (!ios && !web) {
        all_ok &= check_java(verbose);
        all_ok &= check_android(verbose);
    }

    if ios || (!android && !web) {
        #[cfg(target_os = "macos")]
        {
            all_ok &= check_ios(verbose);
        }
        #[cfg(not(target_os = "macos"))]
        {
            if ios {
                println!(
                    "{} iOS: Not available on non-macOS platforms",
                    style("[!]").yellow()
                );
            }
        }
    }

    if web || (!android && !ios) {
        all_ok &= check_web(verbose);
    }

    // Check wgpu support
    all_ok &= check_wgpu(verbose);

    println!();

    if all_ok {
        println!("{}", style("✓ All checks passed!").green().bold());
    } else {
        println!(
            "{}",
            style("✗ Some checks failed. Please fix the issues above.")
                .yellow()
                .bold()
        );
    }

    Ok(())
}

fn check_rust(verbose: bool) -> bool {
    print!("{} Rust: ", style("[✓]").green());

    match Command::new("rustc").arg("--version").output() {
        Ok(output) => {
            let version = String::from_utf8_lossy(&output.stdout);
            let version = version.trim();
            println!("{}", style(version).cyan());

            if verbose {
                if let Ok(path) = which::which("rustc") {
                    println!("    Path: {}", style(path.display()).dim());
                }
            }

            true
        }
        Err(_) => {
            println!("{}", style("Not found").red());
            println!("    Please install Rust: https://rustup.rs/");
            false
        }
    }
}

fn check_cargo(_verbose: bool) -> bool {
    print!("{} Cargo: ", style("[✓]").green());

    match Command::new("cargo").arg("--version").output() {
        Ok(output) => {
            let version = String::from_utf8_lossy(&output.stdout);
            println!("{}", style(version.trim()).cyan());
            true
        }
        Err(_) => {
            println!("{}", style("Not found").red());
            false
        }
    }
}

fn check_flui(verbose: bool) -> bool {
    print!("{} FLUI CLI: ", style("[✓]").green());

    let flui_version = env!("CARGO_PKG_VERSION");
    println!("{}", style(format!("v{}", flui_version)).cyan());

    if verbose {
        println!("    Location: {}", style(env!("CARGO_MANIFEST_DIR")).dim());
    }

    true
}

fn check_java(verbose: bool) -> bool {
    print!("{} Java (JDK): ", style("[✓]").green());

    match Command::new("java").arg("-version").output() {
        Ok(output) => {
            // Java outputs version to stderr
            let version_output = String::from_utf8_lossy(&output.stderr);

            // Extract version from output like "java version \"11.0.12\""
            if let Some(line) = version_output.lines().next() {
                println!("{}", style(line.trim()).cyan());

                if verbose {
                    // Check JAVA_HOME
                    if let Ok(java_home) = std::env::var("JAVA_HOME") {
                        println!("    JAVA_HOME: {}", style(&java_home).dim());
                    } else {
                        println!("    JAVA_HOME: {}", style("Not set").yellow());
                    }

                    // Get java path
                    if let Ok(path) = which::which("java") {
                        println!("    Path: {}", style(path.display()).dim());
                    }
                }

                // Check if version is 11 or higher
                if version_output.contains("version \"1.8") {
                    println!(
                        "    {}: Java 11+ recommended for Android development",
                        style("Warning").yellow()
                    );
                    return true; // Still works, just warn
                }

                true
            } else {
                println!("{}", style("Version not detected").yellow());
                true
            }
        }
        Err(_) => {
            println!("{}", style("Not found").red());
            println!("    Java 11+ is required for Android development");
            println!("    Download from: https://adoptium.net/");
            false
        }
    }
}

fn check_android(verbose: bool) -> bool {
    let mut android_ok = true;

    // Check ANDROID_HOME
    print!("{} Android SDK: ", style("[✓]").green());
    if let Ok(android_home) = std::env::var("ANDROID_HOME") {
        let sdk_path = std::path::Path::new(&android_home);
        if sdk_path.exists() {
            println!("{}", style(&android_home).cyan());

            if verbose {
                // Check for NDK
                let ndk_path = sdk_path.join("ndk");
                if ndk_path.exists() {
                    println!("    NDK: {}", style("Installed").green());
                } else {
                    println!("    NDK: {}", style("Not found").yellow());
                    android_ok = false;
                }

                // Check for platform-tools
                let platform_tools = sdk_path.join("platform-tools");
                if platform_tools.exists() {
                    println!("    Platform tools: {}", style("Installed").green());
                } else {
                    println!("    Platform tools: {}", style("Not found").yellow());
                }
            }
        } else {
            println!("{}", style("Path not found").yellow());
            println!("    ANDROID_HOME set to: {}", android_home);
            println!("    But directory doesn't exist");
            android_ok = false;
        }
    } else {
        println!("{}", style("Not configured").yellow());
        println!("    Please set ANDROID_HOME environment variable");
        println!("    Example: export ANDROID_HOME=$HOME/Android/Sdk");
        android_ok = false;
    }

    // Check Android Rust targets
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

        let mut missing_targets = Vec::new();
        for target in &android_targets {
            if !targets.contains(target) {
                missing_targets.push(*target);
            }
        }

        if missing_targets.is_empty() {
            print!("{} Android targets: ", style("[✓]").green());
            println!("{}", style("All installed").green());
        } else {
            print!("{} Android targets: ", style("[!]").yellow());
            println!("{}", style("Missing some targets").yellow());
            for target in &missing_targets {
                println!("    Missing: {}", style(target).yellow());
            }
            println!(
                "    Install with: rustup target add {}",
                missing_targets.join(" ")
            );
            android_ok = false;
        }
    }

    android_ok
}

#[cfg(target_os = "macos")]
fn check_ios(verbose: bool) -> bool {
    let mut ios_ok = true;

    // Check Xcode
    print!("{} Xcode: ", style("[✓]").green());
    match Command::new("xcode-select").arg("-p").output() {
        Ok(output) => {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout);
                println!("{}", style("Installed").green());

                if verbose {
                    println!("    Path: {}", style(path.trim()).dim());
                }
            } else {
                println!("{}", style("Not found").red());
                println!("    Install with: xcode-select --install");
                ios_ok = false;
            }
        }
        Err(_) => {
            println!("{}", style("Not found").red());
            println!("    Install with: xcode-select --install");
            ios_ok = false;
        }
    }

    // Check iOS Rust targets
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

        let mut missing_targets = Vec::new();
        for target in &ios_targets {
            if !targets.contains(target) {
                missing_targets.push(*target);
            }
        }

        if missing_targets.is_empty() {
            print!("{} iOS targets: ", style("[✓]").green());
            println!("{}", style("All installed").green());
        } else {
            print!("{} iOS targets: ", style("[!]").yellow());
            println!("{}", style("Missing some targets").yellow());
            for target in &missing_targets {
                println!("    Missing: {}", style(target).yellow());
            }
            println!(
                "    Install with: rustup target add {}",
                missing_targets.join(" ")
            );
            ios_ok = false;
        }
    }

    ios_ok
}

#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
fn check_ios(_verbose: bool) -> bool {
    true // Skip on non-macOS
}

fn check_web(_verbose: bool) -> bool {
    let mut web_ok = true;

    // Check wasm-pack
    print!("{} wasm-pack: ", style("[✓]").green());
    match Command::new("wasm-pack").arg("--version").output() {
        Ok(output) => {
            let version = String::from_utf8_lossy(&output.stdout);
            println!("{}", style(version.trim()).cyan());
        }
        Err(_) => {
            println!("{}", style("Not found").yellow());
            println!("    Install with: cargo install wasm-pack");
            println!("    Note: wasm-pack is optional but recommended for Web builds");
            // Not critical, don't mark as failure
        }
    }

    // Check wasm32 target
    if let Ok(output) = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
    {
        let targets = String::from_utf8_lossy(&output.stdout);

        if targets.contains("wasm32-unknown-unknown") {
            print!("{} WASM target: ", style("[✓]").green());
            println!("{}", style("Installed").green());
        } else {
            print!("{} WASM target: ", style("[!]").yellow());
            println!("{}", style("Not installed").yellow());
            println!("    Install with: rustup target add wasm32-unknown-unknown");
            web_ok = false;
        }
    }

    web_ok
}

fn check_wgpu(verbose: bool) -> bool {
    print!("{} wgpu support: ", style("[✓]").green());

    // wgpu is a library dependency, check if target is installed
    match Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
    {
        Ok(output) => {
            let targets = String::from_utf8_lossy(&output.stdout);
            let has_wasm = targets.contains("wasm32-unknown-unknown");

            println!("{}", style("Available").green());

            if verbose {
                println!("    wgpu is provided via Rust dependencies");
                if has_wasm {
                    println!("    wasm32 target: {}", style("Installed").green());
                }
            }

            true
        }
        Err(_) => {
            println!("{}", style("Available").green());
            println!("    (via Rust)");
            true
        }
    }
}
