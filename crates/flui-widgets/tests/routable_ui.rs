//! `#[derive(Routable)]` through rustc: the shapes it accepts, each asserting
//! its round trip, and the ones it rejects, each with the diagnostic a user
//! sees (the sibling `.stderr` files).
//!
//! Regenerate the `.stderr` files after an intentional diagnostic change with
//! `TRYBUILD=overwrite cargo test -p flui-widgets --test routable_ui`.

#[test]
fn routable_derive_ui() {
    let t = trybuild::TestCases::new();
    t.pass("tests/routable_ui/pass/*.rs");
    t.compile_fail("tests/routable_ui/fail/*.rs");
}
