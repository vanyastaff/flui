//! Compiler contracts of the scenario `Tester` (ADR-0400 §2): an acting verb takes a `Find`
//! that names one element, so a finder of many is narrowed (`first`, `nth`, `last`) before a
//! gesture can be aimed at it.

#[test]
fn trybuild_ui() {
    let cases = trybuild::TestCases::new();
    // `tap_on_many`: a tap aimed at a `Find<Many>` does not compile.
    cases.compile_fail("tests/scenario_ui/tap_on_many.rs");
}
