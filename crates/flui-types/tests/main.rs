//! Single-binary consolidation of flui-types' root integration tests.
//!
//! Each former standalone test target linked the full dependency stack
//! separately; compiling them as modules of one `types_it` binary cuts
//! link time and `target/` disk. Source files stay in place (see
//! `autotests = false` + `[[test]]` in `Cargo.toml`), so manifest-relative
//! paths (trybuild's `tests/compile_fail/`) keep working unchanged.
//!
//! Convention: tests that WRITE process-global state live in their own
//! [[test]] target instead — process isolation beats opt-in locking.
//! (flui-types currently has none; see flui-view's error_view_recovery
//! for the reference case.)

#[path = "color_blend_tests.rs"]
mod color_blend_tests;
#[path = "color_operations_tests.rs"]
mod color_operations_tests;
#[path = "color_property_tests.rs"]
mod color_property_tests;
#[path = "unit_mixing_compile_fail.rs"]
mod unit_mixing_compile_fail;
