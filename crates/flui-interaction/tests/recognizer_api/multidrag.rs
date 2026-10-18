//! Independent multi-drag delivery and cancellation recovery.
use flui_foundation::geometry::{Matrix4, Offset};
use flui_interaction::events::{
    PointerEvent, PointerKind, make_down_event_for_id, make_move_event_for_id, make_up_event_for_id,
};
use flui_interaction::gesture::GestureContest;
use flui_interaction::gesture::{GestureArena, GestureArenaMember};
use flui_interaction::gesture::{
    MultiDragAxis, MultiDragEndDetails, MultiDragGestureRecognizer, MultiDragHandle,
    MultiDragUpdateDetails,
};
use flui_interaction::pointer::PointerDispatch;
use flui_interaction::{
    GestureRecognizer, HitTestEntry, HitTestResult, InteractionLane, PointerId, RenderId,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

fn first_multidrag_update_reports_observed_transformed_coordinates() {
    use flui_foundation::MonotonicClock;
    use flui_platform_api::EventTime;
    use std::sync::Arc;
    use std::time::Duration;

    struct Rival;
    impl GestureArenaMember for Rival {
        fn accept_gesture(&self, _: GestureContest) {}
        fn reject_gesture(&self, _: GestureContest) {}
    }
    struct ObservedHandle(Rc<RefCell<Vec<MultiDragUpdateDetails>>>);
    impl MultiDragHandle for ObservedHandle {
        fn update(&self, details: MultiDragUpdateDetails) {
            self.0.borrow_mut().push(details);
        }
        fn end(&self, _: MultiDragEndDetails) {}
        fn cancel(&self) {}
    }

    let clock = flui_foundation::ManualClock::new();
    let arena = GestureArena::binding_driven(Arc::new(clock.clone()));
    let starts = Rc::new(RefCell::new(Vec::new()));
    let updates = Rc::new(RefCell::new(Vec::new()));
    let (observed_starts, observed_updates) = (starts.clone(), updates.clone());
    let drag = MultiDragGestureRecognizer::builder(arena.clone(), MultiDragAxis::Free)
        .on_start(move |pointer, anchor| {
            observed_starts.borrow_mut().push((pointer, anchor));
            Some(Rc::new(ObservedHandle(observed_updates.clone())) as Rc<dyn MultiDragHandle>)
        })
        .build();
    let rival = Rc::new(Rival);
    let lane = InteractionLane::try_new().expect("lane");
    lane.enter(|| {
        let recipient = drag.clone();
        let target = lane
            .dispatch_handle()
            .register_pointer(move |dispatch| {
                if matches!(dispatch.local, PointerEvent::Down(_)) {
                    recipient.add_pointer(dispatch);
                } else {
                    recipient.handle_event(dispatch);
                }
            })
            .expect("target");
        let mut route = HitTestResult::new();
        route
            .with_paint_transform(
                Matrix4::translation(100.0, 200.0, 0.0) * Matrix4::scaling(2.0, 4.0, 1.0),
                |route| route.add(HitTestEntry::new(RenderId::new(1)).pointer_target(target)),
            )
            .expect("invertible transform");
        let first = PointerId::new(std::num::NonZeroU64::new(21).expect("pointer"));
        let second = PointerId::new(std::num::NonZeroU64::new(22).expect("pointer"));
        let stamp = |mut event: PointerEvent, millis: u64| {
            match &mut event {
                PointerEvent::Down(button) => {
                    button.sample.time = EventTime::from_nanos(millis * 1_000_000);
                }
                PointerEvent::Up(button) => {
                    button.sample.time = EventTime::from_nanos(millis * 1_000_000);
                }
                PointerEvent::Move(movement) => {
                    let mut sample = *movement.current();
                    sample.time = EventTime::from_nanos(millis * 1_000_000);
                    *movement = flui_platform_api::pointer::PointerMove::new(
                        movement.pointer,
                        movement.buttons,
                        sample,
                    );
                }
                _ => unreachable!("contact fixture"),
            }
            event
        };
        for (pointer, global) in [
            (first, Offset::new(120.0, 240.0)),
            (second, Offset::new(200.0, 400.0)),
        ] {
            route.dispatch(&stamp(
                make_down_event_for_id(pointer, global, PointerKind::Touch).expect("down"),
                0,
            ));
            arena.add(pointer, &rival);
            arena.close(pointer);
        }
        assert!(
            starts.borrow().is_empty(),
            "contested contacts await movement"
        );
        route.dispatch(&stamp(
            make_move_event_for_id(first, Offset::new(130.0, 260.0), PointerKind::Touch)
                .expect("move"),
            10,
        ));
        assert!(
            updates.borrow().is_empty(),
            "sub-slop sample does not start a client"
        );
        for (pointer, global) in [
            (first, Offset::new(180.0, 320.0)),
            (second, Offset::new(260.0, 480.0)),
        ] {
            route.dispatch(&stamp(
                make_move_event_for_id(pointer, global, PointerKind::Touch).expect("move"),
                20,
            ));
        }
        assert_eq!(
            starts.borrow().as_slice(),
            [
                (first, Offset::new(10.0, 10.0)),
                (second, Offset::new(50.0, 50.0))
            ]
        );
        let observations = updates.borrow();
        assert_eq!(observations.len(), 2);
        for (update, pointer, global, local) in [
            (
                &observations[0],
                first,
                Offset::new(180.0, 320.0),
                Offset::new(40.0, 30.0),
            ),
            (
                &observations[1],
                second,
                Offset::new(260.0, 480.0),
                Offset::new(80.0, 70.0),
            ),
        ] {
            assert_eq!(update.pointer_id, pointer);
            assert_eq!(
                update.global_position, global,
                "first update reports latest root sample"
            );
            assert_eq!(
                update.local_position, local,
                "first update reports latest receiving-node sample"
            );
            assert_eq!(update.delta, Offset::new(30.0, 20.0));
            assert_eq!(update.timestamp, clock.now() + Duration::from_millis(20));
        }
        drop(observations);
        drag.cancel();
        arena.drain_deferred_resolutions();
        assert!(arena.is_empty());
        route.dispatch(&stamp(
            make_down_event_for_id(first, Offset::new(140.0, 280.0), PointerKind::Touch)
                .expect("reused down"),
            0,
        ));
        arena.add(first, &rival);
        arena.close(first);
        route.dispatch(&stamp(
            make_move_event_for_id(first, Offset::new(200.0, 360.0), PointerKind::Touch)
                .expect("reused move"),
            20,
        ));
        assert_eq!(
            starts.borrow().last(),
            Some(&(first, Offset::new(20.0, 20.0)))
        );
        let observations = updates.borrow();
        assert_eq!(observations.len(), 3);
        assert_eq!(observations[2].global_position, Offset::new(200.0, 360.0));
        assert_eq!(observations[2].local_position, Offset::new(50.0, 40.0));
        assert_eq!(observations[2].delta, Offset::new(30.0, 20.0));
        drop(observations);
        route.dispatch(&stamp(
            make_up_event_for_id(first, Offset::new(200.0, 360.0), PointerKind::Touch).expect("up"),
            21,
        ));
        arena.drain_deferred_resolutions();
        assert!(arena.is_empty());
    });
}

struct CountingHandle {
    updates: Rc<Cell<usize>>,
    ends: Rc<Cell<usize>>,
    cancels: Rc<Cell<usize>>,
    fail_cancel: bool,
}
impl MultiDragHandle for CountingHandle {
    fn update(&self, _: MultiDragUpdateDetails) {
        self.updates.set(self.updates.get() + 1);
    }
    fn end(&self, _: MultiDragEndDetails) {
        self.ends.set(self.ends.get() + 1);
    }
    fn cancel(&self) {
        self.cancels.set(self.cancels.get() + 1);
        assert!(!self.fail_cancel, "multi-drag cancel panic");
    }
}

fn multidrag_contacts_update_independently() {
    let arena = GestureArena::new();
    let first_updates = Rc::new(Cell::new(0));
    let second_updates = Rc::new(Cell::new(0));
    let (a, b) = (first_updates.clone(), second_updates.clone());
    let recognizer = MultiDragGestureRecognizer::builder(arena.clone(), MultiDragAxis::Free)
        .on_start(move |pointer, _| {
            Some(Rc::new(CountingHandle {
                updates: if pointer
                    == PointerId::new(std::num::NonZeroU64::new(1).expect("nonzero pointer"))
                {
                    a.clone()
                } else {
                    b.clone()
                },
                ends: Rc::new(Cell::new(0)),
                cancels: Rc::new(Cell::new(0)),
                fail_cancel: false,
            }) as Rc<dyn MultiDragHandle>)
        })
        .build();
    for (raw, position) in [(1, Offset::ZERO), (2, Offset::new(50.0, 50.0))] {
        let pointer = PointerId::new(std::num::NonZeroU64::new(raw).expect("nonzero pointer"));
        let event = make_down_event_for_id(pointer, position, PointerKind::Touch)
            .expect("valid fixture sample");
        recognizer.add_pointer(PointerDispatch::at_root(&event));
        arena.close(pointer);
    }
    for (raw, position) in [
        (1, Offset::new(25.0, 0.0)),
        (1, Offset::new(30.0, 0.0)),
        (2, Offset::new(70.0, 50.0)),
    ] {
        let event = make_move_event_for_id(
            PointerId::new(std::num::NonZeroU64::new(raw).expect("nonzero pointer")),
            position,
            PointerKind::Touch,
        )
        .expect("valid fixture sample");
        recognizer.handle_event(PointerDispatch::at_root(&event));
    }
    assert_eq!((first_updates.get(), second_updates.get()), (2, 1));
    recognizer.cancel();
    assert!(arena.is_empty());
}

fn multidrag_cancel_finishes_every_contact_and_recovers() {
    let arena = GestureArena::new();
    let later_cancels = Rc::new(Cell::new(0));
    let ends = Rc::new(Cell::new(0));
    let (c, e) = (later_cancels.clone(), ends.clone());
    let recognizer = MultiDragGestureRecognizer::builder(arena.clone(), MultiDragAxis::Free)
        .on_start(move |pointer, _| {
            Some(Rc::new(CountingHandle {
                updates: Rc::new(Cell::new(0)),
                ends: e.clone(),
                cancels: if pointer
                    == PointerId::new(std::num::NonZeroU64::new(12).expect("nonzero pointer"))
                {
                    Rc::new(Cell::new(0))
                } else {
                    c.clone()
                },
                fail_cancel: pointer
                    == PointerId::new(std::num::NonZeroU64::new(12).expect("nonzero pointer")),
            }) as Rc<dyn MultiDragHandle>)
        })
        .build();
    for raw in [12, 13] {
        let pointer = PointerId::new(std::num::NonZeroU64::new(raw).expect("nonzero pointer"));
        let event = make_down_event_for_id(pointer, Offset::ZERO, PointerKind::Touch)
            .expect("valid fixture sample");
        recognizer.add_pointer(PointerDispatch::at_root(&event));
        arena.close(pointer);
        let event = make_move_event_for_id(pointer, Offset::new(25.0, 0.0), PointerKind::Touch)
            .expect("valid fixture sample");
        recognizer.handle_event(PointerDispatch::at_root(&event));
    }
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| recognizer.cancel()))
        .expect_err("first client cancellation must propagate");
    assert_eq!(
        flui_foundation::panic::payload_text(failure.as_ref()),
        Some("multi-drag cancel panic")
    );
    assert_eq!(
        later_cancels.get(),
        1,
        "hostile client cannot starve another contact"
    );
    assert!(arena.is_empty());
    let pointer = PointerId::new(std::num::NonZeroU64::new(14).expect("nonzero pointer"));
    let down = make_down_event_for_id(pointer, Offset::ZERO, PointerKind::Touch)
        .expect("valid fixture sample");
    recognizer.add_pointer(PointerDispatch::at_root(&down));
    arena.close(pointer);
    let movement = make_move_event_for_id(pointer, Offset::new(25.0, 0.0), PointerKind::Touch)
        .expect("valid fixture sample");
    recognizer.handle_event(PointerDispatch::at_root(&movement));
    let up = make_up_event_for_id(pointer, Offset::new(25.0, 0.0), PointerKind::Touch)
        .expect("valid fixture sample");
    recognizer.handle_event(PointerDispatch::at_root(&up));
    assert_eq!(
        ends.get(),
        1,
        "the next contact completes after containment"
    );
    assert!(arena.is_empty());
}

#[test]
fn multidrag_lifecycle_contracts() {
    for case in [
        first_multidrag_update_reports_observed_transformed_coordinates as fn(),
        multidrag_contacts_update_independently as fn(),
        multidrag_cancel_finishes_every_contact_and_recovers,
    ] {
        case();
    }
}
