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

fn assert_invalid_project_name(name: &str) {
    let tmp = TempDir::new().expect("temp dir");
    flui()
        .args(["create", name, "--org", "com.test", "--no-check"])
        .arg("--path")
        .arg(tmp.path())
        .assert()
        .failure()
        .stderr(predicates::str::contains("invalid project name"));
}

fn an_empty_project_name_is_rejected() {
    assert_invalid_project_name("");
}
fn a_numeric_project_name_prefix_is_rejected() {
    assert_invalid_project_name("123app");
}
fn a_project_name_containing_space_is_rejected() {
    assert_invalid_project_name("my app");
}
fn a_project_name_containing_dot_is_rejected() {
    assert_invalid_project_name("my.app");
}
fn a_fn_project_name_is_rejected() {
    assert_invalid_project_name("fn");
}
fn a_struct_project_name_is_rejected() {
    assert_invalid_project_name("struct");
}
fn a_gen_project_name_is_rejected() {
    assert_invalid_project_name("gen");
}
fn a_try_project_name_is_rejected() {
    assert_invalid_project_name("try");
}

#[test]
fn create_with_an_invalid_project_name_is_rejected() {
    crate::test_cases::run_cases(&[
        (
            "an_empty_project_name_is_rejected",
            an_empty_project_name_is_rejected,
        ),
        (
            "a_numeric_project_name_prefix_is_rejected",
            a_numeric_project_name_prefix_is_rejected,
        ),
        (
            "a_project_name_containing_space_is_rejected",
            a_project_name_containing_space_is_rejected,
        ),
        (
            "a_project_name_containing_dot_is_rejected",
            a_project_name_containing_dot_is_rejected,
        ),
        (
            "a_fn_project_name_is_rejected",
            a_fn_project_name_is_rejected,
        ),
        (
            "a_struct_project_name_is_rejected",
            a_struct_project_name_is_rejected,
        ),
        (
            "a_gen_project_name_is_rejected",
            a_gen_project_name_is_rejected,
        ),
        (
            "a_try_project_name_is_rejected",
            a_try_project_name_is_rejected,
        ),
    ]);
}
