//! Integration tests for `flui completions` command.
//!
//! Tests that shell completion scripts are generated correctly.

use assert_cmd::cargo::cargo_bin_cmd;
use assert_cmd::Command;
use predicates::prelude::*;

/// Get a command for the `flui` binary.
fn flui() -> Command {
    cargo_bin_cmd!("flui")
}

#[test]
fn completions_bash() {
    flui()
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("complete"));
}

#[test]
fn completions_powershell() {
    flui()
        .args(["completions", "powershell"])
        .assert()
        .success()
        .stdout(predicate::str::contains("flui"));
}

#[test]
fn completions_zsh() {
    flui()
        .args(["completions", "zsh"])
        .assert()
        .success()
        .stdout(predicate::str::contains("flui"));
}

#[test]
fn completions_fish() {
    flui()
        .args(["completions", "fish"])
        .assert()
        .success()
        .stdout(predicate::str::contains("flui"));
}

#[test]
fn completions_elvish() {
    flui()
        .args(["completions", "elvish"])
        .assert()
        .success()
        .stdout(predicate::str::contains("flui"));
}

#[test]
fn completions_invalid_shell() {
    flui()
        .args(["completions", "invalid-shell"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid value"));
}
