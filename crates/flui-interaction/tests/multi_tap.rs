//! Multi-contact events preserve their public pointer identity.
use std::{cell::RefCell, rc::Rc, sync::Arc};

use flui_foundation::geometry::Offset;
use flui_interaction::arena::GestureArena;
use flui_interaction::events::{
    PointerType, make_cancel_event_for_id, make_move_event_for_id, make_up_event_for_id,
};
use flui_interaction::routing::PointerDispatch;
use flui_interaction::{GestureRecognizer, MultiTapGestureRecognizer, PointerId};

fn contact(id: u64) -> PointerId {
    PointerId::new(id).expect("nonzero contact")
}

fn pair(recognizer: &Arc<MultiTapGestureRecognizer>, a: u64, b: u64) {
    for (id, position) in [(a, Offset::new(10.0, 10.0)), (b, Offset::new(100.0, 10.0))] {
        recognizer.add_pointer(contact(id), position, position);
    }
}

fn complete(recognizer: &MultiTapGestureRecognizer, id: u64) {
    let event = make_up_event_for_id(contact(id), Offset::ZERO, PointerType::Touch);
    recognizer.handle_event(PointerDispatch::at_root(&event));
}

fn released_contacts_complete_once_in_either_order() {
    for reverse in [false, true] {
        let taps = Rc::new(RefCell::new(Vec::new()));
        let captured = Rc::clone(&taps);
        let recognizer = MultiTapGestureRecognizer::new(GestureArena::new(), 2)
            .with_on_multi_tap(move |details| captured.borrow_mut().push(details));
        for (a, b) in [(2, 3), (4, 5)] {
            pair(&recognizer, a, b);
            let (first, last) = if reverse { (b, a) } else { (a, b) };
            let before = taps.borrow().len();
            complete(&recognizer, first);
            assert_eq!(
                taps.borrow().len(),
                before,
                "first release cannot complete the pair"
            );
            complete(&recognizer, last);
            let taps = taps.borrow();
            assert_eq!(
                taps.len(),
                before + 1,
                "both contacts complete exactly once"
            );
            let details = taps.last().expect("delivered pair");
            assert_eq!(details.pointer_count, 2);
            assert_eq!(details.center, Offset::new(55.0, 10.0));
            assert!(details.positions.contains(&Offset::new(10.0, 10.0)));
            assert!(details.positions.contains(&Offset::new(100.0, 10.0)));
        }
    }
}

fn secondary_motion_uses_its_own_slop_origin() {
    let taps = Rc::new(RefCell::new(0));
    let captured = Rc::clone(&taps);
    let recognizer = MultiTapGestureRecognizer::new(GestureArena::new(), 2)
        .with_on_multi_tap(move |_| *captured.borrow_mut() += 1);
    pair(&recognizer, 2, 3);
    let motion = make_move_event_for_id(contact(3), Offset::new(101.0, 10.0), PointerType::Touch);
    recognizer.handle_event(PointerDispatch::at_root(&motion));
    complete(&recognizer, 2);
    complete(&recognizer, 3);
    assert_eq!(
        *taps.borrow(),
        1,
        "secondary motion within its own slop must keep the pair"
    );
}

fn unrelated_cancel_does_not_erase_the_pair() {
    let taps = Rc::new(RefCell::new(0));
    let captured = Rc::clone(&taps);
    let recognizer = MultiTapGestureRecognizer::new(GestureArena::new(), 2)
        .with_on_multi_tap(move |_| *captured.borrow_mut() += 1);
    pair(&recognizer, 2, 3);
    let cancel = make_cancel_event_for_id(contact(9), PointerType::Touch);
    recognizer.handle_event(PointerDispatch::at_root(&cancel));
    complete(&recognizer, 2);
    complete(&recognizer, 3);
    assert_eq!(
        *taps.borrow(),
        1,
        "untracked cancellation must not erase tracked contacts"
    );
}

fn tracked_cancel_allows_a_new_pair() {
    let taps = Rc::new(RefCell::new(0));
    let captured = Rc::clone(&taps);
    let recognizer = MultiTapGestureRecognizer::new(GestureArena::new(), 2)
        .with_on_multi_tap(move |_| *captured.borrow_mut() += 1);
    pair(&recognizer, 2, 3);
    let cancel = make_cancel_event_for_id(contact(3), PointerType::Touch);
    recognizer.handle_event(PointerDispatch::at_root(&cancel));
    complete(&recognizer, 2);
    complete(&recognizer, 3);
    assert_eq!(*taps.borrow(), 0, "cancelled pair must not complete");
    pair(&recognizer, 4, 5);
    complete(&recognizer, 4);
    complete(&recognizer, 5);
    assert_eq!(*taps.borrow(), 1, "next pair must still complete");
}

#[test]
fn multi_contact_events_keep_independent_pointer_identity() {
    let mut failures = Vec::new();
    for (name, case) in [
        (
            "release and next pair",
            released_contacts_complete_once_in_either_order as fn(),
        ),
        (
            "secondary motion",
            secondary_motion_uses_its_own_slop_origin as fn(),
        ),
        (
            "unrelated cancel",
            unrelated_cancel_does_not_erase_the_pair as fn(),
        ),
        (
            "tracked cancel recovery",
            tracked_cancel_allows_a_new_pair as fn(),
        ),
    ] {
        if let Err(payload) = std::panic::catch_unwind(case) {
            std::mem::forget(payload);
            failures.push(name);
        }
    }
    assert!(
        failures.is_empty(),
        "multi-contact cases failed: {failures:?}"
    );
}
