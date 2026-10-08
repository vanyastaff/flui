//! Owner lifecycle contracts at the recognizer's public dispatch boundary.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use flui_foundation::geometry::Offset;
use flui_interaction::PointerId;
use flui_interaction::arena::GestureArena;
use flui_interaction::events::{
    PointerEvent, PointerKind, make_down_event_for_id, make_move_event_for_id, make_up_event_for_id,
};
use flui_interaction::recognizers::{
    CancelOutcome, ForcePressGestureRecognizer, GestureRecognizer, ScaleGestureRecognizer,
};
use flui_interaction::routing::PointerDispatch;

fn pointer(raw: u64) -> PointerId {
    PointerId::new(std::num::NonZeroU64::new(raw).expect("nonzero pointer"))
}

fn down(raw: u64, x: f64) -> PointerEvent {
    make_down_event_for_id(pointer(raw), Offset::new(x, 0.0), PointerKind::Touch)
        .expect("valid fixture sample")
}

fn pressure(mut event: PointerEvent, reading: f32) -> PointerEvent {
    match &mut event {
        PointerEvent::Down(data) => {
            data.sample.pressure = Some(
                flui_platform_api::pointer::Pressure::try_new(reading)
                    .expect("valid pressure fixture"),
            );
        }
        PointerEvent::Up(data) => {
            data.sample.pressure = Some(
                flui_platform_api::pointer::Pressure::try_new(reading)
                    .expect("valid pressure fixture"),
            );
        }
        PointerEvent::Move(data) => {
            let sample = data.current().with_pressure(
                flui_platform_api::pointer::Pressure::try_new(reading)
                    .expect("valid pressure fixture"),
            );
            *data = flui_interaction::events::PointerMove::new(data.pointer, data.buttons, sample)
                .with_modifiers(data.modifiers)
                .with_coalesced(data.coalesced().to_vec())
                .with_predicted(data.predicted().to_vec());
        }
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
            make_move_event_for_id(pointer(raw), Offset::ZERO, PointerKind::Touch)
                .expect("valid fixture sample"),
            0.7,
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
    recognizer.handle_event(PointerDispatch::at_root(
        &make_up_event_for_id(pointer(2), Offset::new(100.0, 0.0), PointerKind::Touch)
            .expect("valid fixture sample"),
    ));
    recognizer.handle_event(PointerDispatch::at_root(
        &make_up_event_for_id(pointer(1), Offset::ZERO, PointerKind::Touch)
            .expect("valid fixture sample"),
    ));
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
                make_move_event_for_id(pointer(1), Offset::ZERO, PointerKind::Touch)
                    .expect("valid fixture sample"),
                0.7,
            )));
            drop(recognizer);
        }
        assert_eq!(terminals.get(), 0, "Drop is silent (scale={scale})");
        arena.drain_deferred_resolutions();
        assert!(arena.is_empty());
    }
}

fn force_start_can_cancel_reentrantly_without_delivering_a_stale_peak() {
    let arena = GestureArena::new();
    let owner = Rc::new(RefCell::new(Weak::<ForcePressGestureRecognizer>::new()));
    let starts = Rc::new(Cell::new(0));
    let peaks = Rc::new(Cell::new(0));
    let ends = Rc::new(Cell::new(0));
    let weak = Rc::clone(&owner);
    let s = Rc::clone(&starts);
    let p = Rc::clone(&peaks);
    let e = Rc::clone(&ends);
    let recognizer = ForcePressGestureRecognizer::builder(arena.clone())
        .on_start(move |_| {
            s.set(s.get() + 1);
            if s.get() == 1 {
                let recognizer = weak.borrow().upgrade().expect("live callback owner");
                assert_eq!(recognizer.cancel(), CancelOutcome::Cancelled);
            }
        })
        .on_peak(move |_| p.set(p.get() + 1))
        .on_end(move |_| e.set(e.get() + 1))
        .build();
    *owner.borrow_mut() = Rc::downgrade(&recognizer);
    for raw in [1, 2] {
        admit(&*recognizer, &arena, &pressure(down(raw, 0.0), 0.2), raw);
        recognizer.handle_event(PointerDispatch::at_root(&pressure(
            make_move_event_for_id(pointer(raw), Offset::ZERO, PointerKind::Touch)
                .expect("valid fixture sample"),
            0.9,
        )));
    }
    assert_eq!((starts.get(), peaks.get(), ends.get()), (2, 1, 1));
    recognizer.cancel();
    assert!(arena.is_empty());
}

fn scale_start_can_cancel_reentrantly_and_begin_a_healthy_later_scale() {
    let arena = GestureArena::new();
    let owner = Rc::new(RefCell::new(Weak::<ScaleGestureRecognizer>::new()));
    let starts = Rc::new(Cell::new(0));
    let updates = Rc::new(Cell::new(0));
    let cancels = Rc::new(Cell::new(0));
    let weak = Rc::clone(&owner);
    let s = Rc::clone(&starts);
    let u = Rc::clone(&updates);
    let c = Rc::clone(&cancels);
    let recognizer = ScaleGestureRecognizer::builder(arena.clone())
        .on_start(move |_| {
            s.set(s.get() + 1);
            if s.get() == 1 {
                let recognizer = weak.borrow().upgrade().expect("live callback owner");
                assert_eq!(recognizer.cancel(), CancelOutcome::Cancelled);
            }
        })
        .on_update(move |_| u.set(u.get() + 1))
        .on_cancel(move || c.set(c.get() + 1))
        .build();
    *owner.borrow_mut() = Rc::downgrade(&recognizer);
    for base in [1, 3] {
        admit(&*recognizer, &arena, &down(base, 0.0), base);
        admit(&*recognizer, &arena, &down(base + 1, 100.0), base + 1);
        recognizer.handle_event(PointerDispatch::at_root(
            &make_move_event_for_id(
                pointer(base + 1),
                Offset::new(140.0, 0.0),
                PointerKind::Touch,
            )
            .expect("valid fixture sample"),
        ));
    }
    assert_eq!((starts.get(), updates.get(), cancels.get()), (2, 1, 1));
    recognizer.cancel();
    assert!(arena.is_empty());
}

#[test]
fn scale_and_force_press_owner_lifecycle() {
    let cases: &[(&str, fn())] = &[
        (
            "force cancel is reusable",
            force_cancel_keeps_the_built_callbacks_for_the_next_contact,
        ),
        (
            "scale cancel is reusable",
            scale_cancel_keeps_the_built_callbacks_for_the_next_contacts,
        ),
        (
            "owner Drop is silent",
            dropping_live_scale_and_force_owners_reports_no_terminal_callback,
        ),
        (
            "force start reenters cancel",
            force_start_can_cancel_reentrantly_without_delivering_a_stale_peak,
        ),
        (
            "scale start reenters cancel",
            scale_start_can_cancel_reentrantly_and_begin_a_healthy_later_scale,
        ),
    ];
    for &(name, case) in cases {
        if let Err(payload) = std::panic::catch_unwind(case) {
            eprintln!("case {name}");
            std::panic::resume_unwind(payload);
        }
    }
}
