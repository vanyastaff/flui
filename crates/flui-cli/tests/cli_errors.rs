//! Integration tests for CLI error handling.
//!
//! Tests that invalid inputs produce appropriate error messages and non-zero exit codes.

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use tempfile::TempDir;

/// Get a command for the `flui` binary.
fn flui() -> Command {
    cargo_bin_cmd!("flui")
}

#[test]
fn create_with_unknown_template_exits_with_usage_error() {
    let tmp = TempDir::new().expect("temp dir");

    flui()
        .args([
            "create",
            "good-name",
            "--org",
            "com.test",
            "--template",
            "bogus",
        ])
        .arg("--path")
        .arg(tmp.path())
        .assert()
        .failure()
        .code(2);
}
