//! Integration tests for the `flui` maintenance commands: `test`, `analyze`,
//! `format`, `clean`, and `upgrade`.

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use serde_json::Value;
use std::path::Path;
use tempfile::TempDir;

/// Get a command for the `flui` binary.
fn flui() -> Command {
    cargo_bin_cmd!("flui")
}

/// Parse `stdout` as NDJSON: every non-empty line must be one JSON object,
/// and nothing else may be on stdout (that is the whole point of `--json`).
fn parse_ndjson(stdout: &[u8]) -> Vec<Value> {
    let text = String::from_utf8(stdout.to_vec()).expect("stdout is UTF-8");
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("not one JSON object per line ({e}): {line:?}"))
        })
        .collect()
}

fn find_event<'a>(events: &'a [Value], name: &str) -> &'a Value {
    events
        .iter()
        .find(|event| event["event"] == name)
        .unwrap_or_else(|| panic!("no {name:?} event among: {events:#?}"))
}

/// Scaffold a plain (non-FLUI) cargo binary crate in a fresh temp dir, via a
/// real `cargo new` — `flui test`/`flui format` do not require a FLUI
/// project, and this proves it.
fn new_plain_cargo_project() -> (TempDir, std::path::PathBuf) {
    let tmp = TempDir::new().expect("temp dir");
    let project_dir = tmp.path().join("plain-project");

    let status = std::process::Command::new("cargo")
        .args(["new", "--bin", "--vcs", "none"])
        .arg(&project_dir)
        .status()
        .expect("cargo new runs");
    assert!(status.success(), "cargo new failed");

    (tmp, project_dir)
}

// ============================================================================
// flui test
// ============================================================================

#[test]
fn test_runs_and_passes_on_a_plain_project() {
    let (_tmp, project_dir) = new_plain_cargo_project();

    flui()
        .current_dir(&project_dir)
        .arg("test")
        .assert()
        .success();
}

#[test]
fn test_json_is_pure_ndjson_and_reports_ok() {
    let (_tmp, project_dir) = new_plain_cargo_project();

    let output = flui()
        .current_dir(&project_dir)
        .args(["--json", "test"])
        .output()
        .expect("flui test runs");
    assert!(output.status.success());

    let events = parse_ndjson(&output.stdout);
    assert_eq!(find_event(&events, "test.done")["ok"], true);
}

// ============================================================================
// flui format
// ============================================================================

#[test]
fn format_check_passes_on_freshly_generated_project() {
    let (_tmp, project_dir) = new_plain_cargo_project();

    flui()
        .current_dir(&project_dir)
        .args(["format", "--check"])
        .assert()
        .success();
}

#[test]
fn format_check_fails_on_unformatted_code() {
    let (_tmp, project_dir) = new_plain_cargo_project();

    // `cargo new` already writes well-formatted boilerplate; deliberately
    // break it so `--check` has something to catch.
    std::fs::write(
        project_dir.join("src/main.rs"),
        "fn main( ) {\n    let   x   =    1;\nprintln!(\"{}\",x);\n}\n",
    )
    .expect("write unformatted main.rs");

    flui()
        .current_dir(&project_dir)
        .args(["format", "--check"])
        .assert()
        .code(4)
        .stderr(predicate::str::contains("not formatted"));
}

#[test]
fn format_check_json_is_pure_ndjson_and_reports_failure() {
    let (_tmp, project_dir) = new_plain_cargo_project();

    std::fs::write(
        project_dir.join("src/main.rs"),
        "fn main( ) {\n    let   x   =    1;\nprintln!(\"{}\",x);\n}\n",
    )
    .expect("write unformatted main.rs");

    let output = flui()
        .current_dir(&project_dir)
        .args(["--json", "format", "--check"])
        .output()
        .expect("flui format --check runs");
    assert_eq!(output.status.code(), Some(4));

    let events = parse_ndjson(&output.stdout);
    let done = find_event(&events, "format.done");
    assert_eq!(done["ok"], false);
    assert_eq!(done["checked"], true);
}

// ============================================================================
// flui clean
// ============================================================================

