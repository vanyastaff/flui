//! Reusable owner-local composite recognizer contracts.
use flui_foundation::geometry::Offset;
use flui_interaction::{
    CancelOutcome, EagerGestureRecognizer, GestureArena, GestureRecognizer, PointerId,
    TapAndDragGestureRecognizer,
    events::{PointerKind, make_down_event_for_id, make_up_event_for_id},
    routing::PointerDispatch,
};
use std::{cell::RefCell, rc::Rc};

#[test]
fn cancelling_tapdrag_from_tap_down_invalidates_the_queued_tap_up() {
    let arena = GestureArena::new();
    let log = Rc::new(RefCell::new(Vec::new()));
    let owner = Rc::new(RefCell::new(
        std::rc::Weak::<TapAndDragGestureRecognizer>::new(),
    ));
    let cancellation = owner.clone();
    let down_log = log.clone();
    let up_log = log.clone();
    let cancel_log = log.clone();
    let recognizer = TapAndDragGestureRecognizer::builder(arena.clone())
        .on_tap_down(move |_| {
            down_log.borrow_mut().push("down");
            let recognizer = cancellation
                .borrow()
                .upgrade()
                .expect("live callback owner");
            assert_eq!(recognizer.cancel(), CancelOutcome::Cancelled);
        })
        .on_tap_up(move |_| up_log.borrow_mut().push("up"))
        .on_cancel(move || cancel_log.borrow_mut().push("cancel"))
        .build();
    *owner.borrow_mut() = Rc::downgrade(&recognizer);
    let pointer = PointerId::new(std::num::NonZeroU64::MIN);
    let down = make_down_event_for_id(pointer, Offset::ZERO, PointerKind::Touch)
        .expect("valid fixture sample");
    recognizer.add_pointer(PointerDispatch::at_root(&down));
    arena.close(pointer);
    let up = make_up_event_for_id(pointer, Offset::ZERO, PointerKind::Touch)
        .expect("valid fixture sample");
    recognizer.handle_event(PointerDispatch::at_root(&up));
    assert_eq!(&*log.borrow(), &["down", "cancel"]);
    assert_eq!(recognizer.cancel(), CancelOutcome::Idle);
}

#[test]
fn tapdrag_cancel_is_reusable_and_eager_refuses_a_second_contact() {
    let arena = GestureArena::new();
    let log = Rc::new(RefCell::new(Vec::new()));
    let output = log.clone();
    let recognizer = TapAndDragGestureRecognizer::builder(arena.clone())
        .on_tap_up(move |details| output.borrow_mut().push(details.consecutive_tap_count))
        .build();
    let pointer = PointerId::new(std::num::NonZeroU64::MIN);
    let down = make_down_event_for_id(pointer, Offset::ZERO, PointerKind::Touch)
        .expect("valid fixture sample");
    recognizer.add_pointer(PointerDispatch::at_root(&down));
    assert_eq!(recognizer.cancel(), CancelOutcome::Cancelled);
    assert_eq!(recognizer.cancel(), CancelOutcome::Idle);
    recognizer.add_pointer(PointerDispatch::at_root(&down));
    arena.close(pointer);
    arena.drain_deferred_resolutions();
    let up = make_up_event_for_id(pointer, Offset::ZERO, PointerKind::Touch)
        .expect("valid fixture sample");
    recognizer.handle_event(PointerDispatch::at_root(&up));
    assert_eq!(&*log.borrow(), &[1]);

    let arena = GestureArena::new();
    let eager = EagerGestureRecognizer::builder(arena.clone()).build();
    eager.add_pointer(PointerDispatch::at_root(&down));
    let other = PointerId::new(std::num::NonZeroU64::new(72).expect("nonzero fixture id"));
    let other_down = make_down_event_for_id(other, Offset::ZERO, PointerKind::Touch)
        .expect("valid fixture sample");
    eager.add_pointer(PointerDispatch::at_root(&other_down));
    assert!(
        !arena.contains(other),
        "busy admission must not create a second arena"
    );
    assert_eq!(eager.cancel(), CancelOutcome::Cancelled);
    assert_eq!(eager.cancel(), CancelOutcome::Idle);
    eager.add_pointer(PointerDispatch::at_root(&other_down));
    assert!(
        arena.contains(other),
        "cancel must leave the recognizer reusable"
    );
}
