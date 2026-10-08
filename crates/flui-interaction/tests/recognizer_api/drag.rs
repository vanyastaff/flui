//! Drag notifications belong to the contact that produced them.
use flui_foundation::geometry::Offset;
use flui_interaction::arena::{GestureArena, GestureArenaMember, run_pointer_lifecycle};
use flui_interaction::events::{
    PointerEvent, PointerKind, make_down_event_for_id, make_move_event_for_id, make_up_event_for_id,
};
use flui_interaction::routing::PointerDispatch;
use flui_interaction::{
    DragAxis, DragGestureRecognizer, GestureEndReason, GestureRecognizer, PointerId,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

struct Rival;
impl GestureArenaMember for Rival {
    fn accept_gesture(&self, _: PointerId) {}
    fn reject_gesture(&self, _: PointerId) {}
}

fn up_before_acceptance_rejects_drag_and_preserves_the_competitor() {
    let arena = GestureArena::new();
    let pointer = PointerId::new(std::num::NonZeroU64::new(2).expect("nonzero pointer"));
    let started = Rc::new(Cell::new(0));
    let cancelled = Rc::new(Cell::new(0));
    let won = Rc::new(Cell::new(0));
    let (s, c) = (started.clone(), cancelled.clone());
    let drag = DragGestureRecognizer::builder(arena.clone(), DragAxis::Vertical)
        .on_start(move |_| s.set(s.get() + 1))
        .on_cancel(move || c.set(c.get() + 1))
        .build();
    struct Competitor(Rc<Cell<usize>>);
    impl GestureArenaMember for Competitor {
        fn accept_gesture(&self, _: PointerId) {
            self.0.set(self.0.get() + 1);
        }
        fn reject_gesture(&self, _: PointerId) {}
    }
    let rival = Rc::new(Competitor(won.clone()));
    let down = make_down_event_for_id(pointer, Offset::ZERO, PointerKind::Touch)
        .expect("valid fixture sample");
    drag.add_pointer(PointerDispatch::at_root(&down));
    arena.add(pointer, &rival);
    arena.close(pointer);
    let up = make_up_event_for_id(pointer, Offset::ZERO, PointerKind::Touch)
        .expect("valid fixture sample");
    drag.handle_event(PointerDispatch::at_root(&up));
    run_pointer_lifecycle(&arena, &up);
    arena.drain_deferred_resolutions();
    assert_eq!((started.get(), cancelled.get(), won.get()), (0, 1, 1));
    assert!(arena.is_empty());
}

fn cancelling_drag_from_start_drops_the_stale_update_and_recovers() {
    let arena =
        GestureArena::binding_driven(std::sync::Arc::new(flui_interaction::ManualClock::new()));
    let pointer = PointerId::new(std::num::NonZeroU64::new(2).expect("nonzero pointer"));
    let slot: Rc<RefCell<std::rc::Weak<DragGestureRecognizer>>> = Rc::default();
    let callback_slot = Rc::clone(&slot);
    let cancel_once = Cell::new(true);
    let (starts, updates, cancelled, completed) = (
        Rc::new(Cell::new(0)),
        Rc::new(Cell::new(0)),
        Rc::new(Cell::new(0)),
        Rc::new(Cell::new(0)),
    );
    let (s, u, c, e) = (
        starts.clone(),
        updates.clone(),
        cancelled.clone(),
        completed.clone(),
    );
    let drag = DragGestureRecognizer::builder(arena.clone(), DragAxis::Free)
        .drag_start_behavior(flui_interaction::recognizers::drag::DragStartBehavior::Down)
        .on_start(move |_| {
            s.set(s.get() + 1);
            if cancel_once.replace(false) {
                let recognizer = callback_slot.borrow().upgrade().expect("routed recognizer");
                recognizer.cancel();
            }
        })
        .on_update(move |_| u.set(u.get() + 1))
        .on_end(move |details| match details.reason {
            GestureEndReason::Cancelled => c.set(c.get() + 1),
            GestureEndReason::Completed => e.set(e.get() + 1),
        })
        .build();
    *slot.borrow_mut() = Rc::downgrade(&drag);
    let send = |event: PointerEvent| {
        let dispatch = PointerDispatch::at_root(&event);
        if matches!(event, PointerEvent::Down(_)) {
            drag.add_pointer(dispatch);
        } else {
            drag.handle_event(dispatch);
        }
        run_pointer_lifecycle(&arena, &event);
        arena.drain_deferred_resolutions();
    };
    let rival = Rc::new(Rival);
    arena.add(pointer, &rival);
    send(
        make_down_event_for_id(pointer, Offset::ZERO, PointerKind::Touch)
            .expect("valid fixture sample"),
    );
    send(
        make_move_event_for_id(pointer, Offset::new(40.0, 0.0), PointerKind::Touch)
            .expect("valid fixture sample"),
    );
    assert_eq!((starts.get(), updates.get(), cancelled.get()), (1, 0, 1));
    send(
        make_up_event_for_id(pointer, Offset::new(40.0, 0.0), PointerKind::Touch)
            .expect("valid fixture sample"),
    );
    assert_eq!(completed.get(), 0, "retired contact cannot complete again");
    assert!(arena.is_empty());
    send(
        make_down_event_for_id(pointer, Offset::new(100.0, 0.0), PointerKind::Touch)
            .expect("valid fixture sample"),
    );
    send(
        make_move_event_for_id(pointer, Offset::new(140.0, 0.0), PointerKind::Touch)
            .expect("valid fixture sample"),
    );
    send(
        make_up_event_for_id(pointer, Offset::new(140.0, 0.0), PointerKind::Touch)
            .expect("valid fixture sample"),
    );
    assert_eq!(
        (
            starts.get(),
            updates.get(),
            cancelled.get(),
            completed.get()
        ),
        (2, 1, 1, 1)
    );
    assert!(arena.is_empty());
}

#[test]
fn drag_lifecycle_contracts() {
    for (name, case) in [
        (
            "remaining_touches_continue_in_admission_order_without_a_jump",
            remaining_touches_continue_in_admission_order_without_a_jump as fn(),
        ),
        (
            "continuation_cancel_commits_before_reentry_and_retains_the_first_failure",
            continuation_cancel_commits_before_reentry_and_retains_the_first_failure,
        ),
        (
            "continuation_does_not_take_a_rejected_or_other_device_contact",
            continuation_does_not_take_a_rejected_or_other_device_contact,
        ),
        (
            "up_before_acceptance_rejects_drag_and_preserves_the_competitor",
            up_before_acceptance_rejects_drag_and_preserves_the_competitor as fn(),
        ),
        (
            "cancelling_drag_from_start_drops_the_stale_update_and_recovers",
            cancelling_drag_from_start_drops_the_stale_update_and_recovers,
        ),
    ] {
        if let Err(payload) = std::panic::catch_unwind(case) {
            eprintln!("drag contract {name} failed");
            std::panic::resume_unwind(payload);
        }
    }
}

fn continuation_cancel_commits_before_reentry_and_retains_the_first_failure() {
    use flui_interaction::recognizers::drag::DragPointerStrategy;
    for callback_panics in [false, true] {
        let arena =
            GestureArena::binding_driven(std::sync::Arc::new(flui_interaction::ManualClock::new()));
        let starts = Rc::new(Cell::new(0));
        let ends = Rc::new(Cell::new(0));
        let slot: Rc<RefCell<std::rc::Weak<DragGestureRecognizer>>> = Rc::default();
        let (s, e, weak, callback_arena) =
            (starts.clone(), ends.clone(), slot.clone(), arena.clone());
        let once = Cell::new(true);
        let drag = DragGestureRecognizer::builder(arena.clone(), DragAxis::Horizontal)
            .pointer_strategy(DragPointerStrategy::ContinueWithRemaining)
            .on_start(move |_| s.set(s.get() + 1))
            .on_end(move |details| {
                e.set(e.get() + 1);
                if once.replace(false) {
                    assert_eq!(details.reason, GestureEndReason::Cancelled);
                    let recognizer = weak.borrow().upgrade().expect("live routed recognizer");
                    let event = make_down_event_for_id(
                        PointerId::try_from(2).expect("nonzero contact"),
                        Offset::new(100.0, 0.0),
                        PointerKind::Touch,
                    )
                    .expect("finite replacement");
                    recognizer.add_pointer(PointerDispatch::at_root(&event));
                    run_pointer_lifecycle(&callback_arena, &event);
                    callback_arena.drain_deferred_resolutions();
                    assert!(!callback_panics, "continuation terminal callback");
                }
            })
            .build();
        *slot.borrow_mut() = Rc::downgrade(&drag);
        for id in [2_u64, 3] {
            let down = make_down_event_for_id(
                PointerId::try_from(id).expect("nonzero contact"),
                Offset::ZERO,
                PointerKind::Touch,
            )
            .expect("finite touch");
            drag.add_pointer(PointerDispatch::at_root(&down));
            run_pointer_lifecycle(&arena, &down);
            arena.drain_deferred_resolutions();
        }
        assert_eq!(starts.get(), 1);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drag.cancel()));
        if callback_panics {
            let payload = result.expect_err("first failure resumes after sequence retirement");
            assert_eq!(
                payload.downcast_ref::<&str>(),
                Some(&"continuation terminal callback")
            );
        } else {
            result.expect("healthy cancellation");
        }
        assert_eq!(
            (starts.get(), ends.get()),
            (2, 1),
            "cancel callback can admit an exact same-pointer replacement"
        );
        let up = make_up_event_for_id(
            PointerId::try_from(2).expect("nonzero contact"),
            Offset::new(110.0, 0.0),
            PointerKind::Touch,
        )
        .expect("finite replacement release");
        drag.handle_event(PointerDispatch::at_root(&up));
        run_pointer_lifecycle(&arena, &up);
        arena.drain_deferred_resolutions();
        assert_eq!(
            ends.get(),
            2,
            "old contact retirement does not cancel the replacement"
        );
        assert!(arena.is_empty());
    }
}

