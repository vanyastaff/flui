//! Drag notifications belong to the contact that produced them.
use std::{cell::{Cell, RefCell}, rc::Rc};
use flui_foundation::geometry::Offset;
use flui_interaction::{DragAxis, DragGestureRecognizer, GestureEndReason, GestureRecognizer, PointerId};
use flui_interaction::arena::{GestureArena, GestureArenaMember, run_pointer_lifecycle};
use flui_interaction::events::{PointerEvent, PointerType, make_down_event_for_id, make_move_event_for_id, make_up_event_for_id};
use flui_interaction::routing::PointerDispatch;

struct Rival;
impl GestureArenaMember for Rival {
    fn accept_gesture(&self, _: PointerId) {}
    fn reject_gesture(&self, _: PointerId) {}
}

#[test]
fn cancelling_drag_from_start_drops_the_stale_update_and_recovers() {
    let arena = GestureArena::binding_driven(std::sync::Arc::new(flui_interaction::ManualClock::new()));
    let pointer = PointerId::new(2).expect("nonzero pointer");
    let slot: Rc<RefCell<std::rc::Weak<DragGestureRecognizer>>> = Rc::default();
    let callback_slot = Rc::clone(&slot);
    let cancel_once = Cell::new(true);
    let (starts, updates, cancelled, completed) = (
        Rc::new(Cell::new(0)), Rc::new(Cell::new(0)), Rc::new(Cell::new(0)), Rc::new(Cell::new(0)),
    );
    let (s, u, c, e) = (starts.clone(), updates.clone(), cancelled.clone(), completed.clone());
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
        if matches!(event, PointerEvent::Down(_)) { drag.add_pointer(dispatch); }
        else { drag.handle_event(dispatch); }
        run_pointer_lifecycle(&arena, &event);
        arena.drain_deferred_resolutions();
    };
    let rival = Rc::new(Rival);
    arena.add(pointer, &rival);
    send(make_down_event_for_id(pointer, Offset::ZERO, PointerType::Touch));
    send(make_move_event_for_id(pointer, Offset::new(40.0, 0.0), PointerType::Touch));
    assert_eq!((starts.get(), updates.get(), cancelled.get()), (1, 0, 1));
    send(make_up_event_for_id(pointer, Offset::new(40.0, 0.0), PointerType::Touch));
    assert_eq!(completed.get(), 0, "retired contact cannot complete again");
    assert!(arena.is_empty());
    send(make_down_event_for_id(pointer, Offset::new(100.0, 0.0), PointerType::Touch));
    send(make_move_event_for_id(pointer, Offset::new(140.0, 0.0), PointerType::Touch));
    send(make_up_event_for_id(pointer, Offset::new(140.0, 0.0), PointerType::Touch));
    assert_eq!((starts.get(), updates.get(), cancelled.get(), completed.get()), (2, 1, 1, 1));
    assert!(arena.is_empty());
}
