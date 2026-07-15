//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/navigator_test.dart` and
//! `.../routes_test.dart` (tag `3.44.0`).
//!
//! FLUI already has two heavyweight self-authored `Navigator`/route suites —
//! `tests/navigator_public.rs` (18.7K) and `tests/routes.rs` — driven through
//! the exact public surface this file uses. Their value is regression-guard
//! coverage of FLUI's own design; this file's value is different: every case
//! below is anchored to a **named upstream Flutter test** and asserts the
//! sequence or identity Flutter itself asserts, adapted only where FLUI's
//! public `Navigator` surface differs in shape from Flutter's.
//!
//! ## Ported cases
//! - `'Route management - push, replace, pop sequence'` (routes_test.dart) —
//!   the route-level lifecycle callback *order*: `didAdd`/`didChangeNext` on
//!   the seeded route, then `didPush`/`didChangeNext` on each new top with
//!   `didChangeNext` propagating to the route it displaced —
//!   [`sequential_pushes_call_did_add_did_push_then_propagate_did_change_next`].
//!   The pop half — `didPop`/`didComplete` on the popped route, then
//!   `didPopNext` propagating to the route now on top —
//!   [`sequential_pops_call_did_pop_then_propagate_did_pop_next`].
//!   **Adapted:** the oracle's middle phase drives `NavigatorState.replace`
//!   (`navigator.dart:5353`), an arbitrary mid-stack swap FLUI has no public
//!   equivalent for (only `push_replacement`, which targets the current top —
//!   `navigator_public.rs`'s `public_prelude_exports_exact_approved_surface`
//!   pins generic `replace`/`ModalRoute`/`TransitionRoute` as private). The
//!   top-only shape `push_replacement` *does* have is covered next.
//! - `'pushReplacement correctly reports didReplace to the observer'`
//!   (navigator_test.dart) — the observer sees `didReplace(new, old)`, and the
//!   displaced route is disposed and completes its future rather than being
//!   popped —
//!   [`push_replacement_disposes_the_old_route_and_reports_did_replace`].
//!   **Adapted:** the oracle reaches this state via `popUntil` first (no FLUI
//!   equivalent — see *Not ported*, below); ported directly on a one-route
//!   stack instead, which exercises the identical `pushReplacement` contract
//!   without it.
//! - `'pushAndRemoveUntil triggers secondaryAnimation'` and `'pushAndRemoveUntil
//!   does not remove routes below the first route that pass the predicate'`
//!   (navigator_test.dart) — the removal-set half: every route above the kept
//!   one is gone in the same flush that pushes the new top, and the kept
//!   route survives underneath it. The secondary-animation half is Phase 3
//!   (paint/animation assertions deferred, per this crate's `parity/main.rs`
//!   module doc) —
//!   [`push_and_remove_until_removes_every_route_above_the_kept_one`].
//! - `'Route settings'` (routes_test.dart) — a named `RouteSettings` carries
//!   its name through a one-line `Debug` description —
//!   [`route_settings_named_exposes_the_name_through_debug`].
//! - `'Route settings arguments'` (routes_test.dart) — a `RouteSettings`
//!   arguments payload round-trips through the erased-and-downcast boundary
//!   FLUI's `Arc<dyn Any + Send + Sync>` shape uses in place of Dart's
//!   `Object?` —
//!   [`route_settings_arguments_round_trip_via_downcast`].
//! - `NavigatorState.popUntil` (`navigator.dart:5651-5660`), exercised
//!   inline by `'pushReplacement correctly reports didReplace to the
//!   observer'` and directly by `'Able to pop all routes'`
//!   (navigator_test.dart) — the kept route survives, every route above it
//!   runs its full per-route lifecycle on the way out, and a predicate that
//!   never matches empties the stack instead of panicking —
//!   [`pop_until_stops_at_the_kept_route_without_popping_it`],
//!   [`pop_until_with_a_never_satisfied_predicate_empties_the_stack_without_panicking`].
//! - `'remove route below an other one whose value is awaited'`
//!   (navigator_test.dart) — removing a non-top route still completes its
//!   future, and leaves the top route undisturbed —
//!   [`remove_route_below_the_top_completes_its_future_without_disturbing_the_top`].
//! - `'initial route trigger observer in the right order'` (navigator_test.dart)
//!   — `didPush(route, previous)` carries the correct route *identity* through
//!   a three-deep chain, not merely the right event kind (kind-only ordering
//!   is already pinned by `navigator_public.rs`'s
//!   `public_observer_ordering_survives_the_public_api`) —
//!   [`push_observer_reports_the_correct_previous_route_across_a_chain`].
//! - `'Push and pop should trigger the observers'` (navigator_test.dart) — the
//!   pop half's route/previous identity —
//!   [`pop_observer_reports_the_popped_route_and_its_predecessor`].
//!
//! ## Not ported
//! - `popUntilWithResult` — FLUI's `pop_until` mirrors Flutter's `popUntil`
//!   (no-result) shape only; the result-delivering variant, which requires
//!   deciding *which* popped route receives the caller's value, is deferred
//!   alongside named-route generation below.
//! - `onGenerateRoute`, named-route generation, and `'arguments for named
//!   routes on Navigator'` — there is no route-table / `onGenerateRoute`
//!   mechanism to generate a route from a name; `RouteSettings.arguments`
//!   itself is now ported (see *Ported cases*), but nothing yet consumes it
//!   to build a route.
//! - `'Navigator.of rootNavigator finds root Navigator'` and nested-navigator
//!   scoping generally — already ported in `navigator_public.rs`'s
//!   `public_nested_navigator_lookup_nearest_and_root` and
//!   `public_maybe_of_returns_none_when_absent`; re-porting it here would
//!   assert the identical fact through the identical public calls.
//! - `maybePop`/`canPop` bubble semantics — already ported in
//!   `navigator_public.rs`'s `public_can_pop_contract` and
//!   `public_navigator_maybe_pop_refusal_matches_private_behavior`.
//! - The `Pages` API (`Navigator(pages:, onPopPage:)`, `'Can push, pop, and
//!   replace in sequence'`) — FLUI's `Navigator` is imperative-only; there is
//!   no declarative page-list reconciliation to port this onto.
//!
//! Widget → type mapping: `Navigator` → `NavigatorHandle` + `RouteHistory`
//! (private); a route under test implements the public `Route`/`NavigatorRoute`
//! traits directly, exactly as an app author would.

