//! Public-API tests for `Navigator` (ADR-0019 U4).
//!
//! Driven through the real `flui_widgets::prelude` surface and a real
//! `HeadlessBinding` frame — the path `AppBinding::draw_frame` takes. If a name
//! were not exported, this file would not compile.
//!
//! # Parity oracles
//!
//! `.flutter/packages/flutter/test/widgets/navigator_test.dart` —
//! `'Can navigator navigate to and from a stateful widget'`,
//! `'Navigator.of fails gracefully when not found in context'`,
//! `'Navigator.of rootNavigator finds root Navigator'`,
//! `'Can push, pop, and replace in sequence'`, `'removeRoute'`,
//! `'remove a route whose value is awaited'`.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{lay_out, loose};
use parking_lot::Mutex;

// Exercise the public prelude import path.
use flui_widgets::prelude::*;
use flui_widgets::{
    NavigatorObserver, NavigatorRoute, PushCompletion, Route, RouteContentBuilder, RouteId,
    RouteSettings,
};

// ============================================================================
// PROBES
// ============================================================================

/// Records which route contents were built.
#[derive(Clone, Default)]
struct Built(Arc<Mutex<Vec<&'static str>>>);

impl Built {
    fn contains(&self, name: &str) -> bool {
        self.0.lock().contains(&name)
    }
    fn names(&self) -> Vec<&'static str> {
        self.0.lock().clone()
    }
}

fn page(built: &Built, name: &'static str) -> SimpleRoute<i32> {
    let built = built.clone();
    SimpleRoute::new(move |_ctx| {
        built.0.lock().push(name);
        SizedBox::new(10.0, 10.0).into_view().boxed()
    })
    .named(name)
}

/// A route implemented against the **public** [`Route`] / [`NavigatorRoute`]
/// traits, proving an app author can write one — and that `did_pop`'s refusal is
/// honoured through the public surface.
struct RefusingRoute {
    settings: RouteSettings,
    builder: RouteContentBuilder,
    pops_attempted: Arc<AtomicUsize>,
}

impl RefusingRoute {
    fn new(pops_attempted: &Arc<AtomicUsize>) -> Self {
        Self {
            settings: RouteSettings::named("refusing"),
            builder: Arc::new(|_ctx| SizedBox::new(10.0, 10.0).into_view().boxed()),
            pops_attempted: Arc::clone(pops_attempted),
        }
    }
}

impl Route for RefusingRoute {
    type Output = i32;

    fn settings(&self) -> &RouteSettings {
        &self.settings
    }

    fn did_pop(&mut self) -> bool {
        self.pops_attempted.fetch_add(1, Ordering::Relaxed);
        false
    }
}

impl NavigatorRoute for RefusingRoute {
    fn content_builder(&self) -> RouteContentBuilder {
        Arc::clone(&self.builder)
    }
}

/// A route whose result is a `String`, to exercise the erased pop-result boundary.
struct StringRoute {
    settings: RouteSettings,
    builder: RouteContentBuilder,
}

impl StringRoute {
    fn new() -> Self {
        Self {
            settings: RouteSettings::named("string"),
            builder: Arc::new(|_ctx| SizedBox::new(10.0, 10.0).into_view().boxed()),
        }
    }
}

impl Route for StringRoute {
    type Output = String;

    fn settings(&self) -> &RouteSettings {
        &self.settings
    }
}

impl NavigatorRoute for StringRoute {
    fn content_builder(&self) -> RouteContentBuilder {
        Arc::clone(&self.builder)
    }
}

/// Records observer notifications in the order delivered.
#[derive(Default)]
struct Spy(Mutex<Vec<&'static str>>);

impl Spy {
    fn kinds(&self) -> Vec<&'static str> {
        self.0.lock().clone()
    }
}

impl NavigatorObserver for Spy {
    fn did_push(&self, _route: RouteId, _previous: Option<RouteId>) {
        self.0.lock().push("push");
    }
    fn did_pop(&self, _route: RouteId, _previous: Option<RouteId>) {
        self.0.lock().push("pop");
    }
    fn did_remove(&self, _route: RouteId, _previous: Option<RouteId>) {
        self.0.lock().push("remove");
    }
    fn did_change_top(&self, _top: RouteId, _previous_top: Option<RouteId>) {
        self.0.lock().push("changeTop");
    }
}

// ============================================================================
// TESTS
// ============================================================================

