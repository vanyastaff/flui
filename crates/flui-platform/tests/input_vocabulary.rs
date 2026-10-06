//! The `ui-events` values the backends build today become FLUI's own vocabulary without
//! losing what they carry, and what they cannot carry is read as documented in
//! `flui_platform::shared::input_vocabulary`.

use std::str::FromStr as _;

use dpi::{PhysicalPosition, PhysicalSize};
use flui_foundation::geometry::{Point, Size};
use flui_platform::shared::input_vocabulary::{
    code, key_event, modifiers, named_key, pointer_event,
};
use flui_platform_api::EventTime;
use flui_platform_api::keyboard::{
    Code, ImeComposition, Key, KeyRepeat, KeyState, Location, Modifiers, NamedKey,
};
use flui_platform_api::pointer::{
    CancelReason, ContactSize, PanZoomPhase, PenTool, PointerButton, PointerButtons, PointerEvent,
    PointerKind, Pressure, ScrollDelta, ScrollUnit,
};
use ui_events::ScrollDelta as UpScrollDelta;
use ui_events::keyboard as up_key;
use ui_events::pointer as up;

use crate::run_table;

const OBSERVED: EventTime = EventTime::from_nanos(9_000);

fn info(id: u64, pointer_type: up::PointerType) -> up::PointerInfo {
    up::PointerInfo {
        pointer_id: up::PointerId::new(id),
        persistent_device_id: None,
        pointer_type,
    }
}

fn state(x: f64, y: f64, buttons: &[up::PointerButton]) -> up::PointerState {
    let mut held = up::PointerButtons::new();
    for button in buttons {
        held.insert(*button);
    }
    up::PointerState {
        time: 1_000,
        position: PhysicalPosition::new(x, y),
        buttons: held,
        ..up::PointerState::default()
    }
}

fn convert(event: &up::PointerEvent) -> Option<PointerEvent> {
    pointer_event(event, OBSERVED)
}

/// The Win32 left-button press: 0.5 stand-in pressure, click count 1, Shift held.
fn a_mouse_press_has_no_pressure_sensor() {
    let mut pressed = state(10.5, 20.25, &[up::PointerButton::Primary]);
    pressed.pressure = 0.5;
    pressed.count = 1;
    pressed.modifiers = up_key::Modifiers::SHIFT;
    let Some(PointerEvent::Down(down)) = convert(&up::PointerEvent::Down(up::PointerButtonEvent {
        button: Some(up::PointerButton::Primary),
        pointer: info(1, up::PointerType::Mouse),
        state: pressed,
    })) else {
        panic!("a press is a Down");
    };
    assert_eq!(down.pointer.kind, PointerKind::Mouse);
    assert!(down.pointer.is_primary());
    assert_eq!(down.pointer.id.get().get(), 1);
    assert_eq!(down.button, PointerButton::PRIMARY);
    assert_eq!(down.buttons, PointerButtons::only(PointerButton::PRIMARY));
    assert_eq!(down.click_count.map(core::num::NonZeroU8::get), Some(1));
    assert_eq!(down.modifiers, Modifiers::SHIFT);
    assert_eq!(down.sample.position.get(), Point::new(10.5, 20.25));
    assert_eq!(down.sample.time, EventTime::from_nanos(1_000));
    assert_eq!(down.sample.pressure, None);
    assert_eq!(down.sample.contact_size, None);
}

fn a_release_leaves_the_released_button_out_of_the_set() {
    let Some(PointerEvent::Up(up)) = convert(&up::PointerEvent::Up(up::PointerButtonEvent {
        button: Some(up::PointerButton::Secondary),
        pointer: info(1, up::PointerType::Mouse),
        state: state(
            0.0,
            0.0,
            &[up::PointerButton::Primary, up::PointerButton::Secondary],
        ),
    })) else {
        panic!("a release is an Up");
    };
    assert_eq!(up.button, PointerButton::SECONDARY);
    assert_eq!(up.buttons, PointerButtons::only(PointerButton::PRIMARY));
}