#[test]
fn clean_rejects_an_unknown_platform() {
    flui()
        .args(["clean", "--platform", "bogus"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("invalid platform")
                .and(predicate::str::contains("android"))
                .and(predicate::str::contains("ios"))
                .and(predicate::str::contains("web")),
        );
}

#[test]
fn clean_accepts_a_known_platform_with_nothing_to_remove() {
    let tmp = TempDir::new().expect("temp dir");

    flui()
        .current_dir(tmp.path())
        .args(["clean", "--platform", "android"])
        .assert()
        .success();
}

// ============================================================================
// flui upgrade --check
// ============================================================================

/// Build a directory containing a fake `cargo` that answers `search` with a
/// fixed line and prepend it to `PATH`, so `flui upgrade --check` never
/// touches the network.
#[cfg(unix)]
fn fake_cargo_path_prepended_with(search_line: &str) -> (TempDir, String) {
    use std::os::unix::fs::PermissionsExt;

    let tmp = TempDir::new().expect("temp dir");
    let script = tmp.path().join("cargo");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"search\" ]; then\n  echo '{search_line}'\n  exit 0\nfi\nexit 1\n"
        ),
    )
    .expect("write fake cargo");
    let mut perms = std::fs::metadata(&script).expect("metadata").permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).expect("chmod +x");

    let real_path = std::env::var("PATH").unwrap_or_default();
    let path = format!("{}:{real_path}", tmp.path().display());
    (tmp, path)
}

#[cfg(unix)]
#[test]
fn upgrade_check_reports_a_newer_version_from_a_fake_cargo_search() {
    let (_tmp, path) =
        fake_cargo_path_prepended_with("flui-cli = \"9.9.9\"    # Command-line interface for FLUI");

    let output = flui()
        .env("PATH", path)
        .args(["--json", "upgrade", "--check"])
        .output()
        .expect("flui upgrade --check runs");
    assert!(output.status.success());

    let events = parse_ndjson(&output.stdout);
    let check = find_event(&events, "upgrade.check");
    assert_eq!(check["latest"], "9.9.9");
    assert_eq!(check["update_available"], true);
    assert!(check["current"].is_string());
}

#[cfg(unix)]
#[test]
fn upgrade_check_reports_not_published_when_search_finds_nothing() {
    let (_tmp, path) =
        fake_cargo_path_prepended_with("some-other-crate = \"1.0.0\"    # unrelated");

    let output = flui()
        .env("PATH", path)
        .args(["--json", "upgrade", "--check"])
        .output()
        .expect("flui upgrade --check runs");
    assert!(output.status.success());

    let events = parse_ndjson(&output.stdout);
    let check = find_event(&events, "upgrade.check");
    assert!(check["latest"].is_null());
    assert_eq!(check["update_available"], false);
}

// ============================================================================
// flui completions
// ============================================================================

#[test]
fn completions_zsh_stdout_is_only_the_script() {
    flui()
        .args(["completions", "zsh"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("#compdef flui"));
}

#[test]
fn completions_zsh_stderr_has_install_instructions() {
    flui()
        .args(["completions", "zsh"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Installation Instructions"));
}

#[test]
fn completions_quiet_has_empty_stderr() {
    flui()
        .args(["--quiet", "completions", "zsh"])
        .assert()
        .success()
        .stderr(predicate::str::is_empty());
}

// A basename match, not a substring one: a `$SHELL` that merely contains
// "bash"/"zsh" inside an unrelated directory name (the historical bug) must
// not be mistaken for that shell. This only proves the fix through the
// default-shell fallback path indirectly (`--shell` always wins over
// detection), so it is covered directly in `src/commands/completions.rs`'s
// own unit tests instead of here.
#[test]
fn completions_explicit_shell_overrides_detection() {
    // `$SHELL` here would misdetect as zsh *and* bash under the old
    // substring check; an explicit `bash` argument must win regardless.
    flui()
        .env("SHELL", "/opt/zsh-bash/bin/fish")
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("_flui("));
}

/// Sanity check that the fixture path helper above actually exists and is a
/// directory, guarding against a typo silently making every test above pass
/// vacuously against a directory that was never created.
#[test]
fn plain_cargo_project_fixture_is_a_real_crate() {
    let (_tmp, project_dir) = new_plain_cargo_project();
    assert!(project_dir.join("Cargo.toml").is_file());
    assert!(Path::new(&project_dir).join("src/main.rs").is_file());
}
