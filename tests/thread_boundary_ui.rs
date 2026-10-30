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
    // A catalog's controllers are reachable exactly with its feature.
    if cfg!(feature = "material") {
        cases.compile_fail("tests/ui/thread_boundary/material/*.rs");
    }
    if cfg!(feature = "cupertino") {
        cases.compile_fail("tests/ui/thread_boundary/cupertino/*.rs");
    }
    // The unstable input vocabulary is reachable exactly when the additive
    // feature is on (ADR-0213 §5, ADR-0210 §9); trybuild builds the fixtures
    // with this crate's features.
    if cfg!(feature = "unstable-input") {
        cases.pass("tests/ui/unstable_input/*.rs");
    } else {
        cases.compile_fail("tests/ui/unstable_input/*.rs");
    }
    // The escape hatch exists exactly with the `native` feature, and each
    // system's module exactly on that system (ADR-0254 §8, R3.9).
    const HOST_WINDOW: &str = "tests/ui/native/host_window_needs_native.rs";
    const WINDOWS: &str = "tests/ui/native/windows_extension_needs_windows.rs";
    const MACOS: &str = "tests/ui/native/macos_extension_needs_macos.rs";
    if cfg!(feature = "native") {
        cases.pass(HOST_WINDOW);
        if cfg!(windows) {
            cases.pass(WINDOWS);
        } else {
            cases.compile_fail(WINDOWS);
        }
        if cfg!(target_os = "macos") {
            cases.pass(MACOS);
        } else {
            cases.compile_fail(MACOS);
        }
    } else {
        cases.compile_fail(HOST_WINDOW);
    }
}
