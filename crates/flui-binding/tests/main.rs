//! Single-binary consolidation of flui-binding's root integration tests.
//!
//! Each former standalone test target linked the full dependency stack
//! separately; compiling them as modules of one `binding_it` binary cuts
//! link time and `target/` disk. Source files stay in place (see
//! `autotests = false` + `[[test]]` in `Cargo.toml`).
//!
//! Convention (mirrors `flui-view/tests/main.rs`): tests that WRITE
//! process-global state get their own [[test]] target instead. None of
//! flui-binding's integration tests do — both drive a per-test
//! `HeadlessBinding` instance (no singletons, no env vars, no statics).

#[path = "controller_restart.rs"]
mod controller_restart;
#[path = "long_press_via_pump_frame.rs"]
mod long_press_via_pump_frame;
