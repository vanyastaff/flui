//! `PopScope` — Flutter parity.
//!
//! Flutter source: `packages/flutter/test/widgets/pop_scope_test.dart` (tag
//! `3.44.0`), widget: `packages/flutter/lib/src/widgets/pop_scope.dart`.
//!
//! ## Case ledger
//!
//! 1. `'toggling canPop on root route allows/prevents backs'` — ported, minus
//!    the `SystemNavigator.setFrameworkHandlesBack` platform-channel
//!    assertion (FLUI has no platform channel):
//!    [`toggling_can_pop_on_the_root_route`].
//! 2. `'pop scope can receive result'` — out of scope, missing capability:
//!    `PopScope::on_pop_invoked` is `Rc<dyn Fn(bool)>`
//!    (`crates/flui-widgets/src/navigator/pop_scope.rs:41`) — it reports
//!    *whether* the route left, never the popped *value*, so there is nothing
//!    for this case's `receivedResult` assertion to bind to.
//! 3. `'pop scope can have Object? generic type while route has stricter
//!    generic type'` — out of scope, the same missing capability as case 2
//!    (the assertion under test is again `receivedResult`); the named-route /
//!    generic-variance angle this case also covers is moot without it.
//! 4. `'toggling canPop on secondary route allows/prevents backs'` — ported,
//!    minus the platform channel and minus `pushNamed` (FLUI's
//!    `NavigatorHandle` has no route-name table; driven directly through
//!    `push`): [`toggling_can_pop_on_a_secondary_route`].
//! 5. `'removing PopScope from the tree removes its effect on navigation'` —
//!    ported, the rebuild-removal path specifically. Popping the *route* a
//!    `PopScope` lives in is a different way for it to disappear, and that
//!    path is already covered by `a_disposed_pop_scope_stops_vetoing`
//!    (`crates/flui-widgets/src/navigator/navigator_tests.rs`); this case is
//!    the one that isn't — the route stays current and only the scope is
//!    conditionally rebuilt out of its subtree:
//!    [`removing_pop_scope_by_rebuild_stops_vetoing_while_the_route_stays_current`].
//! 6. `'identical PopScopes'` — ported:
//!    [`identical_sibling_pop_scopes_register_and_deregister_independently`].
//!    Initially declared unportable on the strength of the oracle's own stale
//!    comment ("has only ever registered one PopScopeInterface"); its
//!    assertions say the opposite and they are the contract. See that test's
//!    doc for what the scene can and cannot distinguish.
//!
//! ## Divergences from the oracle, named
//!
//! - `ModalRoute.popDisposition` and the `SystemNavigator.setFrameworkHandlesBack`
//!   platform channel are both unavailable from FLUI's public surface. Every
//!   case below asserts the identical underlying decision through
//!   [`NavigatorHandle::maybe_pop`]'s return value (handled vs. bubble) and
//!   each [`PopScope::on_pop_invoked`] callback — together these observe
//!   exactly the three-way `RoutePopDisposition::{DoNotPop,Pop,Bubble}` switch
//!   `popDisposition` reads (`history.rs`'s `pop_disposition_of_top`, which is
//!   crate-private but is precisely what `maybe_pop` acts on).
//! - Flutter's `StatefulBuilder` + `setState` toggling `canPop` / a scope's
//!   presence has no FLUI widget equivalent. Each test below rebuilds the
//!   same spot with a small local `StatefulView` that captures a
//!   [`RebuildHandle`] in `init_state` — the handle must be acquired during
//!   state init, not during build — and reads a shared `Cell`
//!   mutated from outside the tree — the Rust-shaped `setState`.
//!
//! Widget → type mapping: `PopScope` → `PopScope` (unchanged name);
//! `ModalRoute`/`Navigator` → [`NavigatorHandle`] + [`Navigator`].

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_widgets::{
    Column, Navigator, NavigatorHandle, PageRoute, PopScope, SimpleRoute, SizedBox,
};

use crate::common::{lay_out, loose};

// ============================================================================
// Fixtures — the Rust-shaped `StatefulBuilder` + `setState`.
// ============================================================================

/// Where a [`ViewState::init_state`] parks the [`RebuildHandle`] it captured,
/// so code outside the tree can schedule that element's rebuild later —
/// Flutter's `StateSetter` (`setState`), minted from a rebuild handle captured
/// during state init instead of a
/// Dart closure.
#[derive(Clone, Default)]
struct RebuildSink(Rc<RefCell<Option<RebuildHandle>>>);

