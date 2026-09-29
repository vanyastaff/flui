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
pub fn doctor_json_stdout_is_pure_ndjson() {
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
