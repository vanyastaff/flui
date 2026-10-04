//! The edge-swipe back gesture against a mounted navigator: the release
//! matrix, a programmatic pop mid-drag, and the gesture counter across settle
//! and dispose. The private `BackGestureController`/`BackGestureRuntime` are
//! reached through the temporary `flui_widgets::__test_access` path
//! (ADR-0083 §4); the tests that need no mounted tree stay in
//! `src/navigator/back_gesture.rs`.

use std::time::Duration;

use flui_animation::{Animation, AnimationController, AnimationStatus};
use flui_view::prelude::*;
use flui_widgets::__test_access::{BackGestureController, RouteProbe as _};
use flui_widgets::SizedBox;
use flui_widgets::navigator::{Navigator, NavigatorHandle, PageRoute, RouteId, SimpleRoute};

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

// ---- release matrix: v = -2.0 / +2.0 / 0 at value 0.49 / 0.51 ----

pub(crate) fn release_matrix_fling_and_slow_release() {
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

// ---- dispose-mid-settle: counter returns to 0 ----

// ---- full settle after release: did_stop fires, counter clears ----

// ---- dispose while awaiting settle (post-release, pre-poll): counter clears ----

pub(crate) fn cancelling_a_back_swipe_past_halfway_keeps_the_route() {
    use crate::common::{lay_out_animated, tight};
    use flui_animation::Vsync;
    use flui_painting::styling::Color;
    use flui_widgets::{ColoredBox, VsyncScope};
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(SimpleRoute::<i32>::new(|_| {
        ColoredBox::new(Color::BLACK).boxed()
    }));
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        VsyncScope::new(vsync.clone(), Navigator::new(navigator.clone())),
        tight(300.0, 300.0),
        vsync,
    );
    let route = PageRoute::<i32>::new(|_, _, _| ColoredBox::new(Color::WHITE).boxed())
        .back_gesture(true)
        .transition_duration(Duration::from_millis(100));
    laid.enter_owner_scope(|| {
        drop(navigator.push(route));
    });
    for _ in 0..40 {
        laid.pump_for(Duration::from_millis(16));
    }
    let top = navigator.current().expect("pushed route is current");
    assert_eq!(navigator.route_ids().len(), 2);
    laid.dispatch_pointer_down(10.0, 150.0);
    laid.dispatch_pointer_move(210.0, 150.0);
    assert!(
        navigator.user_gesture_in_progress(),
        "actual edge gesture started"
    );
    laid.dispatch_pointer_cancel();
    for _ in 0..40 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert_eq!(
        navigator.current(),
        Some(top),
        "cancel never commits a pop past halfway"
    );
    assert!(!navigator.user_gesture_in_progress());
    laid.dispatch_pointer_down(10.0, 150.0);
    laid.dispatch_pointer_move(210.0, 150.0);
    laid.dispatch_pointer_up(210.0, 150.0);
    for _ in 0..40 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert_eq!(
        navigator.route_ids().len(),
        1,
        "next completed swipe still pops"
    );
    assert!(!navigator.user_gesture_in_progress());
}
