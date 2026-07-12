//! Tests for the private `TransitionRoute`.
//!
//! # Parity oracles
//!
//! `.flutter/packages/flutter/test/widgets/routes_test.dart` —
//! `'secondary animation is kDismissed when next route finishes pop'`,
//! `'secondary animation is kDismissed when next route is removed'`,
//! `'secondary animation is kDismissed after train hopping finishes and pop'`,
//! `'secondary animation is kDismissed when train hopping is interrupted'`.
//! Expected values are read from `routes.dart`, not from running this code.
//!
//! FLUI's `AnimationController` exposes no `TickerFuture`, so these drive the
//! transition by hand with `set_value` — which is deterministic, and is what makes
//! `_handleStatusChanged`'s four arms individually testable.

use std::sync::Arc;
use std::time::Duration;

use flui_animation::{Animation, AnimationStatus, Vsync};
use flui_view::prelude::*;
use parking_lot::Mutex;

use super::lifecycle::RouteLifecycle;
use super::navigator::{Navigator, NavigatorHandle};
use super::transition_route::{TransitionHandle, TransitionRoute};
use crate::SizedBox;
use crate::animated::VsyncScope;
use crate::test_harness::{Harness, mount};

// ============================================================================
// HELPERS
// ============================================================================

const DURATION: Duration = Duration::from_millis(300);

/// A leaf-content transition route, plus a handle to drive its controller.
fn transition(name: &'static str) -> (TransitionRoute<i32>, TransitionHandle) {
    let route = TransitionRoute::<i32>::new(DURATION, move |_ctx| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    })
    .named(name);
    let handle = route.handle();
    (route, handle)
}

/// A navigator seeded with a plain first route.
fn navigator() -> (NavigatorHandle, Harness) {
    let handle = NavigatorHandle::new();
    handle.seed_initial(super::overlay_route::SimpleRoute::<i32>::new(|_ctx| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }));
    let harness = mount(Navigator::new(handle.clone()));
    (handle, harness)
}

/// Drive a controller to `Completed` (entrance finished).
fn complete(handle: &TransitionHandle) {
    let controller = handle.controller().expect("install created the controller");
    controller.set_value(1.0);
    assert_eq!(controller.status(), AnimationStatus::Completed);
    handle.drain_pending_statuses();
}

/// Drive a controller to `Dismissed` (exit finished).
fn dismiss(handle: &TransitionHandle) {
    let controller = handle.controller().expect("install created the controller");
    controller.set_value(0.0);
    assert_eq!(controller.status(), AnimationStatus::Dismissed);
    handle.drain_pending_statuses();
}

// ============================================================================
// LIFECYCLE
// ============================================================================

/// `didPush` drives the controller forward and the entry parks in `Pushing` until
/// the controller reports `Completed` (`routes.dart:336-350`; the navigator side
/// is `navigator.dart:3274-3290`).
///
/// Red-check: return `PushCompletion::Immediate` from `TransitionRoute::did_push`.
#[test]
fn push_transition_parks_the_entry_in_pushing_until_the_controller_completes() {
    let (navigator_handle, mut harness) = navigator();
    let (route, animation) = transition("second");
    let top = {
        let _result = navigator_handle.push(route);
        navigator_handle.current().expect("a top route")
    };
    harness.tick();

    assert_eq!(
        navigator_handle.route_state(top),
        Some(RouteLifecycle::Pushing),
        "the entrance transition is still running"
    );
    assert_eq!(navigator_handle.route_ids().len(), 2);

    complete(&animation);
    harness.tick();

    assert_eq!(
        navigator_handle.route_state(top),
        Some(RouteLifecycle::Idle),
        "the completed transition settled the entry"
    );
}

