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

/// A probe that starts a long-lived daemon must not keep the CLI's own
/// stdout/stderr open: the caller reading them would wait for the daemon to
/// die. `adb devices` starting the adb server is the real case; here a fake
/// `adb` leaves a `ping` running for ~45 s. Windows only, where every
/// inheritable handle reaches the child (see `proc::keep_own_stdio_private`);
/// Unix never passes the parent's descriptors 0–2 on.
#[cfg(windows)]
#[test]
fn devices_output_closes_even_when_a_probe_leaves_a_daemon_running() {
    let fake_tools = TempDir::new().expect("temp dir");
    std::fs::write(
        fake_tools.path().join("adb.bat"),
        "@echo off\r\n\
         start \"\" /b ping -n 46 127.0.0.1 >nul\r\n\
         echo List of devices attached\r\n",
    )
    .expect("write fake adb");
    let path = std::env::join_paths(std::iter::once(fake_tools.path().to_path_buf()).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .expect("PATH entries");

    let started = Instant::now();
    // `assert()` returns only once flui's stdout and stderr reach EOF, so
    // this measures what a caller reading them waits, not flui's exit.
    let assert = flui()
        .args(["devices", "--platform", "android", "--json"])
        .env("PATH", path)
        .assert()
        .success();
    let elapsed = started.elapsed();

    let events = ndjson_events(&assert.get_output().stdout);
    let summary = events
        .iter()
        .find(|e| e["event"] == "devices.summary")
        .expect("devices.summary event");
    assert_eq!(
        summary["problems"],
        serde_json::json!([]),
        "the fake adb must have been found and run"
    );
    // flui itself may spend its full probe budget draining adb's pipe (the
    // daemon holds that one too); the daemon's own ~45 s must not show up.
    assert!(
        elapsed < Duration::from_secs(30),
        "reading flui's output took {elapsed:?}: a daemon started by a probe is holding \
         the CLI's stdout/stderr open"
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