use std::rc::Rc;
use std::sync::Arc;

use crate::common::{lay_out, loose};
use flui_widgets::prelude::*;
use flui_widgets::{
    NavigatorObserver, NavigatorRoute, PushCompletion, Route, RouteContentBuilder, RouteId,
    RouteSettings,
};
use parking_lot::Mutex;

// ============================================================================
// PROBES
// ============================================================================

/// One lifecycle callback a [`ProbeRoute`] observed about itself.
#[derive(Clone, Debug, PartialEq, Eq)]
enum RouteEvent {
    Add,
    Push,
    Pop,
    Complete(Option<i32>),
    PopNext(RouteId),
    ChangeNext(Option<RouteId>),
    Dispose,
}

/// Callback events in delivery order, tagged with the logging route's own
/// name — the oracle for callback *order and identity*, not content.
#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<(&'static str, RouteEvent)>>>);

impl Log {
    fn record(&self, name: &'static str, event: RouteEvent) {
        self.0.lock().push((name, event));
    }

    fn len(&self) -> usize {
        self.0.lock().len()
    }

    /// Every event recorded from `start` onward — the delta one navigator
    /// operation produced.
    fn since(&self, start: usize) -> Vec<(&'static str, RouteEvent)> {
        self.0.lock()[start..].to_vec()
    }

    fn all(&self) -> Vec<(&'static str, RouteEvent)> {
        self.0.lock().clone()
    }
}

/// A route implemented against the **public** [`Route`] trait that logs every
/// lifecycle hook it receives into a shared [`Log`].
struct ProbeRoute {
    name: &'static str,
    settings: RouteSettings,
    builder: RouteContentBuilder,
    log: Log,
}

