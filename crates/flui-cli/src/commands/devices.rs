//! `flui devices` — list run targets: this desktop, Android devices/emulators
//! visible to `adb`, iOS simulators, and installed web browsers.
//!
//! Every probe is bounded by [`PROBE_TIMEOUT`] and a missing tool is a
//! [`Problem`] row, never a hard failure: the command always exits `0` and
//! reports what it *could* find. This is also the regression guard for the
//! macOS hang this module used to have — `flui devices` once shelled out to
//! `/Applications/Safari.app/Contents/MacOS/Safari -v`, which launches
//! Safari's GUI and never returns. Browser versions are read from
//! `Info.plist` (macOS) or queried with a bounded `--version` (Linux) or a
//! bounded PowerShell file-version query (Windows) instead.

use crate::DevicePlatform;
use crate::error::CliResult;
use crate::proc::{PROBE_TIMEOUT, output_with_timeout, probe_stdout};
use crate::ui;
use console::style;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

// ============================================================================
// Shared device model (reused by `emulators.rs`)
// ============================================================================

/// What kind of thing a [`Device`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind {
    /// This machine.
    Host,
    /// A physical device connected over USB/Wi-Fi (Android).
    Physical,
    /// An Android Virtual Device.
    Emulator,
    /// An iOS Simulator runtime instance (only discovered on macOS; kept
    /// in the model so `--json` has one schema on every host).
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    Simulator,
    /// An installed web browser.
    Browser,
}

/// Current state of a [`Device`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Status {
    /// Connected and authorized (Android).
    Online,
    /// Connected but not responding (Android).
    Offline,
    /// Connected but the host has not authorized USB debugging (Android).
    Unauthorized,
    /// Running (iOS Simulator, or an Android emulator known to be up).
    Booted,
    /// Not running, but could be booted (iOS Simulator, Android AVD).
    Shutdown,
    /// Present and usable with no separate running state (desktop, browsers).
    Available,
}

/// One thing `flui run --device` could target, or an installed browser.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Device {
    /// What `flui run --device` takes: a UDID for iOS, a serial for Android,
    /// a fixed sentinel for the desktop, a synthetic id for browsers.
    pub(crate) id: String,
    /// Human-readable name.
    pub(crate) name: String,
    pub(crate) platform: DevicePlatform,
    pub(crate) kind: Kind,
    pub(crate) status: Status,
    /// Free-form extra fields (model, OS version, runtime, browser path, …).
    pub(crate) details: BTreeMap<String, String>,
}

/// A probe that could not run at all (tool missing, timed out, or failed).
/// Never fails the command — it is folded into [`Discovery::problems`].
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Problem {
    pub(crate) platform: DevicePlatform,
    pub(crate) message: String,
    pub(crate) hint: String,
}

/// Result of a full device scan.
pub(crate) struct Discovery {
    pub(crate) devices: Vec<Device>,
    pub(crate) problems: Vec<Problem>,
}

/// Scan every platform (or just `filter`, when given) for run targets.
///
/// Never returns an error: a missing `adb`/Xcode/etc. becomes a [`Problem`]
/// row instead, so the command can always report *something* and exit `0`.
pub(crate) fn discover(filter: Option<DevicePlatform>, details: bool) -> Discovery {
    let mut devices = Vec::new();
    let mut problems = Vec::new();

    if filter.is_none_or(|p| p == DevicePlatform::Desktop) {
        devices.push(desktop_device());
    }

    if filter.is_none_or(|p| p == DevicePlatform::Android) {
        match probe_android(details) {
            Ok(found) => devices.extend(found),
            Err(problem) => problems.push(problem),
        }
    }

    if filter.is_none_or(|p| p == DevicePlatform::Ios) {
        #[cfg(target_os = "macos")]
        match probe_ios() {
            Ok(found) => devices.extend(found),
            Err(problem) => problems.push(problem),
        }
        #[cfg(not(target_os = "macos"))]
        if filter == Some(DevicePlatform::Ios) {
            problems.push(Problem {
                platform: DevicePlatform::Ios,
                message: "iOS Simulators require macOS".into(),
                hint: "run this on a Mac with Xcode installed".into(),
            });
        }
    }

    if filter.is_none_or(|p| p == DevicePlatform::Web) {
        devices.extend(probe_browsers());
    }

    Discovery { devices, problems }
}

