//! Single-binary consolidation of flui-testing's root integration tests.
//!
//! Each former standalone test target linked the full dependency stack
//! separately; compiling them as modules of one `flui_testing_it` binary cuts
//! link time and `target/` disk. Source files stay in place (see
//! `autotests = false` + `[[test]]` in `Cargo.toml`).
//!
//! Convention (mirrors `flui-view/tests/main.rs`): tests that WRITE
//! process-global state get their own [[test]] target instead. None of
//! flui-testing's integration tests do — both drive a per-test
//! `HeadlessBinding` instance (no singletons, no env vars, no statics).

#[path = "async_driver.rs"]
mod async_driver;
#[path = "controller_restart.rs"]
mod controller_restart;
#[path = "layout_builder_seam.rs"]
mod layout_builder_seam;
#[path = "long_press_via_pump_frame.rs"]
mod long_press_via_pump_frame;
#[path = "owner_scope.rs"]
mod owner_scope;
#[path = "post_frame_after_layout.rs"]
mod post_frame_after_layout;
#[path = "self_rescheduling_local_post_frame.rs"]
mod self_rescheduling_local_post_frame;
#[path = "tree_observer_inspector.rs"]
mod tree_observer_inspector;
