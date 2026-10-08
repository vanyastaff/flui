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
    for (value, velocity) in [(1.0, -2.0), (0.0, 2.0)] {
        let (navigator, _harness, top, controller) = mounted_with_transition_route();
        controller.set_value(value);
        let gesture = BackGestureController::new(navigator.clone(), top, controller.clone());
        assert!(
            !gesture.drag_end(velocity),
            "a reached destination settles synchronously"
        );
        assert!(!controller.is_animating());
        assert!(!navigator.user_gesture_in_progress());
    }

    // Fast negative velocity (screen-widths/s): stay (route animates
    // forward to 1.0 = new page fully covers again) regardless of value.
    {
        let (navigator, _harness, top, c) = mounted_with_transition_route();
        c.set_value(0.51);
        let gesture = BackGestureController::new(navigator, top, c.clone());
        let still_settling = gesture.drag_end(-2.0);
        assert!(still_settling, "an animated release keeps the run going");
        assert_eq!(c.status(), AnimationStatus::Forward);
        // The settle starts at the finger's speed (screen-widths/s are
        // controller units/s), not at a fixed duration's average.
        assert!(
            (c.velocity() - 2.0).abs() < 1e-9,
            "stay settle starts at {} widths/s",
            c.velocity()
        );
    }
    // Fast positive velocity: pop (route animates back to 0.0).
    {
        let (navigator, _harness, top, c) = mounted_with_transition_route();
        c.set_value(0.49);
        let gesture = BackGestureController::new(navigator, top, c.clone());
        let still_settling = gesture.drag_end(2.0);
        assert!(still_settling);
        assert_eq!(c.status(), AnimationStatus::Reverse);
        assert!(
            (c.velocity() + 2.0).abs() < 1e-9,
            "pop settle starts at {} widths/s",
            c.velocity()
        );
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

pub(crate) fn mounted_back_swipe_reads_retained_admission_settings() {
    use crate::common::{SettingsScope, lay_out_animated, tight};
    use flui_animation::Vsync;
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{
        PointerKind, make_cancel_event_for_id, make_down_event_for_id, make_move_event_for_id,
    };
    use flui_interaction::{GestureSettings, GestureSettingsSource, PointerId};
    use flui_painting::styling::Color;
    use flui_widgets::{ColoredBox, GestureDetector, VsyncScope};

    let profile = |slop| {
        GestureSettings::default()
            .try_with_touch_slop(200.0)
            .expect("finite touch slop")
            .try_with_pan_slop_horizontal(slop)
            .expect("finite axis slop")
    };
    let source = GestureSettingsSource::new(profile(20.0));
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(SimpleRoute::<i32>::new(|_| {
        ColoredBox::new(Color::BLACK).boxed()
    }));
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        VsyncScope::new(
            vsync.clone(),
            SettingsScope::new(source.provider(), Navigator::new(navigator.clone())),
        ),
        tight(300.0, 300.0),
        vsync,
    );
    let route = PageRoute::<i32>::new(|_, _, _| {
        GestureDetector::new()
            .on_tap(|_| {})
            .child(ColoredBox::new(Color::WHITE))
            .boxed()
    })
    .back_gesture(true)
    .transition_duration(Duration::from_millis(100));
    laid.enter_owner_scope(|| drop(navigator.push(route)));
    for _ in 0..40 {
        laid.pump_for(Duration::from_millis(16));
    }
    let top = navigator.current().expect("pushed route is current");
    let pointer = PointerId::try_from(1_u64).expect("nonzero touch");
    let down = make_down_event_for_id(pointer, Offset::new(10.0, 150.0), PointerKind::Touch)
        .expect("finite touch fixture");
    let small = make_move_event_for_id(pointer, Offset::new(50.0, 150.0), PointerKind::Touch)
        .expect("finite touch fixture");
    let large = make_move_event_for_id(pointer, Offset::new(130.0, 150.0), PointerKind::Touch)
        .expect("finite touch fixture");
    let cancel = make_cancel_event_for_id(pointer, PointerKind::Touch);

    laid.dispatch_pointer_event(&down);
    source.replace(profile(100.0));
    laid.dispatch_pointer_event(&small);
    assert!(
        navigator.user_gesture_in_progress(),
        "active edge contact retains its short admitted threshold"
    );
    laid.dispatch_pointer_event(&cancel);
    for _ in 0..40 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert_eq!(navigator.current(), Some(top));
    assert!(!navigator.user_gesture_in_progress());

    laid.dispatch_pointer_event(&down);
    laid.dispatch_pointer_event(&small);
    assert!(
        !navigator.user_gesture_in_progress(),
        "fresh edge contact reads the updated large threshold"
    );
    laid.dispatch_pointer_event(&large);
    assert!(
        navigator.user_gesture_in_progress(),
        "a deliberate edge swipe still starts"
    );
    laid.dispatch_pointer_event(&cancel);
    for _ in 0..40 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert_eq!(navigator.current(), Some(top));
    assert!(!navigator.user_gesture_in_progress());
}

