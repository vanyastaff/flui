//! `#[derive(Routable)]` (ADR-0330 §1) through the crate's public surface: the
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
    #[route("/search")]
    Search {
        #[query]
        q: Option<String>,
        #[query(rename = "tag")]
        tags: Vec<String>,
        #[query]
        page: u32,
    },
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
        (
            proptest::option::of(any::<String>()),
            proptest::collection::vec(any::<String>(), 0..4),
            any::<u32>(),
        )
            .prop_map(|(q, tags, page)| TestRoute::Search { q, tags, page }),
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

fn canonical_location(input: &str, expected: &str, name: &str) {
    let path = flui_widgets::RoutePath::parse(input).expect("valid encoded location");
    assert_eq!(path.as_str(), expected);
    assert_eq!(
        TestRoute::from_path(&path),
        Ok(TestRoute::Tag {
            name: name.to_owned()
        }),
    );
    assert_eq!(
        TestRoute::Tag {
            name: name.to_owned()
        }
        .to_path(),
        path
    );
    assert_eq!(
        path.prefixes()
            .map(|prefix| prefix.to_string())
            .collect::<Vec<_>>(),
        ["/", "/tag", expected],
    );
}

fn lowercase_escapes_preserve_segment_boundaries() {
    canonical_location("/tag/a%2fb/", "/tag/a%2Fb", "a/b");
}
fn percent_is_decoded_once() {
    canonical_location("/tag/%252f", "/tag/%252f", "%2f");
}
fn plus_is_literal_path_data() {
    canonical_location("/tag/a+b", "/tag/a+b", "a+b");
}
fn reserved_delimiters_are_segment_data() {
    canonical_location("/tag/%3f%23%25", "/tag/%3F%23%25", "?#%");
}
fn unicode_is_encoded_as_utf8_bytes() {
    canonical_location("/tag/雪💖", "/tag/%E9%9B%AA%F0%9F%92%96", "雪💖");
}
fn controls_and_backslashes_are_encoded() {
    canonical_location("/tag/\0\t\u{7f}\\", "/tag/%00%09%7F%5C", "\0\t\u{7f}\\");
}
fn readable_path_punctuation_is_preserved() {
    canonical_location(
        "/tag/-._~!$&'()*+,;=:@[]^|",
        "/tag/-._~!$&'()*+,;=:@[]^|",
        "-._~!$&'()*+,;=:@[]^|",
    );
}
fn dot_segments_are_literal_route_values() {
    canonical_location("/tag/%2e%2e", "/tag/..", "..");
}
fn malformed_location(input: &str) {
    assert!(
        matches!(
            flui_widgets::RoutePath::parse(input),
            Err(flui_widgets::RouteParseError::Malformed { location, .. }) if location == input
        ),
        "invalid location must be rejected without replacing data: {input:?}"
    );
}
fn missing_escape_digits_are_rejected() {
    malformed_location("/tag/%");
}
fn short_escape_is_rejected() {
    malformed_location("/tag/%0");
}
fn non_hex_escape_is_rejected() {
    malformed_location("/tag/%g0");
}
fn raw_fragment_is_rejected() {
    malformed_location("/tag/x#y");
}
fn invalid_utf8_byte_is_rejected() {
    malformed_location("/tag/%FF");
}
fn overlong_utf8_is_rejected() {
    malformed_location("/tag/%C0%AF");
}
fn utf8_surrogate_is_rejected() {
    malformed_location("/tag/%ED%A0%80");
}
fn truncated_utf8_is_rejected() {
    malformed_location("/tag/%E2%82");
}
fn out_of_range_unicode_is_rejected() {
    malformed_location("/tag/%F4%90%80%80");
}
fn query_location(input: &str, expected: &str, pairs: &[(&str, &str)]) {
    let path = flui_widgets::RoutePath::parse(input).expect("valid encoded location");
    assert_eq!(path.as_str(), expected, "{input:?}");
    let read: Vec<(String, String)> = path
        .query()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    let pairs: Vec<(String, String)> = pairs
        .iter()
        .map(|&(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
    assert_eq!(read, pairs, "{input:?}");
    assert_eq!(
        flui_widgets::RoutePath::parse(expected).as_ref(),
        Ok(&path),
        "the printed location parses back"
    );
}
fn query_plus_is_a_space() {
    query_location("/search?q=a+b", "/search?q=a%20b", &[("q", "a b")]);
}
fn query_encoded_plus_is_a_plus() {
    query_location("/search?q=a%2bb", "/search?q=a%2Bb", &[("q", "a+b")]);
}
fn query_keeps_order_and_repeats() {
    query_location(
        "/s?tag=b&page=2&tag=a",
        "/s?tag=b&page=2&tag=a",
        &[("tag", "b"), ("page", "2"), ("tag", "a")],
    );
}
fn query_empty_value_prints_the_bare_key() {
    query_location("/s?flag=&x", "/s?flag&x", &[("flag", ""), ("x", "")]);
}
fn query_empty_pairs_are_dropped() {
    query_location("/s/?&&q=1&", "/s?q=1", &[("q", "1")]);
    query_location("/?", "/", &[]);
    query_location("/s?=&q=1&=", "/s?q=1", &[("q", "1")]);
    assert_eq!(
        flui_widgets::RoutePath::root().with_query("", "").as_str(),
        "/",
        "a pair with no key and no value appends nothing"
    );
}
fn query_delimiters_are_value_data() {
    query_location(
        "/s?k=%26%3D%23&a=b?c/d",
        "/s?k=%26%3D%23&a=b?c/d",
        &[("k", "&=#"), ("a", "b?c/d")],
    );
}
fn query_bad_escape_is_rejected() {
    malformed_location("/s?q=%");
    malformed_location("/s?q=%FF");
}
fn query_is_kept_by_the_last_prefix_only() {
    let path = flui_widgets::RoutePath::parse("/a/b?q=1").expect("valid location");
    assert_eq!(
        path.prefixes()
            .map(|prefix| prefix.to_string())
            .collect::<Vec<_>>(),
        ["/", "/a", "/a/b?q=1"],
    );
    let root = flui_widgets::RoutePath::parse("/?q=1").expect("valid location");
    assert_eq!(
        root.prefixes()
            .map(|prefix| prefix.to_string())
            .collect::<Vec<_>>(),
        ["/?q=1"],
    );
    assert_eq!(
        flui_widgets::RoutePath::root()
            .with_query("q", 1)
            .join("a")
            .as_str(),
        "/a?q=1",
        "a segment joins before the query"
    );
}
fn query_fields_parse_and_default() {
    let parse = |location: &str| {
        TestRoute::from_path(&flui_widgets::RoutePath::parse(location).expect("valid location"))
    };
    assert_eq!(
        parse("/search?tag=a&utm=x&q=rust&tag=b&page=2"),
        Ok(TestRoute::Search {
            q: Some("rust".to_owned()),
            tags: vec!["a".to_owned(), "b".to_owned()],
            page: 2,
        }),
        "unknown keys are ignored"
    );
    assert_eq!(
        parse("/search"),
        Ok(TestRoute::Search {
            q: None,
            tags: Vec::new(),
            page: 0,
        })
    );
    assert_eq!(
        parse("/search?page=two"),
        Err(flui_widgets::RouteParseError::Param {
            path: flui_widgets::RoutePath::parse("/search?page=two").expect("valid location"),
            field: "page",
            segment: "two".to_owned(),
        })
    );
    assert_eq!(
        TestRoute::back_stack(&flui_widgets::RoutePath::parse("/search?q=a").expect("valid")),
        Ok(vec![
            TestRoute::Home,
            TestRoute::Search {
                q: Some("a".to_owned()),
                tags: Vec::new(),
                page: 0,
            }
        ]),
        "the page below opens without the query"
    );
}
#[test]
fn route_locations_preserve_encoded_segment_identity() {
    crate::common::cases::run_cases(
        "route location encoding",
        &[
            (
                "lowercase escapes and separators",
                lowercase_escapes_preserve_segment_boundaries,
            ),
            ("decode percent once", percent_is_decoded_once),
            ("literal plus", plus_is_literal_path_data),
            ("encoded delimiters", reserved_delimiters_are_segment_data),
            ("UTF-8 bytes", unicode_is_encoded_as_utf8_bytes),
            (
                "controls and backslash",
                controls_and_backslashes_are_encoded,
            ),
            (
                "readable punctuation",
                readable_path_punctuation_is_preserved,
            ),
            (
                "literal dot segments",
                dot_segments_are_literal_route_values,
            ),
            ("missing digits", missing_escape_digits_are_rejected),
            ("short escape", short_escape_is_rejected),
            ("non-hex escape", non_hex_escape_is_rejected),
            ("query plus is a space", query_plus_is_a_space),
            ("query encoded plus", query_encoded_plus_is_a_plus),
            ("query order and repeats", query_keeps_order_and_repeats),
            ("query empty value", query_empty_value_prints_the_bare_key),
            ("query empty pairs", query_empty_pairs_are_dropped),
            ("query delimiters", query_delimiters_are_value_data),
            ("query bad escape", query_bad_escape_is_rejected),
            (
                "query on the last prefix",
                query_is_kept_by_the_last_prefix_only,
            ),
            ("query fields", query_fields_parse_and_default),
            ("raw fragment", raw_fragment_is_rejected),
            ("invalid UTF-8 byte", invalid_utf8_byte_is_rejected),
            ("overlong UTF-8", overlong_utf8_is_rejected),
            ("UTF-8 surrogate", utf8_surrogate_is_rejected),
            ("truncated UTF-8", truncated_utf8_is_rejected),
            ("out-of-range Unicode", out_of_range_unicode_is_rejected),
        ],
    );
}
