//! `#[derive(Routable)]` and `WidgetsApp::router` through rustc: the shapes
//! the derive accepts, each asserting its round trip, and the ones it
//! rejects, each with the diagnostic a user sees (the sibling `.stderr`
//! files), plus the router form's missing navigator builders.
//!
//! Regenerate the `.stderr` files after an intentional diagnostic change with
//! `TRYBUILD=overwrite cargo test -p flui-widgets --test routable_ui`. A
//! toolchain or dependency bump can change rustc's wording or the list of
//! other implementors it prints (`field_not_fromstr` names foreign `FromStr`
//! types); regenerate and review the diff then as well.

#[test]
fn routable_derive_ui() {
    let t = trybuild::TestCases::new();
    t.pass("tests/routable_ui/pass/*.rs");
    t.compile_fail("tests/routable_ui/fail/*.rs");
}
