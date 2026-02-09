//! Emulator and simulator management command.
//!
//! Lists and launches Android emulators (AVDs) and iOS simulators.
//! Uses external SDK tools:
//! - Android: `emulator -list-avds`, `adb devices -l`
//! - iOS: `xcrun simctl list devices --json` (macOS only)

use crate::error::{CliResult, ResultExt};
use console::style;
use std::process::Command;

// ── Data model ──────────────────────────────────────────────────────────────

/// Platform for an emulator or simulator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmulatorPlatform {
    Android,
    Ios,
}

impl std::fmt::Display for EmulatorPlatform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Android => write!(f, "Android"),
            Self::Ios => write!(f, "iOS"),
        }
    }
}

/// Status of an emulator or simulator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmulatorStatus {
    Running,
    Stopped,
}

impl std::fmt::Display for EmulatorStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Running => write!(f, "Running"),
            Self::Stopped => write!(f, "Stopped"),
        }
    }
}

/// An emulator or simulator device.
#[derive(Debug, Clone)]
struct Emulator {
    name: String,
    platform: EmulatorPlatform,
    id: String,
    status: EmulatorStatus,
    version: String,
}

// ── Public API ──────────────────────────────────────────────────────────────

/// List available emulators and simulators.
///
/// Queries Android SDK and iOS Simulator (macOS only) for available devices
/// and displays them in a formatted table.
pub fn execute_list(platform_filter: Option<&str>) -> CliResult<()> {
    cliclack::intro(style(" flui emulators ").on_magenta().black())?;

    let mut emulators = Vec::new();

    let show_android = platform_filter.is_none()
        || platform_filter.is_some_and(|p| p.eq_ignore_ascii_case("android"));
    let show_ios =
        platform_filter.is_none() || platform_filter.is_some_and(|p| p.eq_ignore_ascii_case("ios"));

    if show_android {
        match list_android_avds() {
            Ok(avds) => emulators.extend(avds),
            Err(e) => {
                tracing::debug!("Android SDK not available: {}", e);
                cliclack::log::warning(format!(
                    "Android SDK not found. {}",
                    android_install_hint()
                ))?;
            }
        }
    }

    if show_ios {
        match list_ios_simulators() {
            Ok(sims) => emulators.extend(sims),
            Err(e) => {
                tracing::debug!("iOS simulators not available: {}", e);
                if cfg!(target_os = "macos") {
                    cliclack::log::warning(format!(
                        "Xcode tools not found. {}",
                        ios_install_hint()
                    ))?;
                }
            }
        }
    }

    if emulators.is_empty() {
        cliclack::log::info("No emulators or simulators found.")?;
        display_install_hints()?;
        cliclack::outro(style("0 emulators found").dim())?;
        return Ok(());
    }

    display_emulator_table(&emulators)?;

    cliclack::outro(format!(
        "{} emulator{} found",
        emulators.len(),
        if emulators.len() == 1 { "" } else { "s" }
    ))?;

    Ok(())
}

/// Launch a specific emulator or simulator by name.
///
/// Searches across Android AVDs and iOS simulators, then launches the
/// matching device.
pub fn execute_launch(name: &str) -> CliResult<()> {
    cliclack::intro(style(format!(" Launching: {name} ")).on_cyan().black())?;

    // Collect all known emulators to find the target.
    let mut all_emulators = Vec::new();

    if let Ok(avds) = list_android_avds() {
        all_emulators.extend(avds);
    }
    if let Ok(sims) = list_ios_simulators() {
        all_emulators.extend(sims);
    }

    // Find by name (case-insensitive).
    let target = all_emulators
        .iter()
        .find(|e| e.name.eq_ignore_ascii_case(name) || e.id.eq_ignore_ascii_case(name));

    let Some(target) = target else {
        let available: Vec<_> = all_emulators.iter().map(|e| e.name.as_str()).collect();
        let msg = if available.is_empty() {
            "No emulators found. Install Android SDK or Xcode to get started.".to_string()
        } else {
            format!(
                "Emulator '{}' not found. Available: {}",
                name,
                available.join(", ")
            )
        };
        cliclack::outro(style(msg).red())?;
        return Err(crate::error::CliError::Missing(format!(
            "Emulator '{name}' not found"
        )));
    };

    let spinner = cliclack::spinner();
    spinner.start(format!("Starting {} emulator...", target.platform));

    match target.platform {
        EmulatorPlatform::Android => launch_android_avd(&target.id)?,
        EmulatorPlatform::Ios => launch_ios_simulator(&target.id)?,
    }

    spinner.stop(format!("{} launched successfully", target.name));
    cliclack::outro(format!(
        "Emulator {} is starting",
        style(&target.name).green()
    ))?;

    Ok(())
}

