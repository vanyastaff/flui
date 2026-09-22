//! Integration tests for `flui doctor`.
//!
//! Human-mode narration goes to stderr (see `src/ui.rs`); stdout is reserved
//! for machine output (`--json`) and must stay clean in every mode.

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use tempfile::TempDir;

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
fn doctor_verbose_runs_successfully() {
    flui().args(["doctor", "--verbose"]).assert().success();
}

/// A required tool missing entirely (empty `PATH`) must fail the whole
/// command with the environment exit code, and the summary must say why.
#[test]
fn doctor_with_empty_path_fails_with_environment_exit_code() {
    let empty_path = TempDir::new().expect("temp dir");

    flui()
        .args(["--json", "doctor"])
        .env("PATH", empty_path.path())
        .env_remove("ANDROID_HOME")
        .env_remove("ANDROID_SDK_ROOT")
        .assert()
        .code(3)
        .stdout(predicate::str::contains("\"event\":\"doctor.summary\""))
        .stdout(predicate::function(|out: &str| {
            summary_line(out).is_some_and(|summary| summary["error"].as_u64().unwrap_or(0) > 0)
        }));
}

/// `--android` makes a missing Android SDK a hard failure (exit 3): the
/// section was selected explicitly, so its pieces are required.
#[test]
fn doctor_android_explicit_fails_when_sdk_is_missing() {
    flui()
        .args(["doctor", "--android"])
        .env_remove("ANDROID_HOME")
        .env_remove("ANDROID_SDK_ROOT")
        .assert()
        .code(3)
        .stderr(predicate::str::contains("Android SDK"));
}

/// The same missing SDK is only a warning when Android was not selected —
/// optional toolchains never fail a plain `flui doctor`.
#[test]
fn doctor_plain_succeeds_when_android_sdk_is_missing() {
    flui()
        .args(["doctor"])
        .env_remove("ANDROID_HOME")
        .env_remove("ANDROID_SDK_ROOT")
        .assert()
        .success();
}

/// `--json` must produce nothing but NDJSON on stdout: every line parses,
/// each carries an `event`, there is at least one `doctor.check` and
/// exactly one `doctor.summary`.
#[test]
fn doctor_json_stdout_is_pure_ndjson() {
    let assert = flui().args(["--json", "doctor"]).assert();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut checks = 0usize;
    let mut summaries = 0usize;
    for line in stdout.lines() {
        assert!(!line.trim().is_empty(), "blank line in NDJSON stdout");
        let value: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("non-JSON line on stdout: {line:?}: {e}"));
        match value["event"].as_str() {
            Some("doctor.check") => checks += 1,
            Some("doctor.summary") => summaries += 1,
            other => panic!("unexpected event {other:?} in {line:?}"),
        }
    }
    assert!(checks >= 1, "expected at least one doctor.check event");
    assert_eq!(summaries, 1, "expected exactly one doctor.summary event");
}

/// Human mode (no `--json`) must not put anything on stdout: all narration
/// belongs on stderr so `flui doctor > report.txt` produces an empty file.
#[test]
fn doctor_human_mode_prints_nothing_on_stdout() {
    let assert = flui().args(["doctor"]).assert();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert_eq!(stdout, "", "human mode leaked onto stdout: {stdout:?}");
}

/// `--quiet` drops the narration (`ui::note`'s "Environment Check" box) but
/// keeps warnings and errors — here, the promoted Android-SDK error.
#[test]
fn doctor_quiet_keeps_warnings_and_errors_only() {
    let assert = flui()
        .args(["--quiet", "doctor", "--android"])
        .env_remove("ANDROID_HOME")
        .env_remove("ANDROID_SDK_ROOT")
        .assert()
        .code(3);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("Android SDK"),
        "the promoted error must still reach stderr under --quiet: {stderr:?}"
    );
    assert!(
        !stderr.contains("Environment Check"),
        "--quiet must suppress the narration box: {stderr:?}"
    );
}

/// Pull the one `doctor.summary` object out of NDJSON stdout, if present.
fn summary_line(stdout: &str) -> Option<serde_json::Value> {
    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|value| value["event"] == "doctor.summary")
}

// ============================================================================
// `flui doctor --fix`
// ============================================================================

/// Pull every `doctor.fix` object out of NDJSON stdout.
#[cfg(unix)]
fn fix_events(stdout: &str) -> Vec<serde_json::Value> {
    stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|value| value["event"] == "doctor.fix")
        .collect()
}

