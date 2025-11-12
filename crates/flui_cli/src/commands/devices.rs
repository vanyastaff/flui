use anyhow::Result;
use console::style;
use std::process::Command;

pub fn execute(details: bool, _platform: Option<String>) -> Result<()> {
    // Enable ANSI colors on Windows
    #[cfg(target_os = "windows")]
    {
        let _ = console::set_colors_enabled(true);
        let _ = console::set_colors_enabled_stderr(true);
    }

    println!();
    println!("{}", style("Available Devices").green().bold());
    println!("{}", style("═".repeat(60)).dim());
    println!();

    // Desktop device
    print_section_header("Desktop");
    #[cfg(target_os = "windows")]
    {
        println!("  {} {}",
            style("●").green().bold(),
            style("Windows").cyan()
        );
        println!("    {} {}",
            style("└─").dim(),
            style(get_windows_version()).dim()
        );
    }

    #[cfg(target_os = "linux")]
    {
        println!("  {} {}",
            style("●").green().bold(),
            style("Linux").cyan()
        );
    }

    #[cfg(target_os = "macos")]
    {
        println!("  {} {}",
            style("●").green().bold(),
            style("macOS").cyan()
        );
    }

    if details {
        println!("    {} {}",
            style("  └─").dim(),
            style("Available for development and testing").dim()
        );
    }
    println!();

    // Android devices
    list_android_devices(details);

    // iOS simulators (macOS only)
    #[cfg(target_os = "macos")]
    list_ios_simulators(details);

    // Web browsers
    list_web_browsers(details);

    println!("{}", style("─".repeat(60)).dim());

    Ok(())
}

fn print_section_header(title: &str) {
    println!("{}", style(title).cyan().bold());
}

fn list_android_devices(details: bool) {
    print_section_header("Android");

    // Check if adb is available
    match Command::new("adb").args(["devices"]).output() {
        Ok(output) => {
            let output_str = String::from_utf8_lossy(&output.stdout);
            let mut device_count = 0;

            for line in output_str.lines().skip(1) { // Skip "List of devices attached"
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                // Parse device line: "device_id    device/offline/unauthorized"
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let device_id = parts[0];
                    let status = parts[1];

                    device_count += 1;

                    let (status_icon, status_text) = match status {
                        "device" => (style("●").green().bold(), "online"),
                        "offline" => (style("●").red().bold(), "offline"),
                        "unauthorized" => (style("●").yellow().bold(), "unauthorized"),
                        _ => (style("●").dim(), status),
                    };

                    println!("  {} {} {}",
                        status_icon,
                        style(device_id).cyan(),
                        style(format!("({})", status_text)).dim()
                    );

                    if details {
                        // Get device model
                        if let Ok(model_output) = Command::new("adb")
                            .args(["-s", device_id, "shell", "getprop", "ro.product.model"])
                            .output()
                        {
                            let model = String::from_utf8_lossy(&model_output.stdout).trim().to_string();
                            if !model.is_empty() {
                                println!("    {} Model: {}", style("├─").dim(), style(model).dim());
                            }
                        }

                        // Get Android version
                        if let Ok(version_output) = Command::new("adb")
                            .args(["-s", device_id, "shell", "getprop", "ro.build.version.release"])
                            .output()
                        {
                            let version = String::from_utf8_lossy(&version_output.stdout).trim().to_string();
                            if !version.is_empty() {
                                println!("    {} Android {}", style("└─").dim(), style(version).dim());
                            }
                        }
                    }
                }
            }

            if device_count == 0 {
                println!("  {} {}",
                    style("○").dim(),
                    style("No devices connected").dim()
                );
                println!("    {} {}",
                    style("└─").dim(),
                    style("Connect via USB or start emulator").dim()
                );
            }
        }
        Err(_) => {
            println!("  {} {}",
                style("✗").yellow().bold(),
                style("adb not found").yellow()
            );
            println!("    {} {}",
                style("└─").dim(),
                style("Install Android SDK and add adb to PATH").dim()
            );
        }
    }

    println!();
}

