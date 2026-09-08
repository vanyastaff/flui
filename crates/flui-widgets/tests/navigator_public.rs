//! Public-API tests for `Navigator`.
//!
//! Driven through the real `flui_widgets::prelude` surface and a real
//! `HeadlessBinding` frame — the path `UiRealm::draw_frame` takes. If a name
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

// ADR-0027: these public API tests capture owner-local `NavigatorHandle`s in
// test cells. The production crate has the same lint allowance; integration
// tests are separate crates, so repeat it here.
#![expect(clippy::arc_with_non_send_sync)]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::common::{LaidOut, lay_out, loose};
use parking_lot::Mutex;

// Exercise the public prelude import path.
use flui_widgets::prelude::*;
use flui_widgets::{
    GeneratedRoute, KeyedSettings, NamedRouteError, NavigatorCommand, NavigatorCommandTarget,
    NavigatorObserver, NavigatorRoute, PushCompletion, Route, RouteArguments, RouteContentBuilder,
    RouteId, RouteKey, RouteRequest, RouteSettings,
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
            builder: Rc::new(|_ctx| SizedBox::new(10.0, 10.0).into_view().boxed()),
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
        Rc::clone(&self.builder)
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
            builder: Rc::new(|_ctx| SizedBox::new(10.0, 10.0).into_view().boxed()),
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
        Rc::clone(&self.builder)
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

/// The Rust-forced divergence in the pop-result type, through the public API: a
/// wrong result type logs and completes with `None`, where Flutter throws a cast error.
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
/// drain loops leaves this test green. That asymmetry is pinned by the pure-data
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

/// No `GlobalKey` anywhere on the public `Navigator` path.
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
    const INTERNAL: [&str; 19] = [
        "RouteHistory",
        "RouteLifecycle",
        "RouteEntry",
        "ErasedRoute",
        "AnyResult",
        "FlushOutcome",
        "ObservationQueues",
        "RoutePopDisposition",
        // The transition/modal route seam and its route classes: all private.
        "RouteBinding",
        "RouteCommand",
        "BoundRoute",
        "TransitionRoute",
        "ModalRoute",
        "PageRoute",
        // Named-route generation (ADR-0024) exports `GeneratedRoute` and
        // `NamedRouteError` — imported at the top of this file, so their
        // absence is a compile error — and nothing else. `AnyResult` above is
        // the erasure that feature reuses: it stays internal, which is why
        // §7.2's replacement design adds no public `dyn Any` boundary at all.
        "RouteRegistry",
        "RouteFactory",
        "ErasedPush",
        "TypedPush",
        "PushMode",
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
    let _: fn(&RouteSettings) -> Option<&RouteArguments> = RouteSettings::arguments;
    let _: fn(&NavigatorHandle, fn(RouteId) -> bool) = NavigatorHandle::pop_until;
}

// ============================================================================
// NAMED ROUTES (ADR-0024)
// ============================================================================

/// A leaf a generated route can show, with no probe attached.
fn leaf(_ctx: &dyn BuildContext) -> BoxedView {
    SizedBox::new(10.0, 10.0).into_view().boxed()
}

/// A named route's pop result is its **own** type, delivered by the ordinary
/// `pop_with` path.
///
/// This is the case that kills the erase-the-`Output` design ADR-0024 §3.2
/// proposed: with `Output = Box<dyn Any + Send>`, `RouteRecord::did_complete`
/// would downcast the pop payload to the erasure type and every named route
/// would resolve with `None`. Registration stays typed precisely so this works
/// (§7.2), and nothing about `pop_with` changes.
///
/// Red-check: erase the route's `Output` instead of the result handle, and the
/// delivered value becomes `None`.
#[test]
fn a_named_route_popped_with_a_typed_value_delivers_that_value() {
    let built = Built::default();
    let handle = NavigatorHandle::new();
    handle.route("/details", |_request: &RouteRequest<'_>| {
        Some(SimpleRoute::<String>::new(leaf))
    });
    handle.seed_initial(page(&built, "/"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));

    let details = handle
        .push_named_typed::<String>("/details")
        .expect("'/details' is registered");
    laid.tick();
    assert!(!details.is_completed(), "the route is still on the stack");

    assert!(handle.pop_with("saved".to_owned()));
    laid.tick();

    assert_eq!(
        details.try_take(),
        Some(Some("saved".to_owned())),
        "the popped value reached the named route's own result handle"
    );
}

/// A route whose `Output` the caller never names is still navigable — the
/// regression oracle for the two-entry-point surface.
///
/// This is what the single-typed-entry-point design broke, and the whole reason
/// the split exists: registering a `PageRoute<i32>` and calling
/// `push_named("/settings")` must *navigate*, not answer `Err(ResultType)`. A
/// caller who wants to reach a screen has no reason to know that screen happens
/// to complete with an `i32`, and Flutter's `pushNamed<void>` pushes it without
/// complaint. Only `push_named_typed` — where the caller has asserted a type —
/// can answer `ResultType` at all.
///
/// A `PageRoute` specifically, because that is the route class a route table
/// exists to serve and the one whose `Output` an app is least likely to think
/// about at the call site.
///
/// Red-check: make `push_named` type-check against `()` and every leg here
/// returns `Err(ResultType)` instead of navigating.
#[test]
fn a_route_whose_output_the_caller_never_names_is_still_navigable_by_name() {
    let built = Built::default();
    let handle = NavigatorHandle::new();
    handle.route("/settings", {
        let built = built.clone();
        move |_request: &RouteRequest<'_>| {
            let built = built.clone();
            Some(
                PageRoute::<i32>::new(move |ctx, _animation, _secondary| {
                    built.0.lock().push("settings-page");
                    leaf(ctx)
                })
                .named("/settings"),
            )
        }
    });
    handle.seed_initial(page(&built, "/"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
    let root = handle.current().expect("the seeded route is on the stack");

    // The call the old surface refused: no `T`, and the route delivers `i32`.
    let settings = handle
        .push_named("/settings")
        .expect("a PageRoute<i32> is reachable by name without naming i32");
    laid.tick();

    assert_eq!(
        handle.route_ids(),
        vec![root, settings],
        "the stack grew and the id `push_named` reported is the route on top"
    );
    assert!(
        built.contains("settings-page"),
        "and the route that built is the registered one, got {:?}",
        built.names()
    );
}

/// The other five untyped operations carry the same guarantee as
/// [`a_route_whose_output_the_caller_never_names_is_still_navigable_by_name`]:
/// none of them names the route's result type, so none can refuse on it.
///
/// A `SimpleRoute<i32>` rather than the `PageRoute` its sibling uses, for a
/// reason worth stating: a `PageRoute` is a transition route, so a *replaced*
/// one is not removed from the stack until its exit animation ends
/// (`finished_when_popped` is `controller.isDismissed`). Asserting exact stack
/// contents one tick after a replacement therefore needs an instant route —
/// the transition timing is real behavior, not something to assert around.
///
/// Red-check: make any one of the five type-check against `()` and its leg
/// returns `Err(ResultType)` instead of navigating.
#[test]
fn the_other_five_untyped_operations_also_navigate_an_unnamed_output_route() {
    let built = Built::default();
    let handle = NavigatorHandle::new();
    handle.route("/screen", |_request: &RouteRequest<'_>| {
        Some(SimpleRoute::<i32>::new(leaf))
    });
    handle.seed_initial(page(&built, "/"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
    let root = handle.current().expect("the seeded route is on the stack");

    // Setup, not a leg: every operation below either replaces or pops the top,
    // so there has to be a route above the root for them to act on. Its own
    // `push_named` is covered by this test's sibling.
    handle.push_named("/screen").expect("registered");
    laid.tick();

    let replaced = handle
        .push_replacement_named("/screen")
        .expect("registered");
    laid.tick();
    assert_eq!(handle.route_ids(), vec![root, replaced]);

    let popped_and_pushed = handle.pop_and_push_named("/screen").expect("registered");
    laid.tick();
    assert_eq!(handle.route_ids(), vec![root, popped_and_pushed]);

    let replaced_with = handle
        .push_replacement_named_with("/screen", 1_i32)
        .expect("registered");
    laid.tick();
    assert_eq!(handle.route_ids(), vec![root, replaced_with]);

    let popped_with = handle
        .pop_and_push_named_with("/screen", 2_i32)
        .expect("registered");
    laid.tick();
    assert_eq!(handle.route_ids(), vec![root, popped_with]);

    let swept = handle
        .push_named_and_remove_until("/screen", |candidate| candidate == root)
        .expect("registered");
    laid.tick();
    assert_eq!(handle.route_ids(), vec![root, swept]);

    // Five distinct routes, not one reported five times — which is what a
    // `current()`-derived id would have produced had a push silently failed.
    let mut ids = vec![
        replaced,
        popped_and_pushed,
        replaced_with,
        popped_with,
        swept,
    ];
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 5, "each named push minted its own route");
}

/// `push_replacement_named_with` delivers its `result` to the **replaced**
/// route, not the new one — the meaning `_with` carries everywhere on this
/// handle (`pop_with`, `push_replacement_with`, `remove_route_with`,
/// `maybe_pop_with`).
///
/// Red-check: drop the `result` through to `PushMode::Replace { result: None }`
/// and the replaced route completes with its `None` fallback instead of `99`.
#[test]
fn push_replacement_named_with_delivers_its_result_to_the_replaced_route() {
    let built = Built::default();
    let handle = NavigatorHandle::new();
    handle.route("/saved", |_request: &RouteRequest<'_>| {
        Some(SimpleRoute::<i32>::new(leaf))
    });
    handle.seed_initial(page(&built, "/"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));

    let editor = handle.push(page(&built, "editor"));
    laid.tick();

    let saved = handle
        .push_replacement_named_with("/saved", 99_i32)
        .expect("'/saved' is registered");
    laid.tick();

    assert_eq!(
        editor.try_take(),
        Some(Some(99)),
        "the route that was replaced received the result"
    );
    assert_eq!(
        handle.route_ids().last().copied(),
        Some(saved),
        "and the replacement is on top"
    );
    assert_eq!(handle.route_ids().len(), 2, "root + the replacement");
}

/// `pop_and_push_named_with` delivers its `result` to the **popped** route.
///
/// Red-check: swap it to a plain `pop()` and the popped route completes with
/// `None`.
#[test]
fn pop_and_push_named_with_delivers_its_result_to_the_popped_route() {
    let built = Built::default();
    let handle = NavigatorHandle::new();
    handle.route("/next", |_request: &RouteRequest<'_>| {
        Some(SimpleRoute::<i32>::new(leaf))
    });
    handle.seed_initial(page(&built, "/"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));

    let departing = handle.push(page(&built, "departing"));
    laid.tick();

    let arrived = handle
        .pop_and_push_named_with("/next", 5_i32)
        .expect("'/next' is registered");
    laid.tick();

    assert_eq!(
        departing.try_take(),
        Some(Some(5)),
        "the popped route received the result"
    );
    assert_eq!(handle.route_ids().last().copied(), Some(arrived));
}

