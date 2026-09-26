//! The edge-swipe back gesture against a mounted navigator: the release
//! matrix, a programmatic pop mid-drag, and the gesture counter across settle
//! and dispose. The private `BackGestureController`/`BackGestureRuntime` are
//! reached through the temporary `flui_widgets::__test_access` path
//! (ADR-0083 §4); the tests that need no mounted tree stay in
//! `src/navigator/back_gesture.rs`.

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use flui_animation::{Animation, AnimationController, AnimationStatus};
use flui_interaction::DragStartDetails;
use flui_view::prelude::*;
use flui_widgets::__test_access::{BackGestureController, BackGestureRuntime, RouteProbe as _};
use flui_widgets::SizedBox;
use flui_widgets::navigator::{
    Navigator, NavigatorHandle, NavigatorObserver, PageRoute, RouteId, SimpleRoute,
};

use crate::common::harness::{Harness, mount};

/// A mounted navigator with a pushed [`PageRoute`], and that route's own
/// [`AnimationController`] — the same one `pop_paced` reaches through
/// `did_pop`. Needed by any test that drives `drag_end`'s pop branch:
/// `BackGestureController` must be constructed with the route's *real*
/// controller, or the pacing it applies through `pop_paced` lands on a
/// route with no relationship to the controller the test observes.
fn mounted_with_transition_route() -> (NavigatorHandle, Harness, RouteId, AnimationController) {
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(SimpleRoute::<i32>::new(|_ctx| {
        SizedBox::new(1.0, 1.0).into_view().boxed()
    }));
    let mut harness = mount(Navigator::new(navigator.clone()));

    let route = PageRoute::<i32>::new(|_ctx, _primary, _secondary| {
        SizedBox::new(1.0, 1.0).into_view().boxed()
    })
    .transition_duration(Duration::from_millis(300));
    let transition = route.transition_handle();
    let _pushed = harness.enter_owner_scope(|| navigator.push(route));
    harness.tick();

    let top = *navigator.route_ids().last().expect("pushed");
    let controller = transition
        .controller()
        .expect("install() created the controller");
    (navigator, harness, top, controller)
}

/// The drag start a recognizer would hand over; its fields do not affect the
/// runtime, which only needs to know a drag began.
fn drag_start() -> DragStartDetails {
    DragStartDetails {
        global_position: flui_types::geometry::Offset::ZERO,
        local_position: flui_types::geometry::Offset::ZERO,
        kind: flui_interaction::events::PointerType::Touch,
        timestamp: std::time::Instant::now(),
    }
}

/// A runtime for `route` whose predicate always allows a drag.
fn runtime_for(
    navigator: &NavigatorHandle,
    route: RouteId,
    controller: &AnimationController,
) -> BackGestureRuntime {
    BackGestureRuntime::new(
        navigator.clone(),
        route,
        controller.clone(),
        Rc::new(|| true),
    )
}

// ---- release matrix: v = -2.0 / +2.0 / 0 at value 0.49 / 0.51 ----

#[test]
fn release_matrix_fling_and_slow_release() {
    // Fast negative velocity (screen-widths/s): stay (route animates
    // forward to 1.0 = new page fully covers again) regardless of value.
    {
        let (navigator, _harness, top, c) = mounted_with_transition_route();
        c.set_value(0.51);
        let gesture = BackGestureController::new(navigator, top, c.clone());
        let still_settling = gesture.drag_end(-2.0);
        assert!(still_settling, "an animated release keeps the run going");
        assert_eq!(c.status(), AnimationStatus::Forward);
    }
    // Fast positive velocity: pop (route animates back to 0.0).
    {
        let (navigator, _harness, top, c) = mounted_with_transition_route();
        c.set_value(0.49);
        let gesture = BackGestureController::new(navigator, top, c.clone());
        let still_settling = gesture.drag_end(2.0);
        assert!(still_settling);
        assert_eq!(c.status(), AnimationStatus::Reverse);
    }
    // No meaningful velocity, value > 0.5: stay.
    {
        let (navigator, _harness, top, c) = mounted_with_transition_route();
        c.set_value(0.51);
        let gesture = BackGestureController::new(navigator, top, c.clone());
        let still_settling = gesture.drag_end(0.0);
        assert!(still_settling);
        assert_eq!(c.status(), AnimationStatus::Forward);
    }
    // No meaningful velocity, value <= 0.5: pop.
    {
        let (navigator, _harness, top, c) = mounted_with_transition_route();
        c.set_value(0.49);
        let gesture = BackGestureController::new(navigator, top, c.clone());
        let still_settling = gesture.drag_end(0.0);
        assert!(still_settling);
        assert_eq!(c.status(), AnimationStatus::Reverse);
    }
}