/// The `Completed` status reaches the navigator through the route-binding command queue,
/// never a direct call — a direct call would deadlock on the history mutex.
///
/// Red-check: make `TransitionInner::handle_status_changed` skip
/// `binding.notify_push_completed()`; the entry is stranded in `Pushing`.
#[test]
fn push_completion_travels_through_the_route_binding_command_queue() {
    let (navigator_handle, mut harness) = navigator();
    let (route, animation) = transition("second");
    navigator_handle.push(route);
    let top = navigator_handle.current().expect("a top route");
    harness.tick();
    assert_eq!(
        navigator_handle.route_state(top),
        Some(RouteLifecycle::Pushing)
    );

    // Completing OUTSIDE a flush: `wake`'s `try_lock` succeeds and settles now.
    complete(&animation);

    assert_eq!(
        navigator_handle.route_state(top),
        Some(RouteLifecycle::Idle),
        "the command applied without a further frame"
    );
}

/// A popped route stays in the history **and** in the overlay until its exit
/// transition reaches `dismissed`, because `finishedWhenPopped` is false while the
/// controller runs (`routes.dart:177-178`). Its `RouteResult` resolves **at pop
/// time**, not at disposal (`navigator.dart:458-482`).
///
/// Red-check: make `finished_when_popped` return `true`; the route is disposed
/// during the pop's own flush.
#[test]
fn pop_transition_keeps_the_route_until_dismissed_but_completes_its_result_at_once() {
    let (navigator_handle, mut harness) = navigator();
    let (route, animation) = transition("second");
    let result = navigator_handle.push(route);
    let top = navigator_handle.current().expect("a top route");
    complete(&animation);
    harness.tick();
    assert_eq!(navigator_handle.overlay().len(), 2);

    assert!(navigator_handle.pop_with(9_i32));
    harness.tick();

    assert_eq!(
        result.try_take(),
        Some(Some(9)),
        "the result resolves at pop time, not at disposal"
    );
    assert_eq!(
        navigator_handle.route_state(top),
        Some(RouteLifecycle::Popping),
        "still popping: the exit transition has not finished"
    );
    assert_eq!(navigator_handle.route_ids().len(), 2);
    assert_eq!(
        navigator_handle.overlay().len(),
        2,
        "and its overlay entry is still shown"
    );

    dismiss(&animation);
    harness.tick();

    assert_eq!(navigator_handle.route_ids().len(), 1, "now disposed");
    assert_eq!(navigator_handle.overlay().len(), 1);
    assert_eq!(navigator_handle.tracked_entry_count(), 1);
}

/// A controller that is **already dismissed** when the route is popped finalizes
/// synchronously — `finishedWhenPopped` is `isDismissed && !_popFinalized`, so
/// `OverlayRoute.didPop` finalizes on the spot (`routes.dart:87-94`, `:173-178`).
/// The Cupertino dismiss gesture is the reason this path exists.
///
/// **What this does *not* test.** Dropping the `&& !_popFinalized` term from
/// `finished_when_popped` leaves every test green: FLUI consults it exactly once,
/// in `handle_pop`, and a second `finalize` is a no-op because a `RouteCommand`
/// for a vanished route is dropped. Flutter needs the term because
/// `finalizeRoute` *asserts* `currentState == popping`. The guard is kept because
/// it is faithful and free, and the thing it actually protects — the listener
/// raising `finalize()` twice — is pinned directly by
/// `pop_finalized_stops_a_second_finalize`.
///
/// **A second untested-by-design note.** FLUI's `AnimationController::reverse()`
/// on an already-dismissed controller re-emits `Dismissed` *synchronously*, so the
/// status listener finalizes first and sets `_popFinalized`; `finished_when_popped`
/// then reads `false` and the synchronous branch is never taken. Dart's controller
/// does not notify from `reverse()`, so Flutter *does* take it. The end state is
/// identical — disposed within the pop's own `flush` — and the divergence is
/// recorded, not hidden.
///
/// Red-check: make the `Dismissed` arm of `handle_status_changed` skip
/// `binding.finalize()`; the route never disposes.
#[test]
fn an_already_dismissed_controller_finalizes_synchronously_without_double_finalize() {
    let (navigator_handle, mut harness) = navigator();
    let (route, animation) = transition("second");
    navigator_handle.push(route);
    let top = navigator_handle.current().expect("a top route");
    complete(&animation);
    harness.tick();

    // Drive it to dismissed *while still active*: no finalize (see the next test),
    // and `_popFinalized` stays false.
    dismiss(&animation);
    assert!(!animation.is_pop_finalized());
    assert_eq!(navigator_handle.route_ids().len(), 2);

    // Now pop. `finished_when_popped` is true, so the entry disposes at once.
    assert!(navigator_handle.pop());
    harness.tick();

    assert_eq!(navigator_handle.route_state(top), None, "disposed at once");
    assert_eq!(navigator_handle.route_ids().len(), 1);
    assert_eq!(navigator_handle.overlay().len(), 1);
}

