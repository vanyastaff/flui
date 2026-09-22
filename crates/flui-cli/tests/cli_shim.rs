//! `cargo flui` is the same CLI: the shim forwards arguments, output and the
//! exit code of the `flui` binary built next to it.

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

#[test]
fn cargo_subcommand_spelling_drops_the_repeated_name() {
    let expected = cargo_bin_cmd!("flui").arg("--version").output().unwrap();
    let shimmed = cargo_bin_cmd!("cargo-flui")
        .args(["flui", "--version"])
        .output()
        .unwrap();
    assert!(shimmed.status.success());
    assert_eq!(shimmed.stdout, expected.stdout);
}

#[test]
fn direct_invocation_without_the_name_also_works() {
    cargo_bin_cmd!("cargo-flui")
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("flui "));
}

#[test]
fn exit_code_and_stderr_are_the_clis() {
    cargo_bin_cmd!("cargo-flui")
        .args(["flui", "no-such-command"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("no-such-command"));
}

#[test]
fn json_events_pass_through_on_stdout() {
    cargo_bin_cmd!("cargo-flui")
        .args(["flui", "--json", "completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"event\":\"completions\""));
}
