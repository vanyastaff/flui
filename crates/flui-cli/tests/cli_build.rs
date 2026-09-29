//! Integration tests for `flui build`.

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Get a command for the `flui` binary.
fn flui() -> Command {
    cargo_bin_cmd!("flui")
}

/// Workspace root — this crate lives at `<root>/crates/flui-cli`.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("BUG: flui-cli must sit two levels below the workspace root")
        .canonicalize()
        .expect("canonical workspace root")
}

/// Parse a captured stdout stream as NDJSON: one `serde_json::Value` per
/// non-empty line, panicking (with the offending line) on any line that
/// is not valid JSON — a `--json` run must never interleave plain text.
fn ndjson_events(stdout: &[u8]) -> Vec<serde_json::Value> {
    let text = String::from_utf8(stdout.to_vec()).expect("stdout is UTF-8");
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("not one JSON object per line ({e}): {line:?}"))
        })
        .collect()
}

#[test]
fn desktop_build_outside_a_project_in_json_mode_emits_a_pure_error_event() {
    let tmp = TempDir::new().expect("empty temp dir");

    let output = flui()
        .current_dir(tmp.path())
        .args(["build", "desktop", "--json"])
        .output()
        .expect("run flui build desktop --json");

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(6));

    // The stdout stream is pure NDJSON: every line parses, and the error
    // event carries the exit code and a message naming the missing file.
    let events = ndjson_events(&output.stdout);
    let error_event = events
        .iter()
        .find(|event| event["event"] == "error")
        .unwrap_or_else(|| panic!("no `error` event in {events:?}"));
    assert_eq!(error_event["code"], 6);
    assert!(
        error_event["message"]
            .as_str()
            .expect("message is a string")
            .contains("Cargo.toml not found")
    );
}

/// Real end-to-end build: scaffolds a project against this checkout and
/// builds it for desktop, gated behind `FLUI_CLI_LIVE_BUILD=1` because it
/// runs a genuine `cargo build` (slow, and needs a working toolchain).
#[test]
fn live_desktop_build_produces_an_artifact_on_disk() {
    if std::env::var_os("FLUI_CLI_LIVE_BUILD").is_none() {
        eprintln!(
            "skipping live_desktop_build_produces_an_artifact_on_disk: set FLUI_CLI_LIVE_BUILD=1 to run a real `flui build desktop`"
        );
        return;
    }

    let repo_root = repo_root();
    let workdir = TempDir::new().expect("scaffold workdir");
    let name = "flui-live-build-check";
    let project = workdir.path().join(name);

    flui()
        .current_dir(&repo_root)
        .env("CARGO_NET_OFFLINE", "true")
        .args([
            "create",
            name,
            "--org",
            "com.test",
            "--template",
            "empty",
            "--no-check",
        ])
        .arg(format!("--local={}", repo_root.display()))
        .arg("--path")
        .arg(workdir.path())
        .assert()
        .success();

    // Seed the resolved versions so the build does not need network access.
    std::fs::copy(repo_root.join("Cargo.lock"), project.join("Cargo.lock"))
        .expect("seed generated project with the workspace's resolved versions");

    let output = flui()
        .current_dir(&project)
        .env("CARGO_NET_OFFLINE", "true")
        .args(["build", "desktop", "--json"])
        .output()
        .expect("run flui build desktop --json");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let events = ndjson_events(&output.stdout);
    let done = events
        .iter()
        .find(|event| event["event"] == "build.done")
        .unwrap_or_else(|| panic!("no `build.done` event in {events:?}"));
    assert_eq!(done["ok"], true);
    let artifacts = done["artifacts"].as_array().expect("artifacts array");
    assert_eq!(
        artifacts.len(),
        1,
        "expected exactly one artifact: {artifacts:?}"
    );
    let kind = artifacts[0]["kind"].as_str().expect("artifact kind");
    assert!(
        matches!(kind, "binary" | "app-bundle"),
        "unexpected desktop artifact kind: {kind}"
    );
    let path = artifacts[0]["path"].as_str().expect("artifact path");
    assert!(
        Path::new(path).exists(),
        "reported artifact path does not exist: {path}"
    );
}