// ── Android AVD listing ─────────────────────────────────────────────────────

/// List Android AVDs by running `emulator -list-avds` and cross-referencing
/// with `adb devices -l` for running status.
fn list_android_avds() -> CliResult<Vec<Emulator>> {
    let emulator_path = find_android_tool("emulator")?;

    let output = Command::new(&emulator_path)
        .arg("-list-avds")
        .output()
        .with_context(|| format!("Failed to run '{emulator_path}'"))?;

    if !output.status.success() {
        return Err(crate::error::CliError::CommandFailed {
            context: "emulator -list-avds".to_string(),
            exit_code: output.status.code(),
        });
    }

    let avd_names: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    // Get running emulators from adb.
    let running_devices = list_running_android_devices();

    let emulators = avd_names
        .into_iter()
        .map(|name| {
            let status = if running_devices
                .iter()
                .any(|d| d.eq_ignore_ascii_case(&name))
            {
                EmulatorStatus::Running
            } else {
                EmulatorStatus::Stopped
            };

            Emulator {
                id: name.clone(),
                name: name.clone(),
                platform: EmulatorPlatform::Android,
                status,
                version: String::new(), // AVD names don't include API level in listing
            }
        })
        .collect();

    Ok(emulators)
}

/// Query `adb devices -l` for running Android emulators.
/// Returns AVD names of running emulators (best-effort, empty on failure).
fn list_running_android_devices() -> Vec<String> {
    let Ok(adb_path) = find_android_tool("adb") else {
        return Vec::new();
    };

    let output = match Command::new(&adb_path).args(["devices", "-l"]).output() {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse lines like: emulator-5554  device product:sdk_gphone64_x86_64 model:sdk_gphone64_x86_64 ...
    // The AVD name isn't directly in `adb devices` output, but we can detect running emulators
    // by their serial format "emulator-<port>".
    stdout
        .lines()
        .skip(1) // skip "List of devices attached" header
        .filter(|line| line.starts_with("emulator-") && line.contains("device"))
        .filter_map(|line| {
            // Extract the serial (emulator-NNNN)
            line.split_whitespace().next().map(String::from)
        })
        .collect()
}

/// Find an Android SDK tool by name, checking PATH and standard SDK locations.
fn find_android_tool(tool: &str) -> CliResult<String> {
    // Check PATH first.
    if which::which(tool).is_ok() {
        return Ok(tool.to_string());
    }

    // Check ANDROID_HOME / ANDROID_SDK_ROOT.
    let sdk_root = std::env::var("ANDROID_HOME")
        .or_else(|_| std::env::var("ANDROID_SDK_ROOT"))
        .ok();

    if let Some(sdk) = sdk_root {
        let subdirs = match tool {
            "emulator" => &["emulator"][..],
            "adb" => &["platform-tools"][..],
            _ => &[][..],
        };

        for subdir in subdirs {
            let candidate = std::path::Path::new(&sdk)
                .join(subdir)
                .join(tool_executable(tool));
            if candidate.exists() {
                return Ok(candidate.to_string_lossy().to_string());
            }
        }
    }

    Err(crate::error::CliError::ToolNotFound {
        tool: tool.to_string(),
        suggestion: android_install_hint().to_string(),
    })
}

/// Get the platform-specific executable name.
fn tool_executable(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// Launch an Android AVD by name.
fn launch_android_avd(avd_name: &str) -> CliResult<()> {
    let emulator_path = find_android_tool("emulator")?;

    // Launch as a detached background process.
    Command::new(&emulator_path)
        .args(["-avd", avd_name])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("Failed to launch Android emulator '{avd_name}'"))?;

    Ok(())
}

// ── iOS simulator listing ───────────────────────────────────────────────────

/// List iOS simulators via `xcrun simctl list devices --json`.
/// Returns an error if not on macOS or xcrun is not available.
fn list_ios_simulators() -> CliResult<Vec<Emulator>> {
    if !cfg!(target_os = "macos") {
        return Ok(Vec::new());
    }

    if which::which("xcrun").is_err() {
        return Err(crate::error::CliError::ToolNotFound {
            tool: "xcrun".to_string(),
            suggestion: ios_install_hint().to_string(),
        });
    }

    let output = Command::new("xcrun")
        .args(["simctl", "list", "devices", "--json"])
        .output()
        .context("Failed to run xcrun simctl")?;

    if !output.status.success() {
        return Err(crate::error::CliError::CommandFailed {
            context: "xcrun simctl list devices".to_string(),
            exit_code: output.status.code(),
        });
    }

    let json_str = String::from_utf8_lossy(&output.stdout);
    parse_simctl_json(&json_str)
}

/// Parse the JSON output from `xcrun simctl list devices --json`.
///
/// Expected structure:
/// ```json
/// {
///   "devices": {
///     "com.apple.CoreSimulator.SimRuntime.iOS-17-0": [
///       { "udid": "...", "name": "iPhone 15", "state": "Shutdown", "isAvailable": true }
///     ]
///   }
/// }
/// ```
fn parse_simctl_json(json_str: &str) -> CliResult<Vec<Emulator>> {
    let parsed: serde_json::Value =
        serde_json::from_str(json_str).context("Failed to parse simctl JSON output")?;

    let Some(devices_obj) = parsed.get("devices").and_then(|d| d.as_object()) else {
        return Ok(Vec::new());
    };

    let mut emulators = Vec::new();

    for (runtime_key, device_list) in devices_obj {
        let Some(devices) = device_list.as_array() else {
            continue;
        };

        // Extract OS version from runtime key like "com.apple.CoreSimulator.SimRuntime.iOS-17-0"
        let version = extract_ios_version(runtime_key);

        for device in devices {
            let is_available = device
                .get("isAvailable")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);

            if !is_available {
                continue;
            }

            let name = device
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown")
                .to_string();

            let udid = device
                .get("udid")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let state = device
                .get("state")
                .and_then(|v| v.as_str())
                .unwrap_or("Shutdown");

            let status = if state.eq_ignore_ascii_case("Booted") {
                EmulatorStatus::Running
            } else {
                EmulatorStatus::Stopped
            };

            emulators.push(Emulator {
                name,
                platform: EmulatorPlatform::Ios,
                id: udid,
                status,
                version: version.clone(),
            });
        }
    }

    Ok(emulators)
}

