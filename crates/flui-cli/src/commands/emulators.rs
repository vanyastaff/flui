//! `flui emulators` — list and launch Android AVDs and iOS Simulators.
//!
//! Reuses the [`Device`]/[`Kind`]/[`Status`] model and the bounded iOS
//! `simctl` probe from `devices.rs`. Android AVD enumeration is separate
//! from `devices.rs`'s Android probe: `devices` reports what `adb` can
//! currently see (connected devices/booted emulators); `emulators` reports
//! every *available* AVD, whether running or not, via `emulator -list-avds`.

use crate::commands::devices::{self, Device, Kind, Status};
use crate::error::{CliError, CliResult};
use crate::proc::{PROBE_TIMEOUT, output_with_timeout, probe_stdout};
use crate::ui;
use console::style;
use std::process::Command;

/// List available emulators and simulators.
pub fn execute_list(platform_filter: Option<&str>) -> CliResult<()> {
    let show_android = platform_filter.is_none_or(|p| p.eq_ignore_ascii_case("android"));
    let show_ios = platform_filter.is_none_or(|p| p.eq_ignore_ascii_case("ios"));

    let mut emulators = Vec::new();
    let mut problems = Vec::new();

    if show_android {
        match list_android_avds() {
            Ok(found) => emulators.extend(found),
            Err(problem) => problems.push(problem),
        }
    }

    if show_ios {
        #[cfg(target_os = "macos")]
        match devices::probe_ios() {
            Ok(found) => emulators.extend(found),
            Err(problem) => problems.push(problem),
        }
    }

    if ui::is_json() {
        for emulator in &emulators {
            ui::emit("emulator", emulator);
        }
        ui::emit(
            "emulators.summary",
            &serde_json::json!({
                "count": emulators.len(),
                "problems": problems,
            }),
        );
        return Ok(());
    }

    ui::intro(style(" flui emulators ").on_magenta().black())?;

    let mut lines = if emulators.is_empty() {
        vec!["No emulators or simulators found.".to_string()]
    } else {
        emulator_table(&emulators)
    };
    for problem in &problems {
        lines.push(format!(
            "{} {}: {} — {}",
            style("[!]").yellow(),
            problem.platform_label(),
            problem.message,
            style(&problem.hint).dim()
        ));
    }
    ui::note("Emulators & Simulators", lines.join("\n"))?;

    let count = emulators.len();
    ui::outro(format!(
        "{count} emulator{} found",
        if count == 1 { "" } else { "s" }
    ))?;

    Ok(())
}

/// Launch a specific emulator or simulator by exact id/name, then by unique
/// case-insensitive prefix.
pub fn execute_launch(name: &str) -> CliResult<()> {
    let mut candidates = Vec::new();
    if let Ok(avds) = list_android_avds() {
        candidates.extend(avds);
    }
    #[cfg(target_os = "macos")]
    if let Ok(sims) = devices::probe_ios() {
        candidates.extend(sims);
    }

    let target = resolve_target(&candidates, name)?.clone();

    ui::intro(
        style(format!(" Launching: {} ", target.name))
            .on_cyan()
            .black(),
    )?;
    let spinner = ui::spinner();
    spinner.start(format!("Starting {}…", target.name));

    let launch_result = match target.kind {
        Kind::Emulator => launch_android_avd(&target.id),
        Kind::Simulator => launch_ios_simulator(&target.id),
        Kind::Host | Kind::Physical | Kind::Browser => {
            unreachable!("candidates are only ever Emulator or Simulator kind")
        }
    };

    if let Err(error) = launch_result {
        spinner.error(format!("failed to launch {}", target.name));
        return Err(error);
    }

    spinner.stop(format!("{} launched", target.name));
    ui::emit(
        "emulator.launch",
        &serde_json::json!({ "id": target.id, "name": target.name, "platform": target.platform }),
    );
    ui::outro(format!(
        "Emulator {} is starting",
        style(&target.name).green()
    ))?;

    Ok(())
}

