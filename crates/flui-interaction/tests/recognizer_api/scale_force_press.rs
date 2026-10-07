//! Owner lifecycle contracts at the recognizer's public dispatch boundary.

use std::{cell::Cell, rc::Rc};

use flui_foundation::geometry::Offset;
use flui_interaction::arena::GestureArena;
use flui_interaction::events::{
    PointerEvent, PointerType, make_down_event_for_id, make_move_event_for_id, make_up_event_for_id,
};
use flui_interaction::recognizers::{
    CancelOutcome, ForcePressGestureRecognizer, GestureRecognizer, ScaleGestureRecognizer,
};
use flui_interaction::routing::PointerDispatch;
use flui_interaction::PointerId;

fn pointer(raw: u64) -> PointerId {
    PointerId::new(raw).expect("nonzero pointer")
}

fn down(raw: u64, x: f64) -> PointerEvent {
    make_down_event_for_id(pointer(raw), Offset::new(x, 0.0), PointerType::Touch)
}

fn pressure(mut event: PointerEvent, reading: f32) -> PointerEvent {
    match &mut event {
        PointerEvent::Down(data) | PointerEvent::Up(data) => data.state.pressure = reading,
        PointerEvent::Move(data) => data.current.pressure = reading,
        _ => {}
    }
    event
}

fn admit(recognizer: &dyn GestureRecognizer, arena: &GestureArena, event: &PointerEvent, raw: u64) {
    recognizer.add_pointer(PointerDispatch::at_root(event));
    arena.close(pointer(raw));
    arena.drain_deferred_resolutions();
}

fn force_cancel_keeps_the_built_callbacks_for_the_next_contact() {
    let arena = GestureArena::new();
    let starts = Rc::new(Cell::new(0));
    let ends = Rc::new(Cell::new(0));
    let s = Rc::clone(&starts);
    let e = Rc::clone(&ends);
    let recognizer = ForcePressGestureRecognizer::builder(arena.clone())
        .on_start(move |_| s.set(s.get() + 1))
        .on_end(move |_| e.set(e.get() + 1))
        .build();
    for raw in [1, 2] {
        admit(&*recognizer, &arena, &pressure(down(raw, 0.0), 0.2), raw);
        recognizer.handle_event(PointerDispatch::at_root(&pressure(
            make_move_event_for_id(pointer(raw), Offset::ZERO, PointerType::Touch), 0.7,
        )));
        assert_eq!(recognizer.cancel(), CancelOutcome::Cancelled);
        assert_eq!(recognizer.cancel(), CancelOutcome::Idle);
    }
    assert_eq!((starts.get(), ends.get()), (2, 2));
    assert!(arena.is_empty());
}

fn scale_cancel_keeps_the_built_callbacks_for_the_next_contacts() {
    let arena = GestureArena::new();
    let starts = Rc::new(Cell::new(0));
    let cancels = Rc::new(Cell::new(0));
    let ends = Rc::new(Cell::new(0));
    let s = Rc::clone(&starts);
    let c = Rc::clone(&cancels);
    let e = Rc::clone(&ends);
    let recognizer = ScaleGestureRecognizer::builder(arena.clone())
        .on_start(move |_| s.set(s.get() + 1))
        .on_cancel(move || c.set(c.get() + 1))
        .on_end(move |_| e.set(e.get() + 1))
        .build();
    admit(&*recognizer, &arena, &down(1, 0.0), 1);
    admit(&*recognizer, &arena, &down(2, 100.0), 2);
    assert_eq!(recognizer.cancel(), CancelOutcome::Cancelled);
    assert_eq!(recognizer.cancel(), CancelOutcome::Idle);
    admit(&*recognizer, &arena, &down(1, 0.0), 1);
    admit(&*recognizer, &arena, &down(2, 100.0), 2);
    recognizer.handle_event(PointerDispatch::at_root(&make_up_event_for_id(
        pointer(2), Offset::new(100.0, 0.0), PointerType::Touch,
    )));
    recognizer.handle_event(PointerDispatch::at_root(&make_up_event_for_id(
        pointer(1), Offset::ZERO, PointerType::Touch,
    )));
    assert_eq!((starts.get(), cancels.get(), ends.get()), (2, 1, 1));
    assert!(arena.is_empty());
}

fn dropping_live_scale_and_force_owners_reports_no_terminal_callback() {
    for scale in [false, true] {
        let arena = GestureArena::new();
        let terminals = Rc::new(Cell::new(0));
        let log = Rc::clone(&terminals);
        if scale {
            let recognizer = ScaleGestureRecognizer::builder(arena.clone())
                .on_cancel(move || log.set(log.get() + 1))
                .build();
            admit(&*recognizer, &arena, &down(1, 0.0), 1);
            admit(&*recognizer, &arena, &down(2, 100.0), 2);
            drop(recognizer);
        } else {
            let recognizer = ForcePressGestureRecognizer::builder(arena.clone())
                .on_end(move |_| log.set(log.get() + 1))
                .build();
            admit(&*recognizer, &arena, &pressure(down(1, 0.0), 0.2), 1);
            recognizer.handle_event(PointerDispatch::at_root(&pressure(
                make_move_event_for_id(pointer(1), Offset::ZERO, PointerType::Touch), 0.7,
            )));
            drop(recognizer);
        }
        assert_eq!(terminals.get(), 0, "Drop is silent (scale={scale})");
        arena.drain_deferred_resolutions();
        assert!(arena.is_empty());
    }
}

#[test]
fn scale_and_force_press_owner_lifecycle() {
    let cases: &[(&str, fn())] = &[
        ("force cancel is reusable", force_cancel_keeps_the_built_callbacks_for_the_next_contact),
        ("scale cancel is reusable", scale_cancel_keeps_the_built_callbacks_for_the_next_contacts),
        ("owner Drop is silent", dropping_live_scale_and_force_owners_reports_no_terminal_callback),
    ];
    for &(name, case) in cases {
        if let Err(payload) = std::panic::catch_unwind(case) {
            eprintln!("case {name}");
            std::panic::resume_unwind(payload);
        }
    }
}
