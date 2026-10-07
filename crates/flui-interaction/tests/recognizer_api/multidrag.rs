//! Independent multi-drag delivery and cancellation recovery.
use flui_foundation::geometry::Offset;
use flui_interaction::arena::GestureArena;
use flui_interaction::events::{
    PointerType, make_down_event_for_id, make_move_event_for_id, make_up_event_for_id,
};
use flui_interaction::recognizers::{
    MultiDragAxis, MultiDragEndDetails, MultiDragGestureRecognizer, MultiDragHandle,
    MultiDragUpdateDetails,
};
use flui_interaction::routing::PointerDispatch;
use flui_interaction::{GestureRecognizer, PointerId};
use std::{cell::Cell, rc::Rc};

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
                updates: if pointer.get() == 1 {
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
        let pointer = PointerId::new(raw).expect("nonzero pointer");
        let event = make_down_event_for_id(pointer, position, PointerType::Touch);
        recognizer.add_pointer(PointerDispatch::at_root(&event));
        arena.close(pointer);
    }
    for (raw, position) in [
        (1, Offset::new(25.0, 0.0)),
        (1, Offset::new(30.0, 0.0)),
        (2, Offset::new(70.0, 50.0)),
    ] {
        let event = make_move_event_for_id(
            PointerId::new(raw).expect("nonzero pointer"),
            position,
            PointerType::Touch,
        );
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
                cancels: if pointer.get() == 12 {
                    Rc::new(Cell::new(0))
                } else {
                    c.clone()
                },
                fail_cancel: pointer.get() == 12,
            }) as Rc<dyn MultiDragHandle>)
        })
        .build();
    for raw in [12, 13] {
        let pointer = PointerId::new(raw).expect("nonzero pointer");
        let event = make_down_event_for_id(pointer, Offset::ZERO, PointerType::Touch);
        recognizer.add_pointer(PointerDispatch::at_root(&event));
        arena.close(pointer);
        let event = make_move_event_for_id(pointer, Offset::new(25.0, 0.0), PointerType::Touch);
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
    let pointer = PointerId::new(14).expect("nonzero pointer");
    let down = make_down_event_for_id(pointer, Offset::ZERO, PointerType::Touch);
    recognizer.add_pointer(PointerDispatch::at_root(&down));
    arena.close(pointer);
    let movement = make_move_event_for_id(pointer, Offset::new(25.0, 0.0), PointerType::Touch);
    recognizer.handle_event(PointerDispatch::at_root(&movement));
    let up = make_up_event_for_id(pointer, Offset::new(25.0, 0.0), PointerType::Touch);
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
        multidrag_contacts_update_independently as fn(),
        multidrag_cancel_finishes_every_contact_and_recovers,
    ] {
        case();
    }
}