#[cfg(target_os = "macos")]
fn list_ios_simulators(details: bool) {
    print_section_header("iOS Simulators");

    match Command::new("xcrun")
        .args(["simctl", "list", "devices", "available"])
        .output()
    {
        Ok(output) => {
            let output_str = String::from_utf8_lossy(&output.stdout);
            let mut simulator_count = 0;

            for line in output_str.lines() {
                let line = line.trim();

                // Parse simulator line: "    iPhone 14 (UUID) (Booted)"
                if line.contains("(") && (line.contains("Booted") || line.contains("Shutdown")) {
                    simulator_count += 1;

                    let (status_icon, status_text) = if line.contains("Booted") {
                        (style("●").green().bold(), "booted")
                    } else {
                        (style("○").dim(), "shutdown")
                    };

                    // Extract device name (before first parenthesis)
                    if let Some(name_end) = line.find('(') {
                        let name = line[..name_end].trim();
                        println!("  {} {} {}",
                            status_icon,
                            style(name).cyan(),
                            style(format!("({})", status_text)).dim()
                        );

                        if details && line.contains("Booted") {
                            // Extract UUID
                            if let Some(uuid_start) = line.find('(') {
                                if let Some(uuid_end) = line[uuid_start..].find(')') {
                                    let uuid = &line[uuid_start + 1..uuid_start + uuid_end];
                                    println!("    {} UUID: {}", style("└─").dim(), style(uuid).dim());
                                }
                            }
                        }
                    }
                }
            }

            if simulator_count == 0 {
                println!("  {} {}",
                    style("○").dim(),
                    style("No simulators available").dim()
                );
            }
        }
        Err(_) => {
            println!("  {} {}",
                style("✗").yellow().bold(),
                style("Xcode not installed").yellow()
            );
        }
    }

    println!();
}

#[cfg(target_os = "windows")]
fn get_windows_version() -> String {
    match Command::new("cmd").args(["/C", "ver"]).output() {
        Ok(output) => {
            let version = String::from_utf8_lossy(&output.stdout);
            version.trim().to_string()
        }
        Err(_) => "Unknown".to_string(),
    }
}

fn list_web_browsers(details: bool) {
    print_section_header("Web");

    let browsers = detect_browsers();

    if browsers.is_empty() {
        println!("  {} {}",
            style("○").dim(),
            style("No browsers detected").dim()
        );
        println!("    {} {}",
            style("└─").dim(),
            style("Install Chrome, Edge, Firefox, or Safari").dim()
        );
    } else {
        for (i, browser) in browsers.iter().enumerate() {
            let is_last = i == browsers.len() - 1;

            println!("  {} {}",
                style("●").green().bold(),
                style(&browser.name).cyan()
            );

            if details {
                let tree_char = if is_last { "└─" } else { "├─" };
                if let Some(version) = &browser.version {
                    println!("    {} Version: {}",
                        style(tree_char).dim(),
                        style(version).dim()
                    );
                }
                if let Some(path) = &browser.path {
                    let tree_char = if is_last { "  └─" } else { "  ├─" };
                    println!("    {} Path: {}",
                        style(tree_char).dim(),
                        style(path).dim()
                    );
                }
            }
        }
    }

    println!();
}

#[derive(Debug)]
struct BrowserInfo {
    name: String,
    version: Option<String>,
    path: Option<String>,
}