/// Every route class this crate ships is registerable, and so is a route an app
/// implements itself.
///
/// The v2 design could register none of them: `Route::current_result` returns
/// by value, so every shipped route is bounded `T: Send + Clone + 'static`, and
/// `Box<dyn Any + Send>` is not `Clone` (ADR-0024 §7.2). That was a compile
/// error, which is why this test's *existence* is half its value — the other
/// half is that each route actually pushes and delivers its own typed result.
///
/// Red-check: bound `GeneratedRoute::new` to a sealed set of route types and
/// the `RefusingRoute` arm stops compiling.
#[test]
fn a_page_route_a_popup_route_and_a_simple_route_are_all_registerable() {
    let built = Built::default();
    let attempts = Arc::new(AtomicUsize::new(0));
    let handle = NavigatorHandle::new();
    handle.route("/page", |_request: &RouteRequest<'_>| {
        Some(
            PageRoute::<bool>::new(|ctx, _animation, _secondary| leaf(ctx))
                .with_current_result(true),
        )
    });
    handle.route("/popup", |_request: &RouteRequest<'_>| {
        Some(PopupRoute::<i32>::new(|ctx, _animation, _secondary| leaf(ctx)).with_current_result(7))
    });
    handle.route("/simple", |_request: &RouteRequest<'_>| {
        Some(SimpleRoute::<String>::new(leaf).with_current_result("plain".to_owned()))
    });
    handle.on_generate_route({
        let attempts = Arc::clone(&attempts);
        move |request: &RouteRequest<'_>| match request.name()? {
            // A user-implemented `NavigatorRoute`, erased by exactly the same
            // machinery as the three shipped classes above (ADR-0024 §7.4).
            "/custom" => Some(GeneratedRoute::new(RefusingRoute::new(&attempts))),
            _ => None,
        }
    });
    handle.seed_initial(page(&built, "/"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));

    let page_result = handle
        .push_named_typed::<bool>("/page")
        .expect("registered");
    laid.tick();
    assert!(handle.pop());
    laid.tick();
    assert_eq!(page_result.try_take(), Some(Some(true)), "PageRoute<bool>");

    let popup_result = handle
        .push_named_typed::<i32>("/popup")
        .expect("registered");
    laid.tick();
    assert!(handle.pop());
    laid.tick();
    assert_eq!(popup_result.try_take(), Some(Some(7)), "PopupRoute<i32>");

    let simple_result = handle
        .push_named_typed::<String>("/simple")
        .expect("registered");
    laid.tick();
    assert!(handle.pop());
    laid.tick();
    assert_eq!(
        simple_result.try_take(),
        Some(Some("plain".to_owned())),
        "SimpleRoute<String>"
    );

    let custom_result = handle
        .push_named_typed::<i32>("/custom")
        .expect("registered");
    laid.tick();
    assert!(handle.pop());
    laid.tick();
    assert_eq!(
        attempts.load(Ordering::Relaxed),
        1,
        "the app-implemented route's own did_pop ran"
    );
    assert!(
        !custom_result.is_completed(),
        "and its refusal was honoured — it is still on the stack"
    );
}

/// Asking `push_named_typed` for the wrong result type is refused at **push**
/// time, with the stack untouched — ADR-0024 §7.3's "with nothing pushed" — and
/// the route that was generated for the attempt is **disposed**.
///
/// The disposal half is not incidental. `Route::dispose` is an explicit method,
/// not `Drop`, and this refusal is a routine path rather than a rare one, so a
/// `GeneratedRoute` that is dropped unpushed has to run it — Flutter discharges
/// the same obligation in `defaultGenerateInitialRoutes`' failure branch. The
/// leak is invisible without this assertion.
///
/// Red-check for the refusal: drop the `TypeId` comparison in
/// `GeneratedRoute::checked`. Red-check for the disposal: delete
/// `impl Drop for GeneratedRoute`.
#[test]
fn push_named_typed_with_the_wrong_result_type_errors_disposes_the_route_and_changes_nothing() {
    /// A route that records its own `dispose()` into a shared log.
    struct DisposeProbe {
        settings: RouteSettings,
        builder: RouteContentBuilder,
        disposals: Arc<AtomicUsize>,
    }

    impl Route for DisposeProbe {
        type Output = String;

        fn settings(&self) -> &RouteSettings {
            &self.settings
        }

        fn dispose(&mut self) {
            self.disposals.fetch_add(1, Ordering::Relaxed);
        }
    }

    impl NavigatorRoute for DisposeProbe {
        fn content_builder(&self) -> RouteContentBuilder {
            Rc::clone(&self.builder)
        }
    }

    let built = Built::default();
    let disposals = Arc::new(AtomicUsize::new(0));
    let handle = NavigatorHandle::new();
    handle.route("/details", {
        let disposals = Arc::clone(&disposals);
        move |_request: &RouteRequest<'_>| {
            Some(DisposeProbe {
                settings: RouteSettings::named("/details"),
                builder: Rc::new(leaf),
                disposals: Arc::clone(&disposals),
            })
        }
    });
    handle.seed_initial(page(&built, "/"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
    let before = handle.route_ids();
    let spy = Arc::new(Spy::default());
    handle.add_observer(Arc::clone(&spy) as Arc<dyn NavigatorObserver>);

    match handle.push_named_typed::<i32>("/details") {
        Err(NamedRouteError::ResultType {
            name,
            expected,
            actual,
        }) => {
            assert_eq!(name, "/details");
            assert!(
                expected.contains("i32"),
                "`expected` names the requested type, got {expected}"
            );
            assert!(
                actual.contains("String"),
                "`actual` names the route's own Output, got {actual}"
            );
        }
        other => panic!("expected a ResultType error, got {other:?}"),
    }
    laid.tick();

    assert_eq!(handle.route_ids(), before, "nothing was pushed");
    assert_eq!(spy.kinds(), Vec::<&str>::new(), "and nothing was observed");
    assert_eq!(
        disposals.load(Ordering::Relaxed),
        1,
        "the route generated for the refused push was disposed exactly once"
    );

    // The same refusal against a **shipped** route class. `PageRoute` is what
    // ADR-0024 §7.2 names as the class a route table exists to serve, and its
    // disposal runs `PageRoute -> ModalRoute::dispose -> TransitionRoute::dispose`
    // on a route that was never installed — no binding filled, no overlay entry,
    // no animation controller. Every access down that chain is `Option`-guarded
    // today; nothing pinned it, so a future unguarded `expect` on this path would
    // have turned a routine refusal into a panic with no test to catch it.
    handle.route("/page", |_request: &RouteRequest<'_>| {
        Some(PageRoute::<String>::new(|ctx, _animation, _secondary| {
            leaf(ctx)
        }))
    });
    let before_page = handle.route_ids();

    match handle.push_named_typed::<i32>("/page") {
        Err(NamedRouteError::ResultType { name, actual, .. }) => {
            assert_eq!(name, "/page");
            assert!(
                actual.contains("String"),
                "`actual` names the PageRoute's own Output, got {actual}"
            );
        }
        other => panic!("expected a ResultType error from the PageRoute, got {other:?}"),
    }
    laid.tick();

    assert_eq!(
        handle.route_ids(),
        before_page,
        "the never-installed PageRoute was disposed without reaching the stack"
    );
}

/// `pop_and_push_named` really pops; `push_replacement_named` really replaces.
///
/// The two leave an identical stack (`[root, X]`) and both hand a `_with` value
/// to the departing route, so stack shape and result delivery cannot tell them
/// apart — rewriting `pop_and_push_named` as a `PushMode::Replace` keeps every
/// other test in this file green, the parity leg included. The observer stream
/// is the discriminator: Flutter's `popAndPushNamed` is `pop()` then
/// `pushNamed()`, so a `didPop` is observed; `pushReplacement` reports
/// `didReplace` and never `didPop`.
///
/// Red-check: give `pop_and_push_named` a `PushMode::Replace` body and the
/// `"pop"` assertion fails.
#[test]
fn pop_and_push_named_observes_a_pop_where_push_replacement_named_does_not() {
    /// Mount a navigator two routes deep, run `navigate`, report what the
    /// observer heard *after* the setup.
    fn observed_after_setup(navigate: impl FnOnce(&NavigatorHandle)) -> Vec<&'static str> {
        let built = Built::default();
        let handle = NavigatorHandle::new();
        handle.route("/next", |_request: &RouteRequest<'_>| {
            Some(SimpleRoute::<i32>::new(leaf))
        });
        handle.seed_initial(page(&built, "/"));
        let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
        handle.push(page(&built, "departing"));
        laid.tick();

        // Observe only the operation under test, not the setup.
        let spy = Arc::new(Spy::default());
        handle.add_observer(Arc::clone(&spy) as Arc<dyn NavigatorObserver>);
        navigate(&handle);
        laid.tick();
        spy.kinds()
    }

    let popped = observed_after_setup(|handle| {
        handle.pop_and_push_named("/next").expect("registered");
    });
    let replaced = observed_after_setup(|handle| {
        handle.push_replacement_named("/next").expect("registered");
    });

    assert_eq!(
        popped,
        vec!["pop", "changeTop", "push", "changeTop"],
        "pop_and_push_named is a pop followed by a push, and both are observed"
    );
    assert_eq!(
        replaced,
        vec!["changeTop"],
        "push_replacement_named reports didReplace — which this Spy does not \
         record — and neither didPop nor didPush, so only the top change shows"
    );
    assert!(
        popped.contains(&"pop") && !replaced.contains(&"pop"),
        "the didPop is the discriminator the stack shape cannot provide"
    );
}

/// `pop_and_push_named` resolves the name **before** it pops, so a name nothing
/// answers pops nothing.
///
/// This is the recorded divergence from the oracle. Flutter's
/// `popAndPushNamed` is literally `pop(result); return pushNamed(routeName);` —
/// the pop is committed first, so a generator that declines leaves the app one
/// route shallower with nothing pushed in its place, and throws from the middle
/// of a two-step operation. Reversing the two makes the failure total.
///
/// The success path cannot see the difference — both orderings pop then push —
/// which is exactly why this test exists and why the success-path tests do not
/// cover the divergence.
///
/// Red-check: move `self.pop()` above `self.resolve_named(&request)?` in
/// `pop_and_push_named` and the stack assertion fails.
#[test]
fn pop_and_push_named_with_an_unresolvable_name_pops_nothing() {
    let built = Built::default();
    let handle = NavigatorHandle::new();
    handle.route("/known", |_request: &RouteRequest<'_>| {
        Some(SimpleRoute::<i32>::new(leaf))
    });
    handle.seed_initial(page(&built, "/"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));

    let doomed = handle.push(page(&built, "still-here"));
    laid.tick();
    let before = handle.route_ids();
    assert_eq!(
        before.len(),
        2,
        "the route that must survive is on the stack"
    );

    let spy = Arc::new(Spy::default());
    handle.add_observer(Arc::clone(&spy) as Arc<dyn NavigatorObserver>);

    let refused = handle.pop_and_push_named("/nowhere");
    laid.tick();

    assert_eq!(
        refused.expect_err("no registration answers '/nowhere'"),
        NamedRouteError::Unresolved {
            name: "/nowhere".to_owned()
        }
    );
    assert_eq!(
        handle.route_ids(),
        before,
        "the failed generation popped nothing — Flutter's ordering would have \
         left the stack one route shallower"
    );
    assert!(
        !doomed.is_completed(),
        "and the route that would have been popped never completed"
    );
    assert_eq!(
        spy.kinds(),
        Vec::<&str>::new(),
        "no observer heard anything"
    );
}

