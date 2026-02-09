//! Integration tests for `flui doctor` command.
//!
//! Tests that the doctor command runs successfully and produces expected output.
//! Note: cliclack writes all interactive output to stderr.

use assert_cmd::cargo::cargo_bin_cmd;
use assert_cmd::Command;
use predicates::prelude::*;

/// Get a command for the `flui` binary.
fn flui() -> Command {
    cargo_bin_cmd!("flui")
}

#[test]
fn doctor_runs_successfully() {
    flui().args(["doctor"]).assert().success();
}

#[test]
fn doctor_detects_rust() {
    // cliclack outputs to stderr
    flui()
        .args(["doctor"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Rust"));
}

#[test]
fn doctor_detects_cargo() {
    flui()
        .args(["doctor"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Cargo"));
}

#[test]
fn doctor_shows_flui_version() {
    flui()
        .args(["doctor"])
        .assert()
        .success()
        .stderr(predicate::str::contains("FLUI CLI"));
}

#[test]
fn doctor_verbose_runs_successfully() {
    flui().args(["doctor", "--verbose"]).assert().success();
}

#[test]
fn doctor_android_only() {
    // Should succeed even if Android SDK is not installed (just reports status)
    flui().args(["doctor", "--android"]).assert().success();
}

#[test]
fn doctor_web_only() {
    flui().args(["doctor", "--web"]).assert().success();
}