/// The seeded initial route builds on the first frame, through the prelude.
///
/// Red-check: delete the `flush` in `NavigatorState::init_state`.
#[test]
fn public_navigator_initial_route_builds_through_prelude() {
    let built = Built::default();
    let handle = NavigatorHandle::new();
    handle.seed_initial(page(&built, "/"));

    let laid = lay_out(Navigator::new(handle.clone()), loose(400.0));

    assert_eq!(built.names(), vec!["/"]);
    assert_eq!(handle.route_ids().len(), 1);
    assert!(handle.is_mounted());
    // The overlay's Stack laid out under the navigator.
    assert!(laid.render_node_count() >= 2);
}

/// `push` → `pop_with(result)` → the `RouteResult` resolves. The pushed route's
/// content builds; the popped route's future carries the value.
///
/// Red-check: make `RouteRecord::did_pop` skip `did_complete`.
#[test]
fn public_navigator_push_pop_result_through_handle() {
    let built = Built::default();
    let handle = NavigatorHandle::new();
    handle.seed_initial(page(&built, "/"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));

    let result = handle.push(page(&built, "second"));
    laid.tick();
    assert!(built.contains("second"));
    assert_eq!(handle.route_ids().len(), 2);
    assert!(!result.is_completed());

    assert!(handle.pop_with(42_i32));
    laid.tick();

    assert_eq!(result.try_take(), Some(Some(42)));
    assert_eq!(handle.route_ids().len(), 1);
}

/// A pop with no result delivers the route's `current_result()` fallback —
/// Flutter's `result ?? currentResult` (`navigator.dart:481`).
///
/// Red-check: drop the `None => self.route.current_result()` arm.
#[test]
fn public_pop_without_result_uses_current_result_fallback() {
    let built = Built::default();
    let handle = NavigatorHandle::new();
    handle.seed_initial(page(&built, "/"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));

    let result = handle.push(page(&built, "second").with_current_result(7));
    laid.tick();

    assert!(handle.pop());
    laid.tick();

    assert_eq!(result.try_take(), Some(Some(7)));
}

/// A route written against the public `Route` trait that refuses `did_pop` stays,
/// and completes nothing. `maybe_pop` still reports the request handled, because
/// `popDisposition` was `pop` (`navigator.dart:5608-5610`).
///
/// Red-check: ignore `did_pop`'s return value in `RouteRecord::did_pop`.
#[test]
fn public_navigator_maybe_pop_refusal_matches_private_behavior() {
    let built = Built::default();
    let attempts = Arc::new(AtomicUsize::new(0));
    let handle = NavigatorHandle::new();
    handle.seed_initial(page(&built, "/"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));

    let result = handle.push(RefusingRoute::new(&attempts));
    laid.tick();

    assert!(handle.maybe_pop_with(1_i32), "the pop request was handled");
    laid.tick();

    assert_eq!(attempts.load(Ordering::Relaxed), 1, "did_pop was consulted");
    assert_eq!(handle.route_ids().len(), 2, "the route refused and stayed");
    assert!(!result.is_completed(), "a refused pop completes nothing");
}

/// `canPop` (`navigator.dart:5551-5566`): `false` for a lone route, `true` once a
/// second exists. `maybePop` on a lone route **bubbles** — returns `false` —
/// because `popDisposition` is `isFirst ? bubble : pop` (`:382-390`).
///
/// Red-check: make `RouteHistory::can_pop` return `present.count() > 1` (the
/// `willHandlePopInternally` branch is what it loses) — or make
/// `pop_disposition_of_top` never return `Bubble`, which flips the lone-route
/// `maybe_pop`.
#[test]
fn public_can_pop_contract() {
    let built = Built::default();
    let handle = NavigatorHandle::new();
    handle.seed_initial(page(&built, "/"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));

    assert!(!handle.can_pop(), "a single route cannot pop");
    assert!(!handle.maybe_pop(), "and maybe_pop bubbles");
    assert_eq!(handle.route_ids().len(), 1, "the root route survived");

    handle.push(page(&built, "second"));
    laid.tick();

    assert!(handle.can_pop());
    assert!(handle.maybe_pop());
    laid.tick();
    assert_eq!(handle.route_ids().len(), 1);
}

/// **A removed route still completes its future.** Oracle:
/// `'remove a route whose value is awaited'`.
///
/// Red-check: make `handle_complete` skip `did_complete`; the future never
/// resolves — which in a real app hangs every `await`.
#[test]
fn public_remove_route_completes_result() {
    let built = Built::default();
    let handle = NavigatorHandle::new();
    handle.seed_initial(page(&built, "/"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));

    let result = handle.push(page(&built, "second"));
    laid.tick();
    let top = handle.current().expect("a top route");

    assert!(handle.remove_route_with(top, 9_i32));
    laid.tick();

    assert_eq!(result.try_take(), Some(Some(9)));
    assert_eq!(handle.route_ids().len(), 1);
}