impl RebuildSink {
    fn capture(&self, ctx: &dyn BuildContext) {
        *self.0.borrow_mut() = Some(ctx.rebuild_handle());
    }

    /// Schedule the captured element for rebuild — call, then
    /// [`LaidOut::tick`](crate::common::LaidOut::tick) to actually run it.
    fn schedule(&self) {
        if let Some(handle) = self.0.borrow().as_ref() {
            handle.schedule(RebuildReason::StateChange);
        }
    }
}

/// A `PopScope` whose `can_pop` is read from a shared, externally-mutable
/// `Cell` on every build — Flutter's `PopScope(canPop: canPop, ...)` inside a
/// `StatefulBuilder`.
#[derive(Clone)]
struct CanPopToggle {
    can_pop: Rc<Cell<bool>>,
    sink: RebuildSink,
    outcomes: Rc<RefCell<Vec<bool>>>,
}

impl View for CanPopToggle {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl StatefulView for CanPopToggle {
    type State = CanPopToggleState;

    fn create_state(&self) -> Self::State {
        CanPopToggleState { view: self.clone() }
    }
}

struct CanPopToggleState {
    view: CanPopToggle,
}

impl ViewState<CanPopToggle> for CanPopToggleState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.view.sink.capture(ctx);
    }

    fn build(&self, view: &CanPopToggle, _ctx: &dyn BuildContext) -> impl IntoView {
        let outcomes = Rc::clone(&view.outcomes);
        PopScope::new(SizedBox::new(10.0, 10.0))
            .can_pop(view.can_pop.get())
            .on_pop_invoked(move |did_pop| outcomes.borrow_mut().push(did_pop))
    }
}

/// A route whose subtree either wraps its child in a vetoing `PopScope`, or
/// doesn't, depending on a shared `Cell` — Flutter's `if (!usePopScope) {
/// return child; } return PopScope(canPop: false, child: child);` inside a
/// `StatefulBuilder`.
#[derive(Clone)]
struct ConditionalPopScope {
    use_pop_scope: Rc<Cell<bool>>,
    sink: RebuildSink,
}

impl View for ConditionalPopScope {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl StatefulView for ConditionalPopScope {
    type State = ConditionalPopScopeState;

    fn create_state(&self) -> Self::State {
        ConditionalPopScopeState { view: self.clone() }
    }
}

struct ConditionalPopScopeState {
    view: ConditionalPopScope,
}

impl ViewState<ConditionalPopScope> for ConditionalPopScopeState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.view.sink.capture(ctx);
    }

    fn build(&self, view: &ConditionalPopScope, _ctx: &dyn BuildContext) -> impl IntoView {
        if view.use_pop_scope.get() {
            PopScope::new(SizedBox::new(10.0, 10.0))
                .can_pop(false)
                .into_view()
                .boxed()
        } else {
            SizedBox::new(10.0, 10.0).into_view().boxed()
        }
    }
}

/// Two sibling `PopScope`s, each removable independently — the oracle's
/// `identical PopScopes` scene.
#[derive(Clone)]
struct TwoPopScopes {
    first: Rc<Cell<bool>>,
    second: Rc<Cell<bool>>,
    sink: RebuildSink,
}

impl View for TwoPopScopes {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl StatefulView for TwoPopScopes {
    type State = TwoPopScopesState;

    fn create_state(&self) -> Self::State {
        TwoPopScopesState { view: self.clone() }
    }
}

struct TwoPopScopesState {
    view: TwoPopScopes,
}

impl ViewState<TwoPopScopes> for TwoPopScopesState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.view.sink.capture(ctx);
    }

    fn build(&self, view: &TwoPopScopes, _ctx: &dyn BuildContext) -> impl IntoView {
        let mut children: Vec<BoxedView> = Vec::new();
        if view.first.get() {
            children.push(
                PopScope::new(SizedBox::new(10.0, 10.0))
                    .can_pop(false)
                    .into_view()
                    .boxed(),
            );
        }
        if view.second.get() {
            children.push(
                PopScope::new(SizedBox::new(10.0, 10.0))
                    .can_pop(false)
                    .into_view()
                    .boxed(),
            );
        }
        Column::new(children).into_view().boxed()
    }
}

fn home_page() -> SimpleRoute<i32> {
    SimpleRoute::new(|_ctx| SizedBox::new(10.0, 10.0).into_view().boxed())
}

// ============================================================================
// TESTS
// ============================================================================

