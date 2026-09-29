//! Integration tests for `flui devices` and `flui emulators`.
//!
//! Hermetic by construction: none of these depend on real `adb`/Xcode being
//! present. The "problems, not errors" tests instead force their absence by
//! clearing `PATH`. This file also carries the regression test for the
//! macOS `flui devices` hang (launching Safari's GUI to read its version).

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;

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
    assert!(summaries[0]["count"].as_u64().expect("count is a number") >= 1);
}