// ============================================================================
// Desktop
// ============================================================================

fn desktop_device() -> Device {
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;
    let label = match os {
        "macos" => "macOS",
        "linux" => "Linux",
        "windows" => "Windows",
        other => other,
    };

    let mut details = BTreeMap::new();
    details.insert("arch".to_string(), arch.to_string());
    if let Some(version) = host_os_version() {
        details.insert("os_version".to_string(), version);
    }

    Device {
        id: "desktop".to_string(),
        name: format!("{label} ({arch})"),
        platform: DevicePlatform::Desktop,
        kind: Kind::Host,
        status: Status::Available,
        details,
    }
}

fn host_os_version() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        probe_stdout(
            Command::new("sw_vers").arg("-productVersion"),
            PROBE_TIMEOUT,
        )
    }
    #[cfg(target_os = "windows")]
    {
        probe_stdout(Command::new("cmd").args(["/C", "ver"]), PROBE_TIMEOUT)
    }
    #[cfg(target_os = "linux")]
    {
        linux_pretty_name()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        None
    }
}

#[cfg(target_os = "linux")]
fn linux_pretty_name() -> Option<String> {
    let contents = std::fs::read_to_string("/etc/os-release").ok()?;
    contents.lines().find_map(|line| {
        line.strip_prefix("PRETTY_NAME=")
            .map(|value| value.trim_matches('"').to_string())
    })
}

// ============================================================================
// Android (adb)
// ============================================================================

/// Locate an Android SDK tool on `PATH`, or under `$ANDROID_HOME`/
/// `$ANDROID_SDK_ROOT`'s conventional subdirectory. Shared with
/// `emulators.rs`.
pub(crate) fn find_android_tool(tool: &str, sdk_subdir: &str) -> Option<PathBuf> {
    let found = locate_android_tool(tool, sdk_subdir);
    if found.is_none() {
        crate::ui::debug(format!(
            "probe {tool}: not on PATH and no Android SDK ({sdk_subdir}) to look under"
        ));
    }
    found
}

fn locate_android_tool(tool: &str, sdk_subdir: &str) -> Option<PathBuf> {
    if let Ok(path) = which::which(tool) {
        return Some(path);
    }
    let sdk_root = std::env::var("ANDROID_HOME")
        .or_else(|_| std::env::var("ANDROID_SDK_ROOT"))
        .ok()?;
    let exe = if cfg!(windows) {
        format!("{tool}.exe")
    } else {
        tool.to_string()
    };
    let candidate = Path::new(&sdk_root).join(sdk_subdir).join(exe);
    candidate.exists().then_some(candidate)
}

