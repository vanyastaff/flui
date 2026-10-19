//! The Android motion translation (`shared::android_motion`) over plain
//! `MotionEvent` facts: what the backend delivers for pens, touchpads and
//! touches on every host, without a device.

use std::time::Duration;

use flui_platform::shared::android_motion::{
    AndroidMotion, Axes, DeviceCapabilities, DeviceDiscovery, EXIT_GRACE, Motion, MotionAction,
    PointerFacts, SOURCE_MOUSE, Tool, axis, button, native_buttons, native_device, native_sensors,
    pen_angles, pointer_kind,
};
use flui_platform::shared::pointer_assembly::{NativeCancel, PenAngles};
use flui_platform::traits::PlatformInput;
use flui_platform_api::EventTime;
use flui_platform_api::keyboard::Modifiers;
use flui_platform_api::pointer::{
    CancelReason, PanZoomPhase, PenTool, PointerButton, PointerButtons, PointerEvent, PointerKind,
    ScrollDelta, ScrollPhase,
};
use web_time::Instant;

/// `InputDevice.SOURCE_STYLUS`.
const SOURCE_STYLUS: u32 = 0x0000_4002;
/// `InputDevice.SOURCE_TOUCHSCREEN`.
const SOURCE_TOUCHSCREEN: u32 = 0x0000_1002;
/// Physical pixels per logical pixel.
const SCALE: f64 = 2.0;

/// A device whose motion ranges report every sensor axis.
fn sensing(source: u32) -> DeviceDiscovery {
    DeviceDiscovery::Present {
        capabilities: DeviceCapabilities::new(source, axis::SENSORS),
        replaced: false,
    }
}

fn at(x: f32, y: f32) -> Axes {
    Axes {
        x,
        y,
        ..Axes::default()
    }
}

fn pointer(id: i32, tool: Tool, axes: Axes) -> PointerFacts {
    PointerFacts {
        id,
        tool,
        axes,
        history: Vec::new(),
    }
}

/// A decoder fed events of one device, each at its own millisecond unless
/// told otherwise, all received at one wall-clock instant.
struct Feed {
    motion: AndroidMotion,
    now: Instant,
}

impl Feed {
    fn new() -> Self {
        Self {
            motion: AndroidMotion::default(),
            now: Instant::now(),
        }
    }

    fn event(
        &mut self,
        source: u32,
        action: MotionAction,
        ms: u64,
        buttons: u32,
        pointers: &[PointerFacts],
    ) -> Vec<PlatformInput> {
        self.motion.decode(
            &Motion {
                device: 4,
                source,
                action,
                action_index: 0,
                action_button: 0,
                buttons,
                modifiers: Modifiers::NONE,
                time: Some(EventTime::from_nanos(ms * 1_000_000)),
                pointers,
            },
            SCALE,
            &|_, _| None,
            sensing(source),
            self.now,
        )
    }

    fn pen(&mut self, action: MotionAction, ms: u64, axes: Axes) -> Vec<PlatformInput> {
        self.event(
            SOURCE_STYLUS,
            action,
            ms,
            0,
            &[pointer(0, Tool::Stylus, axes)],
        )
    }

    fn touchpad(&mut self, action: MotionAction, ms: u64, axes: &[Axes]) -> Vec<PlatformInput> {
        let pointers: Vec<_> = axes
            .iter()
            .enumerate()
            .map(|(id, axes)| pointer(i32::try_from(id).expect("two fingers"), Tool::Finger, *axes))
            .collect();
        self.event(SOURCE_MOUSE, action, ms, 0, &pointers)
    }
}

/// `events` as verbs with their pointer ids, device and capability events
/// left out.
fn verbs(events: &[PlatformInput]) -> Vec<String> {
    events
        .iter()
        .filter_map(PlatformInput::as_pointer)
        .filter_map(|event| {
            Some(match event {
                PointerEvent::Enter(signal) => format!("enter {}", signal.pointer.id.get()),
                PointerEvent::Leave(signal) => format!("leave {}", signal.pointer.id.get()),
                PointerEvent::Move(movement) => format!("move {}", movement.pointer.id.get()),
                PointerEvent::Down(press) => format!("down {}", press.pointer.id.get()),
                PointerEvent::Up(release) => format!("up {}", release.pointer.id.get()),
                PointerEvent::Cancel(cancel) => format!("cancel {}", cancel.pointer.id.get()),
                PointerEvent::Scroll(scroll) => format!(
                    "scroll {} {:?} {} {}",
                    scroll.pointer.id.get(),
                    scroll.phase,
                    scroll.delta.x(),
                    scroll.delta.y()
                ),
                PointerEvent::PanZoom(gesture) => match gesture.phase {
                    PanZoomPhase::Start => format!("panzoom {} start", gesture.pointer().id.get()),
                    PanZoomPhase::Update(transform) => format!(
                        "panzoom {} scale {}",
                        gesture.pointer().id.get(),
                        transform.scale()
                    ),
                    PanZoomPhase::End => format!("panzoom {} end", gesture.pointer().id.get()),
                    _ => format!("panzoom {} other", gesture.pointer().id.get()),
                },
                _ => return None,
            })
        })
        .collect()
}

