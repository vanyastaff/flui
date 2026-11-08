//! Compiler contracts for platform accessibility adapters.

#[test]
fn an_accessibility_handler_cannot_silently_drop_assistive_focus() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/assistive_focus_changed_is_required.rs");
    cases.pass("tests/ui_pass/assistive_focus_changed_is_explicit.rs");
}
