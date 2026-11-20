//! Compile-fail tests pinning ADR-0305 §2: a `SurfaceFactory` stays on its
//! owner thread, and it makes surfaces only of owned, `'static` targets, so a
//! borrowed window does not type-check.
//!
//! Uses trybuild, the same harness as `flui-painting`'s compile-fail suite
//! (`crates/flui-painting/tests/compile_fail.rs`).

#[test]
fn trybuild_ui() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/*.rs");
}