/// Toggling a root route's lone `PopScope` between `can_pop = false` and
/// `can_pop = true` flips `maybe_pop` between handled-and-refused
/// (`DoNotPop`, which outranks "only route" `Bubble`) and unhandled
/// (`Bubble`) — without ever touching the route stack.
///
/// Oracle: `'toggling canPop on root route allows/prevents backs'`
/// (pop_scope_test.dart) — `ModalRoute.of(context)!.popDisposition` reads
/// `RoutePopDisposition.doNotPop` while `canPop` is `false`, then
/// `RoutePopDisposition.bubble` once `setState` flips it to `true`. FLUI has
/// no `popDisposition` getter; `maybe_pop`'s handled/bubble return and each
/// `on_pop_invoked` delivery are the same decision, read through the public
/// surface instead.
///
/// Red-check (verified, not merely asserted): temporarily disabled the veto
/// branch in `RouteHistory::pop_disposition_of_top`
/// (`crates/flui-widgets/src/navigator/history.rs:523-525`) — `if false &&
/// top.route.vetoes_pop() { ... }` — so a single vetoing route fell through
/// to the `present.len() == 1 => Bubble` arm regardless of the veto. Ran
/// `cargo nextest run -p flui-widgets --test parity --
/// pop_scope_test::toggling_can_pop_on_the_root_route`: failed on the first
/// assertion, with the exact panic
/// `"a vetoed maybe_pop on a single-route stack is still handled (refused,
/// not bubbled)"` — i.e. `handle.maybe_pop()` evaluated to `false` where the
/// assertion required `true`. Restored the veto-first order; the test passes
/// again.
#[test]
fn toggling_can_pop_on_the_root_route() {
    let can_pop = Rc::new(Cell::new(false));
    let sink = RebuildSink::default();
    let outcomes: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));

    let view = CanPopToggle {
        can_pop: Rc::clone(&can_pop),
        sink: sink.clone(),
        outcomes: Rc::clone(&outcomes),
    };

    let handle = NavigatorHandle::new();
    handle.seed_initial(PageRoute::<i32>::new(move |_ctx, _p, _s| {
        view.clone().into_view().boxed()
    }));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
    let root_id = handle.current().expect("root seeded");

    // can_pop = false: the veto outranks "lone route" bubble — DoNotPop.
    assert!(
        handle.maybe_pop(),
        "a vetoed maybe_pop on a single-route stack is still handled (refused, not bubbled)"
    );
    assert_eq!(
        handle.route_ids(),
        vec![root_id],
        "nothing to pop; the lone route stays, and stays the SAME route"
    );
    assert_eq!(
        outcomes.borrow().as_slice(),
        [false],
        "the veto notifies on_pop_invoked(false)"
    );

    can_pop.set(true);
    sink.schedule();
    laid.tick();

    // can_pop = true, no route below: Bubble — unhandled, no notification.
    assert!(
        !handle.maybe_pop(),
        "no veto and no route beneath it: the request bubbles, unhandled"
    );
    assert_eq!(
        handle.route_ids(),
        vec![root_id],
        "still the same lone route; bubbling pops nothing"
    );
    assert_eq!(
        outcomes.borrow().as_slice(),
        [false],
        "a bubble never reaches on_pop_invoked — the vector is unchanged"
    );
    assert_eq!(
        handle.current(),
        Some(root_id),
        "the route identity never changed across the toggle"
    );
}