// ---- mid-drag programmatic pop: the pop itself must not be clobbered ----

/// Flutter's `dragUpdate` has no `is_active`/`is_current` guard at all —
/// `controller.value -= delta` runs unconditionally, so a drag_update
/// after a programmatic pop still moves the value (this is *not* a
/// no-op, and asserting otherwise would pin a divergence). What must
/// hold is the other direction: the programmatic pop that landed
/// mid-drag stays popped — a later drag_update must not resurrect the
/// route or panic reaching into it.
#[test]
fn mid_drag_programmatic_pop_is_not_undone_by_a_later_drag_update() {
    let (navigator, mut harness, top, c) = mounted_with_transition_route();
    c.set_value(1.0);
    let gesture = BackGestureController::new(navigator.clone(), top, c.clone());
    gesture.drag_update(0.3); // value 0.7, mid-drag

    // A programmatic pop lands while the finger is still down. The route
    // stays in `route_ids()` until its (non-zero-duration) exit
    // transition finishes — `finished_when_popped` — so "the pop took
    // effect" is checked through `current()`, not stack membership.
    assert!(harness.enter_owner_scope(|| navigator.pop()));
    harness.tick();
    assert_ne!(
        navigator.current(),
        Some(top),
        "the mid-drag pop must actually move `current` off this route"
    );

    // A further drag_update on the now-stale gesture must not panic or
    // resurrect the popped route.
    gesture.drag_update(0.05);
    assert_ne!(
        navigator.current(),
        Some(top),
        "a stale drag_update after the pop must not undo it"
    );
}

// ---- dispose-mid-settle: counter returns to 0 ----

#[test]
fn dispose_mid_gesture_returns_the_counter_to_zero() {
    let (navigator, mut harness, top, c) = mounted_with_transition_route();
    let runtime = runtime_for(&navigator, top, &c);
    runtime.on_drag_start(drag_start());
    assert!(navigator.user_gesture_in_progress());

    // The detector unmounts mid-drag (finger still down) — no drag_end
    // ever ran. The navigator IS mounted here (unlike an inert
    // `NavigatorHandle::new()` fixture), so Flutter's own `if (mounted)`
    // gate in `dispose` is actually exercised, not vacuously satisfied.
    // `dispose` runs from within the element tree's own owner scope in
    // production, exactly like `push`/`pop` do — reproduced here so the
    // local post-frame lane is actually active and the report is
    // genuinely deferred, not caught by the synchronous fallback.
    assert!(navigator.is_mounted());
    harness.enter_owner_scope(|| runtime.dispose_safety_net());

    // `mount()` installs a real owner-local post-frame lane, so the
    // report is deferred (Flutter's own `addPostFrameCallback`, not a
    // synchronous call from `dispose`) — a frame tick is what delivers it.
    assert!(
        navigator.user_gesture_in_progress(),
        "the report is deferred to the next frame, not synchronous"
    );
    harness.tick();
    assert!(
        !navigator.user_gesture_in_progress(),
        "dispose must return the counter to 0 by the next frame, even \
         with no drag_end"
    );
}