fn continuation_does_not_take_a_rejected_or_other_device_contact() {
    use flui_interaction::recognizers::drag::DragPointerStrategy;
    use flui_platform_api::pointer::DeviceId;
    for secondary_device in [Some(1_u64), Some(2), None] {
        let arena =
            GestureArena::binding_driven(std::sync::Arc::new(flui_interaction::ManualClock::new()));
        let ends = Rc::new(Cell::new(0));
        let e = ends.clone();
        let drag = DragGestureRecognizer::builder(arena.clone(), DragAxis::Horizontal)
            .pointer_strategy(DragPointerStrategy::ContinueWithRemaining)
            .on_end(move |_| e.set(e.get() + 1))
            .build();
        for (id, device) in [(2_u64, Some(1)), (3, secondary_device)] {
            let mut down = make_down_event_for_id(
                PointerId::try_from(id).expect("nonzero contact"),
                Offset::ZERO,
                PointerKind::Touch,
            )
            .expect("finite touch");
            if let PointerEvent::Down(press) = &mut down
                && let Some(device) = device
            {
                press.pointer = press
                    .pointer
                    .with_device(DeviceId::try_from(device).expect("nonzero device"));
            }
            drag.add_pointer(PointerDispatch::at_root(&down));
            run_pointer_lifecycle(&arena, &down);
            arena.drain_deferred_resolutions();
        }
        let up = make_up_event_for_id(
            PointerId::try_from(2).expect("nonzero contact"),
            Offset::ZERO,
            PointerKind::Touch,
        )
        .expect("finite release");
        drag.handle_event(PointerDispatch::at_root(&up));
        run_pointer_lifecycle(&arena, &up);
        assert_eq!(
            ends.get(),
            usize::from(secondary_device != Some(1)),
            "only the same known touch device can continue: {secondary_device:?}"
        );
        drag.cancel();
        arena.drain_deferred_resolutions();
        assert!(arena.is_empty());
    }
    let arena =
        GestureArena::binding_driven(std::sync::Arc::new(flui_interaction::ManualClock::new()));
    let ends = Rc::new(Cell::new(0));
    let e = ends.clone();
    let drag = DragGestureRecognizer::builder(arena.clone(), DragAxis::Horizontal)
        .pointer_strategy(DragPointerStrategy::ContinueWithRemaining)
        .on_end(move |_| e.set(e.get() + 1))
        .build();
    let send_down = |id: u64| {
        let down = make_down_event_for_id(
            PointerId::try_from(id).expect("nonzero contact"),
            Offset::ZERO,
            PointerKind::Touch,
        )
        .expect("finite touch");
        drag.add_pointer(PointerDispatch::at_root(&down));
        run_pointer_lifecycle(&arena, &down);
        arena.drain_deferred_resolutions();
    };
    send_down(2);
    let pointer = PointerId::try_from(3).expect("nonzero contact");
    let rival = Rc::new(Rival);
    arena.add(pointer, &rival);
    let erased: Rc<dyn GestureArenaMember> = rival;
    arena.accept(pointer, &erased);
    send_down(3);
    assert_eq!(
        ends.get(),
        0,
        "passive arena rejection cannot end the active contact"
    );
    let up = make_up_event_for_id(
        PointerId::try_from(2).expect("nonzero contact"),
        Offset::ZERO,
        PointerKind::Touch,
    )
    .expect("finite release");
    drag.handle_event(PointerDispatch::at_root(&up));
    run_pointer_lifecycle(&arena, &up);
    assert_eq!(
        ends.get(),
        1,
        "rejected secondary contact cannot keep the gesture alive"
    );
    arena.sweep(pointer);
    arena.drain_deferred_resolutions();
    assert!(arena.is_empty());
}