/// A directory of fake `rustc`/`cargo`/`rustup`/`git` shell scripts, first on
/// `PATH`, so `flui doctor --web` runs hermetically: no real toolchain probe
/// escapes, and `rustup` reports every target installed except
/// `wasm32-unknown-unknown`. `rustup`'s argv is also appended (one line per
/// invocation) to `argv_log`, so a test can prove `--fix` really invoked
/// `rustup target add ...` rather than merely rendering that hint.
///
/// Returns `(_tmp, path_value, argv_log)`; `_tmp` must be kept alive for the
/// scripts to exist while the command runs.
#[cfg(unix)]
fn fake_toolchain_path(argv_log: &std::path::Path) -> (TempDir, String) {
    use std::os::unix::fs::PermissionsExt;

    let tmp = TempDir::new().expect("temp dir");

    let write_script = |name: &str, body: &str| {
        let script = tmp.path().join(name);
        std::fs::write(&script, body).expect("write fake tool");
        let mut perms = std::fs::metadata(&script).expect("metadata").permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script, perms).expect("chmod +x");
    };

    write_script(
        "rustc",
        "#!/bin/sh\ncase \"$1\" in\n\
         --version) echo 'rustc 1.99.0 (fake 2026-01-01)';;\n\
         -vV) printf 'rustc 1.99.0-fake\\nhost: x86_64-unknown-linux-gnu\\n';;\n\
         esac\nexit 0\n",
    );
    write_script("cargo", "#!/bin/sh\necho 'cargo 1.99.0 (fake)'\nexit 0\n");
    write_script(
        "git",
        "#!/bin/sh\necho 'git version 2.42.0 (fake)'\nexit 0\n",
    );
    write_script(
        "rustup",
        &format!(
            "#!/bin/sh\necho \"$@\" >> {log:?}\ncase \"$1 $2\" in\n\
             '--version '*) echo 'rustup 1.27.0 (fake)';;\n\
             'target list') echo x86_64-unknown-linux-gnu;;\n\
             'target add') :;;\n\
             esac\nexit 0\n",
            log = argv_log.display()
        ),
    );

    let real_path = std::env::var("PATH").unwrap_or_default();
    let path = format!("{}:{real_path}", tmp.path().display());
    (tmp, path)
}

/// `--fix` must run the real, bounded `rustup target add ...` for a missing
/// target it can automate — not just print the hint — and report it via
/// `doctor.fix`. This is the regression test for `run_autofix`: the fake
/// `rustup`'s own argv log proves the command that ran, independent of what
/// `doctor.fix`'s `command` field claims.
#[cfg(unix)]
#[test]
fn fix_runs_rustup_target_add_for_a_missing_web_target() {
    let argv_log_dir = TempDir::new().expect("temp dir");
    let argv_log = argv_log_dir.path().join("rustup-argv.log");
    let (_tmp, path) = fake_toolchain_path(&argv_log);

    let output = flui()
        .env("PATH", &path)
        .env_remove("ANDROID_HOME")
        .env_remove("ANDROID_SDK_ROOT")
        .args(["--json", "doctor", "--web", "--fix"])
        .output()
        .expect("flui doctor --web --fix runs");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let fixes = fix_events(&stdout);
    let wasm_fix = fixes
        .iter()
        .find(|event| {
            event["command"]
                .as_str()
                .is_some_and(|c| c.starts_with("rustup target add wasm32-unknown-unknown"))
        })
        .unwrap_or_else(|| panic!("no doctor.fix for wasm32-unknown-unknown in {fixes:?}"));
    assert_eq!(
        wasm_fix["ok"], true,
        "fake rustup always exits 0: {wasm_fix:?}"
    );

    let argv = std::fs::read_to_string(&argv_log).unwrap_or_default();
    assert!(
        argv.lines()
            .any(|line| line.starts_with("target add") && line.contains("wasm32-unknown-unknown")),
        "fake rustup's argv log must show a real `target add wasm32-unknown-unknown` \
         invocation, not just the rendered hint: {argv:?}"
    );
}

/// Without `--fix`, `doctor --web` must never invoke `rustup target add` —
/// only report the missing target as a check.
#[cfg(unix)]
#[test]
fn plain_doctor_never_invokes_rustup_target_add() {
    let argv_log_dir = TempDir::new().expect("temp dir");
    let argv_log = argv_log_dir.path().join("rustup-argv.log");
    let (_tmp, path) = fake_toolchain_path(&argv_log);

    let output = flui()
        .env("PATH", &path)
        .env_remove("ANDROID_HOME")
        .env_remove("ANDROID_SDK_ROOT")
        .args(["--json", "doctor", "--web"])
        .output()
        .expect("flui doctor --web runs");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        fix_events(&stdout).is_empty(),
        "no --fix was requested, so there must be no doctor.fix event"
    );

    let argv = std::fs::read_to_string(&argv_log).unwrap_or_default();
    assert!(
        !argv.lines().any(|line| line.starts_with("target add")),
        "no --fix was requested, so rustup must never be asked to add a target: {argv:?}"
    );
}
