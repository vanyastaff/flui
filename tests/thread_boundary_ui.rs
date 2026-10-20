//! Compiler diagnostics for facade authority and owner-thread boundaries.

#[test]
fn thread_boundary_ui() {
    let cases = trybuild::TestCases::new();
    cases.pass("tests/ui_pass/facade_runtime_seam.rs");
    cases.pass("tests/ui_pass/owner_callbacks.rs");
    cases.pass("tests/ui_pass/lifecycle_vsync.rs");
    cases.compile_fail(
        "crates/flui-animation/tests/compile_fail/driven_controller_has_no_bare_dispose.rs",
    );
    cases.compile_fail("crates/flui-animation/tests/compile_fail/registering_a_bare_controller_does_not_compile.rs");
    cases.compile_fail("tests/ui/thread_boundary/*.rs");
    // The unstable input vocabulary is reachable exactly when the additive
    // feature is on (ADR-0213 §5, ADR-0210 §9); trybuild builds the fixtures
    // with this crate's features.
    if cfg!(feature = "unstable-input") {
        cases.pass("tests/ui/unstable_input/*.rs");
    } else {
        cases.compile_fail("tests/ui/unstable_input/*.rs");
    }
}
