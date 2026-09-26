//! The export boundary of the private `TransitionRoute`. The mounted suite
//! lives in `crates/flui-widgets/tests/transition_route.rs`.

/// `TransitionRoute` and `ModalRoute` stay private after the sign-off gate:
/// Rust has no subclassing, so exporting them as extensible bases needs a trait
/// design that is deliberately deferred. Only `PageRoute` / `PopupRoute`
/// came out.
///
/// Red-check: add `pub use transition_route::TransitionRoute;` to
/// `navigator/mod.rs`.
#[test]
fn transition_route_is_not_exported() {
    const LIB: &str = include_str!("../lib.rs");
    const NAV_MOD: &str = include_str!("mod.rs");

    const INTERNAL: [&str; 5] = [
        "TransitionRoute",
        "TransitionHandle",
        "TransitionPeer",
        "TransitionGroup",
        "ModalRoute",
    ];

    super::export_guard::assert_not_exported("lib.rs", LIB, &INTERNAL);
    super::export_guard::assert_not_exported("navigator/mod.rs", NAV_MOD, &INTERNAL);
}