/// The `_with` sibling carries the same guarantee, and additionally must not
/// deliver its result to a route it did not pop.
///
/// Red-check: pop before resolving in `pop_and_push_named_with` and the
/// surviving route completes with `7`.
#[test]
fn pop_and_push_named_with_an_unresolvable_name_delivers_its_result_to_nobody() {
    let built = Built::default();
    let handle = NavigatorHandle::new();
    handle.seed_initial(page(&built, "/"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));

    let doomed = handle.push(page(&built, "still-here"));
    laid.tick();
    let before = handle.route_ids();

    let refused = handle.pop_and_push_named_with("/nowhere", 7_i32);
    laid.tick();

    assert!(refused.is_err(), "the name resolves to nothing");
    assert_eq!(handle.route_ids(), before, "nothing was popped");
    assert_eq!(
        doomed.try_take(),
        None,
        "the surviving route has not completed, so it received no result"
    );
}

/// An unresolvable name with no fallback registered errors, changes nothing,
/// and notifies no observer — Flutter's
/// `'Navigator.onGenerateRoute returned null'` assertion, as a `Result`
/// (`PANIC-POLICY`: a route name is caller input, not a framework invariant).
///
/// Red-check: make `resolve_named` fall back to pushing a placeholder route and
/// the stack assertion fails.
#[test]
fn an_unresolvable_name_errors_without_touching_the_stack_or_the_observers() {
    let built = Built::default();
    let handle = NavigatorHandle::new();
    handle.route("/known", |_request: &RouteRequest<'_>| {
        Some(SimpleRoute::<i32>::new(leaf))
    });
    handle.seed_initial(page(&built, "/"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
    let before = handle.route_ids();
    let spy = Arc::new(Spy::default());
    handle.add_observer(Arc::clone(&spy) as Arc<dyn NavigatorObserver>);

    let refused = handle.push_named("/nowhere");
    laid.tick();

    assert_eq!(
        refused.expect_err("no registration answers '/nowhere'"),
        NamedRouteError::Unresolved {
            name: "/nowhere".to_owned()
        }
    );
    assert_eq!(handle.route_ids(), before, "the stack is untouched");
    assert_eq!(
        spy.kinds(),
        Vec::<&str>::new(),
        "no observer heard anything"
    );
}

/// The unknown-route fallback runs **only after** the generator declined, and
/// is offered the caller's own arguments payload.
///
/// The identity half compares against an `Arc` the *test* built and handed in
/// through `RouteSettings::with_arguments_shared`, so it is a genuinely
/// independent handle: `with_arguments` would have minted a second `Arc` and
/// this assertion would fail. (Comparing the fallback's `&RouteSettings` to
/// itself, or to a `RouteSettings` whose `PartialEq` is already `Arc::ptr_eq`,
/// asserts the same thing twice and cannot fail.)
///
/// Red-check for the ordering: consult the fallback before the generator, and
/// the recorded sequence inverts. Red-check for identity: relay the payload
/// through `with_arguments` instead of `with_arguments_shared`.
#[test]
fn on_unknown_route_runs_only_after_the_generator_declined_and_sees_the_callers_payload() {
    let built = Built::default();
    let stages = Arc::new(Mutex::new(Vec::<&'static str>::new()));
    let offered: Arc<Mutex<Option<RouteArguments>>> = Arc::new(Mutex::new(None));

    let handle = NavigatorHandle::new();
    handle.route("/known", {
        let stages = Arc::clone(&stages);
        move |_request: &RouteRequest<'_>| {
            stages.lock().push("table");
            Some(SimpleRoute::<i32>::new(leaf))
        }
    });
    handle.on_generate_route({
        let stages = Arc::clone(&stages);
        move |_request: &RouteRequest<'_>| {
            stages.lock().push("generate");
            None
        }
    });
    handle.on_unknown_route({
        let stages = Arc::clone(&stages);
        let offered = Arc::clone(&offered);
        move |request: &RouteRequest<'_>| {
            stages.lock().push("unknown");
            *offered.lock() = request.settings().arguments().cloned();
            assert_eq!(request.name(), Some("/missing"));
            Some(GeneratedRoute::new(SimpleRoute::<i32>::new(leaf)))
        }
    });
    handle.seed_initial(page(&built, "/"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));

    let payload: RouteArguments = Arc::new(7_i32);
    let fallback = handle
        .push_named(RouteSettings::named("/missing").with_arguments_shared(Arc::clone(&payload)))
        .expect("the unknown-route fallback answers");
    laid.tick();

    assert_eq!(
        stages.lock().clone(),
        vec!["generate", "unknown"],
        "the table did not match, the generator declined, then the fallback ran"
    );
    let seen = offered.lock().clone().expect("the fallback ran");
    assert!(
        Arc::ptr_eq(&seen, &payload),
        "the fallback got the caller's own payload object, not a rebuilt one"
    );
    assert_eq!(
        handle.route_ids().last().copied(),
        Some(fallback),
        "and its route was pushed"
    );
}

/// `with_arguments_shared` forwards a payload the caller already holds without
/// re-wrapping it; `with_arguments` cannot, because it takes the value and
/// mints a fresh `Arc`.
///
/// Pointer identity is what `RouteSettings`' own `PartialEq` and Flutter's
/// `same(arguments)` oracle mean by "the same arguments", so relaying through
/// the wrong constructor silently changes the answer.
///
/// Red-check: implement `with_arguments_shared` as
/// `self.with_arguments(arguments)` and the relayed payload stops being
/// `ptr_eq` with the original.
#[test]
fn with_arguments_shared_relays_a_payload_without_changing_its_identity() {
    let payload: RouteArguments = Arc::new("shared".to_owned());

    let original = RouteSettings::named("/a").with_arguments_shared(Arc::clone(&payload));
    let relayed = RouteSettings::named("/b").with_arguments_shared(
        original
            .arguments()
            .expect("the original carries one")
            .clone(),
    );

    assert!(
        Arc::ptr_eq(
            relayed.arguments().expect("the relay carries one"),
            &payload
        ),
        "a relayed payload is the same object"
    );
    assert_eq!(
        relayed.argument::<String>().map(String::as_str),
        Some("shared"),
        "and it still downcasts to its concrete type"
    );

    // The contrast that makes the above meaningful: the by-value builder mints
    // a new Arc even from an identical value.
    let rebuilt = RouteSettings::named("/c").with_arguments("shared".to_owned());
    assert!(
        !Arc::ptr_eq(rebuilt.arguments().expect("carries one"), &payload),
        "with_arguments is a fresh allocation, which is why the relay needs its own constructor"
    );
}

/// A table entry short-circuits: the catch-all generator is never consulted for
/// a name the table answers — Flutter's `WidgetsApp._onGenerateRoute`, which
/// returns the `routes` entry without reaching `widget.onGenerateRoute`.
///
/// Red-check: consult the generator before the table and the call counter rises.
#[test]
fn a_table_entry_wins_and_the_generate_hook_is_never_consulted() {
    let built = Built::default();
    let generator_calls = Arc::new(AtomicUsize::new(0));
    let handle = NavigatorHandle::new();
    handle.route("/home", {
        let built = built.clone();
        move |_request: &RouteRequest<'_>| Some(page(&built, "from-table"))
    });
    handle.on_generate_route({
        let generator_calls = Arc::clone(&generator_calls);
        move |_request: &RouteRequest<'_>| {
            generator_calls.fetch_add(1, Ordering::Relaxed);
            None
        }
    });
    handle.seed_initial(page(&built, "/"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));

    handle.push_named("/home").expect("the table answers");
    laid.tick();

    assert_eq!(
        generator_calls.load(Ordering::Relaxed),
        0,
        "the table answered, so the generator was never asked"
    );
    assert!(
        built.contains("from-table"),
        "and the table's route is what built"
    );
}

/// A factory that captures a handle and navigates re-entrantly does not
/// deadlock.
///
/// **The claim this pins changed, and it is weaker now: *survivable*, not
/// supported.** It used to reach the navigator through `RouteRequest::navigator`,
/// which advertised re-entrant navigation as a capability. That accessor is gone
/// (its justification was circular and it had zero production callers), so the
/// factory here obtains a handle the only way left — ordinary capture, which
/// nothing prevents and which this crate does not endorse.
///
/// What is still load-bearing is the lock discipline: every factory is invoked
/// with the registry guard released, because `parking_lot::Mutex` is not
/// reentrant. A caller who captures a handle anyway gets a navigator that
/// behaves, not one that hangs.
///
/// The cell is how the test breaks the `Arc` cycle the capture creates —
/// registry → closure → handle → `NavigatorShared` → registry — which is exactly
/// the cost the withdrawn accessor was introduced to avoid, and exactly why
/// capturing is not the recommended shape.
///
/// Red-check for the lock: hold the registry guard across the factory call in
/// `RouteRegistry::resolve` and this test deadlocks the owner thread.
#[test]
fn a_factory_that_pushes_re_entrantly_does_not_deadlock() {
    let built = Built::default();
    let cell: Rc<RefCell<Option<NavigatorHandle>>> = Rc::new(RefCell::new(None));
    let handle = NavigatorHandle::new();
    handle.route("/inner", {
        let built = built.clone();
        move |_request: &RouteRequest<'_>| Some(page(&built, "inner"))
    });
    handle.route("/outer", {
        let built = built.clone();
        let cell = Rc::clone(&cell);
        move |_request: &RouteRequest<'_>| {
            cell.borrow()
                .clone()
                .expect("the test filled the cell before mounting")
                .push_named("/inner")
                .expect("'/inner' is in the table");
            Some(page(&built, "outer"))
        }
    });
    *cell.borrow_mut() = Some(handle.clone());
    handle.seed_initial(page(&built, "/"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));

    let outer = handle.push_named("/outer").expect("the table answers");
    laid.tick();

    assert_eq!(
        handle.route_ids().len(),
        3,
        "root, then the re-entrant '/inner', then '/outer' on top"
    );
    assert_eq!(
        handle.route_ids().last().copied(),
        Some(outer),
        "the outer route landed above the one its own factory pushed"
    );
    assert!(built.contains("inner") && built.contains("outer"));

    *cell.borrow_mut() = None;
}

/// A factory is handed the caller's own name and arguments.
///
/// **What this test used to pin, and what became of it.** It asserted two
/// claims: that `RouteRequest::navigator()` returned *this* navigator rather than
/// any navigator, and that the request carried the caller's name and arguments.
/// The first claim's subject no longer exists — `navigator()` is withdrawn — so
/// it is not pinned by anything, and nothing needs to pin it. The second claim
/// stands and is what remains here.
///
/// It is also no longer a `Mutex`-in-a-closure exercise: with no handle to
/// record, the probe just records the two values.
///
/// Red-check: build the `RouteSettings` from the name alone in
/// `NavigatorHandle::push_named`, dropping the caller's payload, and the argument
/// assertion fails.
#[test]
fn a_factory_is_handed_the_callers_name_and_arguments() {
    let built = Built::default();
    let seen: Arc<Mutex<Option<(String, u32)>>> = Arc::new(Mutex::new(None));

    let handle = NavigatorHandle::new();
    handle.route("/probe", {
        let seen = Arc::clone(&seen);
        move |request: &RouteRequest<'_>| {
            *seen.lock() = Some((
                request.name()?.to_owned(),
                request.argument::<u32>().copied()?,
            ));
            Some(SimpleRoute::<i32>::new(leaf))
        }
    });
    handle.seed_initial(page(&built, "/"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));

    handle
        .push_named(RouteSettings::named("/probe").with_arguments(1776_u32))
        .expect("registered");
    laid.tick();

    let (name, argument) = seen.lock().take().expect("the factory ran");
    assert_eq!(name, "/probe");
    assert_eq!(argument, 1776);
}

