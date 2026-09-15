//! Compile-fail tests pinning ADR-0045 decision 1 (`Renderer: Send` is a
//! compiler derivation, not a blanket `unsafe impl`; `RasterBackend` carries
//! `Send` as a supertrait) and issue #1043 (`Renderer::new` accepts only an
//! owned, `'static` `WindowTarget` — a borrowed window no longer type-checks).
//!
//! Uses trybuild, the same harness `flui-types` uses for its unit-mixing
//! compile-fail suite (`crates/flui-types/tests/unit_mixing_compile_fail.rs`).

#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/*.rs");
}
