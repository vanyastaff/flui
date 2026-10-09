//! Tests for the private `TransitionRoute`, reached through the temporary
//! `flui_widgets::__test_access` path (ADR-0083 §4).
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
//! individually testable — rather than by awaiting the `AnimationRunFuture`
//! `did_push` returns (ADR-0064). A handful that need the run to have real,
//! not-yet-covered distance left (so a `reverse()` cannot collapse
//! synchronously to `Dismissed`) pump a real `Vsync` instead; those say so.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
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
/// `AnimationRunFuture` — this helper drives `status`, not the future. A test that
/// needs the future to resolve `Ok(())` through natural completion drives a
/// real `Vsync` instead.
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
/// push's `AnimationRunFuture` **inside the flush that runs `did_pop`**: `did_pop`
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
/// `reverse()` collapse straight to `Dismissed` — an already-dismissed
/// controller finalizing synchronously, a different shape from this one — so
/// this pumps the entrance to its midpoint first, leaving `reverse()` real
/// distance to cover.
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
    // actually advances it.
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

pub(crate) fn scope_replacement_moves_an_existing_route_without_restarting_it() {
    let old = Vsync::new();
    let new = Vsync::new();
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(SimpleRoute::<i32>::new(|_ctx| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }));
    let tree = |vsync| VsyncScope::new(vsync, Navigator::new(navigator.clone()));
    let mut laid = crate::common::lay_out_animated(
        tree(old.clone()),
        crate::common::tight(200.0, 200.0),
        old.clone(),
    );
    let (route, animation) = transition("second");
    navigator.push(route);
    let controller = animation.controller().expect("installed controller");
    laid.pump_for(Duration::ZERO);
    laid.pump_for(Duration::from_millis(90));
    let before = controller.value();
    assert!(before > 0.0 && before < 1.0);

    laid.pump_widget(tree(new.clone()));
    assert!(old.is_empty(), "the existing route releases its old clock");
    assert_eq!(new.len(), 1, "the existing route acquires the new clock");
    laid.adopt_vsync(new.clone());
    laid.pump_for(Duration::ZERO);
    assert_eq!(
        controller.value(),
        before,
        "rebinding preserves the sampled elapsed time"
    );
    laid.pump_for(Duration::from_millis(30));
    assert!(
        controller.value() > before,
        "the new clock advances the existing run"
    );

    laid.pump_widget(SizedBox::shrink());
    assert!(
        new.is_empty(),
        "unmount withdraws the route's registry seat"
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

// ============================================================================
// OWNERSHIP
// ============================================================================

/// A route left mid-hop and dropped without `dispose` frees its secondary
/// proxy: the hop's switch callback must not keep the proxy alive through the
/// switch the proxy itself parents (`proxy -> switch -> callback -> proxy`).
///
/// Red-check: capture `Arc<ProxyAnimation>` instead of a `Weak` in the
/// `on_switched` callback of `TransitionRoute::update_secondary_animation`.
pub(crate) fn hopping_route_dropped_without_dispose_frees_proxy() {
    let (bottom, bottom_handle) = transition("bottom");
    let navigator_handle = NavigatorHandle::new();
    navigator_handle.seed_initial(bottom);
    let mut harness = mount(Navigator::new(navigator_handle.clone()));

    let (middle, middle_handle) = transition("middle");
    navigator_handle.push(middle);
    harness.tick();
    let middle_controller = middle_handle
        .controller()
        .expect("install created the controller");
    // Mid-entrance: a moving train at a value the replacement does not share.
    middle_controller.set_value(0.5);

    let (top, _top_handle) = transition("top");
    navigator_handle.push_replacement(top);
    harness.tick();
    assert!(
        bottom_handle.secondary_is_hopping(),
        "the replacement starts at a different value while moving: a hop"
    );

    let proxy = std::rc::Rc::downgrade(&bottom_handle.secondary_animation());
    drop(middle_controller);
    drop(middle_handle);
    drop(bottom_handle);
    drop(harness);
    drop(navigator_handle);
    assert!(
        proxy.upgrade().is_none(),
        "the secondary proxy outlived every owner of the route"
    );
}

std::thread_local! {
    static REENTRANT_HANDLE: RefCell<Option<TransitionHandle>> = const { RefCell::new(None) };
}

/// A listener capture whose `Drop` reads the route's controller through the
/// route's handle (parked in a thread-local: the handle is not `Send`).
struct ReadsControllerOnDrop(Arc<AtomicBool>);

impl Drop for ReadsControllerOnDrop {
    fn drop(&mut self) {
        let handle = REENTRANT_HANDLE.with(|slot| slot.borrow_mut().take());
        if let Some(handle) = handle {
            let _ = handle.controller();
            self.0.store(true, Ordering::SeqCst);
        }
    }
}

/// Disposing a route retires its controller's status listeners; a listener
/// capture whose `Drop` reads the controller through the route's handle must
/// not find the route still holding its controller slot. A held slot is a
/// self-deadlock, so the scenario runs on its own thread and the row fails
/// after ten seconds without a completion signal (the stuck thread is leaked).
///
/// Red-check: dispose the controller inside
/// `if let Some(controller) = self.inner.controller.lock().take() && …` in
/// `TransitionRoute::dispose`.
pub(crate) fn dispose_releases_the_controller_slot_before_disposing_it() {
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let (navigator_handle, mut harness) = navigator();
        let (route, animation) = transition("second");
        navigator_handle.push(route);
        complete(&animation);
        harness.tick();

        let reentered = Arc::new(AtomicBool::new(false));
        let probe = ReadsControllerOnDrop(Arc::clone(&reentered));
        animation
            .controller()
            .expect("install created the controller")
            .add_status_listener(std::rc::Rc::new(move |_| {
                let _ = &probe;
            }));
        REENTRANT_HANDLE.with(|slot| *slot.borrow_mut() = Some(animation.clone()));

        navigator_handle.pop();
        dismiss(&animation);
        harness.tick();
        REENTRANT_HANDLE.with(|slot| slot.borrow_mut().take());

        assert_eq!(navigator_handle.route_ids().len(), 1);
        assert!(
            reentered.load(Ordering::SeqCst),
            "disposing the route retired the listener capture"
        );
        let _ = done.send(());
    });
    match finished.recv_timeout(Duration::from_secs(10)) {
        Ok(()) => {}
        Err(RecvTimeoutError::Timeout) => {
            panic!("deadlock: dispose held the controller slot while dropping a capture")
        }
        Err(RecvTimeoutError::Disconnected) => panic!("the dispose scenario panicked"),
    }
}