fn side_and_extra_buttons_keep_their_numbers() {
    let held = state(
        0.0,
        0.0,
        &[
            up::PointerButton::X1,
            up::PointerButton::X2,
            up::PointerButton::B7,
        ],
    );
    let Some(PointerEvent::Move(moved)) = convert(&up::PointerEvent::Move(up::PointerUpdate {
        pointer: info(1, up::PointerType::Mouse),
        current: held,
        coalesced: Vec::new(),
        predicted: Vec::new(),
    })) else {
        panic!("a move is a Move");
    };
    let expected = PointerButtons::only(PointerButton::BACK)
        .with(PointerButton::FORWARD)
        .with(PointerButton::try_from(7).expect("button 7"));
    assert_eq!(moved.buttons, expected);
}

/// The Android/iOS/winit touch move: a real pressure, a contact ellipse, coalesced history.
fn a_touch_move_keeps_pressure_contact_and_history() {
    let mut current = state(5.0, 6.0, &[up::PointerButton::Primary]);
    current.time = 3_000;
    current.pressure = 0.75;
    current.contact_geometry = PhysicalSize::new(12.0, 8.0);
    let mut earlier = state(4.0, 5.0, &[up::PointerButton::Primary]);
    earlier.time = 2_000;
    let mut broken = earlier.clone();
    broken.position = PhysicalPosition::new(f64::NAN, 0.0);
    let Some(PointerEvent::Move(moved)) = convert(&up::PointerEvent::Move(up::PointerUpdate {
        pointer: info(7, up::PointerType::Touch),
        current,
        coalesced: vec![earlier, broken],
        predicted: Vec::new(),
    })) else {
        panic!("a move is a Move");
    };
    assert_eq!(moved.pointer.kind, PointerKind::Touch);
    assert!(!moved.pointer.is_primary());
    assert_eq!(moved.current.pressure.map(Pressure::get), Some(0.75));
    assert_eq!(
        moved.current.contact_size.map(ContactSize::get),
        Some(Size::new(12.0, 8.0))
    );
    let history: Vec<_> = moved
        .coalesced
        .iter()
        .map(|sample| sample.position.get())
        .collect();
    assert_eq!(history, [Point::new(4.0, 5.0)]);
}

fn the_eraser_button_is_the_pens_tool() {
    let Some(PointerEvent::Down(down)) = convert(&up::PointerEvent::Down(up::PointerButtonEvent {
        button: Some(up::PointerButton::PenEraser),
        pointer: info(3, up::PointerType::Pen),
        state: state(1.0, 1.0, &[up::PointerButton::PenEraser]),
    })) else {
        panic!("a press is a Down");
    };
    assert_eq!(
        down.pointer.kind,
        PointerKind::Pen {
            tool: PenTool::Eraser
        }
    );
    assert_eq!(down.button, PointerButton::PRIMARY);
    assert_eq!(down.buttons, PointerButtons::only(PointerButton::PRIMARY));
}

fn a_press_at_a_non_finite_position_is_dropped() {
    let event = up::PointerEvent::Down(up::PointerButtonEvent {
        button: Some(up::PointerButton::Primary),
        pointer: info(1, up::PointerType::Mouse),
        state: state(f64::NAN, 0.0, &[up::PointerButton::Primary]),
    });
    assert_eq!(convert(&event), None);
}

fn a_release_at_a_non_finite_position_cancels_the_sequence() {
    let event = up::PointerEvent::Up(up::PointerButtonEvent {
        button: Some(up::PointerButton::Primary),
        pointer: info(1, up::PointerType::Mouse),
        state: state(0.0, f64::INFINITY, &[]),
    });
    let Some(PointerEvent::Cancel(cancel)) = convert(&event) else {
        panic!("an unusable release still ends the sequence");
    };
    assert_eq!(cancel.reason, CancelReason::InvalidInput);
    assert_eq!(cancel.time, EventTime::from_nanos(1_000));
}

