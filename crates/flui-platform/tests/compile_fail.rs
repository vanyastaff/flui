//! Compiler contracts for the display and window identities, the admitted window command,
//! the owner-local window and the owner-local completion.

#[test]
fn trybuild_ui() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/compile_fail/display_id_cannot_be_built_from_a_number.rs");
    // `window_id_cannot_be_built_from_a_number` (ADR-0039 §4): a window id
    // comes from a host's `WindowIdMint`, never from a handle or a number.
    cases.compile_fail("tests/compile_fail/window_id_cannot_be_built_from_a_number.rs");
    // `admitted_command_has_one_constructor` (ADR-0254): a command reaches a
    // window only through `WindowCommand::admit`.
    cases.compile_fail("tests/compile_fail/admitted_command_has_one_constructor.rs");
    cases.compile_fail("tests/compile_fail/apply_takes_only_an_admitted_command.rs");
    // `a_platform_window_cannot_cross_a_thread` (ADR-0254 §5): neither an
    // `Rc`, an `Arc` nor a borrow of a window reaches another thread.
    cases.compile_fail("tests/compile_fail/a_platform_window_cannot_cross_a_thread.rs");
    // `a_completion_cannot_cross_a_thread` (ADR-0152 §3).
    cases.compile_fail("tests/compile_fail/a_completion_cannot_cross_a_thread.rs");
}