impl Route for ProbeRoute {
    type Output = i32;

    fn settings(&self) -> &RouteSettings {
        &self.settings
    }

    fn did_add(&mut self) {
        self.log.record(self.name, RouteEvent::Add);
    }

    fn did_push(&mut self) -> PushCompletion {
        self.log.record(self.name, RouteEvent::Push);
        PushCompletion::Immediate
    }

    fn did_pop(&mut self) -> bool {
        self.log.record(self.name, RouteEvent::Pop);
        true
    }

    fn did_complete(&mut self, result: Option<&i32>) {
        self.log
            .record(self.name, RouteEvent::Complete(result.copied()));
    }

    fn did_pop_next(&mut self, popped: RouteId) {
        self.log.record(self.name, RouteEvent::PopNext(popped));
    }

    fn did_change_next(&mut self, next: Option<RouteId>) {
        self.log.record(self.name, RouteEvent::ChangeNext(next));
    }

    fn dispose(&mut self) {
        self.log.record(self.name, RouteEvent::Dispose);
    }
}

impl NavigatorRoute for ProbeRoute {
    fn content_builder(&self) -> RouteContentBuilder {
        Rc::clone(&self.builder)
    }
}

fn probe(log: &Log, name: &'static str) -> ProbeRoute {
    ProbeRoute {
        name,
        settings: RouteSettings::named(name),
        builder: Rc::new(|_ctx| SizedBox::new(10.0, 10.0).into_view().boxed()),
        log: log.clone(),
    }
}

/// A route with no lifecycle instrumentation, for cases that only care about
/// stack shape or observer identity.
fn named_page(name: &'static str) -> SimpleRoute<i32> {
    SimpleRoute::new(|_ctx| SizedBox::new(10.0, 10.0).into_view().boxed()).named(name)
}

/// Records `NavigatorObserver` notifications with full route identity — unlike
/// `navigator_public.rs`'s `Spy`, which records only the event kind.
#[derive(Default)]
struct Spy {
    pushes: Mutex<Vec<(RouteId, Option<RouteId>)>>,
    pops: Mutex<Vec<(RouteId, Option<RouteId>)>>,
    replacements: Mutex<Vec<(Option<RouteId>, Option<RouteId>)>>,
}

impl Spy {
    fn pushes(&self) -> Vec<(RouteId, Option<RouteId>)> {
        self.pushes.lock().clone()
    }
    fn pops(&self) -> Vec<(RouteId, Option<RouteId>)> {
        self.pops.lock().clone()
    }
    fn replacements(&self) -> Vec<(Option<RouteId>, Option<RouteId>)> {
        self.replacements.lock().clone()
    }
}

impl NavigatorObserver for Spy {
    fn did_push(&self, route: RouteId, previous: Option<RouteId>) {
        self.pushes.lock().push((route, previous));
    }
    fn did_pop(&self, route: RouteId, previous: Option<RouteId>) {
        self.pops.lock().push((route, previous));
    }
    fn did_replace(&self, new_route: Option<RouteId>, old_route: Option<RouteId>) {
        self.replacements.lock().push((new_route, old_route));
    }
}

// ============================================================================
// TESTS
// ============================================================================