/// Extract iOS version from a runtime key.
/// e.g. "com.apple.CoreSimulator.SimRuntime.iOS-17-2" → "iOS 17.2"
fn extract_ios_version(runtime_key: &str) -> String {
    // Look for the last segment after "SimRuntime."
    if let Some(suffix) = runtime_key.strip_prefix("com.apple.CoreSimulator.SimRuntime.") {
        suffix.replace('-', ".").replacen('.', " ", 1)
    } else {
        runtime_key.to_string()
    }
}

/// Launch an iOS simulator by UDID.
fn launch_ios_simulator(udid: &str) -> CliResult<()> {
    let output = Command::new("xcrun")
        .args(["simctl", "boot", udid])
        .output()
        .context("Failed to run xcrun simctl boot")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Exit code 164 means "already booted" — not an error.
        if output.status.code() == Some(164)
            || stderr.contains("Unable to boot device in current state: Booted")
        {
            tracing::debug!("Simulator {} is already booted", udid);
            return Ok(());
        }
        return Err(crate::error::CliError::CommandFailed {
            context: format!("xcrun simctl boot {udid}"),
            exit_code: output.status.code(),
        });
    }

    Ok(())
}

// ── Display helpers ─────────────────────────────────────────────────────────

/// Display emulators as a formatted table.
fn display_emulator_table(emulators: &[Emulator]) -> CliResult<()> {
    // Column widths.
    let platform_w = 10;
    let name_w = 35;
    let version_w = 15;

    // Header.
    let header = format!(
        "  {:<platform_w$} {:<name_w$} {:<version_w$} {}",
        style("Platform").bold(),
        style("Name").bold(),
        style("Version").bold(),
        style("Status").bold(),
    );
    cliclack::log::info(header)?;

    for emu in emulators {
        let status_styled = match emu.status {
            EmulatorStatus::Running => style("Running").green().to_string(),
            EmulatorStatus::Stopped => style("Stopped").dim().to_string(),
        };

        let version_display = if emu.version.is_empty() {
            "-".to_string()
        } else {
            emu.version.clone()
        };

        let line = format!(
            "  {:<platform_w$} {:<name_w$} {:<version_w$} {}",
            emu.platform, emu.name, version_display, status_styled,
        );
        cliclack::log::info(line)?;
    }

    Ok(())
}

