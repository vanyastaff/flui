//! Cached contact routing distinguishes identity from changing pointer metadata.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use flui_foundation::{RenderId, geometry::Point};
use flui_interaction::{GestureBinding, HitTestEntry, HitTestResult, InteractionLane};
use flui_platform_api::{
    EventTime,
    pointer::{
        ButtonChange, CancelReason, DeviceId, PenTool, PointerButton, PointerButtons,
        PointerCancel, PointerEvent, PointerId, PointerInfo, PointerKind, PointerMove,
        PointerPosition, PointerPress, PointerRelease, PointerRole, PointerSample, Pressure,
    },
};

fn source(device: u64, kind: PointerKind) -> PointerInfo {
    PointerInfo::new(PointerId::try_from(7_u64).expect("nonzero pointer"), kind)
        .with_device(DeviceId::try_from(device).expect("nonzero device"))
        .with_role(PointerRole::Primary)
}

fn sample(time: u64, x: f64) -> PointerSample {
    PointerSample::new(
        EventTime::from_nanos(time),
        PointerPosition::try_new(Point::new(x, 0.0)).expect("finite position"),
    )
}

fn down(pointer: PointerInfo, time: u64) -> PointerEvent {
    PointerEvent::Down(PointerPress::new(
        pointer,
        PointerButton::PRIMARY,
        PointerButtons::NONE,
        sample(time, 10.0),
    ))
}

fn up(pointer: PointerInfo, time: u64) -> PointerEvent {
    PointerEvent::Up(PointerRelease::new(
        pointer,
        PointerButton::PRIMARY,
        PointerButtons::NONE,
        sample(time, 30.0),
    ))
}

#[derive(Clone, Copy)]
enum ForeignEdge {
    Move,
    Up,
    Cancel,
    ButtonPress,
    ButtonRelease,
}

fn foreign_event(edge: ForeignEdge, pointer: PointerInfo) -> PointerEvent {
    let held = PointerButtons::only(PointerButton::PRIMARY);
    match edge {
        ForeignEdge::Move => PointerEvent::Move(PointerMove::new(pointer, held, sample(30, 200.0))),
        ForeignEdge::Up => up(pointer, 30),
        ForeignEdge::Cancel => PointerEvent::Cancel(PointerCancel::new(
            pointer,
            EventTime::from_nanos(30),
            CancelReason::Platform,
        )),
        ForeignEdge::ButtonPress => PointerEvent::ButtonChange(ButtonChange::Pressed(
            PointerPress::new(pointer, PointerButton::SECONDARY, held, sample(30, 200.0)),
        )),
        ForeignEdge::ButtonRelease => PointerEvent::ButtonChange(ButtonChange::Released(
            PointerRelease::new(pointer, PointerButton::SECONDARY, held, sample(30, 200.0)),
        )),
    }
}

fn foreign_device_does_not_consume_the_captured_contact(edge: ForeignEdge) {
    let lane = InteractionLane::try_new().expect("interaction lane");
    let handle = lane.dispatch_handle();
    let binding = GestureBinding::new();
    let own = source(11, PointerKind::Mouse);
    let foreign = source(12, PointerKind::Mouse);
    let seen = Rc::new(RefCell::new(Vec::new()));
    lane.enter(|| {
        let log = seen.clone();
        let target = handle
            .register_pointer(move |dispatch| {
                log.borrow_mut().push(dispatch.local.clone());
            })
            .expect("pointer target");
        let mut path = HitTestResult::new();
        path.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target));
        let started = down(own, 10);
        let movement = PointerEvent::Move(PointerMove::new(
            own,
            PointerButtons::only(PointerButton::PRIMARY),
            sample(20, 20.0),
        ));
        binding.handle_pointer_event_with_result(&started, &path);
        binding.handle_pointer_event_with_result(&movement, &HitTestResult::new());
        assert!(
            binding.has_pending_motion(),
            "own Move was accepted for the next frame"
        );
        binding.handle_pointer_event(&foreign_event(edge, foreign), |_| {
            panic!("a foreign event must not acquire a fresh route for an existing contact")
        });
        let active_after_foreign = binding.active_pointer_count();
        let pending_after_foreign = binding.has_pending_motion();
        let finished = up(own, 40);
        binding.handle_pointer_event_with_result(&finished, &HitTestResult::new());
        // Also exercise the next independent contact after the valid terminal.
        let recovered_down = down(own, 50);
        let recovered_up = up(own, 60);
        binding.handle_pointer_event_with_result(&recovered_down, &path);
        binding.handle_pointer_event_with_result(&recovered_up, &HitTestResult::new());
        assert_eq!(
            active_after_foreign, 1,
            "foreign edge cannot detach the own contact"
        );
        assert!(
            pending_after_foreign,
            "foreign edge cannot consume own accepted Move debt"
        );
        assert_eq!(
            *seen.borrow(),
            [started, movement, finished, recovered_down, recovered_up]
        );
        assert_eq!(binding.active_pointer_count(), 0);
        assert!(!binding.has_pending_motion());
    });
}