fn probe_android(details: bool) -> Result<Vec<Device>, Problem> {
    let Some(adb) = find_android_tool("adb", "platform-tools") else {
        return Err(Problem {
            platform: DevicePlatform::Android,
            message: "adb not found".to_string(),
            hint: "install Android SDK platform-tools and put `adb` on PATH, or set ANDROID_HOME"
                .to_string(),
        });
    };

    match output_with_timeout(Command::new(&adb).args(["devices", "-l"]), PROBE_TIMEOUT) {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            Ok(parse_adb_devices(&adb, &text, details))
        }
        Ok(output) => Err(Problem {
            platform: DevicePlatform::Android,
            message: "`adb devices -l` failed".to_string(),
            hint: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::TimedOut => Err(Problem {
            platform: DevicePlatform::Android,
            message: format!("adb did not answer within {PROBE_TIMEOUT:?}"),
            hint: "is the adb server hung? try `adb kill-server`".to_string(),
        }),
        Err(error) => Err(Problem {
            platform: DevicePlatform::Android,
            message: format!("failed to run adb: {error}"),
            hint: "check your Android SDK installation".to_string(),
        }),
    }
}

fn parse_adb_devices(adb: &Path, output: &str, details: bool) -> Vec<Device> {
    output
        .lines()
        .skip(1) // "List of devices attached"
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let id = fields.next()?.to_string();
            let state = fields.next()?;

            let mut props: BTreeMap<String, String> = BTreeMap::new();
            for field in fields {
                if let Some((key, value)) = field.split_once(':') {
                    props.insert(key.to_string(), value.to_string());
                }
            }

            let status = match state {
                "device" => Status::Online,
                "unauthorized" => Status::Unauthorized,
                // "offline" and every other/unrecognized state land here:
                // both mean "not usable right now".
                _ => Status::Offline,
            };
            let kind = if id.starts_with("emulator-") {
                Kind::Emulator
            } else {
                Kind::Physical
            };
            let name = props
                .get("model")
                .map_or_else(|| id.clone(), |model| model.replace('_', " "));

            let mut device_details = BTreeMap::new();
            for key in ["model", "device", "product"] {
                if let Some(value) = props.get(key) {
                    device_details.insert(key.to_string(), value.clone());
                }
            }
            if details
                && status == Status::Online
                && let Some(release) = probe_stdout(
                    Command::new(adb).args([
                        "-s",
                        &id,
                        "shell",
                        "getprop",
                        "ro.build.version.release",
                    ]),
                    PROBE_TIMEOUT,
                )
            {
                device_details.insert("android_version".to_string(), release);
            }

            Some(Device {
                id,
                name,
                platform: DevicePlatform::Android,
                kind,
                status,
                details: device_details,
            })
        })
        .collect()
}

// ============================================================================
// iOS (simctl) — reused by `emulators.rs`
// ============================================================================

/// List available iOS Simulators via `xcrun simctl list -j devices
/// available`. `pub(crate)` so `emulators.rs` can reuse the same probe and
/// JSON parsing for `flui emulators list`/`launch`.
#[cfg(target_os = "macos")]
pub(crate) fn probe_ios() -> Result<Vec<Device>, Problem> {
    if which::which("xcrun").is_err() {
        return Err(Problem {
            platform: DevicePlatform::Ios,
            message: "Xcode command line tools not found".to_string(),
            hint: "install Xcode from the App Store, then run `xcode-select --install`".to_string(),
        });
    }

    match output_with_timeout(
        Command::new("xcrun").args(["simctl", "list", "-j", "devices", "available"]),
        PROBE_TIMEOUT,
    ) {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout);
            parse_simctl_json(&text).map_err(|error| Problem {
                platform: DevicePlatform::Ios,
                message: "could not parse `xcrun simctl` output".to_string(),
                hint: error,
            })
        }
        Ok(output) => Err(Problem {
            platform: DevicePlatform::Ios,
            message: "`xcrun simctl list` failed".to_string(),
            hint: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::TimedOut => Err(Problem {
            platform: DevicePlatform::Ios,
            message: format!("xcrun did not answer within {PROBE_TIMEOUT:?}"),
            hint: "Xcode's simulator service may be hung; try restarting it".to_string(),
        }),
        Err(error) => Err(Problem {
            platform: DevicePlatform::Ios,
            message: format!("failed to run xcrun: {error}"),
            hint: "check your Xcode installation".to_string(),
        }),
    }
}

#[cfg(target_os = "macos")]
fn parse_simctl_json(text: &str) -> Result<Vec<Device>, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    let Some(devices_by_runtime) = value.get("devices").and_then(|d| d.as_object()) else {
        return Ok(Vec::new());
    };

    let mut result = Vec::new();
    for (runtime_key, list) in devices_by_runtime {
        let Some(list) = list.as_array() else {
            continue;
        };
        let runtime = runtime_name(runtime_key);

        for entry in list {
            let is_available = entry
                .get("isAvailable")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            if !is_available {
                continue;
            }

            let name = entry
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown")
                .to_string();
            let udid = entry
                .get("udid")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let state = entry
                .get("state")
                .and_then(|v| v.as_str())
                .unwrap_or("Shutdown");
            let status = match state {
                "Booted" => Status::Booted,
                "Shutdown" => Status::Shutdown,
                _ => Status::Available,
            };

            let mut details = BTreeMap::new();
            details.insert("runtime".to_string(), runtime.clone());

            result.push(Device {
                id: udid,
                name,
                platform: DevicePlatform::Ios,
                kind: Kind::Simulator,
                status,
                details,
            });
        }
    }
    Ok(result)
}