fn remaining_touches_continue_in_admission_order_without_a_jump() {
    use flui_foundation::geometry::Point;
    use flui_interaction::recognizers::drag::DragPointerStrategy;
    use flui_platform_api::{
        EventTime,
        pointer::{
            PointerButton, PointerButtons, PointerInfo, PointerMove, PointerPosition, PointerPress,
            PointerRelease, PointerSample,
        },
    };
    for strategy in [
        DragPointerStrategy::PrimaryOnly,
        DragPointerStrategy::ContinueWithRemaining,
    ] {
        let arena =
            GestureArena::binding_driven(std::sync::Arc::new(flui_interaction::ManualClock::new()));
        let starts = Rc::new(Cell::new(0));
        let updates = Rc::new(RefCell::new(Vec::new()));
        let ends = Rc::new(RefCell::new(Vec::new()));
        let (s, u, e) = (starts.clone(), updates.clone(), ends.clone());
        let builder = DragGestureRecognizer::builder(arena.clone(), DragAxis::Horizontal)
            .on_start(move |_| s.set(s.get() + 1))
            .on_update(move |details| u.borrow_mut().push(details.primary_delta))
            .on_end(move |details| e.borrow_mut().push(details));
        // The control exercises the builder's actual default, not an explicit
        // PrimaryOnly selection that could hide an accidental default change.
        let drag = match strategy {
            DragPointerStrategy::PrimaryOnly => builder.build(),
            DragPointerStrategy::ContinueWithRemaining => {
                builder.pointer_strategy(strategy).build()
            }
            _ => unreachable!("listed strategy rows"),
        };
        let send = |id: u64, millis: u64, x: f64, phase| {
            let info = PointerInfo::new(
                PointerId::try_from(id).expect("nonzero contact"),
                PointerKind::Touch,
            );
            let sample = PointerSample::new(
                EventTime::from_nanos(millis * 1_000_000),
                PointerPosition::try_new(Point::new(x, 0.0)).expect("finite sample"),
            );
            let buttons = PointerButtons::NONE.with(PointerButton::PRIMARY);
            let event = match phase {
                0 => PointerEvent::Down(PointerPress::new(
                    info,
                    PointerButton::PRIMARY,
                    buttons,
                    sample,
                )),
                1 => PointerEvent::Move(PointerMove::new(info, buttons, sample)),
                2 => PointerEvent::Up(PointerRelease::new(
                    info,
                    PointerButton::PRIMARY,
                    PointerButtons::NONE,
                    sample,
                )),
                _ => unreachable!("scripted phase"),
            };
            if matches!(event, PointerEvent::Down(_)) {
                drag.add_pointer(PointerDispatch::at_root(&event));
            } else {
                drag.handle_event(PointerDispatch::at_root(&event));
            }
            run_pointer_lifecycle(&arena, &event);
            arena.drain_deferred_resolutions();
        };
        send(2, 0, 0.0, 0);
        send(2, 10, 40.0, 1);
        send(3, 15, 1000.0, 0);
        send(3, 20, 1020.0, 1);
        // The final successor's own measured trajectory is a constant 500
        // px/s. A sparse, sharply decelerating quadratic fit can legitimately
        // have a negative endpoint derivative despite increasing positions.
        send(4, 25, 2022.5, 0);
        send(4, 30, 2025.0, 1);
        assert_eq!(
            &*updates.borrow(),
            &[40.0],
            "passive contacts do not update: {strategy:?}"
        );
        send(2, 35, 40.0, 2);
        assert_eq!(
            ends.borrow().len(),
            usize::from(strategy == DragPointerStrategy::PrimaryOnly)
        );
        send(4, 40, 2030.0, 1);
        send(3, 45, 1030.0, 1);
        send(3, 50, 1030.0, 2);
        send(4, 60, 2040.0, 1);
        send(4, 65, 2040.0, 2);
        assert_eq!(starts.get(), 1, "one start across all handoffs");
        assert_eq!(
            ends.borrow().len(),
            1,
            "only the final tracked contact ends the gesture"
        );
        match strategy {
            DragPointerStrategy::PrimaryOnly => assert_eq!(&*updates.borrow(), &[40.0]),
            DragPointerStrategy::ContinueWithRemaining => {
                assert_eq!(
                    &*updates.borrow(),
                    &[40.0, 10.0, 10.0],
                    "earliest remaining contact wins; latest passive position is the handoff baseline"
                );
                let end = ends.borrow();
                assert_eq!(end[0].global_position.dx, 2040.0);
                assert!(
                    (end[0].primary_velocity - 500.0).abs() < 1.0,
                    "successor's measured tracker must preserve its own trajectory without inter-finger jumps: {:?}",
                    end[0]
                );
            }
            _ => unreachable!("listed strategy rows"),
        }
        assert!(arena.is_empty());
        send(2, 100, 0.0, 0);
        send(2, 110, 40.0, 1);
        send(2, 120, 40.0, 2);
        assert_eq!(
            (starts.get(), ends.borrow().len()),
            (2, 2),
            "same pointer identity recovers"
        );
        assert!(arena.is_empty());
    }
}
