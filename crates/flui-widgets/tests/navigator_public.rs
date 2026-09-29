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

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::common::{lay_out, loose};
use parking_lot::Mutex;

// Exercise the public prelude import path.
use flui_widgets::prelude::*;
use flui_widgets::{
    NamedRouteError, NavigatorCommand, NavigatorCommandTarget, NavigatorObserver, NavigatorRoute,
    Route, RouteContentBuilder, RouteId, RouteKey, RouteRequest, RouteSettings,
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
}

fn page(built: &Built, name: &'static str) -> SimpleRoute<i32> {
    let built = built.clone();
    SimpleRoute::new(move |_ctx| {
        built.0.lock().push(name);
        SizedBox::new(10.0, 10.0).into_view().boxed()
    })
    .named(name)
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

// ============================================================================
// NAMED ROUTES (ADR-0024)
// ============================================================================

/// A leaf a generated route can show, with no probe attached.
fn leaf(_ctx: &dyn BuildContext) -> BoxedView {
    SizedBox::new(10.0, 10.0).into_view().boxed()
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
pub(crate) fn push_named_typed_with_the_wrong_result_type_errors_disposes_the_route_and_changes_nothing()
 {
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
pub(crate) fn a_factory_that_pushes_re_entrantly_does_not_deadlock() {
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
pub(crate) fn a_route_key_carries_its_result_type_from_registration_to_delivery() {
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

// ----------------------------------------------------------------------------
// Re-entrant factories: an operation acts on the route that was current when it
// was CALLED, not on whatever a factory left on top.
// ----------------------------------------------------------------------------

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
pub(crate) fn a_mismatched_pop_result_is_reported_and_dropped_outside_the_history_lock() {
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
pub(crate) fn every_operation_that_cannot_deliver_a_result_reports_it() {
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
pub(crate) fn a_factory_that_panics_after_the_result_is_erased_loses_only_the_report() {
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