fn detect_browsers() -> Vec<BrowserInfo> {
    let mut browsers = Vec::new();

    // Windows browsers
    #[cfg(target_os = "windows")]
    {
        // Chrome
        if let Some(chrome) = check_browser_windows(
            "Chrome",
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            &["--version"]
        ) {
            browsers.push(chrome);
        }

        // Edge
        if let Some(edge) = check_browser_windows(
            "Edge",
            r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
            &["--version"]
        ) {
            browsers.push(edge);
        }

        // Firefox
        if let Some(firefox) = check_browser_windows(
            "Firefox",
            r"C:\Program Files\Mozilla Firefox\firefox.exe",
            &["-v"]
        ) {
            browsers.push(firefox);
        }

        // Brave
        if let Some(brave) = check_browser_windows(
            "Brave",
            r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe",
            &["--version"]
        ) {
            browsers.push(brave);
        }
    }

    // macOS browsers
    #[cfg(target_os = "macos")]
    {
        // Chrome
        if let Some(chrome) = check_browser_macos(
            "Chrome",
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            &["--version"]
        ) {
            browsers.push(chrome);
        }

        // Safari
        if let Some(safari) = check_browser_macos(
            "Safari",
            "/Applications/Safari.app/Contents/MacOS/Safari",
            &["-v"]
        ) {
            browsers.push(safari);
        }

        // Firefox
        if let Some(firefox) = check_browser_macos(
            "Firefox",
            "/Applications/Firefox.app/Contents/MacOS/firefox",
            &["-v"]
        ) {
            browsers.push(firefox);
        }

        // Edge
        if let Some(edge) = check_browser_macos(
            "Edge",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            &["--version"]
        ) {
            browsers.push(edge);
        }

        // Brave
        if let Some(brave) = check_browser_macos(
            "Brave",
            "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
            &["--version"]
        ) {
            browsers.push(brave);
        }
    }

    // Linux browsers
    #[cfg(target_os = "linux")]
    {
        // Chrome
        if let Some(chrome) = check_browser_linux("Chrome", "google-chrome", &["--version"]) {
            browsers.push(chrome);
        }

        // Chromium
        if let Some(chromium) = check_browser_linux("Chromium", "chromium", &["--version"]) {
            browsers.push(chromium);
        }

        // Firefox
        if let Some(firefox) = check_browser_linux("Firefox", "firefox", &["-v"]) {
            browsers.push(firefox);
        }

        // Edge
        if let Some(edge) = check_browser_linux("Edge", "microsoft-edge", &["--version"]) {
            browsers.push(edge);
        }

        // Brave
        if let Some(brave) = check_browser_linux("Brave", "brave-browser", &["--version"]) {
            browsers.push(brave);
        }
    }

    browsers
}

#[cfg(target_os = "windows")]
fn check_browser_windows(name: &str, path: &str, _version_args: &[&str]) -> Option<BrowserInfo> {
    use std::path::Path;

    if !Path::new(path).exists() {
        return None;
    }

    // Try to extract version from path (e.g., Chrome/Application/131.0.6778.86/)
    let version = std::fs::read_dir(Path::new(path).parent()?)
        .ok()
        .and_then(|entries| {
            for entry in entries.flatten() {
                let entry_name = entry.file_name();
                let name_str = entry_name.to_string_lossy();
                // Check if it looks like a version number (starts with digit, contains dots)
                if name_str.chars().next()?.is_ascii_digit() && name_str.contains('.') {
                    return Some(name_str.to_string());
                }
            }
            None
        });

    Some(BrowserInfo {
        name: name.to_string(),
        version,
        path: Some(path.to_string()),
    })
}

#[cfg(target_os = "macos")]
fn check_browser_macos(name: &str, path: &str, version_args: &[&str]) -> Option<BrowserInfo> {
    use std::path::Path;

    if !Path::new(path).exists() {
        return None;
    }

    let version = Command::new(path)
        .args(version_args)
        .output()
        .ok()
        .and_then(|output| {
            let version_str = String::from_utf8_lossy(&output.stdout);
            Some(version_str.trim().to_string())
        });

    Some(BrowserInfo {
        name: name.to_string(),
        version,
        path: Some(path.to_string()),
    })
}

#[cfg(target_os = "linux")]
fn check_browser_linux(name: &str, command: &str, version_args: &[&str]) -> Option<BrowserInfo> {
    // Check if command exists in PATH
    let which_output = Command::new("which")
        .arg(command)
        .output()
        .ok()?;

    if !which_output.status.success() {
        return None;
    }

    let path = String::from_utf8_lossy(&which_output.stdout).trim().to_string();

    let version = Command::new(command)
        .args(version_args)
        .output()
        .ok()
        .and_then(|output| {
            let version_str = String::from_utf8_lossy(&output.stdout);
            Some(version_str.trim().to_string())
        });

    Some(BrowserInfo {
        name: name.to_string(),
        version,
        path: Some(path),
    })
}