/// Resolve `name` against `candidates`: exact id/name match first, then a
/// unique case-insensitive prefix match.
fn resolve_target<'a>(candidates: &'a [Device], name: &str) -> CliResult<&'a Device> {
    if let Some(exact) = candidates
        .iter()
        .find(|d| d.id.eq_ignore_ascii_case(name) || d.name.eq_ignore_ascii_case(name))
    {
        return Ok(exact);
    }

    let needle = name.to_ascii_lowercase();
    let prefix_matches: Vec<&Device> = candidates
        .iter()
        .filter(|d| {
            d.id.to_ascii_lowercase().starts_with(&needle)
                || d.name.to_ascii_lowercase().starts_with(&needle)
        })
        .collect();

    match prefix_matches.as_slice() {
        [one] => Ok(one),
        [] => Err(CliError::DeviceNotFound {
            name: name.to_string(),
            hint: "run `flui emulators list`".to_string(),
        }),
        many => Err(CliError::DeviceNotFound {
            name: name.to_string(),
            hint: format!(
                "ambiguous prefix; candidates: {}",
                many.iter()
                    .map(|d| d.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }),
    }
}

// ============================================================================
// Android AVDs
// ============================================================================

fn list_android_avds() -> Result<Vec<Device>, devices::Problem> {
    let Some(emulator_path) = devices::find_android_tool("emulator", "emulator") else {
        return Err(devices::Problem {
            platform: crate::DevicePlatform::Android,
            message: "`emulator` tool not found".to_string(),
            hint: "install the Android Emulator package via Android Studio's SDK Manager, or set ANDROID_HOME"
                .to_string(),
        });
    };

    match output_with_timeout(
        Command::new(&emulator_path).arg("-list-avds"),
        PROBE_TIMEOUT,
    ) {
        Ok(output) if output.status.success() => {
            let names: Vec<String> = String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_string)
                .collect();
            let running = running_android_avd_names();

            Ok(names
                .into_iter()
                .map(|name| {
                    let status = if running.iter().any(|r| r.eq_ignore_ascii_case(&name)) {
                        Status::Booted
                    } else {
                        Status::Shutdown
                    };
                    Device {
                        id: name.clone(),
                        name,
                        platform: crate::DevicePlatform::Android,
                        kind: Kind::Emulator,
                        status,
                        details: std::collections::BTreeMap::new(),
                    }
                })
                .collect())
        }
        Ok(output) => Err(devices::Problem {
            platform: crate::DevicePlatform::Android,
            message: "`emulator -list-avds` failed".to_string(),
            hint: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::TimedOut => Err(devices::Problem {
            platform: crate::DevicePlatform::Android,
            message: format!("`emulator -list-avds` did not answer within {PROBE_TIMEOUT:?}"),
            hint: "the Android Emulator tool may be hung".to_string(),
        }),
        Err(error) => Err(devices::Problem {
            platform: crate::DevicePlatform::Android,
            message: format!("failed to run `emulator`: {error}"),
            hint: "check your Android SDK installation".to_string(),
        }),
    }
}

/// AVD names of currently-running Android emulators, via `adb devices` plus
/// `adb -s <serial> emu avd name` for each `emulator-*` serial. Best-effort:
/// any failure just yields an empty list, since this only affects the
/// reported `Status`, not whether the AVD is listed at all.
fn running_android_avd_names() -> Vec<String> {
    let Some(adb) = devices::find_android_tool("adb", "platform-tools") else {
        return Vec::new();
    };

    let Ok(output) = output_with_timeout(Command::new(&adb).args(["devices"]), PROBE_TIMEOUT)
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .skip(1) // "List of devices attached"
        .filter(|line| line.starts_with("emulator-") && line.contains("device"))
        .filter_map(|line| line.split_whitespace().next())
        .filter_map(|serial| {
            let raw = probe_stdout(
                Command::new(&adb).args(["-s", serial, "emu", "avd", "name"]),
                PROBE_TIMEOUT,
            )?;
            let name = raw.lines().next().unwrap_or_default().trim().to_string();
            (!name.is_empty()).then_some(name)
        })
        .collect()
}

fn launch_android_avd(avd_name: &str) -> CliResult<()> {
    let Some(emulator_path) = devices::find_android_tool("emulator", "emulator") else {
        return Err(CliError::ToolNotFound {
            tool: "emulator".to_string(),
            suggestion: "install the Android Emulator package via Android Studio's SDK Manager"
                .to_string(),
        });
    };

    // Detached: the emulator is a long-running process, not a probe. We
    // never wait for it to exit.
    Command::new(&emulator_path)
        .args(["-avd", avd_name])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| {
            CliError::context(
                error,
                format!("failed to launch Android emulator '{avd_name}'"),
            )
        })?;

    Ok(())
}

// ============================================================================
// iOS Simulators
// ============================================================================

fn launch_ios_simulator(udid: &str) -> CliResult<()> {
    let boot = output_with_timeout(
        Command::new("xcrun").args(["simctl", "boot", udid]),
        PROBE_TIMEOUT,
    )
    .map_err(|error| CliError::context(error, "failed to run `xcrun simctl boot`"))?;

    if !boot.status.success() {
        let stderr = String::from_utf8_lossy(&boot.stderr);
        let already_booted = boot.status.code() == Some(164)
            || stderr.contains("Unable to boot device in current state: Booted");
        if !already_booted {
            return Err(CliError::CommandFailed {
                context: format!("xcrun simctl boot {udid}"),
                exit_code: boot.status.code(),
            });
        }
        crate::ui::debug(format!("simulator {udid} was already booted"));
    }

    // `open -a Simulator` returns as soon as the app is asked to activate;
    // it does not block on the simulator finishing boot.
    Command::new("open")
        .args(["-a", "Simulator"])
        .spawn()
        .map_err(|error| CliError::context(error, "failed to open the Simulator app"))?;

    Ok(())
}

fn emulator_table(emulators: &[Device]) -> Vec<String> {
    let platform_w = 8;
    let name_w = emulators
        .iter()
        .map(|e| e.name.chars().count())
        .max()
        .unwrap_or(4)
        .max(4);

    let mut lines = vec![format!(
        "{:<platform_w$} {:<name_w$} {:<9} {}",
        style("Platform").bold(),
        style("Name").bold(),
        style("Status").bold(),
        style("Id (--device)").bold(),
    )];

    for emu in emulators {
        let status = match emu.status {
            Status::Booted => style("booted").green().to_string(),
            Status::Shutdown => style("shutdown").dim().to_string(),
            Status::Online | Status::Offline | Status::Unauthorized | Status::Available => {
                style("unknown").dim().to_string()
            }
        };
        let platform_label = match emu.platform {
            crate::DevicePlatform::Android => "Android",
            crate::DevicePlatform::Ios => "iOS",
            crate::DevicePlatform::Desktop | crate::DevicePlatform::Web => "-",
        };
        lines.push(format!(
            "{platform_label:<platform_w$} {:<name_w$} {status:<9} {}",
            emu.name,
            style(&emu.id).dim()
        ));
    }

    lines
}

impl devices::Problem {
    fn platform_label(&self) -> &'static str {
        match self.platform {
            crate::DevicePlatform::Android => "Android",
            crate::DevicePlatform::Ios => "iOS",
            crate::DevicePlatform::Desktop => "Desktop",
            crate::DevicePlatform::Web => "Web",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn avd(name: &str, status: Status) -> Device {
        Device {
            id: name.to_string(),
            name: name.to_string(),
            platform: crate::DevicePlatform::Android,
            kind: Kind::Emulator,
            status,
            details: BTreeMap::new(),
        }
    }

    #[test]
    fn resolve_target_matches_exact_name_case_insensitively() {
        let candidates = vec![avd("Pixel_7_API_34", Status::Shutdown)];
        let found = resolve_target(&candidates, "pixel_7_api_34").expect("exact match");
        assert_eq!(found.name, "Pixel_7_API_34");
    }

    #[test]
    fn resolve_target_matches_unique_prefix() {
        let candidates = vec![avd("Pixel_7_API_34", Status::Shutdown)];
        let found = resolve_target(&candidates, "pixel").expect("unique prefix");
        assert_eq!(found.name, "Pixel_7_API_34");
    }

    #[test]
    fn resolve_target_reports_ambiguous_prefix() {
        let candidates = vec![
            avd("Pixel_6", Status::Shutdown),
            avd("Pixel_7", Status::Shutdown),
        ];
        let error = resolve_target(&candidates, "pixel").expect_err("ambiguous");
        assert!(matches!(error, CliError::DeviceNotFound { .. }));
    }

    #[test]
    fn resolve_target_reports_unknown_name() {
        let candidates = vec![avd("Pixel_7", Status::Shutdown)];
        let error = resolve_target(&candidates, "definitely-not-an-emulator").expect_err("unknown");
        assert!(matches!(error, CliError::DeviceNotFound { .. }));
    }
}