/// Toggling a *secondary* route's `PopScope` between `can_pop = true` and
/// `can_pop = false` flips `maybe_pop` between actually popping the route
/// (`Pop`) and refusing while it stays (`DoNotPop`) — the same three-way
/// switch as the root-route case, but now `present.len() > 1` so the
/// unvetoed disposition is `Pop`, not `Bubble`.
///
/// Oracle: `'toggling canPop on secondary route allows/prevents backs'`
/// (pop_scope_test.dart) — pushed via `Navigator.pushNamed('/one')` there;
/// ported here with `NavigatorHandle::push` directly, since FLUI's navigator
/// has no route-name table. The platform-channel
/// (`lastFrameworkHandlesBack`)/Android-only assertions are dropped; the
/// `popDisposition`/`lastPopSuccess` assertions are ported through
/// `maybe_pop`'s return value and `on_pop_invoked`, exactly as in
/// [`toggling_can_pop_on_the_root_route`].
///
/// Red-check (verified, not merely asserted): temporarily made
/// `PopEntryRegistry::any_vetoes` (`crates/flui-widgets/src/navigator/pop_scope.rs:76-81`)
/// always report `false` — `.any(|_entry| false)` in place of the real
/// predicate — so no scope could ever veto. Ran `cargo nextest run -p
/// flui-widgets --test parity -- pop_scope_test::toggling_can_pop_on_a_secondary_route`:
/// failed at the `can_pop = false` phase, with the exact panic
/// ``assertion `left == right` failed: refused: the secondary route stays
/// put\n  left: 1\n right: 2`` (the pop went through instead of being
/// refused). Restored the real predicate; the test
/// passes again.
#[test]
fn toggling_can_pop_on_a_secondary_route() {
    let handle = NavigatorHandle::new();
    handle.seed_initial(home_page());
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
    let home_id = handle.current().expect("home seeded");

    let can_pop = Rc::new(Cell::new(true));
    let sink = RebuildSink::default();
    let outcomes: Rc<RefCell<Vec<bool>>> = Rc::new(RefCell::new(Vec::new()));

    let view = CanPopToggle {
        can_pop: Rc::clone(&can_pop),
        sink: sink.clone(),
        outcomes: Rc::clone(&outcomes),
    };
    let _secondary = handle.push(PageRoute::<i32>::new(move |_ctx, _p, _s| {
        view.clone().into_view().boxed()
    }));
    laid.tick();
    assert_eq!(
        handle.route_ids().len(),
        2,
        "home, then the guarded secondary"
    );

    // can_pop = true (default): maybe_pop is handled AND actually pops.
    assert!(
        handle.maybe_pop(),
        "canPop=true: a secondary route's maybe_pop is handled"
    );
    laid.tick();
    assert_eq!(
        handle.route_ids(),
        vec![home_id],
        "the secondary route actually left"
    );
    assert_eq!(
        outcomes.borrow().as_slice(),
        [true],
        "on_pop_invoked(true): the pop went through"
    );

    // Push a fresh guarded secondary and drive IT to can_pop = false.
    let view_again = CanPopToggle {
        can_pop: Rc::clone(&can_pop),
        sink: sink.clone(),
        outcomes: Rc::clone(&outcomes),
    };
    let _secondary_again = handle.push(PageRoute::<i32>::new(move |_ctx, _p, _s| {
        view_again.clone().into_view().boxed()
    }));
    laid.tick();
    assert_eq!(handle.route_ids().len(), 2);

    can_pop.set(false);
    sink.schedule();
    laid.tick();

    assert!(
        handle.maybe_pop(),
        "canPop=false on a secondary route: still handled — DoNotPop, not Bubble"
    );
    laid.tick();
    assert_eq!(
        handle.route_ids().len(),
        2,
        "refused: the secondary route stays put"
    );
    assert_eq!(
        outcomes.borrow().as_slice(),
        [true, false],
        "on_pop_invoked(false): the refusal was delivered"
    );

    // Toggle back to true: back works again.
    can_pop.set(true);
    sink.schedule();
    laid.tick();

    assert!(handle.maybe_pop(), "canPop=true again: back works");
    laid.tick();
    assert_eq!(
        handle.route_ids(),
        vec![home_id],
        "toggling canPop back to true restores the pop"
    );
    assert_eq!(outcomes.borrow().as_slice(), [true, false, true]);
}

