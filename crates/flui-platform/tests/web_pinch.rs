//! The browser trackpad pinch decoders (`shared::web_pinch`): Control-held
//! wheel events classified by their sequence's first event, with the End on
//! the event timeline at the deadline, the application's refusal, and WebKit
//! gesture events turned into per-step readings. The DOM side runs in Chrome
//! (`tests/web_browser.rs`).

use std::time::Duration;

use flui_platform::shared::pointer_assembly::{
    ContactRegistry, GestureComponent, GestureDelta, GesturePhase, GestureReading, NativeClock,
    NativeContact, NativeReading, ReadingAction, ReadingTime, decode,
};
use flui_platform::shared::web_pinch::{
    CtrlWheel, GestureEventKind, GestureTracker, PINCH_GAP, WheelRoute, WheelStep,
};
use flui_platform::traits::PlatformInput;
use flui_platform_api::EventTime;
use flui_platform_api::pointer::{PanZoomPhase, PointerEvent, PointerKind};

fn at(millis: u64) -> EventTime {
    EventTime::from_nanos(millis * 1_000_000)
}

fn wheel(ctrl: bool, dy: f64, millis: u64) -> WheelStep {
    WheelStep {
        ctrl,
        mode: 0,
        dx: 0.0,
        dy,
        time: at(millis),
    }
}

fn scale(route: WheelRoute) -> (GesturePhase, f64) {
    match route {
        WheelRoute::Pinch(GestureReading {
            component: GestureComponent::Scale,
            phase,
            delta: GestureDelta::Scale(factor),
        }) => (phase, factor),
        other => panic!("not a pinch step: {other:?}"),
    }
}

fn a_fractional_ctrl_wheel_starts_a_pinch_whose_factor_inverts_chromium() {
    let mut decoder = CtrlWheel::default();
    // Chromium encodes a pinch to 1.25× as -100·ln(1.25) pixels.
    let (phase, factor) = scale(decoder.route(wheel(true, -100.0 * 1.25_f64.ln(), 10)));
    assert_eq!(phase, GesturePhase::Began);
    assert!((factor - 1.25).abs() < 1e-12, "{factor}");
    let (phase, factor) = scale(decoder.route(wheel(true, 100.0 * 2.0_f64.ln(), 20)));
    assert_eq!(phase, GesturePhase::Changed);
    assert!((factor - 0.5).abs() < 1e-12, "{factor}");
}

fn the_first_event_classifies_the_whole_sequence() {
    let mut decoder = CtrlWheel::default();
    // A Control-held mouse detent: whole pixels, so a scroll ...
    assert_eq!(decoder.route(wheel(true, 100.0, 10)), WheelRoute::Scroll);
    // ... and a fractional step of the same sequence stays one.
    assert_eq!(decoder.route(wheel(true, 33.5, 20)), WheelRoute::Scroll);
    assert_eq!(decoder.deadline(), None, "a scroll has no pinch deadline");
    // A pinch's whole-number step stays in the pinch.
    let mut decoder = CtrlWheel::default();
    scale(decoder.route(wheel(true, -3.25, 10)));
    let (phase, factor) = scale(decoder.route(wheel(true, -4.0, 20)));
    assert_eq!(phase, GesturePhase::Changed);
    assert!((factor - (0.04_f64).exp()).abs() < 1e-12);
    // Lines, or a horizontal component, are never a pinch.
    let mut decoder = CtrlWheel::default();
    let lines = WheelStep {
        mode: 1,
        ..wheel(true, 0.5, 10)
    };
    assert_eq!(decoder.route(lines), WheelRoute::Scroll);
    let mut decoder = CtrlWheel::default();
    let sideways = WheelStep {
        dx: 0.5,
        ..wheel(true, 0.5, 10)
    };
    assert_eq!(decoder.route(sideways), WheelRoute::Scroll);
}