fn foreign_move_preserves_contact() {
    foreign_device_does_not_consume_the_captured_contact(ForeignEdge::Move);
}
fn foreign_up_preserves_contact() {
    foreign_device_does_not_consume_the_captured_contact(ForeignEdge::Up);
}
fn foreign_cancel_preserves_contact() {
    foreign_device_does_not_consume_the_captured_contact(ForeignEdge::Cancel);
}
fn foreign_button_press_preserves_contact() {
    foreign_device_does_not_consume_the_captured_contact(ForeignEdge::ButtonPress);
}
fn foreign_button_release_preserves_contact() {
    foreign_device_does_not_consume_the_captured_contact(ForeignEdge::ButtonRelease);
}

fn metadata_change_preserves_contact_and_measured_samples(changed: PointerInfo) {
    let lane = InteractionLane::try_new().expect("interaction lane");
    let handle = lane.dispatch_handle();
    let binding = GestureBinding::new();
    let own = source(11, PointerKind::Pen { tool: PenTool::Tip });
    let seen = Rc::new(RefCell::new(Vec::new()));
    let measured = |time, x, pressure| {
        sample(time, x).with_pressure(Pressure::try_new(pressure).expect("valid pressure"))
    };
    let held = PointerButtons::only(PointerButton::PRIMARY);
    let first = PointerEvent::Move(
        PointerMove::new(own, held, measured(20, 20.0, 0.4))
            .with_coalesced(vec![measured(15, 15.0, 0.3)])
            .with_predicted(vec![measured(25, 25.0, 0.5)]),
    );
    let latest = PointerEvent::Move(
        PointerMove::new(changed, held, measured(40, 40.0, 0.7))
            .with_coalesced(vec![measured(35, 35.0, 0.6)])
            .with_predicted(vec![measured(45, 45.0, 0.8)]),
    );
    lane.enter(|| {
        let log = seen.clone();
        let target = handle
            .register_pointer(move |dispatch| {
                log.borrow_mut().push(dispatch.local.clone());
            })
            .expect("pointer target");
        let mut path = HitTestResult::new();
        path.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target));
        let started = down(own, 10);
        binding.handle_pointer_event_with_result(&started, &path);
        binding.handle_pointer_event_with_result(&first, &HitTestResult::new());
        binding.handle_pointer_event_with_result(&latest, &HitTestResult::new());
        // Different metadata must be delivered as separate packets, because
        // the coalesced samples have no field for the earlier tool/role.
        binding.flush_pending_moves();
        let finished = up(changed, 50);
        binding.handle_pointer_event_with_result(&finished, &HitTestResult::new());
        assert_eq!(*seen.borrow(), [started, first, latest, finished]);
        assert_eq!(binding.active_pointer_count(), 0);
    });
}

fn pen_tool_change_keeps_the_same_contact() {
    metadata_change_preserves_contact_and_measured_samples(source(
        11,
        PointerKind::Pen {
            tool: PenTool::Eraser,
        },
    ));
}

fn role_change_keeps_the_same_contact() {
    metadata_change_preserves_contact_and_measured_samples(
        source(11, PointerKind::Pen { tool: PenTool::Tip }).with_role(PointerRole::Additional),
    );
}

