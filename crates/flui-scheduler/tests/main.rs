//! flui-scheduler's root `tests/*.rs` files, compiled as modules of one binary. A test
//! that writes process-global state keeps its own `[[test]]` target instead (see
//! `Cargo.toml`).

#[path = "async_driver_waker.rs"]
mod async_driver_waker;

#[path = "async_driver_unwind.rs"]
mod async_driver_unwind;

#[path = "wake_delivery.rs"]
mod wake_delivery;

#[path = "frame_completion_recovery.rs"]
mod frame_completion_recovery;

#[path = "end_of_frame_lifecycle.rs"]
mod end_of_frame_lifecycle;

#[path = "frame_panic_recovery.rs"]
mod frame_panic_recovery;

#[path = "integration_tests.rs"]
mod integration_tests;

#[path = "post_frame_callback_ordering.rs"]
mod post_frame_callback_ordering;

#[path = "update_scheduler_reshape.rs"]
mod update_scheduler_reshape;

#[path = "owner_callbacks.rs"]
mod owner_callbacks;

#[path = "owner_background.rs"]
mod owner_background;

/// Runs every case even after one fails, then panics listing the failing case names.
pub(crate) fn run_table(table: &str, cases: &[(&str, fn())]) {
    let failed: Vec<&str> = cases
        .iter()
        .filter(|(_, case)| std::panic::catch_unwind(*case).is_err())
        .map(|(name, _)| *name)
        .collect();
    assert!(failed.is_empty(), "{table}: failing cases: {failed:?}");
}
