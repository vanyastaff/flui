//! flui-platform's integration tests, compiled as modules of one binary.

#[path = "composition_ledger.rs"]
mod composition_ledger;

#[path = "drag_plan.rs"]
mod drag_plan;

#[path = "flag_sets.rs"]
mod flag_sets;

#[path = "haptics.rs"]
mod haptics;

#[path = "input_regions.rs"]
mod input_regions;

#[path = "display.rs"]
mod display;

#[path = "host_notice.rs"]
mod host_notice;

#[path = "input_vocabulary.rs"]
mod input_vocabulary;

#[path = "placement_resolve.rs"]
mod placement_resolve;

#[path = "preferences.rs"]
mod preferences;

#[path = "lock_gate.rs"]
mod lock_gate;

#[path = "window_command_validate.rs"]
mod window_command_validate;

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