/// The ADR-0019 §4 divergence, through the public API: a wrong result type logs
/// and completes with `None`, where Flutter throws a cast error.
///
/// Red-check: `unwrap()` the downcast in `RouteRecord::did_complete`; the test
/// panics instead of resolving.
#[test]
fn public_pop_mismatched_result_logs_and_completes_none() {
    let handle = NavigatorHandle::new();
    handle.seed_initial(StringRoute::new());
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));

    let result = handle.push(StringRoute::new());
    laid.tick();

    // An `i32` for a `String` route.
    assert!(handle.pop_with(3_i32));
    laid.tick();

    assert_eq!(
        result.try_take(),
        Some(None),
        "resolves with None rather than hanging or panicking"
    );
}

/// `maybe_of` finds the **nearest** navigator; `maybe_of_root` the outermost.
/// Oracle: `'Navigator.of rootNavigator finds root Navigator'`.
///
/// Red-check: swap `find_state` / `find_root_state` in `maybe_of` / `maybe_of_root`.
#[test]
fn public_nested_navigator_lookup_nearest_and_root() {
    let nearest: Arc<Mutex<Option<NavigatorHandle>>> = Arc::new(Mutex::new(None));
    let root: Arc<Mutex<Option<NavigatorHandle>>> = Arc::new(Mutex::new(None));

    let inner = NavigatorHandle::new();
    {
        let (nearest, root) = (Arc::clone(&nearest), Arc::clone(&root));
        inner.seed_initial(SimpleRoute::<i32>::new(move |ctx| {
            *nearest.lock() = NavigatorHandle::maybe_of(ctx);
            *root.lock() = NavigatorHandle::maybe_of_root(ctx);
            SizedBox::new(5.0, 5.0).into_view().boxed()
        }));
    }

    let outer = NavigatorHandle::new();
    {
        let inner = inner.clone();
        outer.seed_initial(SimpleRoute::<i32>::new(move |_ctx| {
            Navigator::new(inner.clone()).into_view().boxed()
        }));
    }

    let _laid = lay_out(Navigator::new(outer.clone()), loose(400.0));

    let nearest = nearest.lock().clone().expect("a nearest navigator");
    let root = root.lock().clone().expect("a root navigator");

    assert_eq!(
        nearest.route_ids(),
        inner.route_ids(),
        "nearest is the inner"
    );
    assert_eq!(root.route_ids(), outer.route_ids(), "root is the outer");
    assert_ne!(inner.route_ids(), outer.route_ids());
}

/// `Navigator.maybeOf` with no navigator above returns `None` rather than
/// panicking. Oracle: `'Navigator.of fails gracefully when not found in context'`.
///
/// Red-check: make `maybe_of` fall back to a fresh handle instead of `None`.
#[test]
fn public_maybe_of_returns_none_when_absent() {
    let ran = Arc::new(AtomicUsize::new(0));
    let found: Arc<Mutex<Option<NavigatorHandle>>> = Arc::new(Mutex::new(None));

    let probe = {
        let (ran, found) = (Arc::clone(&ran), Arc::clone(&found));
        SimpleRoute::<i32>::new(move |ctx| {
            ran.fetch_add(1, Ordering::Relaxed);
            *found.lock() = NavigatorHandle::maybe_of(ctx);
            SizedBox::new(1.0, 1.0).into_view().boxed()
        })
    };

    // Build the route's content OUTSIDE any navigator, by calling its builder
    // through a plain widget: `SimpleRoute` is just a builder holder.
    let handle = NavigatorHandle::new();
    handle.seed_initial(probe);
    let _laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
    assert_eq!(ran.load(Ordering::Relaxed), 1);
    assert!(found.lock().is_some(), "inside a navigator it resolves");

    // And with no navigator above at all.
    let outside = Arc::new(Mutex::new(Some(NavigatorHandle::new())));
    {
        let outside = Arc::clone(&outside);
        let _laid = lay_out(
            LayoutBuilder::new(move |ctx, _constraints| {
                *outside.lock() = NavigatorHandle::maybe_of(ctx);
                SizedBox::new(1.0, 1.0).into_view().boxed()
            }),
            loose(400.0),
        );
    }
    assert!(
        outside.lock().is_none(),
        "no navigator above ⇒ None, not a panic"
    );
}

