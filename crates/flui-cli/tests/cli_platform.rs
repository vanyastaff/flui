//! Integration tests for `flui platform` command.
//!
//! Tests the platform list subcommand and validates output.
//! Note: cliclack writes all interactive output to stderr.

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use tempfile::TempDir;

/// Get a command for the `flui` binary.
fn flui() -> Command {
    cargo_bin_cmd!("flui")
}

/// A temp dir containing a minimal `flui.toml` — enough for `FluiConfig::load`.
fn project_with_minimal_config() -> TempDir {
    let tmp = TempDir::new().expect("temp dir");
    std::fs::write(
        tmp.path().join("flui.toml"),
        "[app]\nname = \"test-app\"\nversion = \"0.1.0\"\norganization = \"com.example\"\n",
    )
    .expect("write flui.toml");
    tmp
}

/// A temp dir with a `flui.toml` that already targets `android`, so
/// `platform remove android` has something to remove.
fn project_with_android_configured() -> TempDir {
    let tmp = TempDir::new().expect("temp dir");
    std::fs::write(
        tmp.path().join("flui.toml"),
        "[app]\nname = \"test-app\"\nversion = \"0.1.0\"\norganization = \"com.example\"\n\
         [build]\ntarget_platforms = [\"android\"]\n",
    )
    .expect("write flui.toml");
    tmp
}

#[test]
fn platform_list_runs_successfully() {
    flui().args(["platform", "list"]).assert().success();
}

#[test]
fn platform_list_shows_android() {
    // cliclack outputs to stderr
    flui()
        .args(["platform", "list"])
        .assert()
        .success()
        .stderr(predicate::str::contains("android"));
}

#[test]
fn platform_list_shows_ios() {
    flui()
        .args(["platform", "list"])
        .assert()
        .success()
        .stderr(predicate::str::contains("ios"));
}

#[test]
fn platform_list_shows_web() {
    flui()
        .args(["platform", "list"])
        .assert()
        .success()
        .stderr(predicate::str::contains("web"));
}

#[test]
fn platform_list_shows_desktop_platforms() {
    flui().args(["platform", "list"]).assert().success().stderr(
        predicate::str::contains("windows")
            .and(predicate::str::contains("linux"))
            .and(predicate::str::contains("macos")),
    );
}

#[test]
fn platform_add_without_args_shows_message() {
    // `flui platform add` with no platform names should indicate no platforms specified
    // cliclack outputs to stderr
    flui()
        .args(["platform", "add"])
        .assert()
        .success()
        .stderr(predicate::str::contains("No platforms specified"));
}

#[test]
fn platform_list_json_is_pure_ndjson() {
    let tmp = project_with_minimal_config();

    let output = flui()
        .current_dir(tmp.path())
        .args(["--json", "platform", "list"])
        .output()
        .expect("flui platform list runs");
    assert!(output.status.success());

    let text = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(
        lines.len(),
        1,
        "expected exactly one NDJSON line: {lines:?}"
    );

    let event: serde_json::Value = serde_json::from_str(lines[0]).expect("valid JSON");
    assert_eq!(event["event"], "platform.list");
    assert_eq!(
        event["platforms"]
            .as_array()
            .expect("platforms array")
            .len(),
        6
    );
}

#[test]
fn platform_add_rejects_an_unknown_platform_and_lists_valid_ones() {
    let tmp = project_with_minimal_config();

    flui()
        .current_dir(tmp.path())
        .args(["platform", "add", "fuchsia"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("Invalid platform").and(predicate::str::contains("android")),
        );
}

#[test]
fn platform_remove_rejects_an_unknown_platform_name() {
    let tmp = project_with_minimal_config();

    flui()
        .current_dir(tmp.path())
        .args(["platform", "remove", "fuchsia"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Invalid platform"));
}

#[test]
fn platform_remove_without_yes_is_non_interactive_and_exits_7() {
    let tmp = project_with_android_configured();

    flui()
        .current_dir(tmp.path())
        .args(["platform", "remove", "android"])
        .assert()
        .failure()
        .code(7)
        .stderr(predicate::str::contains("--yes"));
}

#[test]
fn platform_remove_with_yes_skips_the_prompt_and_updates_the_config() {
    let tmp = project_with_android_configured();

    flui()
        .current_dir(tmp.path())
        .args(["platform", "remove", "android", "--yes"])
        .assert()
        .success();

    let config = std::fs::read_to_string(tmp.path().join("flui.toml")).expect("read flui.toml");
    assert!(
        !config.contains("android"),
        "android should be removed from target_platforms: {config}"
    );
}