/// A `RouteKey<T>` carries the result type with the name, so the keyed path
/// never has to name `T` twice and cannot drift.
///
/// `route_keyed` refuses a mismatched route at **compile** time — registering a
/// `SimpleRoute<String>` under a `RouteKey<u32>` is a type error, which is why
/// this test's own compilation is part of what it asserts — and `push_keyed`
/// infers `T` from the key, so the caller writes no turbofish and can produce no
/// `ResultType`.
///
/// Red-check: change `COUNT`'s type parameter to `RouteKey<String>` and the
/// registration stops compiling; change `push_keyed`'s body to drop the result
/// handle and the delivered-value assertion fails.
#[test]
fn a_route_key_carries_its_result_type_from_registration_to_delivery() {
    const COUNT: RouteKey<i32> = RouteKey::new("/count");
    const ORDER: RouteKey<u32> = RouteKey::new("/order");

    let built = Built::default();
    let handle = NavigatorHandle::new();
    handle.route_keyed(COUNT, |_request: &RouteRequest<'_>| {
        Some(SimpleRoute::<i32>::new(leaf).with_current_result(41))
    });
    handle.route_keyed(ORDER, |request: &RouteRequest<'_>| {
        let id = request.argument::<u32>().copied().unwrap_or(0);
        Some(SimpleRoute::<u32>::new(leaf).with_current_result(id))
    });
    handle.seed_initial(page(&built, "/"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));

    // No turbofish anywhere: `T` comes from the key.
    let count = handle.push_keyed(COUNT).expect("registered");
    laid.tick();
    assert!(handle.pop_with(42_i32));
    laid.tick();
    assert_eq!(
        count.try_take(),
        Some(Some(42)),
        "the key's T is the type the pop delivers"
    );

    // Arguments ride on the key's own request builder, never on a `_with`.
    let order = handle
        .push_keyed(ORDER.with_arguments(1776_u32))
        .expect("registered");
    laid.tick();
    assert!(handle.pop());
    laid.tick();
    assert_eq!(
        order.try_take(),
        Some(Some(1776)),
        "the arguments reached the factory and came back through the key's T"
    );
}

/// The keyed path can relay a payload the caller already holds without changing
/// its identity — and without the double-wrap that `request` would produce.
///
/// The asymmetry this closes was ours: `with_arguments_shared` was added to the
/// string path only, and `RouteKey::with_arguments` takes its payload **by value**, so
/// handing it an existing `RouteArguments` wraps an `Arc` in another `Arc`. The
/// stored concrete type becomes `RouteArguments` itself, and the factory's
/// `argument::<OriginalType>()` then answers `None` — silently, since the
/// double-wrap is perfectly well-typed.
///
/// Red-check: implement `with_arguments_shared` as `self.with_arguments(arguments)` and both
/// halves fail — `ptr_eq` because a fresh `Arc` was minted, and the factory's
/// downcast because the payload is now `Arc<Arc<dyn Any …>>`.
#[test]
fn route_key_with_arguments_shared_relays_a_payload_without_changing_its_identity() {
    const ORDER: RouteKey<u32> = RouteKey::new("/order");

    /// What the factory saw: the payload it was handed, and whether that payload
    /// still downcasts to the caller's original concrete type.
    struct RelayedPayload {
        arguments: RouteArguments,
        downcast: Option<u32>,
    }

    let built = Built::default();
    let seen: Arc<Mutex<Option<RelayedPayload>>> = Arc::new(Mutex::new(None));

    let handle = NavigatorHandle::new();
    handle.route_keyed(ORDER, {
        let seen = Arc::clone(&seen);
        move |request: &RouteRequest<'_>| {
            *seen.lock() = Some(RelayedPayload {
                arguments: request.settings().arguments().cloned()?,
                // The half a double-wrap breaks: the ORIGINAL concrete type.
                downcast: request.argument::<u32>().copied(),
            });
            Some(SimpleRoute::<u32>::new(leaf))
        }
    });
    handle.seed_initial(page(&built, "/"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));

    // A payload the caller already holds as a `RouteArguments`, exactly as a
    // relay site would.
    let payload: RouteArguments = Arc::new(1776_u32);
    handle
        .push_keyed(ORDER.with_arguments_shared(Arc::clone(&payload)))
        .expect("registered");
    laid.tick();

    let relayed = seen.lock().take().expect("the factory ran");
    assert!(
        Arc::ptr_eq(&relayed.arguments, &payload),
        "the factory got the caller's own payload object, not a rebuilt one"
    );
    assert_eq!(
        relayed.downcast,
        Some(1776),
        "and it still downcasts to the ORIGINAL concrete type — a double-wrap \
         would make this None while every other assertion still passed"
    );
}

/// `RouteKey` is a name plus a compile-time promise, so its identity is its
/// name — and `T` costs it no trait bounds.
///
/// The `HashMap` line is the load-bearing one: a derived `Hash`/`Eq` would have
/// demanded `T: Hash + Eq`, which a route `Output` need not be, so keys could
/// not live in a collection.
///
/// Red-check: derive `Clone`/`Eq`/`Hash` instead of writing them and this stops
/// compiling for `RouteKey<NotHashable>`.
#[test]
fn route_key_identity_is_its_name_and_costs_its_output_type_no_bounds() {
    use std::collections::HashMap;

    /// Deliberately implements none of `Clone`, `Eq`, `Hash`, `Debug`.
    struct NotHashable;

    const A: RouteKey<NotHashable> = RouteKey::new("/a");
    const ALSO_A: RouteKey<NotHashable> = RouteKey::new("/a");
    const B: RouteKey<NotHashable> = RouteKey::new("/b");

    assert_eq!(A, ALSO_A, "two keys with the same name are the same key");
    assert_ne!(A, B);
    assert_eq!(A.name(), "/a");
    let described = format!("{A:?}");
    assert!(
        described.contains("/a"),
        "Debug names the route: {described}"
    );

    // Copy, and usable as a map key, both for an `Output` with no bounds at all.
    //
    // A local binding, deliberately: `A` is a `const`, so `let copied = A;`
    // materialises a fresh value at every mention whether or not `RouteKey` is
    // `Copy` — that assertion passed with the `Copy` impl removed, surviving on
    // `Clone::clone` returning `*self`. Moving `key` by value twice is what
    // actually requires `Copy`.
    let key = RouteKey::<NotHashable>::new("/a");
    let mut by_key: HashMap<RouteKey<NotHashable>, &str> = HashMap::new();
    by_key.insert(key, "first");
    by_key.insert(B, "second");
    assert_eq!(
        by_key.get(&key),
        Some(&"first"),
        "key survived being moved in"
    );
    assert_eq!(
        by_key.get(&ALSO_A),
        Some(&"first"),
        "and equals its const twin"
    );
    assert_eq!(by_key.len(), 2);

    // The same must hold for the key's request type, which a `#[derive(Debug)]`
    // would have broken by generating `impl<T: Debug> Debug`: a caller could
    // then not put a `KeyedSettings<T>` inside their own derived-`Debug` struct
    // unless the route's `Output` happened to be `Debug`.
    #[derive(Debug)]
    struct CallerHeldRequest {
        pending: KeyedSettings<NotHashable>,
    }

    let held = CallerHeldRequest {
        pending: KeyedSettings::from(A),
    };
    let described = format!("{held:?}");
    assert!(
        described.contains("/a"),
        "the request's Debug names the route: {described}"
    );
    assert_eq!(held.pending.settings().name(), Some("/a"));
}

/// Mixing the typed and untyped registration paths on one name is a collision,
/// and it is guarded.
///
/// A `RouteKey` type-checks one registration site; the table is keyed by
/// **name**, so a name also bound through the untyped `route` with a different
/// `Output` can be reached by a keyed push. The sibling test covers the
/// keyed-vs-keyed form of the same hole; between them they show it takes two
/// registration sites disagreeing, not one caller's mistake.
///
/// Red-check: drop the `TypeId` comparison in `GeneratedRoute::checked` and this
/// **panics** inside `TypedPush::push` — the push lands first and the
/// `RouteResult` downcast then fails a `BUG:` `expect`. A silently wrong result
/// is not reachable; what the guard buys is that the failure is pre-mutation and
/// not a panic.
#[test]
fn a_name_registered_by_both_paths_with_different_outputs_is_reported_not_silently_wrong() {
    const ORDER: RouteKey<u32> = RouteKey::new("/order");

    let built = Built::default();
    let handle = NavigatorHandle::new();
    handle.route_keyed(ORDER, |_request: &RouteRequest<'_>| {
        Some(SimpleRoute::<u32>::new(leaf))
    });
    // The collision: the same name, re-registered through the untyped path with
    // a different `Output`. This compiles — nothing connects the two sites.
    handle.route("/order", |_request: &RouteRequest<'_>| {
        Some(SimpleRoute::<String>::new(leaf))
    });
    handle.seed_initial(page(&built, "/"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
    let before = handle.route_ids();

    match handle.push_keyed(ORDER) {
        Err(NamedRouteError::ResultType {
            name,
            expected,
            actual,
        }) => {
            assert_eq!(name, "/order");
            assert!(
                expected.contains("u32"),
                "`expected` names the key's promise, got {expected}"
            );
            assert!(
                actual.contains("String"),
                "`actual` names what the colliding registration really delivers, got {actual}"
            );
        }
        other => panic!("expected the collision to be reported, got {other:?}"),
    }
    laid.tick();

    assert_eq!(
        handle.route_ids(),
        before,
        "and nothing was pushed on the way to finding out"
    );
}

/// Two navigators resolve the same name through their own registries. There is
/// no process-global route table.
///
/// Red-check: move `RouteRegistry` into a `thread_local!`/`static` and the two
/// results collapse onto one.
#[test]
fn two_handles_resolve_the_same_name_through_their_own_registries() {
    let built = Built::default();

    let first = NavigatorHandle::new();
    first.route("/shared", |_request: &RouteRequest<'_>| {
        Some(SimpleRoute::<i32>::new(leaf).with_current_result(1))
    });
    first.route("/only-here", |_request: &RouteRequest<'_>| {
        Some(SimpleRoute::<i32>::new(leaf))
    });
    first.seed_initial(page(&built, "/"));
    let mut first_laid = lay_out(Navigator::new(first.clone()), loose(400.0));

    let second = NavigatorHandle::new();
    second.route("/shared", |_request: &RouteRequest<'_>| {
        Some(SimpleRoute::<i32>::new(leaf).with_current_result(2))
    });
    second.seed_initial(page(&built, "/"));
    let mut second_laid = lay_out(Navigator::new(second.clone()), loose(400.0));

    let from_first = first
        .push_named_typed::<i32>("/shared")
        .expect("registered");
    let from_second = second
        .push_named_typed::<i32>("/shared")
        .expect("registered");
    first_laid.tick();
    second_laid.tick();
    assert!(first.pop());
    assert!(second.pop());
    first_laid.tick();
    second_laid.tick();

    assert_eq!(from_first.try_take(), Some(Some(1)));
    assert_eq!(from_second.try_take(), Some(Some(2)));
    assert_eq!(
        second
            .push_named("/only-here")
            .expect_err("not registered here"),
        NamedRouteError::Unresolved {
            name: "/only-here".to_owned()
        },
        "a name only the first navigator knows is unresolvable on the second"
    );
}

