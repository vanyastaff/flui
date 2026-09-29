//! Integration tests for `flui platform` command.
//!
//! Tests the platform list subcommand and validates output.
//! Note: cliclack writes all interactive output to stderr.

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use tempfile::TempDir;

/// Get a command for the `flui` binary.
fn flui() -> Command {
    cargo_bin_cmd!("flui")
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

pub fn platform_remove_with_yes_skips_the_prompt_and_updates_the_config() {
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
