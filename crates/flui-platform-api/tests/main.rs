//! flui-platform-api's integration tests, compiled as modules of one binary.

#[path = "composition_ledger.rs"]
mod composition_ledger;

#[path = "input_vocabulary.rs"]
mod input_vocabulary;

#[path = "preferences.rs"]
mod preferences;

#[path = "lock_gate.rs"]
mod lock_gate;

#[path = "transfer_request.rs"]
mod transfer_request;

/// Runs every case even after one fails, then panics listing the failing case names.
fn run_table(table: &str, cases: &[(&str, fn())]) {
    let failed: Vec<&str> = cases
        .iter()
        .filter(|(_, case)| std::panic::catch_unwind(*case).is_err())
        .map(|(name, _)| *name)
        .collect();
    assert!(failed.is_empty(), "{table}: failing cases: {failed:?}");
}