/// `com.apple.CoreSimulator.SimRuntime.iOS-17-2` → `iOS 17.2`.
#[cfg(target_os = "macos")]
fn runtime_name(runtime_key: &str) -> String {
    let Some(suffix) = runtime_key.strip_prefix("com.apple.CoreSimulator.SimRuntime.") else {
        return runtime_key.to_string();
    };
    let Some((os, version)) = suffix.split_once('-') else {
        return suffix.to_string();
    };
    let version = version.replace('-', ".");
    if version.is_empty() {
        os.to_string()
    } else {
        format!("{os} {version}")
    }
}

// ============================================================================
// Web browsers
// ============================================================================

fn probe_browsers() -> Vec<Device> {
    #[cfg(target_os = "macos")]
    {
        macos_browsers()
    }
    #[cfg(target_os = "linux")]
    {
        linux_browsers()
    }
    #[cfg(target_os = "windows")]
    {
        windows_browsers()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Vec::new()
    }
}

/// Version is read from the bundle's `Info.plist`, never by executing the
/// app binary — that is what used to hang on Safari (see module docs).
#[cfg(target_os = "macos")]
fn macos_browsers() -> Vec<Device> {
    const CANDIDATES: &[(&str, &str)] = &[
        ("Chrome", "/Applications/Google Chrome.app"),
        ("Chromium", "/Applications/Chromium.app"),
        ("Edge", "/Applications/Microsoft Edge.app"),
        ("Firefox", "/Applications/Firefox.app"),
        ("Safari", "/Applications/Safari.app"),
        ("Brave", "/Applications/Brave Browser.app"),
    ];

    CANDIDATES
        .iter()
        .filter_map(|(name, bundle)| macos_browser_device(name, bundle))
        .collect()
}

#[cfg(target_os = "macos")]
fn macos_browser_device(name: &str, bundle_path: &str) -> Option<Device> {
    let info_plist = format!("{bundle_path}/Contents/Info.plist");
    if !Path::new(&info_plist).exists() {
        return None;
    }

    let version = probe_stdout(
        Command::new("defaults").args(["read", &info_plist, "CFBundleShortVersionString"]),
        PROBE_TIMEOUT,
    );

    let mut details = BTreeMap::new();
    details.insert("path".to_string(), bundle_path.to_string());
    if let Some(version) = version {
        details.insert("version".to_string(), version);
    }

    Some(Device {
        id: format!("browser:{}", name.to_ascii_lowercase()),
        name: name.to_string(),
        platform: DevicePlatform::Web,
        kind: Kind::Browser,
        status: Status::Available,
        details,
    })
}

#[cfg(target_os = "linux")]
fn linux_browsers() -> Vec<Device> {
    const CANDIDATES: &[(&str, &str)] = &[
        ("Chrome", "google-chrome"),
        ("Chromium", "chromium"),
        ("Edge", "microsoft-edge"),
        ("Firefox", "firefox"),
        ("Brave", "brave-browser"),
    ];

    CANDIDATES
        .iter()
        .filter_map(|(name, command)| linux_browser_device(name, command))
        .collect()
}

#[cfg(target_os = "linux")]
fn linux_browser_device(name: &str, command: &str) -> Option<Device> {
    let Ok(path) = which::which(command) else {
        crate::ui::debug(format!("probe {command}: not on PATH"));
        return None;
    };
    let version = probe_stdout(Command::new(command).arg("--version"), PROBE_TIMEOUT);

    let mut details = BTreeMap::new();
    details.insert("path".to_string(), path.to_string_lossy().into_owned());
    if let Some(version) = version {
        details.insert("version".to_string(), version);
    }

    Some(Device {
        id: format!("browser:{}", name.to_ascii_lowercase()),
        name: name.to_string(),
        platform: DevicePlatform::Web,
        kind: Kind::Browser,
        status: Status::Available,
        details,
    })
}