/// Removing a `PopScope` from the tree by a conditional REBUILD — while its
/// route stays the current, never-popped route — lifts its veto exactly as
/// popping it would, but through a different mechanism: `dispose()` runs on
/// the scope's own element teardown, not on the route's.
///
/// This is deliberately the other half of `PopScope` disposal from
/// `a_disposed_pop_scope_stops_vetoing`
/// (`crates/flui-widgets/src/navigator/navigator_tests.rs`), which drives the
/// scope's `dispose()` by popping the *route* it lives in. Here the route
/// never moves — `handle.current()` names the same id before and after — and
/// only the scope's own subtree presence changes.
///
/// Oracle: `'removing PopScope from the tree removes its effect on
/// navigation'` (pop_scope_test.dart) — `ModalRoute.of(context)!.popDisposition`
/// reads `RoutePopDisposition.doNotPop` while `usePopScope` is `true`, then
/// `RoutePopDisposition.bubble` once `setState` flips it to `false` and the
/// `PopScope` leaves the tree. Ported through `maybe_pop`'s return value, as
/// in the other two cases.
///
/// Red-check (verified, not merely asserted): temporarily removed the
/// deregister call from `PopScopeState::dispose`
/// (`crates/flui-widgets/src/navigator/pop_scope.rs:274-279`) — left the
/// method a no-op. Ran `cargo nextest run -p flui-widgets --test parity --
/// pop_scope_test::removing_pop_scope_by_rebuild_stops_vetoing_while_the_route_stays_current`:
/// failed on the post-toggle assertion, with the exact panic `"the PopScope
/// is gone from the tree: no veto, one route left -> bubble"` — i.e.
/// `handle.maybe_pop()` evaluated to `true` where the assertion required
/// `false` (the stale entry kept vetoing after its element was gone).
/// Restored the deregister call; the test passes again.
#[test]
fn removing_pop_scope_by_rebuild_stops_vetoing_while_the_route_stays_current() {
    let use_pop_scope = Rc::new(Cell::new(true));
    let sink = RebuildSink::default();
    let view = ConditionalPopScope {
        use_pop_scope: Rc::clone(&use_pop_scope),
        sink: sink.clone(),
    };

    let handle = NavigatorHandle::new();
    handle.seed_initial(PageRoute::<i32>::new(move |_ctx, _p, _s| {
        view.clone().into_view().boxed()
    }));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
    let root_id = handle.current().expect("root seeded");

    assert!(
        handle.maybe_pop(),
        "the mounted PopScope(can_pop=false) vetoes: handled, refused"
    );
    assert_eq!(handle.route_ids(), vec![root_id]);

    use_pop_scope.set(false);
    sink.schedule();
    laid.tick();

    assert!(
        !handle.maybe_pop(),
        "the PopScope is gone from the tree: no veto, one route left -> bubble"
    );
    assert_eq!(
        handle.route_ids(),
        vec![root_id],
        "the route itself never left the stack"
    );
    assert_eq!(
        handle.current(),
        Some(root_id),
        "same route identity throughout: this is a rebuild removing the \
         scope, never a pop of the route it lived in"
    );
}

// ============================================================================
// Case 6 — 'identical PopScopes'
// ============================================================================

/// Two structurally identical sibling `PopScope`s register, and deregister,
/// independently: removing one leaves the veto standing, removing the second
/// lifts it.
///
/// The oracle's prose says the route "has only ever registered one
/// PopScopeInterface" and that removing one makes it think both are gone. Its
/// **assertions say the opposite**, and the assertions are the contract:
/// `doNotPop` still holds after the first is removed, and only the second
/// removal yields `bubble`. That is two independent registrations, which is
/// exactly what FLUI does — Dart's `const` canonicalization gives the two
/// slots one *widget* value, but each slot still mounts its own element and
/// state, so each registers its own entry.
///
/// This case was initially declared unportable on the strength of that stale
/// comment; reading the assertions instead of the prose is what makes it
/// portable.
///
/// **What it does NOT prove, despite the tempting reading.** It does not
/// distinguish two independent registrations from a single collapsed one.
/// Verified by simulating the collapse — making `PopEntryRegistry::register`
/// ignore a second entry — and watching this test still pass. The reason is
/// keyless reconciliation: removing the first child makes the surviving
/// element the one at index 0, which under collapse is precisely the element
/// that registered, so a veto survives either way. The oracle's own scene has
/// the same blind spot, which is probably how its comment came to contradict
/// its assertions. What this pins is the observable sequence — vetoed, still
/// vetoed after one removal, bubbling after both — and nothing more.
#[test]
fn identical_sibling_pop_scopes_register_and_deregister_independently() {
    let first = Rc::new(Cell::new(true));
    let second = Rc::new(Cell::new(true));
    let sink = RebuildSink::default();
    let view = TwoPopScopes {
        first: Rc::clone(&first),
        second: Rc::clone(&second),
        sink: sink.clone(),
    };

    let handle = NavigatorHandle::new();
    handle.seed_initial(PageRoute::<i32>::new(move |_ctx, _p, _s| {
        view.clone().into_view().boxed()
    }));
    let mut laid = lay_out(Navigator::new(handle.clone()), loose(400.0));
    let root_id = handle.current().expect("root seeded");

    assert!(handle.maybe_pop(), "two vetoing scopes: handled, refused");

    first.set(false);
    sink.schedule();
    laid.tick();
    assert!(
        handle.maybe_pop(),
        "removing ONE identical scope must leave the other's veto standing — \
         if the two collapsed into a single registration this would bubble",
    );
    assert_eq!(handle.route_ids(), vec![root_id]);

    second.set(false);
    sink.schedule();
    laid.tick();
    assert!(
        !handle.maybe_pop(),
        "with both removed the veto is gone and the pop bubbles",
    );
    assert_eq!(
        handle.route_ids(),
        vec![root_id],
        "the route itself never left the stack",
    );
}
