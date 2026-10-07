//! Raw dispatch and mouse tracking preserve owned source identity and readings.

use flui_foundation::{
    RenderId,
    geometry::{Offset, Point},
};
use flui_interaction::{
    RawInputHandler,
    events::{CursorIcon, PointerEvent},
    routing::{HitTestEntry, HitTestResult, MouseTracker, PointerMotionKind},
};
use flui_platform_api::{
    EventTime,
    pointer::{
        CancelReason, DeviceId, PointerButton, PointerButtons, PointerCancel, PointerDeviceChange,
        PointerId, PointerInfo, PointerKind, PointerMove, PointerPosition, PointerPress,
        PointerSample, Pressure,
    },
};
use std::{cell::RefCell, rc::Rc};

fn info(id: u64) -> PointerInfo {
    PointerInfo::new(
        PointerId::try_from(id).expect("nonzero fixture ID"),
        PointerKind::Mouse,
    )
}

fn sample(time: u64, x: f64) -> PointerSample {
    PointerSample::new(
        EventTime::from_nanos(time),
        PointerPosition::try_new(Point::new(x, 0.0)).expect("finite fixture"),
    )
}

fn raw_view_keeps_source_samples_and_allows_same_handle_reentry() {
    let handler = RawInputHandler::new();
    let callback_handler = handler.clone();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let first = Rc::clone(&seen);
    let second = Rc::clone(&seen);
    handler.set_callback(move |raw| {
        let PointerEvent::Move(movement) = raw.event() else {
            panic!("expected movement")
        };
        assert_eq!(movement.coalesced(), &[sample(0, 1.0)]);
        assert_eq!(movement.predicted(), &[sample(30, 4.0)]);
        assert_eq!(movement.current().pressure.map(Pressure::get), Some(0.25));
        assert_eq!(raw.timestamp(), Some(EventTime::from_nanos(20)));
        assert_eq!(raw.position(), Some(Offset::new(3.0, 0.0)));
        first.borrow_mut().push("first");
        let second = Rc::clone(&second);
        callback_handler.set_callback(move |raw| {
            assert!(raw.is_cancel());
            assert_eq!(raw.position(), None);
            second.borrow_mut().push("second");
        });
    });
    let event = PointerEvent::Move(
        PointerMove::new(
            info(1),
            PointerButtons::NONE,
            sample(20, 3.0).with_pressure(Pressure::try_new(0.25).expect("valid pressure")),
        )
        .with_coalesced(vec![sample(0, 1.0)])
        .with_predicted(vec![sample(30, 4.0)]),
    );
    let raw = handler.handle_event(&event).expect("enabled raw delivery");
    assert!(
        core::ptr::eq(raw.event(), &event),
        "the view borrows the accepted source"
    );
    assert_eq!(raw.delta(), None, "no earlier measured position");
    let cancel = PointerEvent::Cancel(PointerCancel::new(
        info(1),
        EventTime::from_nanos(40),
        CancelReason::Platform,
    ));
    handler.handle_event(&cancel);
    assert_eq!(*seen.borrow(), ["first", "second"]);
    assert_eq!(handler.tracked_pointer_count(), 0);
}

fn raw_delta_overflow_and_device_only_events_preserve_delivery() {
    let handler = RawInputHandler::new();
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);
    handler.set_callback(move |raw| {
        sink.borrow_mut()
            .push((raw.pointer(), raw.position(), raw.delta(), raw.timestamp()))
    });
    let device = DeviceId::try_from(7_u64).expect("nonzero");
    let pointer = info(1).with_device(device);
    let down = PointerEvent::Down(PointerPress::new(
        pointer,
        PointerButton::PRIMARY,
        PointerButtons::NONE,
        sample(0, -f64::MAX),
    ));
    assert_eq!(
        handler.handle_event(&down).expect("down").delta(),
        Some(Offset::ZERO)
    );
    let moved = PointerEvent::Move(PointerMove::new(
        pointer,
        PointerButtons::only(PointerButton::PRIMARY),
        sample(1, f64::MAX),
    ));
    assert_eq!(
        handler
            .handle_event(&moved)
            .expect("overflow still delivers")
            .delta(),
        None
    );
    assert_eq!(
        handler.pointer_position(pointer.id),
        Some(Offset::new(f64::MAX, 0.0))
    );
    assert_eq!(
        handler.handle_event(&moved).expect("recovery").delta(),
        Some(Offset::ZERO)
    );
    let removed = PointerEvent::DeviceRemoved(PointerDeviceChange::new(
        device,
        PointerKind::Mouse,
        EventTime::from_nanos(2),
    ));
    let raw = handler
        .handle_event(&removed)
        .expect("device lifecycle still delivers");
    assert_eq!(raw.pointer(), None);
    assert_eq!(raw.position(), None);
    assert_eq!(raw.delta(), None);
    assert_eq!(handler.active_pointer_count(), 0);
    assert_eq!(seen.borrow().len(), 4);
}

fn known_hardware_and_fallback_contact_do_not_alias_and_callbacks_reenter() {
    let tracker = MouseTracker::new();
    let known = info(2).with_device(DeviceId::try_from(1_u64).expect("nonzero"));
    let fallback = info(1);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = Rc::clone(&seen);
    let reentrant = tracker.clone();
    tracker.set_cursor_change_callback(Rc::new(move |pointer, cursor| {
        assert!(reentrant.source_position(&pointer).is_some());
        sink.borrow_mut().push((pointer, cursor));
    }));
    let mut path = HitTestResult::new();
    path.add(HitTestEntry::new(RenderId::new(1)).cursor(CursorIcon::Text));
    let first = PointerEvent::Move(PointerMove::new(
        known,
        PointerButtons::NONE,
        sample(0, 10.0),
    ));
    let second = PointerEvent::Move(PointerMove::new(
        fallback,
        PointerButtons::NONE,
        sample(0, 20.0),
    ));
    tracker.update_with_motion(&first, PointerMotionKind::Hover, &path);
    tracker.update_with_motion(&second, PointerMotionKind::Hover, &path);
    assert_eq!(
        tracker.source_position(&known),
        Some(Offset::new(10.0, 0.0))
    );
    assert_eq!(
        tracker.source_position(&fallback),
        Some(Offset::new(20.0, 0.0))
    );
    assert_eq!(
        tracker.device_position(known.device.expect("reported device")),
        Some(Offset::new(10.0, 0.0))
    );
    tracker.dispatch_window_left();
    assert_eq!(
        *seen.borrow(),
        [
            (known, CursorIcon::Text),
            (fallback, CursorIcon::Text),
            (known, CursorIcon::Default),
            (fallback, CursorIcon::Default)
        ]
    );
}

#[test]
fn pointer_source_contracts() {
    for (name, row) in [
        (
            "borrowed raw source and reentry",
            raw_view_keeps_source_samples_and_allows_same_handle_reentry as fn(),
        ),
        (
            "raw delta overflow and device lifecycle",
            raw_delta_overflow_and_device_only_events_preserve_delivery,
        ),
        (
            "known and fallback mouse identity",
            known_hardware_and_fallback_contact_do_not_alias_and_callbacks_reenter,
        ),
    ] {
        row();
        let _ = name;
    }
}