/// Enumerates installed browsers from the `StartMenuInternet` registry keys
/// and reads each executable's file version in the same bounded PowerShell
/// call, so no browser is ever launched to learn its version.
#[cfg(target_os = "windows")]
fn windows_browsers() -> Vec<Device> {
    const SCRIPT: &str = r#"
        foreach ($hive in @('HKLM', 'HKCU')) {
            $path = "${hive}:\SOFTWARE\Clients\StartMenuInternet"
            if (Test-Path $path) {
                Get-ChildItem $path | ForEach-Object {
                    $name = $_.PSChildName
                    $shell = Get-ItemProperty -Path "$($_.PSPath)\shell\open\command" -ErrorAction SilentlyContinue
                    if ($shell.'(default)') {
                        $exePath = $shell.'(default)' -replace '"', '' -replace ' --.*$', '' -replace ' -%.*$', ''
                        $ver = ""
                        if (Test-Path $exePath) { $ver = (Get-Item $exePath).VersionInfo.FileVersion }
                        Write-Output "BROWSER:$name|$exePath|$ver"
                    }
                }
            }
        }
    "#;

    let Some(output) = probe_stdout(
        Command::new("powershell").args(["-NoProfile", "-Command", SCRIPT]),
        PROBE_TIMEOUT,
    ) else {
        return Vec::new();
    };

    let mut browsers: Vec<Device> = Vec::new();
    for line in output.lines() {
        let Some(rest) = line.strip_prefix("BROWSER:") else {
            continue;
        };
        let mut parts = rest.split('|');
        let (Some(registry_name), Some(path)) = (parts.next(), parts.next()) else {
            continue;
        };
        let version = parts.next().unwrap_or_default().trim();

        if !Path::new(path).exists() {
            continue;
        }

        let display_name = match registry_name {
            "Google Chrome" | "CHROME.EXE" => "Chrome",
            "Microsoft Edge" | "MSEDGE" => "Edge",
            "Firefox" | "FIREFOX.EXE" => "Firefox",
            "Brave" | "BRAVE.EXE" => "Brave",
            "Opera" | "OPERA.EXE" => "Opera",
            "IEXPLORE.EXE" => "Internet Explorer",
            other => other,
        };

        if browsers.iter().any(|b| b.name == display_name) {
            continue;
        }

        let mut details = BTreeMap::new();
        details.insert("path".to_string(), path.to_string());
        if !version.is_empty() {
            details.insert("version".to_string(), version.to_string());
        }

        browsers.push(Device {
            id: format!("browser:{}", display_name.to_ascii_lowercase()),
            name: display_name.to_string(),
            platform: DevicePlatform::Web,
            kind: Kind::Browser,
            status: Status::Available,
            details,
        });
    }
    browsers
}

// ============================================================================
// Command entry point
// ============================================================================

/// Execute `flui devices`.
pub(crate) fn execute(details: bool, platform: Option<DevicePlatform>) -> CliResult<()> {
    let discovery = discover(platform, details);

    if ui::is_json() {
        for device in &discovery.devices {
            ui::emit("device", device);
        }
        ui::emit(
            "devices.summary",
            &serde_json::json!({
                "count": discovery.devices.len(),
                "problems": discovery.problems,
            }),
        );
        return Ok(());
    }

    ui::intro(style(" flui devices ").on_magenta().black())?;

    let sections = match platform {
        Some(p) => vec![p],
        None => vec![
            DevicePlatform::Desktop,
            DevicePlatform::Android,
            DevicePlatform::Ios,
            DevicePlatform::Web,
        ],
    };
    for section in sections {
        render_group(&discovery, section, details)?;
    }

    let count = discovery.devices.len();
    ui::outro(format!(
        "{count} device{} found",
        if count == 1 { "" } else { "s" }
    ))?;

    Ok(())
}

fn render_group(discovery: &Discovery, platform: DevicePlatform, details: bool) -> CliResult<()> {
    let mut lines: Vec<String> = Vec::new();
    for device in discovery.devices.iter().filter(|d| d.platform == platform) {
        lines.extend(format_device_lines(device, details));
    }
    for problem in discovery.problems.iter().filter(|p| p.platform == platform) {
        lines.push(format!(
            "{} {} — {}",
            style("[!]").yellow(),
            problem.message,
            style(&problem.hint).dim()
        ));
    }
    if lines.is_empty() {
        lines.push(format!("{} no devices", style("○").dim()));
    }
    // One box per platform: the list reads as a table, not as a log.
    ui::note(platform_heading(platform), lines.join("\n"))?;
    Ok(())
}

