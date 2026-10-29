//! Compiler contracts for the display identity, the admitted window command
//! and the owner-local window.

#[test]
fn trybuild_ui() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/compile_fail/display_id_cannot_be_built_from_a_number.rs");
    // `admitted_command_has_one_constructor` (ADR-0254): a command reaches a
    // window only through `WindowCommand::admit`.
    cases.compile_fail("tests/compile_fail/admitted_command_has_one_constructor.rs");
    cases.compile_fail("tests/compile_fail/apply_takes_only_an_admitted_command.rs");
    // `a_platform_window_cannot_cross_a_thread` (ADR-0254 §5): neither an
    // `Rc`, an `Arc` nor a borrow of a window reaches another thread.
    cases.compile_fail("tests/compile_fail/a_platform_window_cannot_cross_a_thread.rs");
}