/// Android ends a hover with an exit at the instant the pen touches and
/// starts it again when it lifts: the pen keeps one identity from hover
/// through contact and back, with no leave and no second enter.
fn a_pen_keeps_its_identity_from_hover_through_contact() {
    let mut feed = Feed::new();
    let mut events = feed.pen(MotionAction::HoverEnter, 1, at(10.0, 10.0));
    events.extend(feed.pen(MotionAction::HoverMove, 2, at(12.0, 10.0)));
    events.extend(feed.pen(MotionAction::HoverExit, 3, at(12.0, 10.0)));
    events.extend(feed.pen(MotionAction::Down, 3, at(12.0, 10.0)));
    events.extend(feed.pen(MotionAction::Up, 4, at(12.0, 10.0)));
    events.extend(feed.pen(MotionAction::HoverEnter, 4, at(12.0, 10.0)));
    assert_eq!(
        verbs(&events),
        ["enter 1", "move 1", "down 1", "up 1", "move 1"]
    );
}

/// An exit no press continues is a leave: delivered by the next event, or
/// at the end of an owner turn once [`EXIT_GRACE`] has passed, never before.
fn an_exit_no_press_continues_is_a_leave() {
    let mut feed = Feed::new();
    feed.pen(MotionAction::HoverEnter, 1, at(10.0, 10.0));
    assert!(
        feed.pen(MotionAction::HoverExit, 2, at(10.0, 10.0))
            .is_empty()
    );
    let early = feed.now + EXIT_GRACE.saturating_sub(Duration::from_millis(1));
    assert!(
        feed.motion.finish_drain(early).is_empty(),
        "held through the grace"
    );
    assert_eq!(
        verbs(&feed.motion.finish_drain(feed.now + EXIT_GRACE)),
        ["leave 1"]
    );
    assert!(
        feed.motion
            .finish_drain(feed.now + EXIT_GRACE * 2)
            .is_empty()
    );

    // A press at another instant does not continue the exit before it.
    let mut feed = Feed::new();
    feed.pen(MotionAction::HoverEnter, 1, at(10.0, 10.0));
    feed.pen(MotionAction::HoverExit, 2, at(10.0, 10.0));
    assert_eq!(
        verbs(&feed.pen(MotionAction::Down, 9, at(10.0, 10.0))),
        ["leave 1", "down 2"]
    );

    // Cancellation delivers a held exit before it ends the contacts.
    let mut feed = Feed::new();
    feed.pen(MotionAction::HoverEnter, 1, at(10.0, 10.0));
    feed.pen(MotionAction::HoverExit, 2, at(10.0, 10.0));
    assert_eq!(
        verbs(&feed.motion.cancel_contacts(NativeCancel::FocusLost)),
        ["leave 1"]
    );
}

/// A touchpad's hovering finger is its cursor, delivered as the mouse; its
/// pinch is `PanZoom` scale about that cursor, its fake fingers no touches.
fn a_touchpad_pinch_is_pan_zoom_about_its_cursor() {
    let mut feed = Feed::new();
    let mut events = feed.touchpad(MotionAction::HoverEnter, 1, &[at(200.0, 100.0)]);
    events.extend(feed.touchpad(MotionAction::HoverExit, 2, &[at(200.0, 100.0)]));
    let pinch = |scale: f32, x: f32| Axes {
        pinch_scale: scale,
        ..at(x, 100.0)
    };
    events.extend(feed.touchpad(MotionAction::Down, 2, &[pinch(1.0, 100.0)]));
    events.extend(feed.touchpad(
        MotionAction::PointerDown,
        2,
        &[pinch(1.0, 100.0), at(300.0, 100.0)],
    ));
    events.extend(feed.touchpad(MotionAction::Move, 3, &[pinch(1.5, 50.0), at(350.0, 100.0)]));
    events.extend(feed.touchpad(MotionAction::Move, 4, &[pinch(2.0, 0.0), at(400.0, 100.0)]));
    events.extend(feed.touchpad(
        MotionAction::PointerUp,
        5,
        &[pinch(1.0, 0.0), at(400.0, 100.0)],
    ));
    events.extend(feed.touchpad(MotionAction::Up, 5, &[pinch(1.0, 0.0)]));
    events.extend(feed.touchpad(MotionAction::HoverEnter, 5, &[at(200.0, 100.0)]));
    assert_eq!(
        verbs(&events),
        [
            "enter 1",
            "panzoom 2 start",
            "panzoom 2 scale 1.5",
            "panzoom 2 scale 3",
            "panzoom 2 end",
            "move 1",
        ]
    );
    let kinds: Vec<PointerKind> = events
        .iter()
        .filter_map(PlatformInput::as_pointer)
        .filter_map(|event| match event {
            PointerEvent::Enter(signal) => Some(signal.pointer.kind),
            PointerEvent::PanZoom(gesture) => Some(gesture.pointer().kind),
            _ => None,
        })
        .collect();
    assert!(kinds.contains(&PointerKind::Mouse) && kinds.contains(&PointerKind::Trackpad));
    let focal = events.iter().find_map(|event| match event.as_pointer() {
        Some(PointerEvent::PanZoom(gesture)) => Some(gesture.position.get()),
        _ => None,
    });
    let focal = focal.expect("a pinch");
    assert_eq!(
        (focal.x, focal.y),
        (100.0, 50.0),
        "the cursor, in logical pixels"
    );
}

