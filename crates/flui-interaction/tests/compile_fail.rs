//! Compiler contracts for owner-local recognizers and gesture extension points.

#[test]
fn trybuild_ui() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/compile_fail/recognizer_stays_on_its_thread.rs");
    cases.compile_fail("tests/compile_fail/builder_stays_on_its_thread.rs");
    cases.compile_fail("tests/compile_fail/recognizer_set_stays_on_its_thread.rs");
    cases.compile_fail("tests/compile_fail/focus_identity_cannot_be_minted.rs");
    cases.compile_fail("tests/compile_fail/recognizer_details_are_non_exhaustive.rs");
    cases.pass("tests/compile_pass/dyn_compatible_extension_points.rs");
    cases.pass("tests/compile_pass/open_arena_member.rs");
    cases.pass("tests/compile_pass/allocated_interaction_identities.rs");
    cases.pass("tests/compile_pass/scale_pan_or_scale.rs");
}
