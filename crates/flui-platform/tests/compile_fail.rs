//! Compiler contracts for the display identity and the admitted window command.

#[test]
fn trybuild_ui() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/compile_fail/display_id_cannot_be_built_from_a_number.rs");
    // `admitted_command_has_one_constructor` (ADR-0254): a command reaches a
    // window only through `WindowCommand::admit`.
    cases.compile_fail("tests/compile_fail/admitted_command_has_one_constructor.rs");
    cases.compile_fail("tests/compile_fail/apply_takes_only_an_admitted_command.rs");
}