/// A two-finger scroll is phased `Scroll` from the cursor by the gesture's
/// scroll distance, in logical pixels and the owned sign: fingers moving
/// down scroll up.
fn a_touchpad_scroll_is_phased_from_its_cursor() {
    let mut feed = Feed::new();
    let mut events = feed.touchpad(MotionAction::HoverMove, 1, &[at(200.0, 100.0)]);
    events.extend(feed.touchpad(MotionAction::HoverExit, 2, &[at(200.0, 100.0)]));
    events.extend(feed.touchpad(MotionAction::Down, 2, &[at(200.0, 100.0)]));
    let moved = Axes {
        gesture_scroll_x: 4.0,
        gesture_scroll_y: -16.0,
        ..at(200.0, 116.0)
    };
    events.extend(feed.touchpad(MotionAction::Move, 3, &[moved]));
    events.extend(feed.touchpad(MotionAction::Up, 4, &[at(200.0, 116.0)]));
    assert_eq!(
        verbs(&events),
        [
            "move 1",
            "scroll 1 Some(Began) 0 0",
            "scroll 1 Some(Changed) 2 -8",
            "scroll 1 Some(Ended) 0 0",
        ]
    );
}

/// A touchpad click presses the cursor's button; a three-finger swipe,
/// which the system navigates with, delivers nothing; a touchscreen finger
/// with a pinch axis is still a touch.
fn touchpad_clicks_swipes_and_touches_stay_what_they_are() {
    let mut feed = Feed::new();
    feed.touchpad(MotionAction::HoverMove, 1, &[at(200.0, 100.0)]);
    let click = feed.event(
        SOURCE_MOUSE,
        MotionAction::Down,
        2,
        button::PRIMARY,
        &[pointer(0, Tool::Finger, at(200.0, 100.0))],
    );
    assert_eq!(verbs(&click), ["down 1"]);

    let mut feed = Feed::new();
    let swipe = Axes {
        swipe_fingers: 3.0,
        ..at(200.0, 100.0)
    };
    let mut events = feed.touchpad(MotionAction::Down, 1, &[swipe]);
    events.extend(feed.touchpad(MotionAction::Move, 2, &[swipe]));
    events.extend(feed.touchpad(MotionAction::Up, 3, &[swipe]));
    assert!(verbs(&events).is_empty(), "{events:?}");

    let mut feed = Feed::new();
    let touch = feed.event(
        SOURCE_TOUCHSCREEN,
        MotionAction::Down,
        1,
        0,
        &[pointer(
            0,
            Tool::Finger,
            Axes {
                pinch_scale: 1.0,
                ..at(10.0, 10.0)
            },
        )],
    );
    assert_eq!(verbs(&touch), ["down 1"]);
    assert!(touch.iter().any(|event| matches!(
        event.as_pointer(),
        Some(PointerEvent::Down(press)) if press.pointer.kind == PointerKind::Touch
    )));
}

