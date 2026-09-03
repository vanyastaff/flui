//! Compile-fail tests pinning the `#[diagnostic::on_unimplemented]` text on
//! the render-object traits: a type that is not a `RenderBox` is told to
//! implement the protocol trait (not `RenderObject` directly), and a type
//! that is not `ParentData` is told the one-line impl that suffices.
//!
//! Uses trybuild, as flui-engine's `compile_fail.rs` and flui-view's
//! `trybuild_ui.rs` do. The expected stderr files also list this crate's
//! own impls of the trait in question, so adding a `ParentData` or
//! `RenderObject<BoxProtocol>` impl here means regenerating them with
//! `TRYBUILD=overwrite`.

#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/*.rs");
}