pub(crate) fn replacing_authored_back_swipe_policy_cancels_the_outgoing_contact() {
    use std::cell::Cell;
    use std::rc::Rc;

    use crate::common::{ProbeSignals, SettingsScope, SignalProbe, lay_out_animated, tight};
    use flui_animation::Vsync;
    use flui_foundation::geometry::Offset;
    use flui_interaction::events::{
        PointerKind, make_down_event_for_id, make_move_event_for_id, make_up_event_for_id,
    };
    use flui_interaction::{GestureSettings, PointerId};
    use flui_painting::styling::Color;
    use flui_widgets::{ColoredBox, GestureDetector, VsyncScope};

    let navigator = NavigatorHandle::new();
    navigator.seed_initial(SimpleRoute::<i32>::new(|_| {
        ColoredBox::new(Color::BLACK).boxed()
    }));
    let threshold = Rc::new(Cell::new(20.0));
    let signal = Rc::new(Cell::new(None));
    let (profile, remembered, navigation) = (threshold.clone(), signal.clone(), navigator.clone());
    let probe = SignalProbe::new(move |ProbeSignals { count, .. }| {
        remembered.set(Some(count));
        SettingsScope::new(
            GestureSettings::default()
                .try_with_touch_slop(250.0)
                .expect("finite touch slop")
                .try_with_pan_slop_horizontal(profile.get())
                .expect("finite axis slop"),
            Navigator::new(navigation.clone()),
        )
    });
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        VsyncScope::new(vsync.clone(), probe.view()),
        tight(300.0, 300.0),
        vsync,
    );
    let route = PageRoute::<i32>::new(|_, _, _| {
        GestureDetector::new()
            .on_tap(|_| {})
            .child(ColoredBox::new(Color::WHITE))
            .boxed()
    })
    .back_gesture(true)
    .transition_duration(Duration::from_millis(100));
    let transition = route.transition_handle();
    laid.enter_owner_scope(|| drop(navigator.push(route)));
    for _ in 0..40 {
        laid.pump_for(Duration::from_millis(16));
    }
    let top = navigator.current().expect("pushed route is current");
    let pointer = PointerId::try_from(1_u64).expect("nonzero touch");
    let down = make_down_event_for_id(pointer, Offset::new(10.0, 150.0), PointerKind::Touch)
        .expect("finite touch fixture");
    let large = make_move_event_for_id(pointer, Offset::new(210.0, 150.0), PointerKind::Touch)
        .expect("finite touch fixture");
    let up = make_up_event_for_id(pointer, Offset::new(210.0, 150.0), PointerKind::Touch)
        .expect("finite touch fixture");
    let small = make_move_event_for_id(pointer, Offset::new(50.0, 150.0), PointerKind::Touch)
        .expect("finite touch fixture");
    let intermediate =
        make_move_event_for_id(pointer, Offset::new(130.0, 150.0), PointerKind::Touch)
            .expect("finite touch fixture");
    let far = make_move_event_for_id(pointer, Offset::new(290.0, 150.0), PointerKind::Touch)
        .expect("finite touch fixture");
    let far_up = make_up_event_for_id(pointer, Offset::new(290.0, 150.0), PointerKind::Touch)
        .expect("finite touch fixture");
    laid.dispatch_pointer_event(&down);
    laid.dispatch_pointer_event(&small);
    laid.dispatch_pointer_event(&large);
    assert!(navigator.user_gesture_in_progress());
    assert!(
        transition
            .controller()
            .expect("mounted route controller")
            .value()
            < 0.5,
        "outgoing swipe has actually crossed halfway"
    );
    probe
        .write(|cx| signal.get().expect("mounted probe").set(cx, 1))
        .expect("publish equal authored back swipe policy");
    laid.pump();
    assert!(
        navigator.user_gesture_in_progress(),
        "equal policy keeps the accepted edge contact"
    );
    threshold.set(100.0);
    probe
        .write(|cx| signal.get().expect("mounted probe").set(cx, 2))
        .expect("replace authored back swipe policy");
    laid.pump();
    for _ in 0..40 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert!(
        !navigator.user_gesture_in_progress(),
        "authored replacement cancels the outgoing edge contact"
    );
    assert_eq!(
        navigator.current(),
        Some(top),
        "replacement cancellation keeps the route past halfway"
    );
    laid.dispatch_pointer_event(&up);
    for _ in 0..40 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert_eq!(
        navigator.current(),
        Some(top),
        "stale release cannot pop the retained route"
    );
    laid.dispatch_pointer_event(&down);
    laid.dispatch_pointer_event(&intermediate);
    laid.dispatch_pointer_event(&far);
    assert!(
        transition
            .controller()
            .expect("mounted route controller")
            .value()
            < 0.5,
        "fresh swipe actually crosses halfway"
    );
    laid.dispatch_pointer_event(&far_up);
    for _ in 0..40 {
        laid.pump_for(Duration::from_millis(16));
    }
    assert_eq!(
        navigator.route_ids().len(),
        1,
        "replacement admits a healthy subsequent swipe"
    );
    assert!(!navigator.user_gesture_in_progress());
}