/// `didAdd`/`didChangeNext` on the seeded route, then `didPush`/`didChangeNext`
/// on each new top, with `didChangeNext` propagating to the route it displaced.
///
/// Oracle: `'Route management - push, replace, pop sequence'`
/// (routes_test.dart), first two phases (`initial: install`/`didAdd`/
/// `didChangeNext null`, then `second: install`/`didPush`/`didChangeNext null`/
/// `initial: didChangeNext second`).
///
/// Red-check: swap the order of `route.did_push()` and
/// `route.did_change_next(None)` in `RouteEntry::handle_push`.
#[test]
fn sequential_pushes_call_did_add_did_push_then_propagate_did_change_next() {
    let log = Log::default();
    let handle = NavigatorHandle::new();
    handle.seed_initial(probe(&log, "initial"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));

    assert_eq!(
        log.all(),
        vec![
            ("initial", RouteEvent::Add),
            ("initial", RouteEvent::ChangeNext(None)),
        ],
        "the seeded route installs, then announces it has no route above it"
    );

    let start = log.len();
    handle.push(probe(&log, "second"));
    laid.tick();
    let second_id = handle.current().expect("second is now the top");

    assert_eq!(
        log.since(start),
        vec![
            ("second", RouteEvent::Push),
            ("second", RouteEvent::ChangeNext(None)),
            ("initial", RouteEvent::ChangeNext(Some(second_id))),
        ],
        "a push calls didPush then didChangeNext(null) on the new top, then \
         propagates didChangeNext(new top) to the route it displaced"
    );

    let start = log.len();
    handle.push(probe(&log, "third"));
    laid.tick();
    let third_id = handle.current().expect("third is now the top");

    assert_eq!(
        log.since(start),
        vec![
            ("third", RouteEvent::Push),
            ("third", RouteEvent::ChangeNext(None)),
            ("second", RouteEvent::ChangeNext(Some(third_id))),
        ],
        "the same three-step sequence repeats for the next push"
    );
}

/// `didPop` then `didComplete` on the popped route, then `didPopNext(popped)`
/// on the route now on top.
///
/// Oracle: `'Route management - push, replace, pop sequence'` (routes_test.dart),
/// final two phases (`'third: didPop hello'`/`'two: didPopNext third'`, then
/// `'two: didPop good bye'`/`'initial: didPopNext two'`).
///
/// Red-check: drop the `did_complete` call from `ErasedRoute::did_pop` in
/// `route.rs` — the popped route would never see its own result.
#[test]
fn sequential_pops_call_did_pop_then_propagate_did_pop_next() {
    let log = Log::default();
    let handle = NavigatorHandle::new();
    handle.seed_initial(probe(&log, "initial"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));

    handle.push(probe(&log, "second"));
    laid.tick();
    let second_id = handle.current().expect("second pushed");

    let third_result = handle.push(probe(&log, "third"));
    laid.tick();
    let third_id = handle.current().expect("third pushed");

    let start = log.len();
    assert!(handle.pop_with(11_i32), "third consents to the pop");
    laid.tick();

    assert_eq!(
        log.since(start),
        vec![
            ("third", RouteEvent::Pop),
            ("third", RouteEvent::Complete(Some(11))),
            ("second", RouteEvent::PopNext(third_id)),
            ("third", RouteEvent::Dispose),
        ],
        "a pop calls didPop then didComplete on the popped route, then \
         didPopNext(popped) on the route now on top, then disposes the \
         finished route"
    );
    assert_eq!(third_result.try_take(), Some(Some(11)));

    let start = log.len();
    assert!(handle.pop_with(22_i32), "second consents to the pop");
    laid.tick();

    assert_eq!(
        log.since(start),
        vec![
            ("second", RouteEvent::Pop),
            ("second", RouteEvent::Complete(Some(22))),
            ("initial", RouteEvent::PopNext(second_id)),
            ("second", RouteEvent::Dispose),
        ],
        "the same sequence repeats one level down the stack"
    );
}

