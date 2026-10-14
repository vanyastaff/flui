//! The nested scroll channel through rustc (ADR-0209 §10): a connection is
//! owner-driven, so it may capture `Rc` and `Cell` state, while the step,
//! cause, device, end-reason, overscroll and snap types stay `Send` data.

#[test]
fn nested_scroll_connection_ui() {
    let t = trybuild::TestCases::new();
    t.pass("tests/nested_scroll_ui/pass/*.rs");
}
