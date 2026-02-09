//! Integration tests for CLI error handling.
//!
//! Tests that invalid inputs produce appropriate error messages and non-zero exit codes.

use assert_cmd::cargo::cargo_bin_cmd;
use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

/// Get a command for the `flui` binary.
fn flui() -> Command {
    cargo_bin_cmd!("flui")
}

#[test]
fn create_with_rust_keyword_fails() {
    let tmp = TempDir::new().expect("temp dir");

    flui()
        .args(["create", "fn", "--org", "com.test"])
        .arg("--path")
        .arg(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Invalid project name"));
}

#[test]
fn create_with_another_keyword_fails() {
    let tmp = TempDir::new().expect("temp dir");

    flui()
        .args(["create", "struct", "--org", "com.test"])
        .arg("--path")
        .arg(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Invalid project name"));
}

#[test]
fn create_with_leading_digit_fails() {
    let tmp = TempDir::new().expect("temp dir");

    flui()
        .args(["create", "123bad", "--org", "com.test"])
        .arg("--path")
        .arg(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Invalid project name"));
}

#[test]
fn create_with_spaces_in_name_fails() {
    let tmp = TempDir::new().expect("temp dir");

    // Clap will treat "my app" as two positional args — the second is invalid
    flui()
        .args(["create", "my app", "--org", "com.test"])
        .arg("--path")
        .arg(tmp.path())
        .assert()
        .failure();
}

#[test]
fn create_with_invalid_org_fails() {
    let tmp = TempDir::new().expect("temp dir");

    flui()
        .args(["create", "good-name", "--org", "com..invalid"])
        .arg("--path")
        .arg(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("Invalid organization ID"));
}

#[test]
fn build_with_invalid_platform_fails() {
    // `flui build foobar` should fail because "foobar" is not a valid build target
    flui()
        .args(["build", "foobar"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value"));
}

#[test]
fn unknown_subcommand_fails() {
    flui()
        .args(["nonexistent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized subcommand"));
}
