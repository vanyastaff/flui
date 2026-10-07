//! Compiler diagnostics for geometry units and validated painting values.

#[test]
fn trybuild_ui() {
    let t = trybuild::TestCases::new();
    t.pass("tests/compile_pass/glyph_image.rs");
    t.compile_fail("tests/compile_fail/*.rs");
}