/// Display installation hints when no emulators are found.
fn display_install_hints() -> CliResult<()> {
    let hints = format!(
        "{}\n  {}\n\n{}\n  {}",
        style("Android").bold(),
        "Install Android SDK: https://developer.android.com/studio",
        style("iOS (macOS only)").bold(),
        "Install Xcode from the App Store",
    );
    cliclack::note("Setup Guide", hints)?;
    Ok(())
}

/// Installation hint for Android SDK.
fn android_install_hint() -> &'static str {
    "Install Android SDK: https://developer.android.com/studio"
}

/// Installation hint for iOS tools.
fn ios_install_hint() -> &'static str {
    "Install Xcode from the App Store, then run: xcode-select --install"
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_ios_version() {
        assert_eq!(
            extract_ios_version("com.apple.CoreSimulator.SimRuntime.iOS-17-2"),
            "iOS 17.2"
        );
        assert_eq!(
            extract_ios_version("com.apple.CoreSimulator.SimRuntime.tvOS-17-0"),
            "tvOS 17.0"
        );
        assert_eq!(extract_ios_version("unknown-key"), "unknown-key");
    }

    #[test]
    fn test_parse_simctl_json_empty() {
        let json = r#"{"devices": {}}"#;
        let result = parse_simctl_json(json).expect("should parse");
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_simctl_json_with_devices() {
        let json = r#"{
            "devices": {
                "com.apple.CoreSimulator.SimRuntime.iOS-17-0": [
                    {
                        "udid": "ABC-123",
                        "name": "iPhone 15 Pro",
                        "state": "Shutdown",
                        "isAvailable": true
                    },
                    {
                        "udid": "DEF-456",
                        "name": "iPhone 15",
                        "state": "Booted",
                        "isAvailable": true
                    },
                    {
                        "udid": "GHI-789",
                        "name": "Unavailable Device",
                        "state": "Shutdown",
                        "isAvailable": false
                    }
                ]
            }
        }"#;

        let result = parse_simctl_json(json).expect("should parse");
        assert_eq!(result.len(), 2); // Unavailable device filtered out.
        assert_eq!(result[0].name, "iPhone 15 Pro");
        assert_eq!(result[0].status, EmulatorStatus::Stopped);
        assert_eq!(result[0].version, "iOS 17.0");
        assert_eq!(result[1].name, "iPhone 15");
        assert_eq!(result[1].status, EmulatorStatus::Running);
    }

    #[test]
    fn test_parse_simctl_json_invalid() {
        let result = parse_simctl_json("not json");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_simctl_json_missing_devices() {
        let json = r#"{"runtimes": []}"#;
        let result = parse_simctl_json(json).expect("should parse");
        assert!(result.is_empty());
    }

    #[test]
    fn test_tool_executable() {
        let name = tool_executable("emulator");
        if cfg!(windows) {
            assert_eq!(name, "emulator.exe");
        } else {
            assert_eq!(name, "emulator");
        }
    }

    #[test]
    fn test_emulator_platform_display() {
        assert_eq!(EmulatorPlatform::Android.to_string(), "Android");
        assert_eq!(EmulatorPlatform::Ios.to_string(), "iOS");
    }

    #[test]
    fn test_emulator_status_display() {
        assert_eq!(EmulatorStatus::Running.to_string(), "Running");
        assert_eq!(EmulatorStatus::Stopped.to_string(), "Stopped");
    }
}
