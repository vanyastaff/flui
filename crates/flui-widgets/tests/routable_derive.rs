//! `#[derive(Routable)]` (ADR-0093 §1) through the crate's public surface: the
//! round trip over generated values, the typed errors, specificity, and the
//! back-stack a derived type opens with.

use flui_widgets::{Routable, RouteParseError, RoutePath};
use proptest::prelude::*;

#[derive(Routable, Debug, Clone, PartialEq)]
enum TestRoute {
    #[route("/")]
    Home,
    #[route("/note/:id")]
    Note { id: u32 },
    #[route("/user/:uid/post/:pid")]
    UserPost { uid: u64, pid: i32 },
    #[route("/tag/:name")]
    Tag { name: String },
    // Declared before the literal sibling it must lose to.
    #[route("/s/:slug")]
    Slug { slug: String },
    #[route("/s/new")]
    SNew,
    #[route("/café/:s")]
    Cafe { s: String },
}

fn non_empty() -> impl Strategy<Value = String> {
    any::<String>().prop_filter("a segment prints non-empty", |s| !s.is_empty())
}

fn route() -> impl Strategy<Value = TestRoute> {
    prop_oneof![
        Just(TestRoute::Home),
        any::<u32>().prop_map(|id| TestRoute::Note { id }),
        (any::<u64>(), any::<i32>()).prop_map(|(uid, pid)| TestRoute::UserPost { uid, pid }),
        non_empty().prop_map(|name| TestRoute::Tag { name }),
        non_empty()
            .prop_filter("the literal sibling claims `new`", |s| s != "new")
            .prop_map(|slug| TestRoute::Slug { slug }),
        Just(TestRoute::SNew),
        non_empty().prop_map(|s| TestRoute::Cafe { s }),
    ]
}

proptest! {
    #[test]
    fn derived_routable_round_trips(route in route()) {
        let path = route.to_path();
        prop_assert_eq!(TestRoute::from_path(&path), Ok(route.clone()));
        // The printed location parses back to the same path, too.
        prop_assert_eq!(TestRoute::parse(path.as_str()), Ok(route));
    }
}

#[test]
fn derived_routable_prints_its_patterns() {
    assert_eq!(TestRoute::Home.to_path(), RoutePath::root());
    assert_eq!(TestRoute::Note { id: 3 }.to_path().as_str(), "/note/3");
    assert_eq!(
        TestRoute::UserPost { uid: 1, pid: -2 }.to_path().as_str(),
        "/user/1/post/-2"
    );
    // The literal is encoded like any segment: `é` is `%C3%A9`.
    let cafe = TestRoute::Cafe { s: "a b".into() }.to_path();
    assert_eq!(cafe, RoutePath::root().join("café").join("a b"));
    assert!(cafe.as_str().ends_with("%C3%A9/a%20b"), "{cafe}");
}

#[test]
fn derived_routable_reports_no_match_and_bad_params() {
    for location in ["/nope", "/note", "/note/1/x", "/user/1/post"] {
        assert!(
            matches!(
                TestRoute::parse(location),
                Err(RouteParseError::NoMatch { .. })
            ),
            "{location} matches no pattern"
        );
    }
    assert!(matches!(
        TestRoute::parse("/note/x"),
        Err(RouteParseError::Param { field: "id", ref segment, .. }) if segment == "x"
    ));
    assert!(matches!(
        TestRoute::parse("/user/1/post/x"),
        Err(RouteParseError::Param { field: "pid", .. })
    ));
    assert!(matches!(
        TestRoute::parse("/user/x/post/1"),
        Err(RouteParseError::Param { field: "uid", .. })
    ));
}

#[test]
fn literal_segments_win_over_parameters() {
    assert_eq!(TestRoute::parse("/s/new"), Ok(TestRoute::SNew));
    assert_eq!(
        TestRoute::parse("/s/old"),
        Ok(TestRoute::Slug { slug: "old".into() })
    );
}

/// Two patterns that cross: each has a literal where the other has a
/// parameter. Declared with the one tried later first.
#[derive(Routable, Debug, Clone, PartialEq)]
enum Crossing {
    #[route("/:a/new")]
    A { a: String },
    #[route("/s/:b")]
    B { b: String },
}

#[test]
fn crossing_patterns_resolve_by_the_first_differing_segment() {
    // The documented overlap on `Routable`: `A { a: "s" }` prints a path the
    // earlier-tried `/s/:b` matches, so it parses as `B`.
    let a = Crossing::A { a: "s".into() };
    assert_eq!(a.to_path().as_str(), "/s/new");
    assert_eq!(
        Crossing::from_path(&a.to_path()),
        Ok(Crossing::B { b: "new".into() })
    );
    // Values outside the overlap round-trip.
    for route in [
        Crossing::A { a: "t".into() },
        Crossing::B { b: "old".into() },
    ] {
        assert_eq!(Crossing::from_path(&route.to_path()), Ok(route));
    }
}

#[test]
fn derived_back_stack_skips_gaps() {
    let path = RoutePath::parse("/note/1").expect("parses");
    assert_eq!(
        TestRoute::back_stack(&path),
        Ok(vec![TestRoute::Home, TestRoute::Note { id: 1 }])
    );
}