fn platform_heading(platform: DevicePlatform) -> &'static str {
    match platform {
        DevicePlatform::Desktop => "Desktop",
        DevicePlatform::Android => "Android",
        DevicePlatform::Ios => "iOS Simulators",
        DevicePlatform::Web => "Web Browsers",
    }
}

fn format_device_lines(device: &Device, show_details: bool) -> Vec<String> {
    let icon = match device.status {
        Status::Online | Status::Booted | Status::Available => style("●").green(),
        Status::Offline | Status::Shutdown => style("○").dim(),
        Status::Unauthorized => style("●").yellow(),
    };
    let status_word = match device.status {
        Status::Online => "online",
        Status::Offline => "offline",
        Status::Unauthorized => "unauthorized",
        Status::Booted => "booted",
        Status::Shutdown => "shutdown",
        Status::Available => "available",
    };
    // A UDID/serial is what `flui run --device` takes; surface it explicitly
    // rather than only in `--details`, per the CLI's own discoverability
    // requirement.
    let id_hint = match device.kind {
        Kind::Simulator | Kind::Physical | Kind::Emulator => {
            format!(" — use with --device {}", device.id)
        }
        Kind::Host | Kind::Browser => String::new(),
    };

    let mut lines = vec![format!("  {icon} {} ({status_word}){id_hint}", device.name)];
    if show_details {
        for (key, value) in &device.details {
            lines.push(format!("    {key}: {value}"));
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(target_os = "macos")]
    fn runtime_name_formats_ios_and_other_families() {
        assert_eq!(
            runtime_name("com.apple.CoreSimulator.SimRuntime.iOS-17-2"),
            "iOS 17.2"
        );
        assert_eq!(
            runtime_name("com.apple.CoreSimulator.SimRuntime.tvOS-17-0"),
            "tvOS 17.0"
        );
        assert_eq!(runtime_name("unknown-key"), "unknown-key");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn parse_simctl_json_filters_unavailable_and_reads_state() {
        let json = r#"{
            "devices": {
                "com.apple.CoreSimulator.SimRuntime.iOS-17-0": [
                    {"udid": "ABC-123", "name": "iPhone 15 Pro", "state": "Shutdown", "isAvailable": true},
                    {"udid": "DEF-456", "name": "iPhone 15", "state": "Booted", "isAvailable": true},
                    {"udid": "GHI-789", "name": "Unavailable", "state": "Shutdown", "isAvailable": false}
                ]
            }
        }"#;

        let result = parse_simctl_json(json).expect("valid json");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].status, Status::Shutdown);
        assert_eq!(result[1].status, Status::Booted);
        assert_eq!(
            result[1].details.get("runtime").map(String::as_str),
            Some("iOS 17.0")
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn parse_simctl_json_rejects_invalid_json() {
        assert!(parse_simctl_json("not json").is_err());
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn parse_simctl_json_tolerates_missing_devices_key() {
        let result = parse_simctl_json(r#"{"runtimes": []}"#).expect("valid json");
        assert!(result.is_empty());
    }

    #[test]
    fn parse_adb_devices_reads_serial_state_and_model() {
        let output = "List of devices attached\n\
             emulator-5554\tdevice product:sdk_gphone64_arm64 model:sdk_gphone64_arm64 device:emu64a\n\
             0123456789ABCDEF\tunauthorized usb:1-1 product:bullhead model:Nexus_5X device:bullhead\n";
        let devices = parse_adb_devices(Path::new("adb"), output, false);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].kind, Kind::Emulator);
        assert_eq!(devices[0].status, Status::Online);
        assert_eq!(devices[1].kind, Kind::Physical);
        assert_eq!(devices[1].status, Status::Unauthorized);
        assert_eq!(devices[1].name, "Nexus 5X");
    }
}