/// The status listener raises `finalize()` **once**, however many times the
/// controller re-enters `dismissed`. Flutter's `_popFinalized` (`routes.dart:180`,
/// set at `:316`).
///
/// Tested directly, because through the navigator a second `finalize` is
/// invisible: the command queue drops a `RouteCommand` naming a vanished route. Flutter's
/// `finalizeRoute` asserts instead. Same posture as ADR-0018's `apply_fold`.
///
/// Red-check: replace `pop_finalized.swap(true, …)` with a plain `load`.
#[test]
fn pop_finalized_stops_a_second_finalize() {
    let (navigator_handle, mut harness) = navigator();
    let (route, animation) = transition("second");
    navigator_handle.push(route);
    complete(&animation);
    harness.tick();

    navigator_handle.pop();
    let controller = animation.controller().expect("installed");

    dismiss(&animation);
    assert_eq!(animation.finalize_calls(), 1);
    assert!(animation.is_pop_finalized());

    // Re-enter `dismissed` from a running state. The route is gone from the
    // history by now, but its controller is still ours to drive.
    controller.set_value(0.5);
    controller.set_value(0.0);
    assert_eq!(controller.status(), AnimationStatus::Dismissed);
    animation.drain_pending_statuses();

    assert_eq!(
        animation.finalize_calls(),
        1,
        "_popFinalized must stop the second finalize"
    );
}

/// `dismissed` while the route is **still active** must not finalize:
/// "We might still be an active route if a subclass is controlling the
/// transition" (`routes.dart:310-313`). Naively finalizing on every `dismissed`
/// destroys a live route.
///
/// Red-check: remove the `!self.is_active()` guard from
/// `TransitionInner::handle_status_changed`.
#[test]
fn dismissed_while_still_active_does_not_finalize() {
    let (navigator_handle, mut harness) = navigator();
    let (route, animation) = transition("second");
    navigator_handle.push(route);
    let top = navigator_handle.current().expect("a top route");
    complete(&animation);
    harness.tick();

    // The route is still active — nothing was popped.
    dismiss(&animation);
    harness.tick();

    assert_eq!(
        navigator_handle.route_state(top),
        Some(RouteLifecycle::Idle),
        "an active route survives a dismissed controller"
    );
    assert_eq!(navigator_handle.route_ids().len(), 2);
    assert_eq!(navigator_handle.overlay().len(), 2);
    assert!(!animation.is_pop_finalized());
}