fn metadata_boundary_failure_preserves_the_newer_delivery_debt() {
    let lane = InteractionLane::try_new().expect("interaction lane");
    let handle = lane.dispatch_handle();
    let binding = GestureBinding::new();
    let own = source(11, PointerKind::Pen { tool: PenTool::Tip });
    let changed = source(
        11,
        PointerKind::Pen {
            tool: PenTool::Eraser,
        },
    );
    let fail = Rc::new(Cell::new(true));
    let seen = Rc::new(RefCell::new(Vec::new()));
    lane.enter(|| {
        let should_fail = fail.clone();
        let log = seen.clone();
        let target = handle.register_pointer(move |dispatch| {
            log.borrow_mut().push(dispatch.local.clone());
            if matches!(dispatch.local, PointerEvent::Move(movement) if movement.pointer.kind == PointerKind::Pen { tool: PenTool::Tip })
                && should_fail.replace(false)
            {
                panic!("older metadata packet first failure");
            }
        }).expect("pointer target");
        let mut path = HitTestResult::new();
        path.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target));
        let started = down(own, 10);
        let older = PointerEvent::Move(PointerMove::new(own, PointerButtons::only(PointerButton::PRIMARY), sample(20, 20.0)));
        let newer = PointerEvent::Move(PointerMove::new(changed, PointerButtons::only(PointerButton::PRIMARY), sample(30, 30.0)));
        binding.handle_pointer_event_with_result(&started, &path);
        binding.handle_pointer_event_with_result(&older, &HitTestResult::new());
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            binding.handle_pointer_event_with_result(&newer, &HitTestResult::new());
        })).expect_err("crossing metadata delivers the earlier packet");
        assert_eq!(failure.downcast_ref::<&str>(), Some(&"older metadata packet first failure"));
        assert!(binding.has_pending_motion(), "newer accepted packet survives first callback failure");
        binding.flush_pending_moves();
        let finished = up(changed, 40);
        binding.handle_pointer_event_with_result(&finished, &HitTestResult::new());
        assert_eq!(*seen.borrow(), [started, older, newer, finished]);
        assert_eq!(binding.active_pointer_count(), 0);
    });
}

fn metadata_boundary_reentry_does_not_restore_a_stale_newer_packet() {
    let lane = InteractionLane::try_new().expect("interaction lane");
    let handle = lane.dispatch_handle();
    let binding = Rc::new(GestureBinding::new());
    let own = source(11, PointerKind::Pen { tool: PenTool::Tip });
    let changed = source(
        11,
        PointerKind::Pen {
            tool: PenTool::Eraser,
        },
    );
    let seen = Rc::new(RefCell::new(Vec::new()));
    lane.enter(|| {
        let log = seen.clone();
        let replacement = handle
            .register_pointer(move |dispatch| {
                log.borrow_mut()
                    .push(("replacement", dispatch.local.clone()))
            })
            .expect("replacement target");
        let mut replacement_path = HitTestResult::new();
        replacement_path.add(HitTestEntry::new(RenderId::new(2)).pointer_target(replacement));
        let restarted = down(own, 60);
        let reentrant_down = restarted.clone();
        let nested = binding.clone();
        let log = seen.clone();
        let target = handle
            .register_pointer(move |dispatch| {
                log.borrow_mut().push(("original", dispatch.local.clone()));
                if matches!(dispatch.local, PointerEvent::Move(_)) {
                    nested.handle_pointer_event_with_result(&reentrant_down, &replacement_path);
                }
            })
            .expect("pointer target");
        let mut path = HitTestResult::new();
        path.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target));
        let started = down(own, 10);
        let older = PointerEvent::Move(PointerMove::new(
            own,
            PointerButtons::only(PointerButton::PRIMARY),
            sample(20, 20.0),
        ));
        let newer = PointerEvent::Move(PointerMove::new(
            changed,
            PointerButtons::only(PointerButton::PRIMARY),
            sample(30, 30.0),
        ));
        binding.handle_pointer_event_with_result(&started, &path);
        binding.handle_pointer_event_with_result(&older, &HitTestResult::new());
        binding.handle_pointer_event_with_result(&newer, &HitTestResult::new());
        assert_eq!(
            seen.borrow().as_slice(),
            &[
                ("original", started.clone()),
                ("original", older.clone()),
                ("replacement", restarted.clone())
            ]
        );
        assert_eq!(
            binding.flush_pending_moves(),
            0,
            "replacement must not receive the stale original metadata packet"
        );
        let finished = up(own, 70);
        binding.handle_pointer_event_with_result(&finished, &HitTestResult::new());
        assert_eq!(
            *seen.borrow(),
            [
                ("original", started),
                ("original", older),
                ("replacement", restarted),
                ("replacement", finished)
            ]
        );
        assert_eq!(binding.active_pointer_count(), 0);
    });
}

