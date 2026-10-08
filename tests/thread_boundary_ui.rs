//! Compiler diagnostics for facade authority and owner-thread boundaries.

#[test]
fn thread_boundary_ui() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui_pass/facade_runtime_seam.rs");
    cases.compile_fail("tests/ui/thread_boundary/*.rs");
}
