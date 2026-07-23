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

#[path = "color_approx_eq_tests.rs"]
mod color_approx_eq_tests;
#[path = "color_operations_tests.rs"]
mod color_operations_tests;
#[path = "corners_tests.rs"]
mod corners_tests;
#[path = "device_pixels_geometry_tests.rs"]
mod device_pixels_geometry_tests;
#[path = "edge_insets_tests.rs"]
mod edge_insets_tests;
#[path = "geometric_calculations_tests.rs"]
mod geometric_calculations_tests;
#[path = "geometry_property_tests.rs"]
mod geometry_property_tests;
#[path = "layout_tests.rs"]
mod layout_tests;
#[path = "rems_unit_tests.rs"]
mod rems_unit_tests;
#[path = "rtl_support_tests.rs"]
mod rtl_support_tests;
#[path = "scale_conversion_tests.rs"]
mod scale_conversion_tests;
#[path = "typed_geometry_integration.rs"]
mod typed_geometry_integration;
#[path = "typography_tests.rs"]
mod typography_tests;
#[path = "unit_conversions_tests.rs"]
mod unit_conversions_tests;
#[path = "unit_mixing_compile_fail.rs"]
mod unit_mixing_compile_fail;
#[path = "unit_trait_tests.rs"]
mod unit_trait_tests;