fn lifecycle_cancel_preserves_the_latest_tool_role_and_time() {
    let lane = InteractionLane::try_new().expect("interaction lane");
    let handle = lane.dispatch_handle();
    let binding = GestureBinding::new();
    let own = source(11, PointerKind::Pen { tool: PenTool::Tip });
    let changed = source(
        11,
        PointerKind::Pen {
            tool: PenTool::Eraser,
        },
    )
    .with_role(PointerRole::Additional);
    let seen = Rc::new(RefCell::new(Vec::new()));
    lane.enter(|| {
        let log = seen.clone();
        let target = handle
            .register_pointer(move |dispatch| log.borrow_mut().push(dispatch.local.clone()))
            .expect("pointer target");
        let mut path = HitTestResult::new();
        path.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target));
        let started = down(own, 10);
        let latest = PointerEvent::Move(PointerMove::new(
            changed,
            PointerButtons::only(PointerButton::PRIMARY),
            sample(40, 40.0),
        ));
        binding.handle_pointer_event_with_result(&started, &path);
        binding.handle_pointer_event_with_result(&latest, &HitTestResult::new());
        binding.cancel_active_pointers();
        let cancelled = PointerEvent::Cancel(PointerCancel::new(
            changed,
            EventTime::from_nanos(40),
            CancelReason::FocusLost,
        ));
        assert_eq!(*seen.borrow(), [started, latest, cancelled]);
        assert_eq!(binding.active_pointer_count(), 0);
        assert!(!binding.has_pending_motion());
    });
}

#[test]
fn pointer_identity_contracts() {
    let mut failures = Vec::new();
    for (name, row) in [
        (
            "lifecycle_cancel_preserves_the_latest_tool_role_and_time",
            lifecycle_cancel_preserves_the_latest_tool_role_and_time as fn(),
        ),
        (
            "metadata_boundary_failure_preserves_the_newer_delivery_debt",
            metadata_boundary_failure_preserves_the_newer_delivery_debt as fn(),
        ),
        (
            "metadata_boundary_reentry_does_not_restore_a_stale_newer_packet",
            metadata_boundary_reentry_does_not_restore_a_stale_newer_packet,
        ),
        (
            "foreign_move_preserves_contact",
            foreign_move_preserves_contact as fn(),
        ),
        ("foreign_up_preserves_contact", foreign_up_preserves_contact),
        (
            "foreign_cancel_preserves_contact",
            foreign_cancel_preserves_contact,
        ),
        (
            "foreign_button_press_preserves_contact",
            foreign_button_press_preserves_contact,
        ),
        (
            "foreign_button_release_preserves_contact",
            foreign_button_release_preserves_contact,
        ),
        (
            "pen_tool_change_keeps_the_same_contact",
            pen_tool_change_keeps_the_same_contact,
        ),
        (
            "role_change_keeps_the_same_contact",
            role_change_keeps_the_same_contact,
        ),
    ] {
        if let Err(payload) = std::panic::catch_unwind(row) {
            flui_foundation::panic::retain_opaque_payload(payload);
            failures.push(name);
        }
    }
    assert!(
        failures.is_empty(),
        "pointer_identity_contracts: {failures:?}"
    );
}
