//! Integration tests for CLI error handling.
//!
//! Tests that invalid inputs produce appropriate error messages and non-zero exit codes.

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
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
        .stderr(predicate::str::contains("invalid project name"));
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
        .stderr(predicate::str::contains("invalid project name"));
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
        .stderr(predicate::str::contains("invalid project name"));
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
        .stderr(predicate::str::contains("invalid organization ID"));
}

#[test]
fn create_with_invalid_name_exits_with_generic_failure() {
    let tmp = TempDir::new().expect("temp dir");

    flui()
        .args(["create", "fn", "--org", "com.test"])
        .arg("--path")
        .arg(tmp.path())
        .assert()
        .failure()
        .code(1);
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

#[test]
fn create_into_existing_directory_exits_with_generic_failure() {
    let tmp = TempDir::new().expect("temp dir");
    std::fs::create_dir(tmp.path().join("taken")).expect("pre-existing directory");

    flui()
        .args(["create", "taken", "--org", "com.test", "--no-check"])
        .arg("--path")
        .arg(tmp.path())
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("already exists"));
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

#[test]
fn desktop_build_reports_its_underlying_selection_error() {
    let tmp = TempDir::new().expect("temp dir");
    std::fs::create_dir(tmp.path().join("src")).expect("src");
    std::fs::write(tmp.path().join("Cargo.toml"), "[workspace]\n[package]\nname = \"library-only\"\nversion = \"0.1.0\"\nedition = \"2024\"\n").expect("manifest");
    std::fs::write(tmp.path().join("src/lib.rs"), "pub fn helper() {}\n").expect("library");
    flui()
        .current_dir(tmp.path())
        .args(["build", "desktop"])
        .env("CARGO_NET_OFFLINE", "true")
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("failed to build the binary")
                .and(predicate::str::contains("expected one executable"))
                .and(predicate::str::contains("package.default-run")),
        );
}

#[test]
fn desktop_build_stages_the_cargo_artifact_with_configured_app_identity() {
    let tmp = TempDir::new().expect("temp dir");
    let app = tmp.path().join("source");
    std::fs::create_dir_all(app.join("src")).expect("src");
    std::fs::write(app.join("Cargo.toml"), "[workspace]\n[package]\nname = \"tiny-desktop\"\nversion = \"0.1.0\"\nedition = \"2024\"\n").expect("manifest");
    std::fs::write(app.join("src/main.rs"), "fn main() {}\n").expect("binary");
    std::fs::write(
        app.join("flui.toml"),
        "[app]\nname = \"Named App\"\nversion = \"0.1.0\"\norganization = \"org.example\"\n",
    )
    .expect("app identity");
    let output = tmp.path().join("output");
    flui()
        .current_dir(&app)
        .args(["build", "desktop", "--output"])
        .arg(&output)
        .env("CARGO_TARGET_DIR", tmp.path().join("external target"))
        .env("CARGO_NET_OFFLINE", "true")
        .assert()
        .success();
    #[cfg(target_os = "macos")]
    {
        let contents = output.join("Named App.app/Contents");
        assert!(contents.join("MacOS/tiny-desktop").is_file());
        let plist = std::fs::read_to_string(contents.join("Info.plist")).expect("plist");
        assert!(plist.contains("<string>Named App</string>"));
        assert!(plist.contains("<string>org.example.named-app</string>"));
    }
    #[cfg(not(target_os = "macos"))]
    assert!(
        output
            .join(format!("tiny-desktop{}", std::env::consts::EXE_SUFFIX))
            .is_file()
    );
}

#[cfg(target_os = "macos")]
#[test]
fn desktop_build_rejects_invalid_present_app_configuration() {
    let tmp = TempDir::new().expect("temp dir");
    std::fs::write(tmp.path().join("flui.toml"), "[app").expect("invalid config");
    flui()
        .current_dir(tmp.path())
        .args(["build", "desktop"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("failed to parse").and(predicate::str::contains("flui.toml")),
        );
}