/// `install()` creates the controller, and `dispose()` disposes it and unregisters
/// it from the navigator's clock (`routes.dart:323-334`, `:627-638`).
///
/// `VsyncRegistration` has no `Drop`, so a missed unregister leaves a disposed
/// route's controller registered and ticking forever.
///
/// Red-check: delete the `vsync.unregister(registration)` call in `dispose`; the
/// registry still holds the controller after the route is gone.
#[test]
fn dispose_unregisters_the_controller_from_the_navigators_clock() {
    let vsync = Vsync::new();
    let navigator_handle = NavigatorHandle::new();
    navigator_handle.seed_initial(super::overlay_route::SimpleRoute::<i32>::new(|_ctx| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }));

    let before = vsync.len();
    let mut harness = mount(VsyncScope::new(
        vsync.clone(),
        Navigator::new(navigator_handle.clone()),
    ));

    let (route, animation) = transition("second");
    navigator_handle.push(route);
    harness.tick();

    assert_eq!(
        vsync.len(),
        before + 1,
        "the route registered its controller with the navigator's clock"
    );

    complete(&animation);
    harness.tick();
    navigator_handle.pop();
    dismiss(&animation);
    harness.tick();

    assert_eq!(navigator_handle.route_ids().len(), 1, "the route disposed");
    assert_eq!(
        vsync.len(),
        before,
        "and unregistered its controller — VsyncRegistration has no Drop"
    );
}

// ============================================================================
// SECONDARY ANIMATION
// ============================================================================

/// `_updateSecondaryAnimation` points a route's `secondaryAnimation` at the **next**
/// route's primary animation (`routes.dart:429-437`, `:487-489`), so the lower
/// route can animate out as the upper animates in.
///
/// Red-check: make `did_change_next` a no-op; the proxy stays at always-dismissed.
#[test]
fn secondary_animation_tracks_the_next_routes_primary_animation() {
    let (navigator_handle, mut harness) = navigator();

    let (lower, lower_animation) = transition("lower");
    navigator_handle.push(lower);
    complete(&lower_animation);
    harness.tick();
    assert!(
        lower_animation.secondary_is_dismissed(),
        "nothing above it yet"
    );

    let (upper, upper_animation) = transition("upper");
    navigator_handle.push(upper);
    harness.tick();

    assert!(!lower_animation.secondary_is_dismissed());
    let secondary = lower_animation.secondary_animation();
    let upper_controller = upper_animation.controller().expect("installed");

    upper_controller.set_value(0.25);
    assert!(
        (secondary.value() - 0.25).abs() < 1e-5,
        "the lower route's secondary tracks the upper route's primary: {}",
        secondary.value()
    );
    upper_controller.set_value(0.75);
    assert!((secondary.value() - 0.75).abs() < 1e-5);
}

/// `canTransitionTo` / `canTransitionFrom` gate the coordination
/// (`routes.dart:429-431`). When either is false the proxy is left at
/// `kAlwaysDismissedAnimation` (`:491`).
///
/// Red-check: drop either predicate from the filter in `update_secondary_animation`.
#[test]
fn can_transition_predicates_leave_the_secondary_at_always_dismissed() {
    // The lower route refuses to transition *to* anything.
    let (navigator_handle, mut harness) = navigator();
    let lower = TransitionRoute::<i32>::new(DURATION, |_ctx| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    })
    .can_transition_to(false);
    let lower_animation = lower.handle();
    navigator_handle.push(lower);
    complete(&lower_animation);
    harness.tick();

    let (upper, upper_animation) = transition("upper");
    navigator_handle.push(upper);
    harness.tick();

    assert!(
        lower_animation.secondary_is_dismissed(),
        "canTransitionTo == false ⇒ always-dismissed"
    );
    assert_eq!(lower_animation.secondary_animation().value(), 0.0);
    let _ = upper_animation;

    // And the mirror: the upper route refuses to transition *from* the lower.
    let (navigator_handle, mut harness) = navigator();
    let (lower, lower_animation) = transition("lower");
    navigator_handle.push(lower);
    complete(&lower_animation);
    harness.tick();

    let upper = TransitionRoute::<i32>::new(DURATION, |_ctx| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    })
    .can_transition_from(false);
    navigator_handle.push(upper);
    harness.tick();

    assert!(
        lower_animation.secondary_is_dismissed(),
        "next.canTransitionFrom == false ⇒ always-dismissed"
    );
}