/// A named push is the *same* push, driving the route the registry named.
///
/// The control arm is a **direct** `handle.push` of a route built by hand, not
/// a second call through the named path — comparing `push_named` to `push_named`
/// would have compared `push` to `push` and stayed green even for a resolver
/// that returned an arbitrary wrong route. So the observer streams are compared
/// *and* the pushed route's identity is checked against the id `push_named`
/// reported and the content the registry's own builder recorded.
///
/// Red-check: give the named path its own history mutation instead of routing
/// through `push`, and the streams diverge; or register a different route under
/// the name, and the identity assertions fail while the streams still match.
#[test]
fn push_named_drives_the_registered_route_through_the_same_push_as_an_unnamed_one() {
    /// Mount a navigator seeded with `/`, run `navigate`, and report what the
    /// observer heard alongside the id now on top.
    fn observed(
        built: &Built,
        navigate: impl FnOnce(&NavigatorHandle, &Built) -> Option<RouteId>,
    ) -> (Vec<&'static str>, Option<RouteId>, Option<RouteId>) {
        let handle = NavigatorHandle::new();
        handle.seed_initial(page(built, "/"));
        let spy = Arc::new(Spy::default());
        handle.add_observer(Arc::clone(&spy) as Arc<dyn NavigatorObserver>);
        let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
        let reported = navigate(&handle, built);
        laid.tick();
        (spy.kinds(), reported, handle.current())
    }

    let hand_built = Built::default();
    let (unnamed_stream, _, _) = observed(&hand_built, |handle, built| {
        let _pushed = handle.push(page(built, "target"));
        None
    });

    let via_registry = Built::default();
    let (named_stream, reported, on_top) = observed(&via_registry, |handle, built| {
        handle.route("/target", {
            let built = built.clone();
            move |_request: &RouteRequest<'_>| Some(page(&built, "from-registry"))
        });
        Some(
            handle
                .push_named("/target")
                .expect("'/target' is registered"),
        )
    });

    assert!(
        unnamed_stream.contains(&"push"),
        "the direct-push baseline observed a push at all, got {unnamed_stream:?}"
    );
    assert_eq!(
        named_stream, unnamed_stream,
        "the named push observes exactly what a direct push does"
    );
    assert!(
        reported.is_some() && reported == on_top,
        "push_named reported the id of the route it actually left on top: \
         reported {reported:?}, on top {on_top:?}"
    );
    assert!(
        via_registry.contains("from-registry"),
        "and the route that built is the registry's, not some other one: {:?}",
        via_registry.names()
    );
}
/// A keyed table entry that **declines** falls through to the erased generator,
/// which can answer with a differently-typed route — the third collision shape,
/// and the only one the registration-time warning cannot catch.
///
/// This is why `GeneratedRoute::checked`'s runtime comparison survives Decision
/// B: `on_generate_route` and `on_unknown_route` are erased, so nothing at their
/// registration site knows what `Output` they will produce for a given name.
/// The push is the first moment the answer exists.
///
/// Red-check: drop the `TypeId` comparison in `GeneratedRoute::checked` and this
/// panics inside `TypedPush::push` after the push has landed.
#[test]
fn a_keyed_entry_that_declines_falls_through_to_a_generator_whose_type_is_still_checked() {
    const ORDER: RouteKey<u32> = RouteKey::new("/order");

    let built = Built::default();
    let handle = NavigatorHandle::new();
    // Registered with the right type — the warning has nothing to report — but
    // it declines at resolve time.
    handle.route_keyed(
        ORDER,
        |_request: &RouteRequest<'_>| None::<SimpleRoute<u32>>,
    );
    // And the erased generator answers with something else entirely.
    handle.on_generate_route(|_request: &RouteRequest<'_>| {
        Some(GeneratedRoute::new(SimpleRoute::<String>::new(leaf)))
    });
    handle.seed_initial(page(&built, "/"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
    let before = handle.route_ids();

    match handle.push_keyed(ORDER) {
        Err(NamedRouteError::ResultType {
            name,
            expected,
            actual,
        }) => {
            assert_eq!(name, "/order");
            assert!(expected.contains("u32"), "got {expected}");
            assert!(actual.contains("String"), "got {actual}");
        }
        other => panic!("expected the fall-through collision to be reported, got {other:?}"),
    }
    laid.tick();

    assert_eq!(handle.route_ids(), before, "and nothing was pushed");
}

/// Two `RouteKey`s sharing a name is a **keyed-only** collision, and it is
/// reported rather than silently wrong.
///
/// This is the case that falsifies "a caller using only the keyed path cannot
/// produce `ResultType`". Both registrations compile — each `route_keyed` call
/// is internally consistent, and nothing connects two `RouteKey` constants that
/// happen to spell the same string. The second replaces the first in the
/// name-keyed table, so the key's compile-time promise stops describing what is
/// registered even though no untyped registration was involved.
///
/// Red-check: drop the `TypeId` comparison in `GeneratedRoute::checked` and this
/// **panics** inside `TypedPush::push` — see the sibling collision test for what
/// the guard actually buys.
#[test]
fn two_route_keys_sharing_a_name_collide_even_though_both_registrations_compile() {
    const AS_NUMBER: RouteKey<u32> = RouteKey::new("/order");
    const AS_TEXT: RouteKey<String> = RouteKey::new("/order");

    let built = Built::default();
    let handle = NavigatorHandle::new();
    handle.route_keyed(AS_NUMBER, |_request: &RouteRequest<'_>| {
        Some(SimpleRoute::<u32>::new(leaf))
    });
    handle.route_keyed(AS_TEXT, |_request: &RouteRequest<'_>| {
        Some(SimpleRoute::<String>::new(leaf))
    });
    handle.seed_initial(page(&built, "/"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
    let before = handle.route_ids();

    match handle.push_keyed(AS_NUMBER) {
        Err(NamedRouteError::ResultType {
            name,
            expected,
            actual,
        }) => {
            assert_eq!(name, "/order");
            assert!(expected.contains("u32"), "got {expected}");
            assert!(actual.contains("String"), "got {actual}");
        }
        other => panic!("expected the keyed-vs-keyed collision to be reported, got {other:?}"),
    }
    laid.tick();

    assert_eq!(
        handle.route_ids(),
        before,
        "and nothing was pushed on the way to finding out"
    );
}

// ----------------------------------------------------------------------------
// Re-entrant factories: an operation acts on the route that was current when it
// was CALLED, not on whatever a factory left on top.
// ----------------------------------------------------------------------------

/// Mounts a navigator with `/nested` registered, and `/next` registered to a
/// factory that pushes `/nested` **during resolution** before answering.
///
/// This is the collision between two things this slice added: resolve-before-pop
/// (so the factory runs before the departing route is dealt with) and
/// `RouteRequest::navigator`, which makes navigating from inside a factory a
/// supported shape rather than an obscure one.
fn navigator_with_a_re_entrant_factory() -> (NavigatorHandle, Built, LaidOut, NestedId) {
    let built = Built::default();
    let nested: NestedId = Arc::new(Mutex::new(None));
    let handle = NavigatorHandle::new();
    handle.route("/nested", {
        let built = built.clone();
        move |_request: &RouteRequest<'_>| Some(page(&built, "nested"))
    });
    // The factory captures a handle, which is the only way left to navigate from
    // one now that `RouteRequest::navigator` is withdrawn — and is unsupported
    // rather than impossible, which is precisely the claim these tests pin.
    let cell: Rc<RefCell<Option<NavigatorHandle>>> = Rc::new(RefCell::new(None));
    handle.route("/next", {
        let built = built.clone();
        let nested = Arc::clone(&nested);
        let cell = Rc::clone(&cell);
        move |_request: &RouteRequest<'_>| {
            *nested.lock() = Some(
                cell.borrow()
                    .clone()
                    .expect("the fixture filled the cell")
                    .push_named("/nested")
                    .expect("'/nested' is registered"),
            );
            Some(page(&built, "next"))
        }
    });
    *cell.borrow_mut() = Some(handle.clone());
    handle.seed_initial(page(&built, "/"));
    let laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
    (handle, built, laid, nested)
}

/// The id of the route a re-entrant factory pushed, recorded as it happens.
type NestedId = Arc<Mutex<Option<RouteId>>>;

/// `pop_and_push_named` pops the route that was current **when it was called**,
/// not whatever a re-entrant factory left on top.
///
/// Resolving before popping is what makes an unresolvable name atomic, and it
/// stays. What must not follow from it is acting on a stale notion of "the
/// top": a factory that navigates during resolution changes the top between the
/// resolve and the pop, and the caller's intent is fixed at the call.
///
/// Red-check: read `current()` *after* resolving instead of before, and the
/// nested route is popped while the caller's route survives — the stack ends
/// `[root, departing, arrived]` instead of `[root, arrived]`.
#[test]
fn pop_and_push_named_pops_the_route_that_was_current_when_it_was_called() {
    let (handle, built, mut laid, nested) = navigator_with_a_re_entrant_factory();
    let root = handle.current().expect("seeded");
    let departing = handle.push(page(&built, "departing"));
    laid.tick();
    let departing_id = handle.current().expect("departing is on top");

    let arrived = handle.pop_and_push_named("/next").expect("registered");
    laid.tick();
    let nested_id = nested.lock().expect("the factory navigated");

    assert!(
        !handle.route_ids().contains(&departing_id),
        "the route that was current at the call is gone; stack is {:?}",
        handle.route_ids()
    );
    assert!(
        departing.is_completed(),
        "and it completed, so its awaiter is not left hanging"
    );
    assert_eq!(
        handle.route_ids(),
        vec![root, nested_id, arrived],
        "the factory's own navigation SURVIVES: it pushed that route deliberately, \
         and an operation acting on the route the CALLER named has no business \
         undoing it"
    );
}

/// The `_with` variant delivers its result to the route that was current when it
/// was called.
///
/// The sharpest form of the same defect: the caller's value reaching a route it
/// has never heard of is a silent misdelivery, not a stack-shape surprise.
///
/// Red-check: as above — resolve first and pop the *current* top, and `99` is
/// delivered to the factory's nested route while the caller's route completes
/// with nothing.
#[test]
fn pop_and_push_named_with_delivers_its_result_to_the_route_that_was_current() {
    let (handle, built, mut laid, _nested) = navigator_with_a_re_entrant_factory();
    let departing = handle.push(page(&built, "departing"));
    laid.tick();

    handle
        .pop_and_push_named_with("/next", 99_i32)
        .expect("registered");
    laid.tick();

    assert_eq!(
        departing.try_take(),
        Some(Some(99)),
        "the result reached the route the caller meant to dismiss"
    );
}

