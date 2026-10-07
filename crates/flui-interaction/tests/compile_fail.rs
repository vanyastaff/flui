//! Compiler contracts for owner-local recognizers and gesture extension points.

#[test]
fn trybuild_ui() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/compile_fail/recognizer_stays_on_its_thread.rs");
    cases.pass("tests/compile_pass/dyn_compatible_extension_points.rs");
}