fn a_platform_cancel_and_crossings_take_the_observed_time() {
    let touch = info(4, up::PointerType::Touch);
    let Some(PointerEvent::Cancel(cancel)) = convert(&up::PointerEvent::Cancel(touch)) else {
        panic!("a cancel is a Cancel");
    };
    assert_eq!(
        (cancel.reason, cancel.time),
        (CancelReason::Platform, OBSERVED)
    );
    let Some(PointerEvent::Enter(enter)) = convert(&up::PointerEvent::Enter(touch)) else {
        panic!("an enter is an Enter");
    };
    assert_eq!((enter.time, enter.position), (OBSERVED, None));
    assert!(matches!(
        convert(&up::PointerEvent::Leave(touch)),
        Some(PointerEvent::Leave(_))
    ));
}

fn scroll(delta: UpScrollDelta, pointer: up::PointerInfo) -> Option<PointerEvent> {
    convert(&up::PointerEvent::Scroll(up::PointerScrollEvent {
        pointer,
        delta,
        state: state(2.0, 3.0, &[]),
    }))
}

fn every_scroll_unit_is_kept() {
    let rows = [
        (
            UpScrollDelta::LineDelta(0.0, -1.5),
            (ScrollUnit::Lines, 0.0, -1.5),
        ),
        (
            UpScrollDelta::PageDelta(1.0, 0.0),
            (ScrollUnit::Pages, 1.0, 0.0),
        ),
        (
            UpScrollDelta::PixelDelta(PhysicalPosition::new(4.5, -7.25)),
            (ScrollUnit::Pixels, 4.5, -7.25),
        ),
    ];
    for (upstream, expected) in rows {
        let Some(PointerEvent::Scroll(event)) = scroll(upstream, info(1, up::PointerType::Mouse))
        else {
            panic!("a scroll is a Scroll");
        };
        assert_eq!(
            (event.delta.unit(), event.delta.x(), event.delta.y()),
            expected
        );
        assert_eq!(event.position.get(), Point::new(2.0, 3.0));
        assert_eq!(event.phase, None);
    }
}

/// The web backend's wheel names no pointer.
fn a_wheel_without_a_pointer_id_is_the_primary_mouse() {
    let Some(PointerEvent::Scroll(event)) = scroll(
        UpScrollDelta::LineDelta(0.0, 1.0),
        up::PointerInfo {
            pointer_id: None,
            persistent_device_id: None,
            pointer_type: up::PointerType::Mouse,
        },
    ) else {
        panic!("a scroll is a Scroll");
    };
    assert_eq!(event.pointer.id.get().get(), 1);
    assert!(event.pointer.is_primary());
}

fn a_non_finite_scroll_delta_scrolls_nowhere() {
    let Some(PointerEvent::Scroll(event)) = scroll(
        UpScrollDelta::LineDelta(f32::NAN, f32::INFINITY),
        info(1, up::PointerType::Mouse),
    ) else {
        panic!("a scroll is a Scroll");
    };
    assert_eq!(event.delta, ScrollDelta::zero(ScrollUnit::Lines));
}

fn gesture(gesture: up::PointerGesture) -> Option<PointerEvent> {
    convert(&up::PointerEvent::Gesture(up::PointerGestureEvent {
        pointer: info(u64::MAX, up::PointerType::Touch),
        gesture,
        state: state(8.0, 9.0, &[]),
    }))
}

fn a_gesture_tick_is_a_trackpad_pan_zoom_update() {
    let Some(PointerEvent::PanZoom(pinch)) = gesture(up::PointerGesture::Pinch(0.25)) else {
        panic!("a pinch is a pan/zoom event");
    };
    assert_eq!(pinch.pointer.kind, PointerKind::Trackpad);
    let PanZoomPhase::Update(transform) = pinch.phase else {
        panic!("a tick is an Update");
    };
    assert_eq!((transform.scale(), transform.rotation()), (1.25, 0.0));
    let Some(PointerEvent::PanZoom(rotate)) = gesture(up::PointerGesture::Rotate(0.5)) else {
        panic!("a rotation is a pan/zoom event");
    };
    let PanZoomPhase::Update(transform) = rotate.phase else {
        panic!("a tick is an Update");
    };
    assert_eq!((transform.scale(), transform.rotation()), (1.0, 0.5));
    assert_eq!(gesture(up::PointerGesture::Pinch(-1.0)), None);
    assert_eq!(gesture(up::PointerGesture::Pinch(f32::NAN)), None);
}