/// `push_replacement_named` replaces the route that was current when it was
/// called.
///
/// **Better than the reference here, deliberately.** Flutter's
/// `pushReplacementNamed` evaluates `_routeNamed(..)` in argument position, so
/// its generator also runs before `pushReplacement`, and a Dart factory reaching
/// `Navigator.of(context)` can move the top exactly the same way. Flutter has
/// this defect; after capturing the target first, FLUI does not.
///
/// Red-check: replace whatever is on top after resolving and the nested route is
/// replaced while the caller's route survives.
#[test]
fn push_replacement_named_replaces_the_route_that_was_current_when_it_was_called() {
    let (handle, built, mut laid, nested) = navigator_with_a_re_entrant_factory();
    let root = handle.current().expect("seeded");
    let replaced = handle.push(page(&built, "replaced"));
    laid.tick();
    let replaced_id = handle.current().expect("on top");

    let arrived = handle.push_replacement_named("/next").expect("registered");
    laid.tick();
    let nested_id = nested.lock().expect("the factory navigated");

    assert!(
        !handle.route_ids().contains(&replaced_id),
        "the route that was current at the call was replaced; stack is {:?}",
        handle.route_ids()
    );
    assert!(replaced.is_completed(), "and it completed");
    assert_eq!(
        handle.route_ids(),
        vec![root, nested_id, arrived],
        "the factory's own route survives, as in the pop-and-push case"
    );
}

/// A factory that **pops** during resolution leaves the captured route already
/// gone. The push still happens and the removal is a no-op.
///
/// A caller asking to dismiss something that no longer exists gets the route it
/// asked for and no spurious failure — the alternative, an error, would make a
/// factory's unrelated navigation able to fail an operation that otherwise
/// succeeded.
///
/// Red-check: make the removal an error when the captured route is absent, and
/// this returns `Err` instead of the new route.
#[test]
fn a_factory_that_pops_during_resolution_leaves_the_removal_a_no_op() {
    let built = Built::default();
    let handle = NavigatorHandle::new();
    let cell: Rc<RefCell<Option<NavigatorHandle>>> = Rc::new(RefCell::new(None));
    handle.route("/next", {
        let built = built.clone();
        let cell = Rc::clone(&cell);
        move |_request: &RouteRequest<'_>| {
            // Dismiss the route the outer operation was about to act on, through
            // a captured handle — unsupported, and survivable.
            cell.borrow().clone().expect("filled").pop();
            Some(page(&built, "next"))
        }
    });
    *cell.borrow_mut() = Some(handle.clone());
    handle.seed_initial(page(&built, "/"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
    let root = handle.current().expect("seeded");
    let departing = handle.push(page(&built, "departing"));
    laid.tick();

    let arrived = handle
        .pop_and_push_named("/next")
        .expect("an already-dismissed route is not a failure");
    laid.tick();

    assert!(
        departing.is_completed(),
        "the factory's own pop completed it"
    );
    assert_eq!(
        handle.route_ids(),
        vec![root, arrived],
        "and the new route landed exactly once"
    );
}

/// The observer ordering for a re-entrant factory, pinned rather than assumed.
///
/// This is the half of CodeRabbit's finding that survives the fix: because the
/// factory runs before the departing route is dealt with, the nested `didPush`
/// is observed **before** the outer `didPop`. Flutter's `popAndPushNamed` pops
/// first and cannot produce that order, so `ARCHITECTURE.md` §5's "identical
/// observer stream" holds only when no factory navigates.
///
/// Red-check: revert to pop-then-resolve and the sequence starts with `pop`.
#[test]
fn a_re_entrant_factory_is_observed_before_the_pop_it_precedes() {
    let (handle, built, mut laid, _nested) = navigator_with_a_re_entrant_factory();
    handle.push(page(&built, "departing"));
    laid.tick();

    let spy = Arc::new(Spy::default());
    handle.add_observer(Arc::clone(&spy) as Arc<dyn NavigatorObserver>);
    handle.pop_and_push_named("/next").expect("registered");
    laid.tick();

    assert_eq!(
        spy.kinds(),
        vec![
            "push",
            "changeTop", // the factory's nested route, during resolution
            "remove",    // then the departing route the CALLER named — see below
            "push",
            "changeTop", // then the route the factory produced
        ],
        "two facts, both deliberate. Resolve-before-pop puts the factory's own \
         navigation ahead of the departing route, which Flutter's pop-first \
         `popAndPushNamed` cannot produce. And the departing route is now BURIED \
         under what the factory pushed, so `didRemove` is the honest event: a \
         route that is not on top cannot be popped. The ordinary, non-re-entrant \
         call is unaffected and still observes `didPop` — pinned by \
         `pop_and_push_named_observes_a_pop_where_push_replacement_named_does_not`"
    );
}

/// Records `didReplace` with its full payload, which [`Spy`] flattens to a kind.
#[derive(Default)]
struct ReplaceSpy(Mutex<Vec<(Option<RouteId>, Option<RouteId>)>>);

impl NavigatorObserver for ReplaceSpy {
    fn did_replace(&self, new_route: Option<RouteId>, old_route: Option<RouteId>) {
        self.0.lock().push((new_route, old_route));
    }
}

/// A re-entrant `push_replacement_named` reports the route it **actually
/// replaced**, not whatever ended up one slot below.
///
/// Completing by id and observing by position are two sources of truth for "which
/// route was replaced", and they disagree exactly when a factory navigates: the
/// captured route is completed but never reported, while the factory's route —
/// still very much on the stack — is named as replaced. An observer acting on
/// `didReplace(new, old)` would tear down a live route.
///
/// Red-check: derive the reported id from `previous_present` (the nearest present
/// entry below the new route) instead of from the id the completion used, and
/// this reports the factory's `/nested` route.
#[test]
fn a_re_entrant_replacement_reports_the_route_it_actually_replaced() {
    let (handle, built, mut laid, nested) = navigator_with_a_re_entrant_factory();
    let replaced_route = handle.push(page(&built, "replaced"));
    laid.tick();
    let replaced_id = handle.current().expect("on top");

    let spy = Arc::new(ReplaceSpy::default());
    handle.add_observer(Arc::clone(&spy) as Arc<dyn NavigatorObserver>);

    let arrived = handle.push_replacement_named("/next").expect("registered");
    laid.tick();
    let nested_id = nested.lock().expect("the factory navigated");

    assert_eq!(
        spy.0.lock().clone(),
        vec![(Some(arrived), Some(replaced_id))],
        "didReplace names the captured route — the one that was completed — and \
         not the factory's route, which is still on the stack"
    );
    assert!(
        replaced_route.is_completed(),
        "precondition: the captured route really was completed as replaced"
    );
    assert!(
        handle.route_ids().contains(&nested_id),
        "precondition: the factory's route really is still present, so naming it \
         as replaced would be reporting a live route as gone"
    );
}

/// The full observer stream for a re-entrant `push_replacement_named`, pinned
/// beside its `pop_and_push_named` counterpart.
///
/// `ARCHITECTURE.md` §5 documents the re-entrant ordering; this is the
/// replacement half of it. Note there is no `pop` and no `remove`: a replacement
/// completes its target *as replaced*, which is what `didReplace` reports and
/// why it emits no removal — the contrast
/// `pop_and_push_named_observes_a_pop_where_push_replacement_named_does_not`
/// draws for the non-re-entrant case holds here too.
///
/// Red-check: give the replacement path a positional target and the stream is
/// unchanged — which is the point: kinds alone cannot see this defect, only the
/// payload can. That is why the sibling test above asserts identity.
#[test]
fn a_re_entrant_replacements_observer_stream_is_pinned() {
    let (handle, built, mut laid, _nested) = navigator_with_a_re_entrant_factory();
    handle.push(page(&built, "replaced"));
    laid.tick();

    let spy = Arc::new(Spy::default());
    handle.add_observer(Arc::clone(&spy) as Arc<dyn NavigatorObserver>);
    handle.push_replacement_named("/next").expect("registered");
    laid.tick();

    assert_eq!(
        spy.kinds(),
        vec![
            "push",
            "changeTop", // the factory's nested route, during resolution
            "changeTop", // then the replacement — didReplace, which Spy does not record
        ],
        "the factory's push precedes the replacement it triggered"
    );
}

// ----------------------------------------------------------------------------
// A route whose pop does NOT finalise it — the state every production
// PageRoute/PopupRoute occupies for the length of its exit transition.
// ----------------------------------------------------------------------------

/// A route that answers `finished_when_popped() == false`, like every
/// `TransitionRoute` descendant.
///
/// Every other named-route fixture in this file is a `SimpleRoute`, which takes
/// the `Route` default `true` and is finalised inside the same flush as its pop.
/// That made the `Popping`/`Removing` window — where a real `PageRoute` lives for
/// the whole of its exit transition — unreachable from this suite, and it is
/// where three of the defects this PR fixed were hiding.
///
/// It carries no `RouteBindingSlot`, so nothing finalises it: once popped it
/// stays in `Popping` indefinitely. That is deliberate — it is the *widest*
/// version of the window, so anything that reads the stack while a pop is in
/// flight has to be correct against it.
struct DeferredExitRoute {
    settings: RouteSettings,
    builder: RouteContentBuilder,
    current_result: i32,
}

impl DeferredExitRoute {
    fn new(name: &'static str, current_result: i32) -> Self {
        Self {
            settings: RouteSettings::named(name),
            builder: Rc::new(leaf),
            current_result,
        }
    }
}

impl Route for DeferredExitRoute {
    type Output = i32;

    fn settings(&self) -> &RouteSettings {
        &self.settings
    }

    fn current_result(&mut self) -> Option<i32> {
        Some(self.current_result)
    }

    /// The whole point of this fixture.
    fn finished_when_popped(&self) -> bool {
        false
    }
}

impl NavigatorRoute for DeferredExitRoute {
    fn content_builder(&self) -> RouteContentBuilder {
        Rc::clone(&self.builder)
    }
}

/// Mount a navigator whose named routes all defer their exit.
fn navigator_with_deferred_exits() -> (NavigatorHandle, LaidOut) {
    let handle = NavigatorHandle::new();
    handle.route("/next", |_request: &RouteRequest<'_>| {
        Some(DeferredExitRoute::new("/next", 1))
    });
    handle.seed_initial(DeferredExitRoute::new("/", 0));
    let laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
    (handle, laid)
}

/// `pop_and_push_named` against a route whose exit is still in flight.
///
/// The contract this pins is **not** the one every other named-route test in
/// this file pins, and the difference is the point. `route_ids()` returns every
/// entry, including one whose exit transition has not finished; `current()`
/// returns the topmost *present* one. With a `SimpleRoute` those two agree,
/// because the pop finalises inside the same flush and the entry is gone before
/// anyone looks. With a deferred exit they diverge for the whole transition — so
/// asserting `route_ids()` equality, as the rest of the suite does, is asserting
/// a fact about the fixture as much as about the operation.
#[test]
fn deferred_exit_pop_and_push_named() {
    let (handle, mut laid) = navigator_with_deferred_exits();
    let root = handle.current().expect("seeded");
    let departing = handle.push(DeferredExitRoute::new("departing", 7));
    laid.tick();
    let departing_id = handle.current().expect("on top");

    let arrived = handle.pop_and_push_named("/next").expect("registered");
    laid.tick();

    assert_eq!(
        handle.current(),
        Some(arrived),
        "the new route is what a caller sees on top"
    );
    assert_eq!(
        departing.try_take(),
        Some(Some(7)),
        "the departing route completed with its own fallback, immediately — \
         completion does not wait for the exit transition"
    );
    assert!(
        handle.route_ids().contains(&departing_id),
        "and its entry is still in the stack, because its exit has not finished: \
         {:?}",
        handle.route_ids()
    );
    assert_eq!(
        handle.route_ids(),
        vec![root, departing_id, arrived],
        "in that order — the unfinalised route sits below the one that replaced it"
    );
}