fn a_pinch_ends_at_its_deadline_on_the_event_timeline() {
    let mut decoder = CtrlWheel::default();
    scale(decoder.route(wheel(true, -1.5, 10)));
    let deadline = at(10).checked_add(PINCH_GAP).expect("in range");
    assert_eq!(decoder.deadline(), Some(deadline));
    assert_eq!(decoder.expire(deadline), None, "not due at the deadline");
    let (end, time) = decoder
        .expire(
            deadline
                .checked_add(Duration::from_millis(500))
                .expect("in range"),
        )
        .expect("due after it");
    assert_eq!(end.phase, GesturePhase::Ended);
    assert_eq!(
        time, deadline,
        "stamped with the deadline, not the late check"
    );
    assert_eq!(decoder.expire(at(10_000)), None, "ends once");
    // The next Control-held event after the gap is a new sequence.
    assert_eq!(
        scale(decoder.route(wheel(true, -1.5, 1_000))).0,
        GesturePhase::Began
    );
}

fn a_step_within_the_gap_extends_the_deadline() {
    let mut decoder = CtrlWheel::default();
    scale(decoder.route(wheel(true, -1.5, 10)));
    let gap = u64::try_from(PINCH_GAP.as_millis()).expect("small");
    assert_eq!(
        scale(decoder.route(wheel(true, -1.5, 10 + gap))).0,
        GesturePhase::Changed
    );
    assert_eq!(
        decoder.deadline(),
        at(10 + gap).checked_add(PINCH_GAP),
        "measured from the last step"
    );
}

fn a_wheel_without_control_ends_the_pinch_at_its_own_time() {
    let mut decoder = CtrlWheel::default();
    scale(decoder.route(wheel(true, -1.5, 10)));
    let (end, time) = decoder.finish(at(40)).expect("an open pinch ends");
    assert_eq!((end.phase, time), (GesturePhase::Ended, at(40)));
    assert_eq!(decoder.route(wheel(false, 3.5, 40)), WheelRoute::Scroll);
    assert_eq!(decoder.finish(at(50)), None);
}

fn a_refused_pinch_is_cancelled_and_its_sequence_left_to_the_browser() {
    let mut decoder = CtrlWheel::default();
    scale(decoder.route(wheel(true, -1.5, 10)));
    let cancel = decoder.refuse().expect("the open pinch is cancelled");
    assert_eq!(cancel.phase, GesturePhase::Cancelled);
    assert_eq!(decoder.route(wheel(true, -1.5, 20)), WheelRoute::Refused);
    assert_eq!(decoder.deadline(), None, "a refused sequence has no End");
    assert_eq!(decoder.expire(at(10_000)), None);
    assert_eq!(
        scale(decoder.route(wheel(true, -1.5, 20_000))).0,
        GesturePhase::Began,
        "the next sequence is decided afresh"
    );
    assert_eq!(CtrlWheel::default().refuse(), None, "nothing to refuse");
}

fn webkit_gesture_events_become_per_step_scale_and_rotation() {
    let mut tracker = GestureTracker::default();
    let readings =
        |values: [Option<GestureReading>; 2]| values.map(|reading| reading.expect("a reading"));
    let [scale, rotation] = readings(tracker.step(GestureEventKind::Start, 1.0, 0.0));
    assert_eq!(scale.phase, GesturePhase::Began);
    assert_eq!(scale.delta, GestureDelta::Scale(1.0));
    assert_eq!(rotation.delta, GestureDelta::Rotation(0.0));
    let [scale, rotation] = readings(tracker.step(GestureEventKind::Change, 2.0, 90.0));
    assert_eq!(scale.delta, GestureDelta::Scale(2.0));
    assert_eq!(
        rotation.delta,
        GestureDelta::Rotation(90.0_f64.to_radians())
    );
    // Cumulative values: a step is the change since the last one.
    let [scale, rotation] = readings(tracker.step(GestureEventKind::Change, 3.0, 45.0));
    assert_eq!(scale.delta, GestureDelta::Scale(1.5));
    assert_eq!(
        rotation.delta,
        GestureDelta::Rotation((-45.0_f64).to_radians())
    );
    // A value that is not a number changes nothing.
    let [scale, _] = readings(tracker.step(GestureEventKind::Change, f64::NAN, 45.0));
    assert_eq!(scale.delta, GestureDelta::None);
    let [scale, rotation] = readings(tracker.step(GestureEventKind::End, 3.0, 45.0));
    assert_eq!(
        (scale.phase, rotation.phase),
        (GesturePhase::Ended, GesturePhase::Ended)
    );
    assert_eq!(
        tracker.step(GestureEventKind::Change, 1.0, 0.0),
        [None, None]
    );
    // A refused gesture delivers nothing more.
    tracker.step(GestureEventKind::Start, 1.0, 0.0);
    assert_eq!(
        tracker.refuse().map(|cancel| cancel.phase),
        Some(GesturePhase::Cancelled)
    );
    assert!(tracker.is_refused());
    assert_eq!(
        tracker.step(GestureEventKind::Change, 2.0, 0.0),
        [None, None]
    );
    assert_eq!(tracker.step(GestureEventKind::End, 2.0, 0.0), [None, None]);
}