/// A route that is **not** a `TransitionRoute` has no `TransitionPeer`, which is
/// FLUI's spelling of Flutter's `nextRoute is TransitionRoute` (`routes.dart:429`).
///
/// Red-check: have `update_secondary_animation` fall back to the route's own
/// animation when the peer is missing.
#[test]
fn a_plain_route_above_leaves_the_secondary_at_always_dismissed() {
    let (navigator_handle, mut harness) = navigator();
    let (lower, lower_animation) = transition("lower");
    navigator_handle.push(lower);
    complete(&lower_animation);
    harness.tick();

    navigator_handle.push(super::overlay_route::SimpleRoute::<i32>::new(|_ctx| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }));
    harness.tick();

    assert!(lower_animation.secondary_is_dismissed());
}

/// When the route above is popped and disposed, the flush announces
/// `didChangeNext(null)` and the proxy resets to always-dismissed.
///
/// This is why FLUI needs no `completed` future: Flutter uses `nextRoute.completed`
/// to reset the proxy (`routes.dart:503-509`), but the announcement already does.
/// Oracle: `'secondary animation is kDismissed when next route finishes pop'`.
///
/// Red-check: make `update_secondary_animation` return early on `next == None`.
#[test]
fn secondary_animation_resets_when_the_next_route_is_popped() {
    let (navigator_handle, mut harness) = navigator();
    let (lower, lower_animation) = transition("lower");
    navigator_handle.push(lower);
    complete(&lower_animation);
    harness.tick();

    let (upper, upper_animation) = transition("upper");
    navigator_handle.push(upper);
    complete(&upper_animation);
    harness.tick();
    assert!(!lower_animation.secondary_is_dismissed());

    navigator_handle.pop();
    dismiss(&upper_animation);
    harness.tick();

    assert_eq!(navigator_handle.route_ids().len(), 2);
    assert!(
        lower_animation.secondary_is_dismissed(),
        "the popped route's animation must not be retained"
    );
    assert_eq!(lower_animation.secondary_animation().value(), 0.0);
}

// ============================================================================
// TRAIN HOPPING
// ============================================================================

/// When the outgoing and incoming animations sit at **different** values and the
/// incoming one is moving, the proxy cannot snap: an `AnimationSwitch` (FLUI's
/// `TrainHoppingAnimation`) proxies the old train until the two cross, then hops
/// (`routes.dart:440-486`).
///
/// `on_switched` fires exactly once — pinned in `flui-animation` by
/// `on_switched_fires_exactly_once`, this suite's preflight.
///
/// Red-check: always take the `jump` branch in `update_secondary_animation`; the
/// secondary snaps to the new train immediately and `secondary_is_hopping` is false.
#[test]
fn train_hopping_proxies_the_old_train_until_the_two_cross() {
    let (navigator_handle, mut harness) = navigator();

    let (bottom, bottom_animation) = transition("bottom");
    navigator_handle.push(bottom);
    complete(&bottom_animation);
    harness.tick();

    // `middle` rises to 0.8; `bottom`'s secondary follows it directly.
    let (middle, middle_animation) = transition("middle");
    navigator_handle.push(middle);
    harness.tick();
    let middle_controller = middle_animation.controller().expect("installed");
    middle_controller.set_value(0.8);
    let secondary = bottom_animation.secondary_animation();
    assert!((secondary.value() - 0.8).abs() < 1e-5);

    // Remove `middle` so `top` becomes `bottom`'s next while `top` is moving at a
    // *different* value: the trains must be hopped, not snapped.
    let (top, top_animation) = transition("top");
    navigator_handle.push(top);
    harness.tick();
    let top_controller = top_animation.controller().expect("installed");
    top_controller.set_value(0.2);

    // `middle` is still bottom's next (top sits above middle), so rewire bottom by
    // removing middle from the stack.
    let middle_id = navigator_handle.route_ids()[2];
    navigator_handle.remove_route(middle_id);
    harness.tick();

    assert!(
        bottom_animation.secondary_is_hopping(),
        "trains at 0.8 and 0.2, target moving ⇒ hop, not snap"
    );
    // The hopper proxies the OLD train's value until they cross.
    assert!(
        (secondary.value() - middle_controller.value()).abs() < 1e-5,
        "the hopper still reports the old train: {}",
        secondary.value()
    );

    // Drive the old train down past the new one: `maximize` mode hops when
    // `next >= current`.
    middle_controller.set_value(0.1);

    assert!(
        !bottom_animation.secondary_is_hopping() || (secondary.value() - 0.2).abs() < 1e-5,
        "after the hop the proxy reports the target train: {}",
        secondary.value()
    );

    let _ = top_animation;
}

