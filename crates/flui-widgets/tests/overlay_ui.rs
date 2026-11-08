//! Owner-affine overlay handles through rustc (ADR-0076 §2b).

#[test]
fn overlay_handles_are_owner_local() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/overlay_ui/handles_are_not_send_or_sync.rs");
}
