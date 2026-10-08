//! Mouse tracking preserves owned hardware and fallback contact identities.

use flui_foundation::{
    RenderId,
    geometry::{Offset, Point},
};
use flui_interaction::{
    events::{CursorIcon, PointerEvent},
    routing::{HitTestEntry, HitTestResult, MouseTracker, PointerMotionKind},
};
use flui_platform_api::{
    EventTime,
    pointer::{
        DeviceId, PointerButtons, PointerId, PointerInfo, PointerKind, PointerMove,
        PointerPosition, PointerSample,
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
    known_hardware_and_fallback_contact_do_not_alias_and_callbacks_reenter();
}
