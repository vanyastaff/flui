//! Device listing command.
//!
//! Lists all available devices for running FLUI applications,
//! including desktop, Android devices, iOS simulators, and web browsers.

use crate::error::CliResult;
use console::style;
use std::process::Command;

/// Execute the devices command.
///
/// # Arguments
///
/// * `details` - Show detailed device information
/// * `_platform` - Filter by platform (not yet implemented)
pub fn execute(details: bool, _platform: Option<String>) -> CliResult<()> {
    cliclack::intro(style(" flui devices ").on_magenta().black())?;

    let mut sections = Vec::with_capacity(12);

    // Desktop
    sections.push(format!("{}", style("Desktop").bold()));
    #[cfg(target_os = "windows")]
    {
        sections.push(format!(
            "  {} Windows - {}",
            style("●").green(),
            windows_version()
        ));
    }
    #[cfg(target_os = "linux")]
    {
        sections.push(format!("  {} Linux", style("●").green()));
    }
    #[cfg(target_os = "macos")]
    {
        sections.push(format!("  {} macOS", style("●").green()));
    }

    // Android
    sections.push(String::new());
    sections.push(format!("{}", style("Android").bold()));
    sections.push(android_devices(details));

    // iOS (macOS only)
    #[cfg(target_os = "macos")]
    {
        sections.push(String::new());
        sections.push(format!("{}", style("iOS Simulators").bold()));
        sections.push(ios_simulators(details));
    }

    // Web browsers
    sections.push(String::new());
    sections.push(format!("{}", style("Web Browsers").bold()));
    sections.push(browsers(details));

    cliclack::note("Available Devices", sections.join("\n"))?;

    cliclack::outro("Device scan complete")?;

    Ok(())
}

fn android_devices(details: bool) -> String {
    match Command::new("adb").args(["devices"]).output() {
        Ok(output) => {
            let output_str = String::from_utf8_lossy(&output.stdout);
            let mut devices = Vec::with_capacity(4);

            for line in output_str.lines().skip(1) {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let device_id = parts[0];
                    let status = parts[1];

                    let (icon, status_styled) = match status {
                        "device" => (style("●").green(), style("online").green()),
                        "offline" => (style("●").red(), style("offline").red()),
                        "unauthorized" => (style("●").yellow(), style("unauthorized").yellow()),
                        _ => (style("●").dim(), style(status).dim()),
                    };

                    devices.push(format!("  {} {} ({})", icon, device_id, status_styled));

                    if details && status == "device" {
                        if let Ok(model_output) = Command::new("adb")
                            .args(["-s", device_id, "shell", "getprop", "ro.product.model"])
                            .output()
                        {
                            let model = String::from_utf8_lossy(&model_output.stdout)
                                .trim()
                                .to_string();
                            if !model.is_empty() {
                                devices.push(format!(
                                    "    {}",
                                    style(format!("Model: {}", model)).dim()
                                ));
                            }
                        }
                    }
                }
            }

            if devices.is_empty() {
                format!(
                    "  {} {}\n    {}",
                    style("○").dim(),
                    style("No devices connected").dim(),
                    style("Connect via USB or start emulator").dim()
                )
            } else {
                devices.join("\n")
            }
        }
        Err(_) => {
            format!(
                "  {} {}\n    {}",
                style("✗").yellow(),
                style("adb not found").yellow(),
                style("Install Android SDK").dim()
            )
        }
    }
}

#[cfg(target_os = "macos")]
fn ios_simulators(details: bool) -> String {
    match Command::new("xcrun")
        .args(["simctl", "list", "devices", "available"])
        .output()
    {
        Ok(output) => {
            let output_str = String::from_utf8_lossy(&output.stdout);
            let mut simulators = Vec::with_capacity(8);

            for line in output_str.lines() {
                let line = line.trim();
                if line.contains("(") && (line.contains("Booted") || line.contains("Shutdown")) {
                    let is_booted = line.contains("Booted");

                    if let Some(name_end) = line.find('(') {
                        let name = line[..name_end].trim();

                        let (icon, status) = if is_booted {
                            (style("●").green(), style("booted").green())
                        } else {
                            (style("○").dim(), style("shutdown").dim())
                        };

                        simulators.push(format!("  {} {} ({})", icon, name, status));

                        if details && is_booted {
                            if let Some(uuid_start) = line.find('(') {
                                if let Some(uuid_end) = line[uuid_start..].find(')') {
                                    let uuid = &line[uuid_start + 1..uuid_start + uuid_end];
                                    simulators.push(format!(
                                        "    {}",
                                        style(format!("UUID: {}", uuid)).dim()
                                    ));
                                }
                            }
                        }
                    }
                }
            }

            if simulators.is_empty() {
                format!(
                    "  {} {}",
                    style("○").dim(),
                    style("No simulators available").dim()
                )
            } else {
                simulators.join("\n")
            }
        }
        Err(_) => {
            format!(
                "  {} {}",
                style("✗").yellow(),
                style("Xcode not installed").yellow()
            )
        }
    }
}

