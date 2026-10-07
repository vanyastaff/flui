//! flui-app's root `tests/*.rs` files, compiled as modules of one binary. A test
//! that writes process-global state keeps its own `[[test]]` target instead (see
//! `Cargo.toml`).

#[path = "data_transfer_transport.rs"]
mod data_transfer_transport;

#[path = "execution_public_paths.rs"]
mod execution_public_paths;

#[path = "runner_frame_ordering.rs"]
mod runner_frame_ordering;

#[path = "runner_teardown.rs"]
mod runner_teardown;

/// Runs every case even after one fails, then panics listing the failing case names.
fn run_table(table: &str, cases: &[(&str, fn())]) {
    let failed: Vec<&str> = cases
        .iter()
        .filter(|(_, case)| std::panic::catch_unwind(*case).is_err())
        .map(|(name, _)| *name)
        .collect();
    assert!(failed.is_empty(), "{table}: failing cases: {failed:?}");
}
