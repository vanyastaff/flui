//! Integration tests for `flui doctor`.
//!
//! Human-mode narration goes to stderr (see `src/ui.rs`); stdout is reserved
//! for machine output (`--json`) and must stay clean in every mode.

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;

/// Get a command for the `flui` binary.
fn flui() -> Command {
    cargo_bin_cmd!("flui")
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
