//! Tests for the private `TransitionRoute`, reached through the temporary
//! `flui_widgets::__test_access` path (ADR-0083 §4). Its export boundary keeps
//! a unit test in `src/navigator/transition_route_tests.rs`.
//!
//! # Scenarios
//!
//! The secondary animation is dismissed when the next route finishes its pop,
//! when it is removed, after train hopping finishes and pops, and when train
//! hopping is interrupted. Expected values are fixed by the documented
//! contract, not by running this code.
//!
//! Most of these drive the transition by hand with `set_value` — which is
//! deterministic, and is what makes the status-change handler's four arms
//! individually testable — rather than by awaiting the `TickerFuture`
//! `did_push` returns (ADR-0064). A handful that need the run to have real,
//! not-yet-covered distance left (so a `reverse()` cannot collapse
//! synchronously to `Dismissed`) pump a real `Vsync` instead; those say so.

use std::sync::Arc;
use std::time::Duration;

use flui_animation::{Animation, AnimationStatus, Vsync};
use flui_view::prelude::*;
use parking_lot::Mutex;

use flui_widgets::__test_access::{
    NavigatorProbe as _, RouteLifecycle, TransitionHandle, TransitionRoute,
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
/// the controller reports `Completed`.
///
/// Red-check: return `PushCompletion::Immediate` from `TransitionRoute::did_push`.
pub(crate) fn push_transition_parks_the_entry_in_pushing_until_the_controller_completes() {
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
pub(crate) fn pop_mid_push_cancels_the_push_future_inside_the_flush_and_ends_popping() {
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

// ============================================================================
// SECONDARY ANIMATION
// ============================================================================

// ============================================================================
// TRAIN HOPPING
// ============================================================================

/// The `Mutex` in `TransitionInner` must never be held across a `RouteBinding`
/// call: `binding.finalize()` runs `wake`, which `try_lock`s the history. A
/// deadlock here would be a hang, not a failure.
///
/// This is a smoke test rather than an assertion — it completes, or nextest's
/// timeout catches it.
pub(crate) fn status_listener_does_not_hold_a_lock_across_the_binding_call() {
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