/// The readings through the shared accumulator: the PanZoom grammar a
/// pinch decoded from wheel events reaches the application in.
fn a_ctrl_wheel_pinch_assembles_into_one_pan_zoom_sequence() {
    let mut registry = ContactRegistry::new(NativeClock::epoch_only());
    let contact = NativeContact(1 << 32);
    let mut decoder = CtrlWheel::default();
    let mut phases = Vec::new();
    let mut push = |reading: GestureReading, time: EventTime, registry: &mut ContactRegistry| {
        let native = NativeReading::new(
            contact,
            PointerKind::Trackpad,
            ReadingAction::Gesture(reading),
            ReadingTime::epoch(time),
        )
        .with_primary(true)
        .at(40.0, 30.0, 1.0);
        for input in decode(registry, &native).expect("decodes") {
            match input {
                PlatformInput::Pointer(PointerEvent::PanZoom(event)) => {
                    phases.push((event.phase, event.time));
                }
                other => panic!("not a pan-zoom: {other:?}"),
            }
        }
    };
    for (dy, millis) in [(-10.5, 10), (-10.5, 20)] {
        let WheelRoute::Pinch(reading) = decoder.route(wheel(true, dy, millis)) else {
            panic!("a pinch step");
        };
        push(reading, at(millis), &mut registry);
    }
    let (end, time) = decoder.expire(at(1_000)).expect("due");
    push(end, time, &mut registry);
    let names: Vec<&str> = phases
        .iter()
        .map(|(phase, _)| match phase {
            PanZoomPhase::Start => "start",
            PanZoomPhase::Update(_) => "update",
            PanZoomPhase::End => "end",
            _ => "other",
        })
        .collect();
    assert_eq!(names, ["start", "update", "update", "end"]);
    let expected = (10.5_f64 / 100.0).exp().powi(2);
    let PanZoomPhase::Update(transform) = phases[2].0 else {
        panic!("an update");
    };
    assert!((transform.scale() - expected).abs() < 1e-9);
    assert_eq!(
        phases.last().map(|(_, time)| *time),
        at(20).checked_add(PINCH_GAP)
    );
}

#[test]
fn web_pinch_table() {
    super::run_table(
        "web_pinch_table",
        &[
            (
                "a_fractional_ctrl_wheel_starts_a_pinch_whose_factor_inverts_chromium",
                a_fractional_ctrl_wheel_starts_a_pinch_whose_factor_inverts_chromium,
            ),
            (
                "the_first_event_classifies_the_whole_sequence",
                the_first_event_classifies_the_whole_sequence,
            ),
            (
                "a_pinch_ends_at_its_deadline_on_the_event_timeline",
                a_pinch_ends_at_its_deadline_on_the_event_timeline,
            ),
            (
                "a_step_within_the_gap_extends_the_deadline",
                a_step_within_the_gap_extends_the_deadline,
            ),
            (
                "a_wheel_without_control_ends_the_pinch_at_its_own_time",
                a_wheel_without_control_ends_the_pinch_at_its_own_time,
            ),
            (
                "a_refused_pinch_is_cancelled_and_its_sequence_left_to_the_browser",
                a_refused_pinch_is_cancelled_and_its_sequence_left_to_the_browser,
            ),
            (
                "webkit_gesture_events_become_per_step_scale_and_rotation",
                webkit_gesture_events_become_per_step_scale_and_rotation,
            ),
            (
                "a_ctrl_wheel_pinch_assembles_into_one_pan_zoom_sequence",
                a_ctrl_wheel_pinch_assembles_into_one_pan_zoom_sequence,
            ),
        ],
    );
}
