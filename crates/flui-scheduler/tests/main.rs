//! Single-binary consolidation of flui-scheduler's root integration tests.
//!
//! Each former standalone test target linked the full dependency stack
//! separately; compiling them as modules of one `scheduler_it` binary cuts link time
//! and `target/` disk. Source files stay in place (see `autotests = false` +
//! `[[test]]` in `Cargo.toml`).
//!
//! Convention (mirrors `flui-testing/tests/main.rs`): a test that WRITES
//! process-global state keeps its own `[[test]]` target instead.

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