pub(crate) fn mounted_back_swipe_settle_uses_the_admitted_fling_bound() {
    use crate::common::{SettingsScope, lay_out_animated, tight};
    use flui_animation::Vsync;
    use flui_foundation::geometry::Point;
    use flui_interaction::{GestureSettings, GestureSettingsSource};
    use flui_painting::styling::Color;
    use flui_platform_api::{
        EventTime,
        pointer::{
            PointerButton, PointerButtons, PointerEvent, PointerId, PointerInfo, PointerKind,
            PointerMove, PointerPosition, PointerPress, PointerRelease, PointerSample,
        },
    };
    use flui_widgets::{ColoredBox, VsyncScope};

    let profile = |max| {
        GestureSettings::default()
            .try_with_fling_velocity(50.0, max)
            .expect("finite fling range")
    };
    let source = GestureSettingsSource::new(profile(200.0));
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(SimpleRoute::<i32>::new(|_| {
        ColoredBox::new(Color::BLACK).boxed()
    }));
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(
        VsyncScope::new(
            vsync.clone(),
            SettingsScope::new(source.provider(), Navigator::new(navigator.clone())),
        ),
        tight(300.0, 300.0),
        vsync,
    );
    let route = PageRoute::<i32>::new(|_, _, _| ColoredBox::new(Color::WHITE).boxed())
        .back_gesture(true)
        .transition_duration(Duration::from_millis(100));
    laid.enter_owner_scope(|| drop(navigator.push(route)));
    for _ in 0..40 {
        laid.pump_for(Duration::from_millis(16));
    }
    let top = navigator.current().expect("pushed route is current");
    let held = PointerButtons::only(PointerButton::PRIMARY);
    for contact in [1_u64, 2] {
        let info = PointerInfo::new(
            PointerId::try_from(contact).expect("nonzero touch"),
            PointerKind::Touch,
        );
        let sample = |millis: u64, x| {
            PointerSample::new(
                EventTime::from_nanos((contact * 1_000 + millis) * 1_000_000),
                PointerPosition::try_new(Point::new(x, 150.0)).expect("finite touch position"),
            )
        };
        laid.dispatch_pointer_event(&PointerEvent::Down(PointerPress::new(
            info,
            PointerButton::PRIMARY,
            PointerButtons::NONE,
            sample(0, 10.0),
        )));
        if contact == 1 {
            source.replace(profile(8_000.0));
        }
        for (millis, x) in [(10, 40.0), (20, 70.0), (30, 100.0), (40, 130.0)] {
            laid.dispatch_pointer_event(&PointerEvent::Move(PointerMove::new(
                info,
                held,
                sample(millis, x),
            )));
        }
        assert!(
            navigator.user_gesture_in_progress(),
            "actual fast edge swipe is admitted"
        );
        laid.dispatch_pointer_event(&PointerEvent::Up(PointerRelease::new(
            info,
            PointerButton::PRIMARY,
            held,
            sample(40, 130.0),
        )));
        for _ in 0..40 {
            laid.pump_for(Duration::from_millis(16));
        }
        assert!(!navigator.user_gesture_in_progress());
        if contact == 1 {
            assert_eq!(
                navigator.current(),
                Some(top),
                "retained 200px/s release stays below the product fling threshold before halfway"
            );
        } else {
            assert_eq!(
                navigator.route_ids().len(),
                1,
                "fresh contact uses the new bound and commits a fast release"
            );
        }
    }
}
