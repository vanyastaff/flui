//! flui-scheduler's root `tests/*.rs` files, compiled as modules of one binary. A test
//! that writes process-global state keeps its own `[[test]]` target instead (see
//! `Cargo.toml`).

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
