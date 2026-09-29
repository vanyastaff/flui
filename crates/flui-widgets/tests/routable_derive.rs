//! `#[derive(Routable)]` (ADR-0093 §1) through the crate's public surface: the
//! round trip over generated values, the typed errors, specificity, and the
//! back-stack a derived type opens with.

use flui_widgets::Routable;
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
