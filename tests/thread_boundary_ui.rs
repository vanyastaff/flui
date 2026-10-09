//! Compiler diagnostics for facade authority and owner-thread boundaries.

#[test]
fn thread_boundary_ui() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui_pass/facade_runtime_seam.rs");
    cases.pass("tests/ui_pass/owner_callbacks.rs");
    cases.compile_fail("crates/flui-animation/tests/compile_fail/registering_a_bare_controller_does_not_compile.rs");
    cases.compile_fail("tests/ui/thread_boundary/*.rs");
}
