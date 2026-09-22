//! Integration tests for `flui devices` and `flui emulators`.
//!
//! Hermetic by construction: none of these depend on real `adb`/Xcode being
//! present. The "problems, not errors" tests instead force their absence by
//! clearing `PATH`. This file also carries the regression test for the
//! macOS `flui devices` hang (launching Safari's GUI to read its version).

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use std::time::{Duration, Instant};
use tempfile::TempDir;

fn flui() -> Command {
    cargo_bin_cmd!("flui")
}

/// Parse stdout as NDJSON, panicking on the first line that is not valid
/// JSON — the assertion a "pure NDJSON stream" test actually needs.
fn ndjson_events(stdout: &[u8]) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(stdout)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|error| panic!("stdout line is not valid JSON ({error}): {line:?}"))
        })
        .collect()
}

#[test]
fn devices_json_reports_at_least_the_host_desktop() {
    let assert = flui().args(["devices", "--json"]).assert().success();
    let events = ndjson_events(&assert.get_output().stdout);

    let device_events: Vec<_> = events.iter().filter(|e| e["event"] == "device").collect();
    assert!(
        !device_events.is_empty(),
        "expected at least one device event"
    );
    assert!(
        device_events.iter().any(|e| e["platform"] == "desktop"),
        "the host desktop must always be reported: {device_events:?}"
    );

    let summaries: Vec<_> = events
        .iter()
        .filter(|e| e["event"] == "devices.summary")
        .collect();
    assert_eq!(
        summaries.len(),
        1,
        "expected exactly one devices.summary event"
    );
    assert!(summaries[0]["count"].as_u64().unwrap() >= 1);
}

#[test]
fn devices_json_platform_filter_desktop_only() {
    let assert = flui()
        .args(["devices", "--platform", "desktop", "--json"])
        .assert()
        .success();
    let events = ndjson_events(&assert.get_output().stdout);

    let device_events: Vec<_> = events.iter().filter(|e| e["event"] == "device").collect();
    assert!(!device_events.is_empty());
    for event in device_events {
        assert_eq!(event["platform"], "desktop");
    }
}

#[test]
fn devices_human_mode_writes_nothing_to_stdout() {
    let assert = flui().arg("devices").assert().success();
    assert_eq!(
        assert.get_output().stdout,
        b"".to_vec(),
        "human-mode narration must go to stderr, never stdout"
    );
}

#[test]
fn devices_with_no_tools_on_path_reports_problems_not_errors() {
    let empty_path = TempDir::new().expect("temp dir");

    let assert = flui()
        .args(["devices", "--json"])
        .env_clear()
        .env("PATH", empty_path.path())
        .assert()
        .success();

    let events = ndjson_events(&assert.get_output().stdout);

    // The desktop device is still reported: a missing tool never fails the
    // whole command.
    assert!(
        events
            .iter()
            .any(|e| e["event"] == "device" && e["platform"] == "desktop"),
        "desktop must still be reported even with no tools on PATH"
    );

    let summary = events
        .iter()
        .find(|e| e["event"] == "devices.summary")
        .expect("devices.summary event");
    let problems = summary["problems"].as_array().expect("problems array");
    assert!(
        !problems.is_empty(),
        "missing adb (and xcrun on macOS) must surface as problems"
    );
    assert!(
        problems.iter().any(|p| p["platform"] == "android"),
        "expected an android problem row: {problems:?}"
    );

    #[cfg(target_os = "macos")]
    assert!(
        problems.iter().any(|p| p["platform"] == "ios"),
        "xcrun lives on PATH (/usr/bin), so an empty PATH must turn iOS into a problem too: {problems:?}"
    );
}

/// Regression guard for the bug this module was built to fix: `flui
/// devices` used to shell out to Safari's binary to read its version, which
/// launches the GUI and never returns.
///
/// A `--version` baseline is still measured (and still reported in the
/// panic message) as a diagnostic, but it is not used to scale the bound
/// down — a first version of this test used `(baseline * 5).max(5s)`, and
/// it flaked on *both* `ubuntu-latest` and `cli-macos` the same day it
/// landed: `--version` pays only fork+exec/dynamic-linking cost, near-zero
/// on a quiet runner (1.5ms / 5.3ms observed), while `devices` does real
/// subprocess probing (`adb`/`xcrun`) whose cost has nothing to do with
/// process-spawn overhead — so `baseline * 5` collapsed to a bound tighter
/// than `devices`' own legitimate work (9.15s / 11.26s observed, both
/// comfortably normal). The ratio was never meaningful; only the floor
/// was ever doing anything, so the floor is now the whole bound, set well
/// above every real number seen for `devices` on this runner class so far
/// (the original flake's 23s/28.9s under the old flat `< 20s`, and the
/// 9.15s/11.26s above) — 60s. Safari's GUI launching and never returning
/// hangs indefinitely, not for tens of seconds, so 60s stays a wide
/// margin below the regression this guards against while comfortably
/// clearing realistic shared-runner noise.
#[test]
fn devices_finishes_well_under_the_old_gui_launch_hang() {
    let baseline_started = Instant::now();
    flui().arg("--version").assert().success();
    let baseline = baseline_started.elapsed();

    let started = Instant::now();
    flui().arg("devices").assert().success();
    let elapsed = started.elapsed();

    let threshold = Duration::from_secs(60);
    assert!(
        elapsed < threshold,
        "flui devices took {elapsed:?} (baseline `flui --version`: {baseline:?}, threshold: \
         {threshold:?}) -- regression of the Safari GUI-launch hang?"
    );
}

#[test]
fn emulators_launch_unknown_name_exits_device_not_found() {
    flui()
        .args(["emulators", "launch", "definitely-not-an-emulator"])
        .assert()
        .failure()
        .code(5);
}

#[test]
fn emulators_list_json_is_pure_ndjson_and_succeeds() {
    let assert = flui()
        .args(["emulators", "list", "--json"])
        .assert()
        .success();
    // Parsing alone is the assertion: any non-JSON line panics.
    let _events = ndjson_events(&assert.get_output().stdout);
}

/// `-v` is the CLI's whole logging story: probe diagnostics appear as dimmed
/// `debug:` lines on stderr only under `--verbose`, in JSON mode too (stderr
/// is not the machine stream), and never otherwise.
#[test]
fn verbose_prints_probe_diagnostics_to_stderr_only() {
    let empty_path = TempDir::new().expect("temp dir");

    let verbose = flui()
        .args(["devices", "--json", "--verbose", "--color", "never"])
        .env_clear()
        .env("PATH", empty_path.path())
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&verbose.get_output().stderr);
    assert!(
        stderr.contains("debug: probe"),
        "with no tools on PATH every probe fails, and -v must say so; stderr was:\n{stderr}"
    );
    for line in String::from_utf8_lossy(&verbose.get_output().stdout).lines() {
        assert!(
            line.starts_with('{'),
            "diagnostics must never reach the JSON stream; stdout line: {line}"
        );
    }

    let quiet = flui()
        .args(["devices", "--json"])
        .env_clear()
        .env("PATH", empty_path.path())
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&quiet.get_output().stderr);
    assert!(
        !stderr.contains("debug:"),
        "without -v there are no diagnostics; stderr was:\n{stderr}"
    );
}