/// Observers still fire through the public API, after the history is mutated and
/// never inline, and `did_change_top` follows the additions/deletions.
///
/// **What this deliberately does not claim.** The additions-LIFO / deletions-FIFO
/// asymmetry (`navigator.dart:4621-4636`) is only observable on a flush carrying
/// *two or more* observations of the same kind, or one of each. The signed-off
/// public surface has no such operation — `pushReplacement` and
/// `pushAndRemoveUntil` are ported but **not exported** — so swapping the two
/// drain loops leaves this test green. That asymmetry is pinned by U2's pure-data
/// suite (`push_adds_route_and_notifies_observer_lifo`,
/// `delete_notifications_are_fifo`, `additions_precede_deletions_within_one_flush`),
/// which drives the batching APIs directly. Stated rather than implied.
///
/// Red-check: enqueue the observation inline in `handle_push` instead of returning
/// it; `push` is then observed before `changeTop` of the *previous* flush settles,
/// and the recorded sequence changes.
#[test]
fn public_observer_ordering_survives_the_public_api() {
    let built = Built::default();
    let handle = NavigatorHandle::new();
    handle.seed_initial(page(&built, "/"));

    let spy = Arc::new(Spy::default());
    handle.add_observer(Arc::clone(&spy) as Arc<dyn NavigatorObserver>);

    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
    // The mount flush announced the seeded route.
    assert_eq!(spy.kinds(), vec!["push", "changeTop"]);

    handle.push(page(&built, "second"));
    laid.tick();
    assert_eq!(spy.kinds(), vec!["push", "changeTop", "push", "changeTop"]);

    handle.pop();
    laid.tick();
    assert_eq!(
        spy.kinds(),
        vec!["push", "changeTop", "push", "changeTop", "pop", "changeTop"],
        "each flush: its observations, then didChangeTop"
    );
}

/// A removed route is announced as `didRemove`, not `didPop`
/// (`navigator.dart:3399-3402`).
///
/// Red-check: enqueue an `Observation::Pop` from `handle_removal`.
#[test]
fn public_remove_route_is_observed_as_remove_not_pop() {
    let built = Built::default();
    let handle = NavigatorHandle::new();
    handle.seed_initial(page(&built, "/"));
    let spy = Arc::new(Spy::default());
    handle.add_observer(Arc::clone(&spy) as Arc<dyn NavigatorObserver>);

    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
    handle.push(page(&built, "second"));
    laid.tick();
    let top = handle.current().expect("a top route");

    handle.remove_route(top);
    laid.tick();

    let kinds = spy.kinds();
    assert!(kinds.contains(&"remove"), "{kinds:?}");
    assert!(
        !kinds.contains(&"pop"),
        "a removed route is not popped: {kinds:?}"
    );
}

/// No `GlobalKey` anywhere on the public `Navigator` path (ADR-0019 §3.2).
///
/// Red-check: add `use flui_view::GlobalKey;` to `src/navigator/navigator.rs`.
#[test]
fn no_global_key_in_public_navigator_path() {
    const SOURCES: [(&str, &str); 3] = [
        (
            "navigator.rs",
            include_str!("../src/navigator/navigator.rs"),
        ),
        (
            "overlay_route.rs",
            include_str!("../src/navigator/overlay_route.rs"),
        ),
        ("overlay/mod.rs", include_str!("../src/overlay/mod.rs")),
    ];
    for (name, source) in SOURCES {
        for (number, line) in source.lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            assert!(
                !code.contains("GlobalKey"),
                "{name}:{} uses GlobalKey: {line}",
                number + 1
            );
        }
    }
}

/// The prelude exports exactly the approved surface, and no internals.
///
/// The *positive* half is structural: every name below is imported at the top of
/// this file or used in these tests, so a missing export is a compile error, not
/// an assertion failure. This test guards the *negative* half — that the route
/// stack's internals did not leak into the crate root.
///
/// Red-check: add `pub use navigator::history::RouteHistory;` to `lib.rs`.
#[test]
fn public_prelude_exports_exact_approved_surface() {
    const LIB: &str = include_str!("../src/lib.rs");
    const INTERNAL: [&str; 8] = [
        "RouteHistory",
        "RouteLifecycle",
        "RouteEntry",
        "ErasedRoute",
        "AnyResult",
        "FlushOutcome",
        "ObservationQueues",
        "RoutePopDisposition",
    ];

    for line in LIB.lines() {
        let code = line.trim_start();
        if !code.starts_with("pub use") && !code.starts_with("pub mod") {
            continue;
        }
        for internal in INTERNAL {
            assert!(
                !code.contains(internal),
                "lib.rs leaks the internal `{internal}`: {line}"
            );
        }
        assert!(
            !code.starts_with("pub mod overlay"),
            "the overlay module must stay private: {line}"
        );
    }

    // Approved names, named here so their absence is a compile error.
    let _: fn() -> NavigatorHandle = NavigatorHandle::new;
    let _: fn(NavigatorHandle) -> Navigator = Navigator::new;
    let _ = PushCompletion::Immediate;
    let _ = RouteSettings::named("x");
}
