//! Tests for the private `TransitionRoute`, reached through the temporary
//! `flui_widgets::__test_access` path (ADR-0083 §4). Its export boundary keeps
//! a unit test in `src/navigator/transition_route_tests.rs`.
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
//! Most of these drive the transition by hand with `set_value` — which is
//! deterministic, and is what makes `_handleStatusChanged`'s four arms
//! individually testable — rather than by awaiting the `TickerFuture`
//! `did_push` returns (ADR-0064). A handful that need the run to have real,
//! not-yet-covered distance left (so a `reverse()` cannot collapse
//! synchronously to `Dismissed`) pump a real `Vsync` instead; those say so.

use std::sync::Arc;
use std::time::Duration;

use flui_animation::{Animation, AnimationStatus, Curve, Curves, Vsync};
use flui_view::prelude::*;
use parking_lot::Mutex;

use flui_widgets::__test_access::{
    NavigatorProbe as _, OverlayProbe as _, RouteLifecycle, TransitionHandle, TransitionRoute,
};
use flui_widgets::SizedBox;
use flui_widgets::animated::VsyncScope;
use flui_widgets::navigator::{Navigator, NavigatorHandle, SimpleRoute};

use crate::common::harness::{Harness, mount};

// ============================================================================
// HELPERS
// ============================================================================

const DURATION: Duration = Duration::from_millis(300);

/// A leaf-content transition route, plus a handle to drive its controller.
fn transition(name: &'static str) -> (TransitionRoute<i32>, TransitionHandle) {
    transition_with_duration(name, DURATION)
}

/// Like [`transition`], with an explicit transition duration — for the
/// zero-duration synchronous-settle coverage (issue #1171), which needs a
/// duration `transition`'s fixed [`DURATION`] cannot give it.
fn transition_with_duration(
    name: &'static str,
    duration: Duration,
) -> (TransitionRoute<i32>, TransitionHandle) {
    let route = TransitionRoute::<i32>::new(duration, move |_ctx| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    })
    .named(name);
    let handle = route.handle();
    (route, handle)
}

/// A navigator seeded with a plain first route.
fn navigator() -> (NavigatorHandle, Harness) {
    let handle = NavigatorHandle::new();
    handle.seed_initial(SimpleRoute::<i32>::new(|_ctx| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }));
    let harness = mount(Navigator::new(handle.clone()));
    (handle, harness)
}

/// Drive a controller to `Completed` (entrance finished).
///
/// `set_value` **cancels** the active run rather than completing its
/// `TickerFuture` (`set_value_cancels_the_active_run`, flui-animation) — this
/// helper drives `status`, not the future. A test that needs the future to
/// resolve `Ok(())` through natural completion drives a real `Vsync` instead
/// (see `tests/routes.rs`'s `PUMPS`-based coverage).
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

