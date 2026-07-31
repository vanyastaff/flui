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
//! 6. `'identical PopScopes'` — not portable. The assertion is about Dart
//!    `const`-widget canonicalization: two `const PopScope(canPop: false,
//!    child: Text('hello'))` literals with identical arguments are the *same*
//!    constant object, so `ModalRoute` only ever sees one registered
//!    `PopEntry`, and removing either sibling from the tree leaves that one
//!    entry (and the veto) in place — the test's whole point is that
//!    `usePopScope1 = false` does *not* lift the veto, because the surviving
//!    `usePopScope2` scope is secretly the same object. FLUI views are
//!    per-call values with no `const` canonicalization step; two sibling
//!    `PopScope`s here are always two distinct `PopEntry` registrations, so
//!    removing one always *does* lift its half of the veto (only the
//!    remaining one keeps vetoing, for the ordinary reason that it's still
//!    mounted). Porting the oracle's assertion verbatim would require FLUI to
//!    secretly merge the two scopes into one registration — a divergence this
//!    port would be inventing, not one that exists. Nothing here is a gap in
//!    FLUI's `PopScope`; it's a widget-identity model Dart's `const` has that
//!    Rust values don't.
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
//!   [`RebuildHandle`] in `init_state` (ADR-0018) and reads a shared `Cell`
//!   mutated from outside the tree — the Rust-shaped `setState`.
//!
//! Widget → type mapping: `PopScope` → `PopScope` (unchanged name);
//! `ModalRoute`/`Navigator` → [`NavigatorHandle`] + [`Navigator`].

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_widgets::{Navigator, NavigatorHandle, PageRoute, PopScope, SimpleRoute, SizedBox};

use crate::common::{lay_out, loose};

// ============================================================================
// Fixtures — the Rust-shaped `StatefulBuilder` + `setState`.
// ============================================================================

/// Where a [`ViewState::init_state`] parks the [`RebuildHandle`] it captured,
/// so code outside the tree can schedule that element's rebuild later —
/// Flutter's `StateSetter` (`setState`), minted through ADR-0018 instead of a
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
