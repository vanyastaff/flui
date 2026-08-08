//! Compile-fail tests pinning ADR-0045 decision 1: `Renderer: Send` is now
//! a compiler derivation (not a blanket `unsafe impl`), and `RasterBackend`
//! carries `Send` as a supertrait.
//!
//! Uses trybuild, the same harness `flui-types` uses for its unit-mixing
//! compile-fail suite (`crates/flui-types/tests/unit_mixing_compile_fail.rs`).

#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/*.rs");
}