fn every_named_key_and_code_round_trips_through_keyboard_types() {
    for key in NamedKey::ALL {
        let upstream = up_key::NamedKey::from_str(key.as_str())
            .unwrap_or_else(|_| panic!("keyboard-types lacks {key:?}"));
        assert_eq!(named_key(upstream), *key);
    }
    for physical in Code::ALL {
        let upstream = up_key::Code::from_str(physical.as_str())
            .unwrap_or_else(|_| panic!("keyboard-types lacks {physical:?}"));
        assert_eq!(code(upstream), *physical);
    }
}

#[expect(deprecated, reason = "the legacy names are what is under test")]
fn legacy_meta_names_are_meta() {
    assert_eq!(named_key(up_key::NamedKey::Hyper), NamedKey::Meta);
    assert_eq!(named_key(up_key::NamedKey::Super), NamedKey::Meta);
    assert_eq!(
        modifiers(up_key::Modifiers::SUPER | up_key::Modifiers::CONTROL),
        Modifiers::META | Modifiers::CONTROL
    );
}

fn a_key_event_keeps_every_field() {
    let upstream = up_key::KeyboardEvent {
        state: up_key::KeyState::Down,
        key: up_key::Key::Character("q".to_owned()),
        code: up_key::Code::KeyQ,
        location: up_key::Location::Left,
        modifiers: up_key::Modifiers::ALT,
        repeat: true,
        is_composing: true,
    };
    let event = key_event(&upstream, OBSERVED);
    assert_eq!(event.state, KeyState::Down);
    assert_eq!(event.key, Key::Character("q".to_owned()));
    assert_eq!(event.code, Code::KeyQ);
    assert_eq!(event.location, Location::Left);
    assert_eq!(event.modifiers, Modifiers::ALT);
    assert_eq!(event.repeat, KeyRepeat::AutoRepeat);
    assert_eq!(event.composition, ImeComposition::Active);
    assert_eq!(event.time, OBSERVED);
}

#[test]
fn input_vocabulary_conversion() {
    run_table(
        "input_vocabulary_conversion",
        &[
            (
                "a_mouse_press_has_no_pressure_sensor",
                a_mouse_press_has_no_pressure_sensor,
            ),
            (
                "a_release_leaves_the_released_button_out_of_the_set",
                a_release_leaves_the_released_button_out_of_the_set,
            ),
            (
                "side_and_extra_buttons_keep_their_numbers",
                side_and_extra_buttons_keep_their_numbers,
            ),
            (
                "a_touch_move_keeps_pressure_contact_and_history",
                a_touch_move_keeps_pressure_contact_and_history,
            ),
            (
                "the_eraser_button_is_the_pens_tool",
                the_eraser_button_is_the_pens_tool,
            ),
            (
                "a_press_at_a_non_finite_position_is_dropped",
                a_press_at_a_non_finite_position_is_dropped,
            ),
            (
                "a_release_at_a_non_finite_position_cancels_the_sequence",
                a_release_at_a_non_finite_position_cancels_the_sequence,
            ),
            (
                "a_platform_cancel_and_crossings_take_the_observed_time",
                a_platform_cancel_and_crossings_take_the_observed_time,
            ),
            ("every_scroll_unit_is_kept", every_scroll_unit_is_kept),
            (
                "a_wheel_without_a_pointer_id_is_the_primary_mouse",
                a_wheel_without_a_pointer_id_is_the_primary_mouse,
            ),
            (
                "a_non_finite_scroll_delta_scrolls_nowhere",
                a_non_finite_scroll_delta_scrolls_nowhere,
            ),
            (
                "a_gesture_tick_is_a_trackpad_pan_zoom_update",
                a_gesture_tick_is_a_trackpad_pan_zoom_update,
            ),
            (
                "every_named_key_and_code_round_trips_through_keyboard_types",
                every_named_key_and_code_round_trips_through_keyboard_types,
            ),
            ("legacy_meta_names_are_meta", legacy_meta_names_are_meta),
            (
                "a_key_event_keeps_every_field",
                a_key_event_keeps_every_field,
            ),
        ],
    );
}