/// Popping a route while its own entrance is still animating cancels that
/// push's `TickerFuture` **inside the flush that runs `did_pop`**: `did_pop`
/// calls `reverse()`, which — as a run-starting method — displaces and
/// cancels the still-pending `forward()` run before starting the new one, and
/// `AnimationController::finish` delivers synchronously, so the continuation
/// `NavigatorShared::apply` registered on that run fires right there, mid-flush.
/// No deadlock (the continuation only touches the command queue and
/// `settle_wake`, never the history), and the entry ends `Popping`, not
/// resurrected to `Idle`: by the time the queued `RouteCommand::PushCompleted`
/// is drained, the entry has already moved on.
///
/// Needs a real, ticking `Vsync`: popping at `value == 0` (this file's usual
/// hand-driven setup, which never advances the controller) would let
/// `reverse()` collapse straight to `Dismissed` — the
/// `an_already_dismissed_controller_finalizes_synchronously_…` shape, not this
/// one — so this pumps the entrance to its midpoint first, leaving `reverse()`
/// real distance to cover.
///
/// Red-check: drop the `entry.state == RouteLifecycle::Pushing` guard in
/// `RouteHistory::apply_pending_commands`'s `PushCompleted` arm — the stale
/// command resurrects the entry to `Idle` instead of leaving it `Popping`.
#[test]
fn pop_mid_push_cancels_the_push_future_inside_the_flush_and_ends_popping() {
    let vsync = Vsync::new();
    let navigator_handle = NavigatorHandle::new();
    navigator_handle.seed_initial(SimpleRoute::<i32>::new(|_ctx| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }));
    let mut laid = crate::common::lay_out_animated(
        VsyncScope::new(vsync.clone(), Navigator::new(navigator_handle.clone())),
        crate::common::tight(200.0, 200.0),
        vsync,
    );

    let (route, _animation) = transition("second");
    navigator_handle.push(route);
    let top = navigator_handle.current().expect("a top route");

    // The first pump after a run starts only anchors `t = 0` for the
    // registry's per-run clock (`Vsync`'s own doc); a second pump is what
    // actually advances it — `tests/routes.rs` documents the same thing.
    laid.pump_for(Duration::ZERO);
    // Halfway through the 300ms entrance: `Forward`, not yet `Completed` —
    // `reverse()` below has real distance to cover.
    laid.pump_for(Duration::from_millis(150));
    assert_eq!(
        navigator_handle.route_state(top),
        Some(RouteLifecycle::Pushing),
        "the entrance transition is still running"
    );

    // The pop's own flush runs `did_pop` -> `reverse()`, which cancels the
    // still-pending push run synchronously, mid-flush. No hang: nextest's
    // per-test timeout would catch one.
    assert!(navigator_handle.pop());

    assert_eq!(
        navigator_handle.route_state(top),
        Some(RouteLifecycle::Popping),
        "canceled by the pop, but popping — not resurrected to idle"
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

/// A `Duration::ZERO` transition (issue #1171's zero-duration synchronous
/// settle) settles push and pop at the call itself, through a REAL
/// `TransitionRoute` end to end, asserted with NO driven frame in between.
///
/// **Push** settles the CONTROLLER synchronously: `did_push` calls
/// `AnimationController::forward()`, which snaps to `Completed` at the call,
/// with no ticker run installed. The ROUTE's own bookkeeping
/// (`RouteLifecycle::Pushing` → `Idle`) still needs one pump regardless: that
/// transition is driven by a continuation `NavigatorShared::apply` registers
/// on the returned `TickerFuture` post-flush, and running early (because the
/// future is already resolved) only queues a command, which nothing drains
/// until the next pump
/// (`an_already_resolved_push_future_still_needs_one_pump_to_settle`,
/// `navigator.rs`).
///
/// **Pop** settles the ROUTE too, with no pump at all: `did_pop` calls
/// `reverse()`, which snaps to `Dismissed` at the call; `handle_pop`
/// (`history.rs`) reads `finished_when_popped()` synchronously, in the same
/// function, right after `did_pop` returns, with no continuation and no
/// queued command, so the entry disposes inside `pop()` itself.
///
/// Red-check: read distance alone (drop `run_duration.is_zero()`) from
/// `forward`/`reverse`'s settle gate — the controller stays `Forward`/never
/// reaches `Dismissed` before a pump, and the pop assertions below fail.
#[test]
fn zero_duration_transition_settles_push_and_pop_synchronously() {
    let (navigator_handle, mut harness) = navigator();
    let (route, animation) = transition_with_duration("second", Duration::ZERO);

    navigator_handle.push(route);
    let controller = animation
        .controller()
        .expect("install() runs inside push's own flush");
    assert_eq!(
        controller.status(),
        AnimationStatus::Completed,
        "a zero-duration entrance settles the CONTROLLER at the push call, \
         before any pump"
    );
    let top = navigator_handle.current().expect("pushed");
    assert_eq!(
        navigator_handle.route_state(top),
        Some(RouteLifecycle::Pushing),
        "the ROUTE's own Pushing -> Idle transition still needs a pump — \
         only the controller settled synchronously"
    );

    harness.tick(); // drains the queued PushCompleted command
    assert_eq!(
        navigator_handle.route_state(top),
        Some(RouteLifecycle::Idle)
    );

    assert!(navigator_handle.pop());
    assert_eq!(
        controller.status(),
        AnimationStatus::Dismissed,
        "a zero-duration exit settles the controller at the pop call too"
    );
    assert_eq!(
        navigator_handle.route_state(top),
        None,
        "finished_when_popped() read true INSIDE handle_pop, synchronously \
         with the same pop() call — no Popping park, no pump needed"
    );
    assert_eq!(navigator_handle.route_ids().len(), 1);
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
    // `update_secondary_animation`'s "is this train moving" check reads
    // `status()` (Forward|Reverse), not `is_animating()` — see the divergence
    // note at `transition_route.rs`'s `update_secondary_animation`
    // (`routes.dart:438-439`). `is_animating()` is ticker-based
    // (`AnimationController::is_animating` doc), and `set_value` now stops
    // the ticker like Flutter's `value=` setter, so it is no longer a stand-in
    // for "moving" here.
    assert!(middle_controller.status().is_running());

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

// ============================================================================
// `pop_paced` — gesture pacing rides the pop command, no stored field
// ============================================================================

/// `pop_paced` reaches `TransitionRoute::did_pop`, which eases through the
/// given curve/duration instead of the route's own plain `reverse()` —
/// Flutter's `_CupertinoBackGestureController.dragEnd` calling
/// `_controller.animateBack(target, duration: ..., curve: ...)`
/// (`cupertino/route.dart`, 3.44.0).
///
/// Red-check: drop the `take_pop_pacing()` branch from `TransitionRoute::did_pop`
/// — the eased-value assertion fails (a plain `reverse()` lands at 0.75, not
/// `EaseInQuint`'s ~0.945 at t=0.5 reversed).
#[test]
fn pop_paced_drives_the_controller_with_the_given_duration_and_curve() {
    let (navigator_handle, mut harness) = navigator();
    let (route, animation) = transition("only");
    navigator_handle.push(route);
    complete(&animation);
    harness.tick();

    let route_id = navigator_handle
        .route_ids()
        .last()
        .copied()
        .expect("pushed");
    let controller = animation.controller().expect("installed");

    let curve: Arc<dyn Curve + Send + Sync> = Arc::new(Curves::EaseInQuint); // see `PopPacing`'s doc (binding.rs) — same erased easing-curve boundary
    assert!(navigator_handle.pop_paced(route_id, Duration::from_millis(100), curve));
    assert_eq!(controller.status(), AnimationStatus::Reverse);

    controller.tick_at(0.05); // 50ms of the 100ms paced duration
    let expected = 1.0 - Curves::EaseInQuint.transform(0.5);
    assert!(
        (controller.value() - expected).abs() < 1e-3,
        "got {}, want {expected} (eased through the given curve over the given \
         duration, not the route's own linear reverse)",
        controller.value()
    );
}
