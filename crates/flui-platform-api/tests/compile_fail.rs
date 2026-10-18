//! Compiler contracts for the display identity.

#[test]
fn trybuild_ui() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/compile_fail/display_id_cannot_be_built_from_a_number.rs");
}