/// A second `_updateSecondaryAnimation` arriving **mid-hop** must (a) install the
/// new parent before disposing the old hopper (`routes.dart:495`), and (b) leave
/// the *stale* route's `completed` unable to clobber the newer parent — Flutter's
/// `if (_secondaryAnimation.parent == animation)` guard (`routes.dart:503`).
///
/// Oracle: `'secondary animation is kDismissed when train hopping is interrupted'`.
///
/// Note the shape: the **top** route must be `Idle` (its transition complete),
/// because a `Pushing` route above keeps `can_remove_or_add` false and nothing
/// beneath it is ever disposed — so `completed` would never fire and the guard
/// would go untested. That is how the first draft of this test fooled itself.
///
/// Red-check: delete the `if !still_ours { return; }` guard in the `on_completed`
/// callback — the disposed `middle` resets a proxy that has already moved on.
#[test]
fn a_stale_train_does_not_clobber_a_newer_parent() {
    let (navigator_handle, mut harness) = navigator();

    let (bottom, bottom_animation) = transition("bottom");
    navigator_handle.push(bottom);
    complete(&bottom_animation);
    harness.tick();

    // `low` is bottom's current next, sitting at 0.8 and moving.
    let (low, low_animation) = transition("low");
    navigator_handle.push(low);
    harness.tick();
    low_animation
        .controller()
        .expect("installed")
        .set_value(0.8);

    // `middle` is moving at 0.2 — the hop target once `low` goes away.
    let (middle, middle_animation) = transition("middle");
    navigator_handle.push(middle);
    harness.tick();
    let middle_controller = middle_animation.controller().expect("installed");
    middle_controller.set_value(0.2);
    assert!(middle_controller.is_animating());

    // `top` is settled, so `can_remove_or_add` is true and removals actually
    // dispose. Without this, nothing below is ever disposed.
    let (top, top_animation) = transition("top");
    navigator_handle.push(top);
    complete(&top_animation);
    harness.tick();

    let ids = navigator_handle.route_ids();
    let (low_id, middle_id) = (ids[2], ids[3]);

    // Remove `low`: bottom's next becomes `middle` (0.8 vs 0.2, moving) ⇒ hop.
    navigator_handle.remove_route(low_id);
    harness.tick();
    assert!(bottom_animation.secondary_is_hopping(), "first rewire hops");

    // Interrupt the hop: remove `middle`, so bottom's next becomes the settled
    // `top` (value 1.0, not animating) ⇒ a jump, and the old hopper is replaced.
    navigator_handle.remove_route(middle_id);
    harness.tick();

    assert!(
        !bottom_animation.secondary_is_hopping(),
        "the interrupted hop was replaced by a direct parent"
    );
    assert!(
        !bottom_animation.secondary_is_dismissed(),
        "a stale route's `completed` must not clobber the newer parent"
    );

    // And the proxy really follows `top`, not a disposed train.
    let secondary = bottom_animation.secondary_animation();
    top_animation
        .controller()
        .expect("installed")
        .set_value(0.65);
    assert!(
        (secondary.value() - 0.65).abs() < 1e-5,
        "the secondary follows the newer parent: {}",
        secondary.value()
    );
}

