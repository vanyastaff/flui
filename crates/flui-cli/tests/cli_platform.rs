//! Integration tests for `flui platform` command.
//!
//! Tests the platform list subcommand and validates output.
//! Note: cliclack writes all interactive output to stderr.

use assert_cmd::cargo::cargo_bin_cmd;
use assert_cmd::Command;
use predicates::prelude::*;

/// Get a command for the `flui` binary.
fn flui() -> Command {
    cargo_bin_cmd!("flui")
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