/// `push_replacement` disposes the route it displaces (completing its future,
/// not popping it) and reports `didReplace(new, old)` to the observer, in place
/// — the stack length does not grow.
///
/// Oracle: `'pushReplacement correctly reports didReplace to the observer'`
/// (navigator_test.dart), the `didReplace` assertion (`observations[2]`) —
/// ported without the `popUntil` setup that test uses to get there, since the
/// same `pushReplacement` contract holds on a one-route stack.
///
/// Red-check: make `push_replacement_with_id` call `arm_pop` instead of
/// `arm_complete(result, true)` on the old top — the displaced route would be
/// popped (and its future left dangling on refusal) instead of completed.
#[test]
fn push_replacement_disposes_the_old_route_and_reports_did_replace() {
    let log = Log::default();
    let handle = NavigatorHandle::new();
    let root_result = handle.seed_initial(probe(&log, "root"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
    let root_id = handle.current().expect("root seeded");

    let spy = Arc::new(Spy::default());
    handle.add_observer(Arc::clone(&spy) as Arc<dyn NavigatorObserver>);

    handle.push_replacement_with(probe(&log, "replacement"), 7_i32);
    laid.tick();

    assert_eq!(
        root_result.try_take(),
        Some(Some(7)),
        "the displaced route's future resolves with the value \
         push_replacement_with carried, not the default fallback"
    );

    let new_top = handle.current().expect("the replacement is now the top");
    assert_ne!(
        new_top, root_id,
        "the id changed: this is a new route, not a mutation of root"
    );
    assert_eq!(
        handle.route_ids().len(),
        1,
        "push_replacement swaps in place; it never appends"
    );
    assert!(
        log.all().contains(&("root", RouteEvent::Dispose)),
        "the displaced route is disposed, not merely popped: {:?}",
        log.all()
    );
    assert!(
        !log.all().contains(&("root", RouteEvent::Pop)),
        "disposed, not merely popped: root must never receive a Pop event: {:?}",
        log.all()
    );
    assert_eq!(
        spy.replacements(),
        vec![(Some(new_top), Some(root_id))],
        "the observer sees didReplace(new, old)"
    );
}

/// `push_and_remove_until` removes every present route above the one the
/// predicate keeps, in the same flush that pushes the new top — the kept
/// route survives underneath it, untouched.
///
/// Oracle: `'pushAndRemoveUntil triggers secondaryAnimation'` and
/// `'pushAndRemoveUntil does not remove routes below the first route that pass
/// the predicate'` (navigator_test.dart) — the removal-set half of both.
///
/// Red-check: make `push_and_remove_until_with_id` stop its downward walk one
/// entry early — `mid` would survive alongside `kept`.
#[test]
fn push_and_remove_until_removes_every_route_above_the_kept_one() {
    let handle = NavigatorHandle::new();
    handle.seed_initial(named_page("root"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
    let root_id = handle.current().expect("root seeded");

    handle.push(named_page("mid"));
    laid.tick();
    assert_eq!(
        handle.route_ids().len(),
        2,
        "root and mid are both present before the sweep"
    );

    handle.push_and_remove_until(named_page("kept"), |id| id == root_id);
    laid.tick();

    let stack = handle.route_ids();
    assert_eq!(
        stack.len(),
        2,
        "mid was removed in the same flush that pushed kept"
    );
    assert_eq!(
        stack[0], root_id,
        "the predicate's route survives at the bottom"
    );
    assert_ne!(
        stack[1], root_id,
        "the new top is the pushed route, not a duplicate of root"
    );
}

/// `pop_until` pops one route at a time until the predicate accepts the
/// route now on top; the accepted route is never popped, and every route
/// above it runs its full per-route lifecycle on the way out (matching
/// [`sequential_pops_call_did_pop_then_propagate_did_pop_next`], repeated
/// once per popped route rather than batched).
///
/// Oracle: `NavigatorState.popUntil` (`navigator.dart:5651-5660`) — exercised
/// inline by `'pushReplacement correctly reports didReplace to the
/// observer'` (`Navigator.of(context).popUntil((route) => route.isFirst)`),
/// which this crate ports without the `popUntil` setup (see the module doc's
/// *Ported cases*). This test targets the `popUntil` contract itself.
///
/// Red-check: this test was written and run against a `pop_until` that
/// called `history.pop(None)` **before** checking `keep(candidate)` — an
/// off-by-one that pops one route too many. On this three-route stack it
/// popped `root` too (the kept route), which the assertions below caught:
/// the final stack was empty instead of `[root_id]`, and `log.all()`
/// contained a `("root", RouteEvent::Pop)` entry. Fixed by checking `keep`
/// before popping, as written now.
#[test]
fn pop_until_stops_at_the_kept_route_without_popping_it() {
    let log = Log::default();
    let handle = NavigatorHandle::new();
    handle.seed_initial(probe(&log, "root"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
    let root_id = handle.current().expect("root seeded");

    handle.push(probe(&log, "mid"));
    laid.tick();
    let mid_id = handle.current().expect("mid pushed");

    handle.push(probe(&log, "top"));
    laid.tick();
    let top_id = handle.current().expect("top pushed");

    let start = log.len();
    handle.pop_until(|id| id == root_id);
    laid.tick();

    assert_eq!(
        handle.route_ids(),
        vec![root_id],
        "everything above root is gone; root itself remains"
    );
    assert_eq!(
        log.since(start),
        vec![
            ("top", RouteEvent::Pop),
            ("top", RouteEvent::Complete(None)),
            ("mid", RouteEvent::PopNext(top_id)),
            ("top", RouteEvent::Dispose),
            ("mid", RouteEvent::Pop),
            ("mid", RouteEvent::Complete(None)),
            ("root", RouteEvent::PopNext(mid_id)),
            ("mid", RouteEvent::Dispose),
        ],
        "each popped route runs its full didPop/didComplete/didPopNext/dispose \
         sequence, in its own flush, before the next candidate is read"
    );
    assert!(
        !log.all().contains(&("root", RouteEvent::Pop)),
        "the kept route is never popped: {:?}",
        log.all()
    );
}

/// A predicate that never accepts empties the whole stack instead of
/// looping forever or panicking.
///
/// Oracle: `'Able to pop all routes'` (navigator_test.dart) —
/// `Navigator.of(context).popUntil((route) => false)`, asserting
/// `tester.takeException()` is `null`. FLUI has no exception channel to
/// assert against; a clean return (no panic) plus an emptied stack is the
/// Rust-shaped equivalent.
///
/// Red-check: make `pop_until`'s loop condition ignore `history.current()`
/// returning `None` (e.g. `.unwrap_or(true)` instead of an early `false`) —
/// it would spin forever popping nothing once the stack is empty, and this
/// test would hang instead of completing.
#[test]
fn pop_until_with_a_never_satisfied_predicate_empties_the_stack_without_panicking() {
    let handle = NavigatorHandle::new();
    handle.seed_initial(named_page("root"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));

    handle.push(named_page("mid"));
    laid.tick();
    handle.push(named_page("top"));
    laid.tick();

    handle.pop_until(|_| false);
    laid.tick();

    assert!(
        handle.route_ids().is_empty(),
        "a predicate that never matches pops every route: {:?}",
        handle.route_ids()
    );
}

/// A named [`RouteSettings`] carries its name through a one-line `Debug`
/// description.
///
/// Oracle: `'Route settings'` (routes_test.dart) — `hasOneLineDescription`.
///
/// Red-check: derive `Debug` with `#[derive(Debug)]`'s multi-field default
/// intact but rename the `name` field without updating this assertion — a
/// name typo would slip through `cargo build` silently.
#[test]
fn route_settings_named_exposes_the_name_through_debug() {
    let settings = RouteSettings::named("A");
    assert_eq!(settings.name(), Some("A"));

    let rendered = format!("{settings:?}");
    assert!(
        rendered.contains("RouteSettings") && rendered.contains('A'),
        "a description carrying the type and the name: {rendered}"
    );
    assert!(!rendered.contains('\n'), "one line: {rendered}");
}

/// `RouteSettings` with no `with_arguments` call reports no payload; one
/// built with it reports the exact value back through a downcast.
///
/// Oracle: `'Route settings arguments'` (routes_test.dart) —
/// `settings.arguments` is `null` by default, and `same(arguments)` for a
/// settings built with one. FLUI's arguments payload is an owned value
/// behind an `Arc`, not a reference cell, so "the exact object" is checked
/// by downcasting and comparing the value rather than Dart's `same()`
/// identity matcher.
///
/// Red-check: swap `argument`'s `downcast_ref::<T>()` for a call that always
/// returns `None` — this test's `Some(&Marker(7))` assertion would fail
/// where a type-confused stub would still pass a bare `is_some()` check.
#[test]
fn route_settings_arguments_round_trip_via_downcast() {
    let settings = RouteSettings::named("A");
    assert!(settings.arguments().is_none());

    #[derive(Debug, PartialEq)]
    struct Marker(u32);

    let settings = RouteSettings::named("A").with_arguments(Marker(7));
    assert_eq!(settings.argument::<Marker>(), Some(&Marker(7)));
    assert!(
        settings.argument::<String>().is_none(),
        "a type-mismatched downcast finds nothing, it does not panic"
    );
}

/// Removing a route that sits *below* the top completes its future, and
/// leaves the top route's identity undisturbed.
///
/// Oracle: `'remove route below an other one whose value is awaited'`
/// (navigator_test.dart).
///
/// Red-check: make `RouteHistory::remove_route` require the target to be the
/// last present entry — removing `mid` would then be refused instead of
/// completing it.
#[test]
fn remove_route_below_the_top_completes_its_future_without_disturbing_the_top() {
    let handle = NavigatorHandle::new();
    handle.seed_initial(named_page("root"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));

    let mid_result = handle.push(named_page("mid"));
    laid.tick();
    let mid_id = handle.current().expect("mid pushed");

    handle.push(named_page("top"));
    laid.tick();
    let top_id = handle.current().expect("top pushed");

    assert!(handle.remove_route_with(mid_id, 5_i32));
    laid.tick();

    assert_eq!(
        mid_result.try_take(),
        Some(Some(5)),
        "the removed route's future still resolves, though it was never on top"
    );
    let stack = handle.route_ids();
    assert_eq!(stack.len(), 2, "root and top remain; mid is gone");
    assert_eq!(
        handle.current(),
        Some(top_id),
        "the top route is undisturbed by a removal below it"
    );
}

/// Each `didPush(route, previous)` carries the *exact* route it displaced —
/// not merely the right event kind, in the right count, at the right position.
///
/// Oracle: `'initial route trigger observer in the right order'`
/// (navigator_test.dart) — `observations[n].previous` across a three-deep
/// chain.
///
/// Red-check: pass `previous_present` from the wrong entry (e.g. the entry
/// being displaced two levels down) into `Observation::Push` — the middle
/// pair would carry a stale identity that this test, but not a kind-only
/// check, would catch.
#[test]
fn push_observer_reports_the_correct_previous_route_across_a_chain() {
    let handle = NavigatorHandle::new();
    let spy = Arc::new(Spy::default());
    handle.add_observer(Arc::clone(&spy) as Arc<dyn NavigatorObserver>);

    handle.seed_initial(named_page("root"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
    let root_id = handle.current().expect("root seeded");

    handle.push(named_page("a"));
    laid.tick();
    let a_id = handle.current().expect("a pushed");

    handle.push(named_page("b"));
    laid.tick();
    let b_id = handle.current().expect("b pushed");

    assert_eq!(
        spy.pushes(),
        vec![(root_id, None), (a_id, Some(root_id)), (b_id, Some(a_id))],
        "each push's previous is the exact route it displaced"
    );
}

/// `didPop(route, previous)` carries the popped route and the route beneath
/// it, by identity.
///
/// Oracle: `'Push and pop should trigger the observers'` (navigator_test.dart)
/// — the pop half's `route`/`previousRoute` assertions.
///
/// Red-check: enqueue `Observation::Pop` with `previous_present` computed
/// *before* the pop instead of after — on a two-route stack this happens to
/// read the same value, so a deeper stack (used here) is what would catch it.
#[test]
fn pop_observer_reports_the_popped_route_and_its_predecessor() {
    let handle = NavigatorHandle::new();
    handle.seed_initial(named_page("root"));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
    let root_id = handle.current().expect("root seeded");

    handle.push(named_page("a"));
    laid.tick();
    let a_id = handle.current().expect("a pushed");

    let spy = Arc::new(Spy::default());
    handle.add_observer(Arc::clone(&spy) as Arc<dyn NavigatorObserver>);

    assert!(handle.pop());
    laid.tick();

    assert_eq!(
        spy.pops(),
        vec![(a_id, Some(root_id))],
        "the popped route and the route beneath it, by identity"
    );
}