/// When the route above is popped, `didPopNext` hands this route the **popped**
/// route (`navigator.dart:3312`, `routes.dart:393-402`), so the secondary keeps
/// tracking it while it reverses away.
///
/// **Untested claim, stated rather than implied.** `update_secondary_animation`
/// early-returns when the proxy already points at that route, and `did_change_next`
/// had already wired it — so `did_pop_next` is idempotent here, and making it a
/// no-op leaves every test green. It is implemented because it is faithful and
/// because the early-return is an optimisation, not a contract. What *does* the
/// releasing is `completed` — pinned by
/// `secondary_animation_resets_when_the_next_route_is_popped`.
#[test]
fn secondary_keeps_tracking_the_exiting_route_while_it_reverses() {
    let (navigator_handle, mut harness) = navigator();
    let (lower, lower_animation) = transition("lower");
    navigator_handle.push(lower);
    complete(&lower_animation);
    harness.tick();

    let (upper, upper_animation) = transition("upper");
    navigator_handle.push(upper);
    complete(&upper_animation);
    harness.tick();

    let secondary = lower_animation.secondary_animation();
    assert!((secondary.value() - 1.0).abs() < 1e-5);

    navigator_handle.pop();
    let upper_controller = upper_animation.controller().expect("still installed");
    upper_controller.set_value(0.4);

    assert!(
        (secondary.value() - 0.4).abs() < 1e-5,
        "the lower route animates back as the upper one reverses: {}",
        secondary.value()
    );
}

// ============================================================================
// PRIVACY
// ============================================================================

/// `TransitionRoute` and `ModalRoute` stay private after the sign-off gate:
/// Rust has no subclassing, so exporting them as extensible bases needs a trait
/// design that is deliberately deferred. Only `PageRoute` / `PopupRoute`
/// came out.
///
/// Red-check: add `pub use transition_route::TransitionRoute;` to
/// `navigator/mod.rs`.
#[test]
fn transition_route_is_not_exported() {
    const LIB: &str = include_str!("../lib.rs");
    const NAV_MOD: &str = include_str!("mod.rs");

    const INTERNAL: [&str; 5] = [
        "TransitionRoute",
        "TransitionHandle",
        "TransitionPeer",
        "TransitionGroup",
        "ModalRoute",
    ];

    super::export_guard::assert_not_exported("lib.rs", LIB, &INTERNAL);
    super::export_guard::assert_not_exported("navigator/mod.rs", NAV_MOD, &INTERNAL);
}

/// The secondary proxy is shared, and the route drives it from a status listener,
/// so a `TransitionHandle` outliving its route must not resurrect anything.
#[test]
fn a_handle_outliving_its_route_is_inert() {
    let (navigator_handle, mut harness) = navigator();
    let (route, animation) = transition("second");
    navigator_handle.push(route);
    complete(&animation);
    harness.tick();

    navigator_handle.pop();
    dismiss(&animation);
    harness.tick();
    assert_eq!(navigator_handle.route_ids().len(), 1);

    // The route is disposed; its controller is gone.
    assert!(
        animation.controller().is_none(),
        "dispose() took the controller"
    );
    assert!(animation.secondary_is_dismissed());
}

/// The `Mutex` in `TransitionInner` must never be held across a `RouteBinding`
/// call: `binding.finalize()` runs `wake`, which `try_lock`s the history. A
/// deadlock here would be a hang, not a failure.
///
/// This is a smoke test rather than an assertion — it completes, or nextest's
/// timeout catches it.
#[test]
fn status_listener_does_not_hold_a_lock_across_the_binding_call() {
    let (navigator_handle, mut harness) = navigator();
    let (route, animation) = transition("second");
    navigator_handle.push(route);
    complete(&animation);
    harness.tick();
    navigator_handle.pop();
    dismiss(&animation);
    harness.tick();
    assert_eq!(navigator_handle.route_ids().len(), 1);
    let _ = Arc::new(Mutex::new(()));
}