/// Tools, buttons, pen angles and sensor presence by the device's motion
/// ranges, not by whether an axis reads zero.
fn readings_follow_the_device_and_its_ranges() {
    assert_eq!(
        pointer_kind(Tool::Eraser),
        PointerKind::Pen {
            tool: PenTool::Eraser
        }
    );
    assert_eq!(pointer_kind(Tool::Unknown), PointerKind::Unknown);
    let held = native_buttons(button::STYLUS_PRIMARY | button::STYLUS_SECONDARY, true);
    for expected in [
        PointerButton::PRIMARY,
        PointerButton::SECONDARY,
        PointerButton::AUXILIARY,
    ] {
        assert!(held.contains(expected));
    }
    assert_eq!(native_buttons(0, false), PointerButtons::NONE);
    assert_eq!(
        pen_angles(Some(core::f64::consts::FRAC_PI_4), Some(0.0)),
        Some(PenAngles::Spherical {
            altitude: Some(core::f64::consts::FRAC_PI_4),
            azimuth: Some(-core::f64::consts::FRAC_PI_2),
        })
    );
    assert_eq!(pen_angles(Some(f64::NAN), None), None);
    let pen = PointerKind::Pen { tool: PenTool::Tip };
    let zero = Axes::default();
    let all = native_sensors(pen, Some(DeviceCapabilities::new(0, axis::SENSORS)), &zero);
    assert_eq!(all.pressure, Some(0.0), "a known zero is a reading");
    assert_eq!(all.contact_size, Some((0.0, 0.0)));
    assert!(all.angles.is_some());
    assert_eq!(
        native_sensors(pen, Some(DeviceCapabilities::new(0, [])), &zero),
        native_sensors(pen, None, &zero),
        "no range, no sensor"
    );
    let overshoot = Axes {
        pressure: 1.4,
        ..zero
    };
    assert_eq!(
        native_sensors(
            PointerKind::Touch,
            Some(DeviceCapabilities::new(0, [axis::PRESSURE])),
            &overshoot
        )
        .pressure,
        Some(1.0)
    );
    assert_ne!(native_device(1), native_device(-1));
}

/// Pausing ends every admitted contact with the lifecycle's reason, once.
fn lifecycle_cancellation_retires_contacts() {
    let mut feed = Feed::new();
    assert!(
        feed.motion
            .cancel_contacts(NativeCancel::FocusLost)
            .is_empty()
    );
    feed.event(
        SOURCE_TOUCHSCREEN,
        MotionAction::Down,
        5,
        0,
        &[pointer(4, Tool::Finger, at(1.0, 1.0))],
    );
    let cancelled = feed.motion.cancel_contacts(NativeCancel::FocusLost);
    assert!(matches!(
        cancelled.iter().find_map(PlatformInput::as_pointer),
        Some(PointerEvent::Cancel(cancel)) if cancel.reason == CancelReason::FocusLost
    ));
    assert!(
        feed.motion
            .cancel_contacts(NativeCancel::FocusLost)
            .is_empty()
    );
}

/// A wheel reports through the projection the backend hands in, from where
/// the cursor hovers, with no phase.
fn a_wheel_scrolls_unphased_from_the_cursor() {
    let mut motion = AndroidMotion::default();
    let events = motion.decode(
        &Motion {
            device: 2,
            source: SOURCE_MOUSE,
            action: MotionAction::Scroll,
            action_index: 0,
            action_button: 0,
            buttons: 0,
            modifiers: Modifiers::NONE,
            time: Some(EventTime::from_nanos(7)),
            pointers: &[pointer(
                0,
                Tool::Mouse,
                Axes {
                    vscroll: 1.0,
                    ..at(40.0, 20.0)
                },
            )],
        },
        SCALE,
        &|horizontal, vertical| {
            ScrollDelta::try_new(
                flui_platform_api::pointer::ScrollUnit::Lines,
                f64::from(-horizontal),
                f64::from(-vertical),
            )
            .ok()
        },
        DeviceDiscovery::Unavailable,
        Instant::now(),
    );
    let scroll = events.iter().find_map(|event| match event.as_pointer() {
        Some(PointerEvent::Scroll(scroll)) => Some(*scroll),
        _ => None,
    });
    let scroll = scroll.expect("a wheel step");
    assert_eq!(
        (scroll.delta.y(), scroll.phase),
        (-1.0, None::<ScrollPhase>)
    );
    assert_eq!(scroll.position.get().x, 20.0);
}

#[test]
fn android_motion_translation() {
    crate::run_table(
        "android_motion_translation",
        &[
            (
                "a_pen_keeps_its_identity_from_hover_through_contact",
                a_pen_keeps_its_identity_from_hover_through_contact,
            ),
            (
                "an_exit_no_press_continues_is_a_leave",
                an_exit_no_press_continues_is_a_leave,
            ),
            (
                "a_touchpad_pinch_is_pan_zoom_about_its_cursor",
                a_touchpad_pinch_is_pan_zoom_about_its_cursor,
            ),
            (
                "a_touchpad_scroll_is_phased_from_its_cursor",
                a_touchpad_scroll_is_phased_from_its_cursor,
            ),
            (
                "touchpad_clicks_swipes_and_touches_stay_what_they_are",
                touchpad_clicks_swipes_and_touches_stay_what_they_are,
            ),
            (
                "readings_follow_the_device_and_its_ranges",
                readings_follow_the_device_and_its_ranges,
            ),
            (
                "lifecycle_cancellation_retires_contacts",
                lifecycle_cancellation_retires_contacts,
            ),
            (
                "a_wheel_scrolls_unphased_from_the_cursor",
                a_wheel_scrolls_unphased_from_the_cursor,
            ),
        ],
    );
}