#[cfg(target_os = "windows")]
fn windows_version() -> String {
    match Command::new("cmd").args(["/C", "ver"]).output() {
        Ok(output) => {
            let version = String::from_utf8_lossy(&output.stdout);
            version.trim().to_string()
        }
        Err(_) => "Unknown".to_string(),
    }
}

fn browsers(details: bool) -> String {
    let browsers = detect_browsers();

    if browsers.is_empty() {
        return format!(
            "  {} {}\n    {}",
            style("○").dim(),
            style("No browsers detected").dim(),
            style("Install Chrome, Edge, or Firefox").dim()
        );
    }

    let mut result = Vec::with_capacity(browsers.len() * 2);
    for browser in &browsers {
        result.push(format!("  {} {}", style("●").green(), browser.name));

        if details {
            if let Some(version) = &browser.version {
                result.push(format!(
                    "    {}",
                    style(format!("Version: {}", version)).dim()
                ));
            }
        }
    }

    result.join("\n")
}

#[derive(Debug)]
struct BrowserInfo {
    name: String,
    version: Option<String>,
    #[allow(dead_code)]
    path: Option<String>,
}

fn detect_browsers() -> Vec<BrowserInfo> {
    let mut browsers = Vec::with_capacity(4);

    #[cfg(target_os = "windows")]
    {
        browsers.extend(enumerate_browsers_windows());
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(chrome) = check_browser_macos(
            "Chrome",
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            &["--version"],
        ) {
            browsers.push(chrome);
        }
        if let Some(safari) = check_browser_macos(
            "Safari",
            "/Applications/Safari.app/Contents/MacOS/Safari",
            &["-v"],
        ) {
            browsers.push(safari);
        }
        if let Some(firefox) = check_browser_macos(
            "Firefox",
            "/Applications/Firefox.app/Contents/MacOS/firefox",
            &["-v"],
        ) {
            browsers.push(firefox);
        }
        if let Some(edge) = check_browser_macos(
            "Edge",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            &["--version"],
        ) {
            browsers.push(edge);
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(chrome) = check_browser_linux("Chrome", "google-chrome", &["--version"]) {
            browsers.push(chrome);
        }
        if let Some(firefox) = check_browser_linux("Firefox", "firefox", &["-v"]) {
            browsers.push(firefox);
        }
        if let Some(edge) = check_browser_linux("Edge", "microsoft-edge", &["--version"]) {
            browsers.push(edge);
        }
    }

    browsers
}

#[cfg(target_os = "windows")]
fn enumerate_browsers_windows() -> Vec<BrowserInfo> {
    use std::path::Path;

    let mut browsers = Vec::with_capacity(6);

    let ps_command = r#"
        foreach ($hive in @('HKLM', 'HKCU')) {
            $path = "${hive}:\SOFTWARE\Clients\StartMenuInternet"
            if (Test-Path $path) {
                Get-ChildItem $path | ForEach-Object {
                    $name = $_.PSChildName
                    $shell = Get-ItemProperty -Path "$($_.PSPath)\shell\open\command" -ErrorAction SilentlyContinue
                    if ($shell.'(default)') {
                        $exePath = $shell.'(default)' -replace '"', '' -replace ' --.*$', '' -replace ' -%.*$', ''
                        Write-Output "BROWSER:$name|$exePath"
                    }
                }
            }
        }
    "#;

    if let Ok(output) = Command::new("powershell")
        .args(["-NoProfile", "-Command", ps_command])
        .output()
    {
        if output.status.success() {
            let output_str = String::from_utf8_lossy(&output.stdout);

            for line in output_str.lines() {
                if let Some(stripped) = line.strip_prefix("BROWSER:") {
                    let parts: Vec<&str> = stripped.split('|').collect();
                    if parts.len() == 2 {
                        let registry_name = parts[0].trim();
                        let path = parts[1].trim().to_string();

                        if Path::new(&path).exists() {
                            let display_name = match registry_name {
                                "Google Chrome" | "CHROME.EXE" => "Chrome",
                                "Microsoft Edge" | "MSEDGE" => "Edge",
                                "Firefox" | "FIREFOX.EXE" => "Firefox",
                                "Brave" | "BRAVE.EXE" => "Brave",
                                "Opera" | "OPERA.EXE" => "Opera",
                                "IEXPLORE.EXE" => "Internet Explorer",
                                _ => registry_name,
                            }
                            .to_string();

                            if !browsers
                                .iter()
                                .any(|b: &BrowserInfo| b.name == display_name)
                            {
                                browsers.push(BrowserInfo {
                                    name: display_name,
                                    version: None,
                                    path: Some(path),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    browsers
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
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string());

    Some(BrowserInfo {
        name: name.to_string(),
        version,
        path: Some(path.to_string()),
    })
}

#[cfg(target_os = "linux")]
fn check_browser_linux(name: &str, command: &str, version_args: &[&str]) -> Option<BrowserInfo> {
    let which_output = Command::new("which").arg(command).output().ok()?;

    if !which_output.status.success() {
        return None;
    }

    let path = String::from_utf8_lossy(&which_output.stdout)
        .trim()
        .to_string();

    let version = Command::new(command)
        .args(version_args)
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string());

    Some(BrowserInfo {
        name: name.to_string(),
        version,
        path: Some(path),
    })
}