/// A one-shot observer that counts `did_stop_user_gesture` calls.
#[derive(Default)]
struct GestureStopObserver {
    stops: AtomicUsize,
}
impl NavigatorObserver for GestureStopObserver {
    fn did_stop_user_gesture(&self) {
        self.stops.fetch_add(1, Ordering::SeqCst);
    }
}

// ---- full settle after release: did_stop fires, counter clears ----

/// A released drag settled by genuinely ticking the run out (not
/// `set_value`) must report `did_stop_user_gesture` to observers exactly
/// once and leave `user_gesture_in_progress()` false — Flutter's
/// trailing `AnimationStatusListener` in `dragEnd` firing on the run's
/// real terminal status.
///
/// Red-check: drop `poll_settle`'s call entirely — `awaiting_settle`
/// stays `true` forever and this test hangs on the final assertion
/// (never becomes `false`).
#[test]
fn full_settle_after_release_reports_did_stop_and_clears_the_counter() {
    let (navigator, _harness, top, c) = mounted_with_transition_route();
    let observer = Arc::new(GestureStopObserver::default());
    navigator.add_observer(Arc::clone(&observer) as Arc<dyn NavigatorObserver>);

    c.set_value(0.49); // <= 0.5, no fling: dragEnd's pop branch
    let runtime = runtime_for(&navigator, top, &c);
    runtime.on_drag_start(drag_start());

    runtime.finish_drag(0.0);
    assert!(
        runtime.awaiting_settle(),
        "the 350ms reverse run is still going"
    );
    assert!(navigator.user_gesture_in_progress());

    // Genuinely tick the run out (not `set_value`) — mid-flight polls
    // must not report early.
    c.tick_at(0.10);
    runtime.poll_settle();
    assert!(
        navigator.user_gesture_in_progress(),
        "must not report stopped before the run actually settles"
    );
    assert_eq!(observer.stops.load(Ordering::SeqCst), 0);

    c.tick_at(0.35); // >= the 350ms pacing -> settles to Dismissed
    assert_eq!(c.status(), AnimationStatus::Dismissed);
    runtime.poll_settle();

    assert!(
        !navigator.user_gesture_in_progress(),
        "the counter must clear once the run genuinely settles"
    );
    assert_eq!(
        observer.stops.load(Ordering::SeqCst),
        1,
        "did_stop_user_gesture must fire exactly once"
    );
}

// ---- dispose while awaiting settle (post-release, pre-poll): counter clears ----

/// `dispose_safety_net` must own the deferred report for a
/// gesture that already *released* (so `self.gesture` is `None` —
/// `finish_drag` always takes it) but whose settle animation is still
/// running when the detector unmounts — e.g. the route was swept away by
/// a `push_and_remove_until` mid-settle, or lost the race between the
/// pop's own settle and this detector's final rebuild. Checking only
/// `self.gesture` (as if a live drag were the only case that owes a
/// report) leaks the counter forever.
///
/// Red-check: guard `dispose_safety_net` on `self.gesture` alone (drop
/// the `awaiting_settle` check) — this test's final assertion fails,
/// `user_gesture_in_progress()` stays `true` forever.
#[test]
fn dispose_while_awaiting_settle_after_release_returns_the_counter_to_zero() {
    let (navigator, mut harness, top, c) = mounted_with_transition_route();
    c.set_value(0.49);
    let runtime = runtime_for(&navigator, top, &c);
    runtime.on_drag_start(drag_start());

    runtime.finish_drag(0.0);
    assert!(!runtime.has_gesture(), "finish_drag always takes it");
    assert!(
        runtime.awaiting_settle(),
        "the release animation is still running"
    );
    assert!(navigator.user_gesture_in_progress());

    // The detector unmounts before the settle run's next poll — no
    // `poll_settle` call ever ran.
    assert!(navigator.is_mounted());
    harness.enter_owner_scope(|| runtime.dispose_safety_net());
    harness.tick();

    assert!(
        !navigator.user_gesture_in_progress(),
        "dispose must clear the counter for a release still awaiting \
         settle, not only for a still-dragging gesture"
    );
}