/// `push_replacement_named` against a route whose exit is still in flight.
#[test]
fn deferred_exit_push_replacement_named() {
    let (handle, mut laid) = navigator_with_deferred_exits();
    let replaced = handle.push(DeferredExitRoute::new("replaced", 7));
    laid.tick();

    let arrived = handle.push_replacement_named("/next").expect("registered");
    laid.tick();

    assert_eq!(handle.current(), Some(arrived));
    assert!(
        replaced.is_completed(),
        "a replacement completes its target at once, exit transition or not"
    );
}

/// Two named operations back to back, the first still unfinalised when the
/// second runs — the sequence a user tapping twice produces, and the one that
/// cannot happen at all with a synchronously-finalised fixture.
#[test]
fn deferred_exit_two_named_operations_back_to_back() {
    let (handle, mut laid) = navigator_with_deferred_exits();
    let root = handle.current().expect("seeded");
    handle.push(DeferredExitRoute::new("first", 7));
    laid.tick();

    let second = handle.pop_and_push_named("/next").expect("registered");
    laid.tick();
    let third = handle.pop_and_push_named("/next").expect("registered");
    laid.tick();

    assert_ne!(second, third);
    assert_eq!(
        handle.current(),
        Some(third),
        "the second operation acted on what the first left present, not on a \
         still-unfinalised entry"
    );
    assert!(
        !handle.route_ids().is_empty() && handle.route_ids()[0] == root,
        "and the root is untouched underneath"
    );
}

/// `push_named_and_remove_until` sweeping past unfinalised routes.
#[test]
fn deferred_exit_push_named_and_remove_until() {
    let (handle, mut laid) = navigator_with_deferred_exits();
    let root = handle.current().expect("seeded");
    handle.push(DeferredExitRoute::new("a", 7));
    laid.tick();
    handle.push(DeferredExitRoute::new("b", 8));
    laid.tick();
    handle.pop();
    laid.tick();

    let arrived = handle
        .push_named_and_remove_until("/next", |candidate| candidate == root)
        .expect("registered");
    laid.tick();

    assert_eq!(handle.current(), Some(arrived));
    assert_eq!(
        handle
            .route_ids()
            .into_iter()
            .filter(|id| *id == root)
            .count(),
        1,
        "the kept route survived exactly once"
    );
}

/// The combination the fixture exists for: a re-entrant factory **pops** the
/// captured route, so by the time the replacement runs its target is already in
/// the un-finalised `Popping` window — and `push_replacement_with_id` looks its
/// target up by id across *all* entries, present or not.
///
/// Without `arm_complete`'s `>= remove` guard this would complete a route that
/// has already completed. That guard is what makes the by-id lookup safe, and
/// nothing pinned the combination before this fixture existed.
#[test]
fn deferred_exit_a_factory_that_pops_the_captured_route_before_it_is_replaced() {
    let cell: Rc<RefCell<Option<NavigatorHandle>>> = Rc::new(RefCell::new(None));
    let handle = NavigatorHandle::new();
    handle.route("/next", {
        let cell = Rc::clone(&cell);
        move |_request: &RouteRequest<'_>| {
            cell.borrow()
                .clone()
                .expect("the test filled the cell")
                .pop();
            Some(DeferredExitRoute::new("/next", 1))
        }
    });
    *cell.borrow_mut() = Some(handle.clone());
    handle.seed_initial(DeferredExitRoute::new("/", 0));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
    let root = handle.current().expect("seeded");
    let victim = handle.push(DeferredExitRoute::new("victim", 7));
    laid.tick();

    let arrived = handle.push_replacement_named("/next").expect("registered");
    laid.tick();

    assert_eq!(
        victim.try_take(),
        Some(Some(7)),
        "completed exactly once, by the factory's own pop — the replacement must \
         not complete it a second time"
    );
    assert_eq!(handle.current(), Some(arrived));
    assert!(handle.route_ids().contains(&root));

    *cell.borrow_mut() = None;
}

/// A named replacement whose capture came back empty must complete **nothing** —
/// not "whatever is on top by the time we look".
///
/// `Option<RouteId>`'s `None` meant two different things: "replace the current
/// top" for the unnamed front doors, and "there was nothing to replace" for a
/// named capture that came back empty. Both landed on `last_present_index()`.
///
/// Reachable without an empty stack: a route mid-exit-transition is not
/// `is_present`, so `current()` answers `None` while the stack is still visibly
/// occupied. If a factory then pushes, the caller's result is delivered to *the
/// factory's own route* — a route the caller has never heard of, which was not
/// replacing anything.
///
/// Red-check: collapse the target back to `Option<RouteId>` with
/// `None => last_present_index()` and the factory's route receives `99`.
#[test]
fn a_named_replacement_with_no_captured_target_completes_nothing() {
    let nested: NestedId = Arc::new(Mutex::new(None));
    let cell: Rc<RefCell<Option<NavigatorHandle>>> = Rc::new(RefCell::new(None));

    let handle = NavigatorHandle::new();
    handle.route("/nested", |_request: &RouteRequest<'_>| {
        Some(DeferredExitRoute::new("/nested", 11))
    });
    handle.route("/next", {
        let nested = Arc::clone(&nested);
        let cell = Rc::clone(&cell);
        move |_request: &RouteRequest<'_>| {
            *nested.lock() = Some(
                cell.borrow()
                    .clone()
                    .expect("filled")
                    .push_named("/nested")
                    .expect("registered"),
            );
            Some(DeferredExitRoute::new("/next", 1))
        }
    });
    *cell.borrow_mut() = Some(handle.clone());
    handle.seed_initial(DeferredExitRoute::new("/", 0));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));

    // Pop the only route. Its exit is deferred, so the entry stays but is no
    // longer `is_present` — `current()` is now `None` over a non-empty stack.
    assert!(handle.pop());
    laid.tick();
    assert_eq!(
        handle.current(),
        None,
        "precondition: nothing is present, though the stack is not empty"
    );
    assert!(
        !handle.route_ids().is_empty(),
        "precondition: the mid-transition entry is still there"
    );

    let arrived = handle
        .push_replacement_named_with("/next", 99_i32)
        .expect("registered");
    laid.tick();
    let nested_id = nested.lock().expect("the factory navigated");

    assert_ne!(
        nested_id, arrived,
        "precondition: the factory's route and the pushed route are distinct"
    );
    assert!(
        handle.route_ids().contains(&nested_id),
        "the factory's route was not completed as replaced — it is still present, \
         because it was never anybody's replacement target"
    );
    assert_eq!(
        handle.current(),
        Some(arrived),
        "and the named route landed on top regardless"
    );
}

/// A captured target that is mid-exit-transition is **not** a valid replacement
/// target, is not reported as replaced, and the caller's result is **logged**
/// rather than silently dropped.
///
/// One test for three linked fixes, because they have one observable
/// consequence. The target is in `Popping`: it has already completed and is only
/// awaiting finalisation, so `is_present()` excludes it — both target arms now
/// agree on that, where `Route(id)` previously used an unfiltered `position()`
/// and `CurrentTop` filtered. Nothing is completed, so `replacing` is `None` and
/// no `didReplace` names it. And the `99` the caller supplied reaches no route,
/// which now says so.
///
/// **Red-check, and the honest version is narrower than it looks.** Only one of
/// the three mutations reds this test: dropping the undelivered result instead of
/// recording it empties the captured log. The other two do **not**, and the
/// reason is worth knowing —
///
/// - giving the `Route(id)` arm back its unfiltered `position()` still passes,
///   because `arm_complete` then refuses the `Popping` entry itself
///   (`Popping >= Remove`) and reports `armed: false`;
/// - deriving `replacing` from the lookup instead of from the arming still
///   passes, because the presence filter already made `target_index` `None`, so
///   the mutated line is unreachable.
///
/// The two guards cover each other. They differ on exactly one state — `Remove`,
/// which passes `is_present()` (`Add..=Remove`) and is refused by `arm_complete`
/// (`>= Remove`) — and `Remove` is transient *within* a flush and never
/// observable between operations, so no public-API scenario separates them. Both
/// are kept as defence in depth for one invariant, not as two independent
/// checks, and this comment exists so nobody deletes one on the strength of a
/// green suite.
#[test]
fn a_target_mid_exit_transition_is_not_replaced_and_its_result_is_reported() {
    let ((), log) = flui_testing::log_capture::capture(|| {
        let cell: Rc<RefCell<Option<NavigatorHandle>>> = Rc::new(RefCell::new(None));
        let handle = NavigatorHandle::new();
        handle.route("/next", {
            let cell = Rc::clone(&cell);
            move |_request: &RouteRequest<'_>| {
                // Pop the route the outer replacement captured. Its exit is
                // deferred, so it lands in `Popping` — present no longer, gone
                // not yet.
                cell.borrow().clone().expect("filled").pop();
                Some(DeferredExitRoute::new("/next", 1))
            }
        });
        *cell.borrow_mut() = Some(handle.clone());
        handle.seed_initial(DeferredExitRoute::new("/", 0));
        let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
        let victim = handle.push(DeferredExitRoute::new("victim", 7));
        laid.tick();
        let victim_id = handle.current().expect("on top");

        let spy = Arc::new(ReplaceSpy::default());
        handle.add_observer(Arc::clone(&spy) as Arc<dyn NavigatorObserver>);

        let arrived = handle
            .push_replacement_named_with("/next", 99_i32)
            .expect("registered");
        laid.tick();

        assert_eq!(
            victim.try_take(),
            Some(Some(7)),
            "the victim completed once, from the factory's own pop, with its own \
             fallback — not with the caller's 99"
        );
        assert_eq!(
            spy.0.lock().clone(),
            vec![(Some(arrived), None)],
            "the replacement reports replacing nothing, because nothing was armed \
             — naming the mid-transition route would report a second replacement \
             of a route this operation never touched"
        );
        assert!(
            !handle.route_ids().contains(&victim_id) || handle.current() == Some(arrived),
            "and the new route is on top regardless"
        );
        *cell.borrow_mut() = None;
    });

    assert_eq!(
        log.count_containing("reached no route and was discarded"),
        1,
        "the caller's 99 was reported, not silently swallowed; captured:\n{}",
        log.render_at_least(tracing::Level::WARN)
    );
}

