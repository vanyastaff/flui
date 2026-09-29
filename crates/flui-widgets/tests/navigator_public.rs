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
    GeneratedRoute, NamedRouteError, NavigatorCommand, NavigatorCommandTarget, NavigatorObserver,
    NavigatorRoute, PushCompletion, Route, RouteArguments, RouteContentBuilder, RouteId, RouteKey,
    RouteRequest, RouteSettings,
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

/// A pop result whose `Drop` is observable.
///
/// Several undelivered-result paths run under the history mutex, where dropping
/// a caller's value inline runs user code with a lock held. A counter proves the
/// drop happened once *and* — because the navigator is still usable after it —
/// that it happened outside the guard.
struct DropCounter(Arc<AtomicUsize>);

impl DropCounter {
    fn new(counter: &Arc<AtomicUsize>) -> Self {
        Self(Arc::clone(counter))
    }
}

impl Drop for DropCounter {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

/// The `operation` field of every undeliverable-result report in `log`, in order.
///
/// Discriminating per row on the operation name, rather than counting matches of
/// a message substring: a count cannot tell "the path I meant reported" from
/// "some other path reported instead", which is the difference between an
/// assertion and a coincidence.
fn undelivered_operations(log: &flui_testing::log_capture::CapturedLog) -> Vec<&str> {
    log.records()
        .iter()
        .filter(|record| {
            record.contains("reached no route") || record.contains("could not be delivered")
        })
        .map(|record| record.field("operation").unwrap_or("<no operation field>"))
        .collect()
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
            let _prev = std::mem::replace(&mut *nearest.lock(), NavigatorHandle::maybe_of(ctx));
            let _prev = std::mem::replace(&mut *root.lock(), NavigatorHandle::maybe_of_root(ctx));
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
/// This is the case that kills the erase-the-`Output` design ADR-0024
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
/// `Box<dyn Any + Send>` is not `Clone` (ADR-0024). That was a compile
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
            // machinery as the three shipped classes above (ADR-0024).
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
/// time, with the stack untouched — ADR-0024's "with nothing pushed" — and
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
    // ADR-0024 names as the class a route table exists to serve, and its
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

/// An unresolvable name with no fallback registered errors, changes nothing,
/// and notifies no observer — Flutter's
/// `'Navigator.onGenerateRoute returned null'` assertion, as a `Result`
/// (`PANIC-POLICY`: a route name is caller input, not a framework invariant).
///
/// Red-check: make `resolve_named` fall back to pushing a placeholder route.
/// What a maintainer then sees is `expect_err("no registration answers
/// '/nowhere'")` panicking on an `Ok` — the operation succeeds, so the error
/// assertion is reached first and the stack assertion below it never runs.
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
            let _prev = std::mem::replace(
                &mut *offered.lock(),
                request.settings().arguments().cloned(),
            );
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
    let _prev = cell.borrow_mut().replace(handle.clone());
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

    let _prev = cell.borrow_mut().take();
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
    let _prev = cell.borrow_mut().replace(handle.clone());
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
    let _prev = cell.borrow_mut().replace(handle.clone());
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
/// This is the half of a review finding that survives the fix: because the
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
///
/// Red-check: have `dismiss_captured` resolve the name before capturing
/// `current()` and the departing route is never completed — `try_take()` returns
/// `None` instead of its fallback.
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

/// The combination the fixture exists for: a re-entrant factory **pops** the
/// captured route, so by the time the replacement runs its target is already in
/// the un-finalised `Popping` window — and `push_replacement_with_id` looks its
/// target up by id across *all* entries, present or not.
///
/// Without `arm_complete`'s `>= remove` guard this would complete a route that
/// has already completed. That guard is what makes the by-id lookup safe, and
/// nothing pinned the combination before this fixture existed.
///
/// Red-check: remove `arm_complete`'s `>= Remove` early return and the
/// already-popped target is re-armed, so the replacement reports a `didReplace`
/// for a route that had already left — the observer assertion below fails.
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
    let _prev = cell.borrow_mut().replace(handle.clone());
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

    let _prev = cell.borrow_mut().take();
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
    let _prev = cell.borrow_mut().replace(handle.clone());
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
        let _prev = cell.borrow_mut().replace(handle.clone());
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
        let _prev = cell.borrow_mut().take();
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
/// documents for the registration-conflict warning. And asserted per row on the
/// report's `operation` field rather than on a count of matching messages — a
/// count cannot distinguish "the path I meant reported" from "a different path
/// reported instead", which for the two `maybe_pop` arms below is the whole
/// question.
///
/// One site is deliberately **not** a row here: the `pending_result` displacement
/// in `RouteEntry::arm_pop` / `arm_complete`. Reaching it needs one entry armed
/// twice before its flush, and every public mutation flushes before returning
/// while `debug_assert!(!flushing)` rejects re-entry from inside a flush — so it
/// is a defence with no public caller, not an untested path. If a future
/// operation arms without flushing, that is when it becomes reachable.
///
/// Red-check: restore any one early return to dropping `result` inline and that
/// row's operation list goes empty.
#[test]
fn every_operation_that_cannot_deliver_a_result_reports_it() {
    /// Drive one scenario and report how many "no route" warnings it emitted.
    fn warnings_from(drive: impl FnOnce(&NavigatorHandle)) -> Vec<String> {
        let ((), log) = flui_testing::log_capture::capture(|| {
            let handle = NavigatorHandle::new();
            handle.route("/next", |_request: &RouteRequest<'_>| {
                Some(DeferredExitRoute::new("/next", 1))
            });
            drive(&handle);
        });
        undelivered_operations(&log)
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    // 1. `pop_and_push_named_with` with nothing captured — Codex's report.
    assert_eq!(
        warnings_from(|handle| {
            let _ = handle.pop_and_push_named_with("/next", 7_i32);
        }),
        ["pop_and_push_named_with"],
        "an empty capture reports the result it could not deliver, under the \
         composed operation's name rather than the inner pop's"
    );

    // 2. `maybe_pop_with` while unmounted.
    assert_eq!(
        warnings_from(|handle| {
            assert!(
                handle.maybe_pop_with(7_i32),
                "an unmounted navigator swallows the pop"
            );
        }),
        ["maybe_pop"],
        "the unmounted early return reports too"
    );

    // 3. `pop_with` on an empty stack — reachable with no factory at all.
    assert_eq!(
        warnings_from(|handle| {
            assert!(!handle.pop_with(7_i32), "nothing to pop");
        }),
        ["pop"],
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
        ["remove_route"],
        "a missing removal target reports too"
    );

    // 5. An **unresolved name** on both `_with` methods. Distinct from row 1: there
    //    the name resolved and the capture was empty, so `dismiss_captured`
    //    reported. Here `resolve_named` fails, and the result is still the caller's
    //    bare `TO` — dropped by a `?` before it was ever erased, which no
    //    `Option<AnyResult>` audit could see.
    assert_eq!(
        warnings_from(|handle| {
            assert!(
                handle
                    .push_replacement_named_with("/nowhere", 7_i32)
                    .is_err(),
                "unresolved"
            );
        }),
        ["push_replacement_named_with"],
        "push_replacement_named_with reports on the unresolved path"
    );
    assert_eq!(
        warnings_from(|handle| {
            assert!(handle.pop_and_push_named_with("/nowhere", 7_i32).is_err());
        }),
        ["pop_and_push_named_with"],
        "and so does pop_and_push_named_with"
    );

    // 6. `maybe_pop_with` on a lone route, which bubbles BY DESIGN — the one that
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
        undelivered_operations(&bubbled),
        ["maybe_pop"],
        "the Bubble arm reports, and it used to drop under the history guard; \
         captured:\n{}",
        bubbled.render_at_least(tracing::Level::WARN)
    );

    // 7. `DoNotPop` — the arm that reports *and* answers "handled". A vetoing
    //    route keeps the stack and the caller's result has nowhere to go, so this
    //    is the one arm where "the request was dealt with" and "your value was
    //    discarded" are true at once. It is also the row carrying an observable
    //    `Drop`: the arm runs under the history guard, so the proof that matters
    //    is that the value's `Drop` ran and the navigator still works afterwards.
    let vetoed_drops = Arc::new(AtomicUsize::new(0));
    let vetoed = flui_testing::log_capture::capture(|| {
        let built = Built::default();
        let handle = NavigatorHandle::new();
        handle.seed_initial(page(&built, "/"));
        let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
        handle.push(VetoingRoute::new());
        laid.tick();
        assert!(
            handle.maybe_pop_with(DropCounter::new(&vetoed_drops)),
            "a veto HANDLES the request — it just does not pop"
        );
        laid.tick();
        assert_eq!(
            handle.route_ids().len(),
            2,
            "and the vetoing route is still there"
        );
        assert_eq!(
            vetoed_drops.load(Ordering::Relaxed),
            1,
            "the value's Drop ran once, after the guard released — the navigator \
             below still answers, which it could not do from inside its own lock"
        );
        assert!(handle.can_pop(), "still usable");
    })
    .1;
    assert_eq!(
        undelivered_operations(&vetoed),
        ["maybe_pop"],
        "the DoNotPop arm reports; captured:\n{}",
        vetoed.render_at_least(tracing::Level::WARN)
    );

    // 8. `pop_disposition_of_top() == None` on a **mounted** navigator — distinct
    //    from row 2 (unmounted) and row 3 (`pop`, which never asks for a
    //    disposition). Reached by removing the only route: a removed entry is not
    //    `is_present`, so the disposition query has no top to ask.
    let dispositionless = flui_testing::log_capture::capture(|| {
        let built = Built::default();
        let handle = NavigatorHandle::new();
        handle.seed_initial(page(&built, "/"));
        let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
        let root = handle.current().expect("seeded");
        assert!(handle.remove_route(root), "removed");
        laid.tick();
        assert_eq!(handle.current(), None, "nothing is present any more");
        assert!(
            handle.is_mounted(),
            "and yet the navigator is mounted — this is not row 2"
        );
        assert!(
            !handle.maybe_pop_with(7_i32),
            "no disposition means nothing handled it"
        );
    })
    .1;
    assert_eq!(
        undelivered_operations(&dispositionless),
        ["maybe_pop"],
        "the no-disposition arm reports; captured:\n{}",
        dispositionless.render_at_least(tracing::Level::WARN)
    );
}

/// An operation's own observations reach observers **before** anything a
/// re-entrant drop triggers.
///
/// Wave 5 moved undelivered-result reporting out of the history guard, which was
/// right, and opened a window: history had advanced but the `FlushOutcome` had not
/// been applied, so a nested operation launched from a drop path notified
/// observers *first*. The effect preceded its cause in the stream.
///
/// The oracle is the **sequence**, not presence. A test asserting only "the warn
/// was emitted" or "the nested pop happened" cannot see this at all — both are
/// true in either order, which is precisely the class of assertion this PR has
/// spent its length finding.
///
/// Red-check: report undelivered values before applying the outcome in
/// `NavigatorShared::mutate` and the two pops swap places.
#[test]
fn an_operations_own_observations_precede_anything_a_re_entrant_drop_triggers() {
    /// Pops the navigator from its own `Drop`, through the `Send` command target —
    /// the same supported re-entrant path the mismatch-reporting test uses.
    struct PopsNavigatorOnDrop {
        target: NavigatorCommandTarget,
    }

    impl Drop for PopsNavigatorOnDrop {
        fn drop(&mut self) {
            let _ = NavigatorCommand::pop(self.target).apply_on_owner();
        }
    }

    /// Records which route each pop named, so the two are distinguishable.
    #[derive(Default)]
    struct PopOrder(Mutex<Vec<RouteId>>);

    impl NavigatorObserver for PopOrder {
        fn did_pop(&self, route: RouteId, _previous: Option<RouteId>) {
            self.0.lock().push(route);
        }
    }

    let built = Built::default();
    let handle = NavigatorHandle::new();
    handle.seed_initial(page(&built, "/"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
    let middle = handle.push(page(&built, "middle"));
    laid.tick();
    let middle_id = handle.current().expect("middle on top");
    handle.push(page(&built, "target"));
    laid.tick();
    let target_id = handle.current().expect("target on top");

    let order = Arc::new(PopOrder::default());
    handle.add_observer(Arc::clone(&order) as Arc<dyn NavigatorObserver>);

    // `page` is a `SimpleRoute<i32>`, so this payload mismatches and travels the
    // undelivered-result path — where its `Drop` pops again.
    assert!(handle.pop_with(PopsNavigatorOnDrop {
        target: handle.command_target(),
    }));
    laid.tick();

    assert_eq!(
        order.0.lock().clone(),
        vec![target_id, middle_id],
        "the pop that CAUSED the drop is observed first; the nested pop the drop \
         triggered comes second. Reversed, an observer sees an effect before its \
         cause and cannot reconstruct the sequence"
    );
    assert!(
        middle.is_completed(),
        "and the nested pop really did happen — without this the ordering \
         assertion could pass on a single event"
    );
}

/// A route that answers `Animating` from `did_push`, like every
/// `TransitionRoute` descendant — and nothing ever completes it.
///
/// The mirror of [`DeferredExitRoute`], on the axis that fixture left at its
/// default. `TransitionRoute::did_push` returns `Animating`, so every production
/// `PageRoute`/`PopupRoute`/`ModalRoute` sits in `Pushing` for its **entire
/// entrance transition** — and `Pushing` is inside `is_present()`, so `current()`
/// returns it and a named operation will capture and act on it.
struct DeferredEntranceRoute {
    settings: RouteSettings,
    builder: RouteContentBuilder,
    current_result: i32,
    /// Held for the route's whole lifetime: dropping it would cancel and
    /// settle the very push this fixture exists to leave unresolved
    /// (`Drop for TickerCompleter` publishes `Canceled` and delivers).
    completer: Option<flui_scheduler::TickerCompleter>,
}

impl DeferredEntranceRoute {
    fn new(name: &'static str, current_result: i32) -> Self {
        Self {
            settings: RouteSettings::named(name),
            builder: Rc::new(leaf),
            current_result,
            completer: None,
        }
    }
}

impl Route for DeferredEntranceRoute {
    type Output = i32;

    fn settings(&self) -> &RouteSettings {
        &self.settings
    }

    fn current_result(&mut self) -> Option<i32> {
        Some(self.current_result)
    }

    /// The whole point of this fixture: hold the completer, hand out the
    /// future, and never resolve it.
    fn did_push(&mut self) -> PushCompletion {
        let (completer, future) = flui_scheduler::TickerFuture::pending();
        self.completer = Some(completer);
        PushCompletion::Animating(future)
    }
}

impl NavigatorRoute for DeferredEntranceRoute {
    fn content_builder(&self) -> RouteContentBuilder {
        Rc::clone(&self.builder)
    }
}

/// `DeferredEntranceRoute` holds its completer for the route's whole
/// lifetime, so the future `did_push` hands out never resolves — the fixture
/// every other test in this section depends on. Proven directly, across
/// several pumps rather than one: a route it replaces cannot be disposed,
/// because disposing anything below an animating top route needs that top
/// route to reach `Idle` first (`navigator.dart:4499-4512`), and this one
/// never does.
///
/// Red-check: let `did_push` discard the completer instead of storing it —
/// `Drop for TickerCompleter` cancels and delivers immediately, and the
/// replaced route disposes on the very next pump.
#[test]
fn deferred_entrance_route_stays_pushing_while_its_completer_lives() {
    let handle = NavigatorHandle::new();
    handle.seed_initial(DeferredEntranceRoute::new("/", 0));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
    let root = handle.current().expect("seeded");

    let replaced = handle.push(DeferredEntranceRoute::new("first", 1));
    laid.tick();
    let replaced_id = handle.current().expect("mid-entrance");

    let _arriving = handle.push_replacement(DeferredEntranceRoute::new("second", 2));
    let arrived = handle.current().expect("replaced");

    for _ in 0..5 {
        laid.tick();
    }

    assert!(
        replaced.is_completed(),
        "the replaced route's result resolved at replacement time"
    );
    let ids = handle.route_ids();
    assert_eq!(
        ids.len(),
        3,
        "the replaced route is still held after five pumps: {ids:?}"
    );
    assert!(ids.contains(&root));
    assert!(
        ids.contains(&replaced_id),
        "disposing it needs `arrived` to reach `Idle`, which never happens"
    );
    assert!(ids.contains(&arrived));
}

/// A route whose `popDisposition` is `DoNotPop` — the third arm of
/// `maybe_pop`'s match, and the only one that reports *and* returns "handled".
struct VetoingRoute {
    settings: RouteSettings,
    builder: RouteContentBuilder,
}

impl VetoingRoute {
    fn new() -> Self {
        Self {
            settings: RouteSettings::named("vetoing"),
            builder: Rc::new(|_ctx| SizedBox::new(10.0, 10.0).into_view().boxed()),
        }
    }
}

impl Route for VetoingRoute {
    type Output = i32;

    fn settings(&self) -> &RouteSettings {
        &self.settings
    }

    fn vetoes_pop(&self) -> bool {
        true
    }
}

impl NavigatorRoute for VetoingRoute {
    fn content_builder(&self) -> RouteContentBuilder {
        Rc::clone(&self.builder)
    }
}

/// The same refusal, with a caller-supplied result.
///
/// `did_pop`'s refusal arm hands the value back as `UndeliveredResult::NoTarget`
/// rather than dropping it — which matters because that arm runs **under the
/// history mutex**, where the value's own `Drop` is user code that may re-enter.
///
/// Red-check: replace that arm's `result.map(UndeliveredResult::no_target)` with
/// `None`. The value is then dropped inline under the guard, no warning is
/// emitted, and this test's `operation`/`supplied` assertion fails while every
/// stack-shape assertion above it still passes.
#[test]
fn a_refused_pop_hands_the_callers_result_back_instead_of_dropping_it() {
    let built = Built::default();
    let attempts = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicUsize::new(0));

    let ((), log) = flui_testing::log_capture::capture(|| {
        let handle = NavigatorHandle::new();
        let next = built.clone();
        handle.route("/next", move |_request: &RouteRequest<'_>| {
            Some(page(&next, "/next"))
        });
        handle.seed_initial(page(&built, "/"));
        let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
        handle.push(RefusingRoute::new(&attempts));
        laid.tick();

        handle
            .pop_and_push_named_with("/next", DropCounter::new(&dropped))
            .expect("the name is registered");
        laid.tick();

        assert_eq!(
            dropped.load(Ordering::Relaxed),
            1,
            "the undeliverable value was dropped exactly once, and the navigator \
             is still usable afterwards — proof it did not run under the guard"
        );
        assert!(handle.maybe_pop(), "still usable");
    });

    let reported: Vec<&str> = undelivered_operations(&log);
    assert_eq!(
        reported,
        vec!["pop_and_push_named_with"],
        "the refusal arm reported under the composed operation's own name; \
         captured:\n{}",
        log.render_at_least(tracing::Level::WARN)
    );
}

/// A route factory that **panics** after the caller's result has been erased.
///
/// The `_with` methods erase to `AnyResult` *before* resolving, deliberately: the
/// alternative loses the value in a `?` on the unresolved path. That opens a
/// window where the erased value is a local held across user code that may unwind,
/// and this test establishes what actually happens in it. Measured, in this order:
///
/// 1. **No hang.** `RouteRegistry::resolve` invokes the factory with its own lock
///    released (`pick` clones the factory out from under the guard), and nothing
///    else is held. Worth stating precisely, because the obvious reason is the
///    wrong one: even *with* the factory called under the guard there is no hang,
///    since a `parking_lot::MutexGuard` releases as its frame unwinds. What `pick`
///    actually buys is safety against a factory that **re-enters** the registry,
///    which is a different scenario from one that panics.
/// 2. **No leak.** The value's `Drop` runs during the unwind.
/// 3. **No report**, deliberately. A `warn!` here would run a `tracing`
///    subscriber during an **unwind**, where a subscriber that panics turns a
///    recoverable panic into a process **abort**. And the log line it would buy is
///    redundant: the panic is already propagating in the caller's own frame, so
///    nobody is left guessing where their value went. Trading a redundant log line
///    for an abort risk is the wrong direction. (This is a sharper argument than
///    the general "user code must not run under a lock" one that governs the other
///    undelivered paths — no lock is involved here at all.)
/// 4. **The navigator survives.** `parking_lot` does not poison, the stack is
///    untouched (resolution panicked before the pop was issued), and the next
///    operation succeeds.
///
/// Red-check: swap the order in `pop_and_push_named_with` so `dismiss_captured`
/// runs *before* `resolve_named`. Assertion (3) then fails with `left: []` against
/// `right: [RouteId(1)]` — the pop went through, the factory panicked, and the
/// navigator is stranded with an empty stack. That is the fact this ordering
/// protects, and it is the reason resolve-before-dismiss is not merely tidier.
///
/// (Moving the factory invocation inside the registry guard, which this line first
/// claimed would hang, does **not** go red: the guard releases as the frame
/// unwinds. Measured under an external timeout, not assumed.)
#[test]
fn a_factory_that_panics_after_the_result_is_erased_loses_only_the_report() {
    let built = Built::default();
    let dropped = Arc::new(AtomicUsize::new(0));
    let payload_counter = Arc::clone(&dropped);

    let ((), log) = flui_testing::log_capture::capture(|| {
        let handle = NavigatorHandle::new();
        handle.route(
            "/boom",
            |_request: &RouteRequest<'_>| -> Option<SimpleRoute<i32>> {
                panic!("a route factory is user code and may unwind");
            },
        );
        handle.seed_initial(page(&built, "/"));
        let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
        let before = handle.route_ids();

        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            handle.pop_and_push_named_with("/boom", DropCounter::new(&payload_counter))
        }));

        assert!(
            outcome.is_err(),
            "the panic reaches the caller rather than being swallowed"
        );
        assert_eq!(
            dropped.load(Ordering::Relaxed),
            1,
            "and the erased value was dropped exactly once on the unwind path"
        );

        laid.tick();
        assert_eq!(
            handle.route_ids(),
            before,
            "the stack is untouched — resolution panicked before the pop was issued"
        );

        // The load-bearing half: this line HANGS if the factory ran under a lock.
        handle.route("/after", {
            let built = built.clone();
            move |_request: &RouteRequest<'_>| Some(page(&built, "/after"))
        });
        let arrived = handle
            .push_named("/after")
            .expect("registered after a panic");
        laid.tick();
        assert_eq!(
            handle.current(),
            Some(arrived),
            "and the navigator is fully usable afterwards"
        );
    });

    assert!(
        undelivered_operations(&log).is_empty(),
        "nothing is reported, by design — see this test's doc; captured:\n{}",
        log.render_at_least(tracing::Level::WARN)
    );
}
