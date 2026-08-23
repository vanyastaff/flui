//! Single-binary consolidation of flui-testing's root integration tests.
//!
//! Each former standalone test target linked the full dependency stack
//! separately; compiling them as modules of one `flui_testing_it` binary cuts
//! link time and `target/` disk. Source files stay in place (see
//! `autotests = false` + `[[test]]` in `Cargo.toml`).
//!
//! Convention (mirrors `flui-view/tests/main.rs`): tests that WRITE
//! process-global state get their own [[test]] target instead. The modules
//! here each drive a per-test `HeadlessBinding` (no singletons, no env vars,
//! no statics) — with one bounded exception worth naming, since it is exactly
//! the kind of thing this convention exists to catch.
//!
//! `log_capture` registers two sentinel dispatchers that live for the process.
//! They are process-wide by necessity — `tracing`'s per-callsite interest cache
//! is — but they hold no state, receive no events, and never occupy the global
//! default subscriber slot, so nothing here can observe another module's
//! capture. The test that DOES claim that slot lives in its own target,
//! `log_capture_global_subscriber.rs`.

#[path = "a11y_query.rs"]
mod a11y_query;
#[path = "async_driver.rs"]
mod async_driver;
#[path = "controller_restart.rs"]
mod controller_restart;
#[path = "layout_builder_seam.rs"]
mod layout_builder_seam;
#[path = "log_capture.rs"]
mod log_capture;
#[path = "long_press_via_pump_frame.rs"]
mod long_press_via_pump_frame;
#[path = "mount_bootstrap.rs"]
mod mount_bootstrap;
#[path = "multi_presentation_clock.rs"]
mod multi_presentation_clock;
#[path = "owner_scope.rs"]
mod owner_scope;
#[path = "pointer_script_replay.rs"]
mod pointer_script_replay;
#[path = "post_frame_after_layout.rs"]
mod post_frame_after_layout;
#[path = "self_rescheduling_local_post_frame.rs"]
mod self_rescheduling_local_post_frame;
#[path = "tree_observer_inspector.rs"]
mod tree_observer_inspector;