/// A mismatched pop result is reported and dropped **outside** the history lock.
///
/// `RouteRecord::did_complete` runs inside the flush, under the history mutex. It
/// used to emit `tracing::error!` and drop the mismatched box right there — and
/// both are user code: a subscriber is user-written, and the box wraps a value the
/// caller supplied, so its `Drop` is theirs. Either can call back into the
/// navigator, and the mutex is not reentrant.
///
/// # Reaching it, given `pop_with`'s `Send` bound
///
/// The payload cannot simply capture a `NavigatorHandle` — `pop_with<T: Send>`
/// forbids it, since the handle is deliberately `!Send`. It can hold a
/// [`NavigatorCommandTarget`], which **is** `Send + Sync` and is this crate's
/// documented cross-thread route to a navigator; `apply_on_owner()` resolves the
/// handle from owner-thread storage and takes the history lock.
///
/// **The detour is what makes this legitimate.** Had the reproduction needed a
/// type the public API forbids, it would have been a contrivance and the defect
/// arguably unreachable. Instead the deadlock is reachable through a *supported
/// API on a drop path* — which is exactly the shape a real caller hits, and the
/// reason the fix is not defensive.
///
/// # Expected failure shape
///
/// Under the defect this **hangs**, it does not assert: `timeout 60` → exit 124,
/// with no output after the start line. The drop counter is therefore asserted
/// inside the capture, as the test proceeds — a final assertion never runs in a
/// deadlock.
///
/// The subscriber half of the same hazard is real but not demonstrated here:
/// `flui_testing::log_capture`'s subscriber is inert, which is exactly why an
/// existing test drives this path and passes. This covers the value's `Drop`.
///
/// Red-check: emit the `tracing::error!` and drop the box inside
/// `RouteRecord::did_complete` again, and this deadlocks the owner thread.
#[test]
fn a_mismatched_pop_result_is_reported_and_dropped_outside_the_history_lock() {
    /// A payload whose `Drop` commands the navigator — a supported API, on the
    /// drop path, from a `Send` value.
    struct CommandsNavigatorOnDrop {
        target: NavigatorCommandTarget,
        drops: Arc<AtomicUsize>,
    }

    impl Drop for CommandsNavigatorOnDrop {
        fn drop(&mut self) {
            // Takes the history lock. Deadlocks if this drop happens under it.
            let _ = NavigatorCommand::maybe_pop(self.target).apply_on_owner();
            self.drops.fetch_add(1, Ordering::Relaxed);
        }
    }

    let built = Built::default();
    let drops = Arc::new(AtomicUsize::new(0));
    let handle = NavigatorHandle::new();
    handle.seed_initial(page(&built, "/"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));

    // `page` is a `SimpleRoute<i32>`, so this payload is the wrong type for it and
    // takes the mismatch path.
    let route = handle.push(page(&built, "target"));
    laid.tick();

    let ((), log) = flui_testing::log_capture::capture(|| {
        assert!(handle.pop_with(CommandsNavigatorOnDrop {
            target: handle.command_target(),
            drops: Arc::clone(&drops),
        }));
        // Reached only if the drop did not deadlock.
        assert_eq!(
            drops.load(Ordering::Relaxed),
            1,
            "the mismatched payload was dropped, and its Drop reached the navigator \
             without deadlocking"
        );
    });
    laid.tick();

    assert_eq!(
        log.count_containing("pop result has the wrong type"),
        1,
        "and the mismatch was reported once, from outside the lock; captured:\n{}",
        log.render_at_least(tracing::Level::WARN)
    );
    assert_eq!(
        route.try_take(),
        Some(None),
        "the route completed with None, as the contract says"
    );
}

/// `KeyedSettings::untyped` lets a key's arguments reach the seven operations
/// `push_keyed` is not.
///
/// The gap it closes was composability, not semantics: dropping `T` on an untyped
/// operation is their documented behaviour, and the same discard was already
/// reachable as `push_replacement_named(KEY.name())` — it simply had no verb, so
/// carrying a key's *arguments* onto an untyped operation had no spelling at all.
///
/// Red-check: make `untyped()` return `RouteSettings::named(name)` without the
/// arguments and the factory sees `None` instead of `1776`.
#[test]
fn keyed_settings_untyped_carries_a_keys_arguments_onto_an_untyped_operation() {
    const ORDER: RouteKey<u32> = RouteKey::new("/order");

    let built = Built::default();
    let seen: Arc<Mutex<Vec<Option<u32>>>> = Arc::new(Mutex::new(Vec::new()));

    let handle = NavigatorHandle::new();
    handle.route_keyed(ORDER, {
        let seen = Arc::clone(&seen);
        move |request: &RouteRequest<'_>| {
            seen.lock().push(request.argument::<u32>().copied());
            Some(SimpleRoute::<u32>::new(leaf))
        }
    });
    handle.seed_initial(page(&built, "/"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
    let root = handle.current().expect("seeded");

    // The typed push, for contrast: `T` is kept.
    let typed = handle
        .push_keyed(ORDER.with_arguments(1776_u32))
        .expect("registered");
    laid.tick();
    assert!(!typed.is_completed(), "a live RouteResult<u32>");

    // The same key, the same arguments, on an operation that has no `T` at all.
    let replaced = handle
        .push_replacement_named(ORDER.with_arguments(1776_u32).untyped())
        .expect("registered");
    laid.tick();

    assert_eq!(
        seen.lock().clone(),
        vec![Some(1776), Some(1776)],
        "both routes were built from the key's arguments — the untyped operation \
         carried them just as the typed one did"
    );
    assert_eq!(handle.route_ids(), vec![root, replaced]);

    // And the shared-payload form composes the same way.
    let payload: RouteArguments = Arc::new(99_u32);
    handle
        .pop_and_push_named(ORDER.with_arguments_shared(Arc::clone(&payload)).untyped())
        .expect("registered");
    laid.tick();
    assert_eq!(
        seen.lock().last().copied().flatten(),
        Some(99),
        "including through with_arguments_shared, which preserves payload identity"
    );
}

/// A failing named operation adds **nothing of its own** — but a factory that
/// navigated before declining keeps what it did.
///
/// `ARCHITECTURE.md` §4's totality guarantee is qualified for exactly this shape,
/// and until now the qualifier rested on a measurement with no test behind it. A
/// quoted measurement with nothing pinning it reads as evidence, which is worse
/// than a bare claim.
///
/// The factory captures a handle — the only way left, and unsupported rather than
/// impossible — pushes a route, and *then* answers `None`, so the name is
/// unresolved. Its push is its own and is not rolled back, for the same reason
/// §5's captured-target fix does not undo a factory's nested push: it is not this
/// operation's to undo.
///
/// Red-check: make `resolve_named` roll back on failure and the stack assertion
/// fails; make the failing operation push a placeholder and the "nothing of its
/// own" assertions fail.
#[test]
fn an_unresolvable_name_after_a_navigating_factory_adds_nothing_of_its_own() {
    let built = Built::default();
    let cell: Rc<RefCell<Option<NavigatorHandle>>> = Rc::new(RefCell::new(None));
    let nested: NestedId = Arc::new(Mutex::new(None));

    let handle = NavigatorHandle::new();
    handle.route("/nested", {
        let built = built.clone();
        move |_request: &RouteRequest<'_>| Some(page(&built, "nested"))
    });
    // Navigates, then declines — so nothing answers "/missing".
    handle.on_generate_route({
        let cell = Rc::clone(&cell);
        let nested = Arc::clone(&nested);
        move |_request: &RouteRequest<'_>| {
            *nested.lock() = Some(
                cell.borrow()
                    .clone()
                    .expect("the test filled the cell")
                    .push_named("/nested")
                    .expect("'/nested' is registered"),
            );
            None
        }
    });
    *cell.borrow_mut() = Some(handle.clone());
    handle.seed_initial(page(&built, "/"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
    let root = handle.current().expect("seeded");

    let spy = Arc::new(Spy::default());
    handle.add_observer(Arc::clone(&spy) as Arc<dyn NavigatorObserver>);

    let refused = handle.push_named("/missing");
    laid.tick();
    let nested_id = nested
        .lock()
        .expect("the factory navigated before declining");

    assert_eq!(
        refused.expect_err("the generator declined, so nothing answered the name"),
        NamedRouteError::Unresolved {
            name: "/missing".to_owned()
        }
    );
    assert_eq!(
        handle.route_ids(),
        vec![root, nested_id],
        "one deeper than before — and that route is the FACTORY's, not the failing \
         operation's: it pushed deliberately and is not rolled back"
    );
    assert_eq!(
        spy.kinds(),
        vec!["push", "changeTop"],
        "exactly the factory's own push was observed; the failing operation \
         notified nothing"
    );
    assert_eq!(
        handle.current(),
        Some(nested_id),
        "and the operation left no route of its own on top"
    );

    *cell.borrow_mut() = None;
}

/// Every named and unnamed operation that cannot deliver a caller's result
/// **reports** it — including the five paths that used to drop it silently.
///
/// Four of the five ran **under the history guard**, so the value's `Drop` — user
/// code — could have deadlocked, not merely vanished. And one of them,
/// `maybe_pop`'s `Bubble` arm, is not an edge case at all: `popDisposition` is
/// `isFirst ? bubble : pop`, so a lone route bubbles *by design* and
/// `maybe_pop_with` on a one-route navigator took it every time.
///
/// Asserted on the **captured log**, not a counter: a counter would pin a parallel
/// predicate rather than the emission, which is the trap this module already
/// documents for the registration-conflict warning.
///
/// Red-check: restore any one early return to dropping `result` inline and that
/// row's expected count falls to zero.
#[test]
fn every_operation_that_cannot_deliver_a_result_reports_it() {
    /// Drive one scenario and report how many "no route" warnings it emitted.
    fn warnings_from(drive: impl FnOnce(&NavigatorHandle)) -> usize {
        let ((), log) = flui_testing::log_capture::capture(|| {
            let handle = NavigatorHandle::new();
            handle.route("/next", |_request: &RouteRequest<'_>| {
                Some(DeferredExitRoute::new("/next", 1))
            });
            drive(&handle);
        });
        log.count_containing("reached no route and was discarded")
    }

    // 1. `pop_and_push_named_with` with nothing captured — Codex's report.
    assert_eq!(
        warnings_from(|handle| {
            let _ = handle.pop_and_push_named_with("/next", 7_i32);
        }),
        1,
        "an empty capture reports the result it could not deliver"
    );

    // 2. `maybe_pop_with` while unmounted.
    assert_eq!(
        warnings_from(|handle| {
            assert!(
                handle.maybe_pop_with(7_i32),
                "an unmounted navigator swallows the pop"
            );
        }),
        1,
        "the unmounted early return reports too"
    );

    // 3. `pop_with` on an empty stack — reachable with no factory at all.
    assert_eq!(
        warnings_from(|handle| {
            assert!(!handle.pop_with(7_i32), "nothing to pop");
        }),
        1,
        "the plainest case of all"
    );

    // 4. `remove_route_with` for an id that is not there. The id comes from a
    //    *different* navigator, since `RouteId::next` is crate-private — which
    //    also makes it a genuinely absent id rather than a fabricated one.
    let stranger = NavigatorHandle::new();
    stranger.seed_initial(DeferredExitRoute::new("/elsewhere", 0));
    let absent = stranger.route_ids()[0];
    assert_eq!(
        warnings_from(move |handle| {
            assert!(
                !handle.remove_route_with(absent, 7_i32),
                "that route belongs to another navigator"
            );
        }),
        1,
        "a missing removal target reports too"
    );

    // 5. `maybe_pop_with` on a lone route, which bubbles BY DESIGN — the one that
    //    is not an edge case.
    let bubbled = flui_testing::log_capture::capture(|| {
        let built = Built::default();
        let handle = NavigatorHandle::new();
        handle.seed_initial(page(&built, "/"));
        let _laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
        assert!(
            !handle.maybe_pop_with(7_i32),
            "a lone route bubbles: popDisposition is `isFirst ? bubble : pop`"
        );
    })
    .1;
    assert_eq!(
        bubbled.count_containing("reached no route and was discarded"),
        1,
        "the Bubble arm reports, and it used to drop under the history guard; \
         captured:\n{}",
        bubbled.render_at_least(tracing::Level::WARN)
    );
}
