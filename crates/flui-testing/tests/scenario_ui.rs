//! Compiler contracts of the scenario `Tester` (ADR-0400 §1, §2, §4): an acting verb takes a
//! `Find` that names one element, so a finder of many is narrowed (`first`, `nth`, `last`) before
//! a gesture can be aimed at it; and a frame is read in process, so a scenario over a generic
//! connection cannot read one.

#[test]
fn trybuild_ui() {
    let cases = trybuild::TestCases::new();
    // `tap_on_many`: a tap aimed at a `Find<Many>` does not compile.
    cases.compile_fail("tests/scenario_ui/tap_on_many.rs");
    // `frame_on_a_connection`: `frame()` on a `Tester<C>` for a generic `C: Connection` does not
    // compile; it exists on `Tester<Headless<_>>` alone.
    cases.compile_fail("tests/scenario_ui/frame_on_a_connection.rs");
}
