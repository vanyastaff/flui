//! Owner lifecycle contracts at the recognizer's public dispatch boundary.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use flui_foundation::geometry::Offset;
use flui_interaction::PointerId;
use flui_interaction::events::{
    PointerEvent, PointerKind, make_down_event_for_id, make_move_event_for_id, make_up_event_for_id,
};
use flui_interaction::gesture::GestureArena;
use flui_interaction::gesture::{
    CancelOutcome, ForcePressGestureRecognizer, GestureRecognizer, ScaleGestureRecognizer,
};
use flui_interaction::pointer::PointerDispatch;

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

/// `event` stamped `ms` milliseconds into the timeline.
fn timed(mut event: PointerEvent, ms: u64) -> PointerEvent {
    let time = flui_platform_api::EventTime::from_nanos(ms * 1_000_000);
    match &mut event {
        PointerEvent::Down(down) => down.sample.time = time,
        PointerEvent::Up(up) => up.sample.time = time,
        PointerEvent::Move(movement) => {
            let mut sample = *movement.current();
            sample.time = time;
            *movement = flui_interaction::events::PointerMove::new(
                movement.pointer,
                movement.buttons,
                sample,
            );
        }
        _ => {}
    }
    event
}

/// Contacts pan together at 1000 px/s and lift one at a time, 8 ms apart,
/// with no move between: the release still flings at their speed, in a
/// pan-or-scale lifting 2 → 0 and a scale lifting 3 → 1.
fn a_staggered_lift_keeps_the_focal_fling() {
    use flui_interaction::gesture::ScaleStartMode;
    for (mode, contacts) in [
        (ScaleStartMode::PanOrScale, 2_u64),
        (ScaleStartMode::Scale, 3),
    ] {
        let arena = GestureArena::new();
        let flings = Rc::new(RefCell::new(Vec::new()));
        let observed = Rc::clone(&flings);
        let recognizer = ScaleGestureRecognizer::builder(arena.clone())
            .start_mode(mode)
            .on_end(move |end| observed.borrow_mut().push(end.focal_fling_velocity()))
            .build();
        let start = |raw: u64| 100.0 * raw as f64;
        for raw in 1..=contacts {
            admit(&*recognizer, &arena, &timed(down(raw, start(raw)), 0), raw);
        }
        for step in 1..=5_u64 {
            for raw in 1..=contacts {
                let x = start(raw) + 10.0 * step as f64;
                let moved =
                    make_move_event_for_id(pointer(raw), Offset::new(x, 0.0), PointerKind::Touch)
                        .expect("finite move");
                recognizer.handle_event(PointerDispatch::at_root(&timed(moved, step * 10)));
            }
        }
        for (lift, raw) in (1..=contacts).enumerate() {
            let x = start(raw) + 50.0;
            let up = make_up_event_for_id(pointer(raw), Offset::new(x, 0.0), PointerKind::Touch)
                .expect("finite release");
            recognizer.handle_event(PointerDispatch::at_root(&timed(up, 50 + 8 * lift as u64)));
            arena.sweep(pointer(raw));
            arena.drain_deferred_resolutions();
        }
        let fling = flings.borrow().first().copied().expect("the scale ended");
        assert!(
            fling.pixels_per_second.dx > 500.0,
            "{mode:?}: released at {fling:?}"
        );
        assert!(arena.is_empty());
    }
}

/// Two touches in one column have no horizontal span: a half-pixel drift
/// against it cannot set a baseline a later 40 px slide divides into a
/// 40x horizontal scale.
fn a_same_column_pinch_does_not_jump_its_horizontal_scale() {
    let arena = GestureArena::new();
    let horizontal = Rc::new(RefCell::new(Vec::new()));
    let observed = Rc::clone(&horizontal);
    let recognizer = ScaleGestureRecognizer::builder(arena.clone())
        .on_update(move |update| observed.borrow_mut().push(update.horizontal_scale))
        .build();
    let touch = |raw: u64, x: f64, y: f64| {
        make_down_event_for_id(pointer(raw), Offset::new(x, y), PointerKind::Touch)
            .expect("finite touch")
    };
    admit(&*recognizer, &arena, &touch(1, 200.0, 100.0), 1);
    admit(&*recognizer, &arena, &touch(2, 200.0, 300.0), 2);
    for x in [201.0, 240.0] {
        let moved = make_move_event_for_id(pointer(2), Offset::new(x, 300.0), PointerKind::Touch)
            .expect("finite move");
        recognizer.handle_event(PointerDispatch::at_root(&moved));
    }
    let published = horizontal.borrow().clone();
    assert!(
        published.iter().all(|factor| (factor - 1.0).abs() < 1e-9),
        "horizontal scale {published:?}"
    );
    assert!(!published.is_empty(), "the scale published its motion");
    recognizer.cancel();
    assert!(arena.is_empty());
}

#[test]
fn scale_and_force_press_owner_lifecycle() {
    let cases: &[(&str, fn())] = &[
        (
            "carried scale preserves representable contraction",
            carried_scale_does_not_underflow_its_intermediate_ratio,
        ),
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
            "measured pressure excursions survive packet coalescing",
            force_history_preserves_thresholds_and_first_failure,
        ),
        (
            "measured pressure and geometry keep chronological order",
            force_history_geometry_keeps_chronological_order,
        ),
        (
            "replacement revokes old pressure history",
            force_history_replacement_revokes_tail,
        ),
        (
            "missing pressure preserves the measured claim coordinates",
            force_missing_pressure_preserves_claim_coordinates,
        ),
        (
            "scale start reenters cancel",
            scale_start_can_cancel_reentrantly_and_begin_a_healthy_later_scale,
        ),
        (
            "a staggered lift keeps the focal fling",
            a_staggered_lift_keeps_the_focal_fling,
        ),
        (
            "a same-column pinch does not jump its horizontal scale",
            a_same_column_pinch_does_not_jump_its_horizontal_scale,
        ),
    ];
    let mut failures = Vec::new();
    for &(name, case) in cases {
        if let Err(payload) = std::panic::catch_unwind(case) {
            flui_foundation::panic::retain_opaque_payload(payload);
            failures.push(name);
        }
    }
    assert!(failures.is_empty(), "failed cases: {failures:?}");
}

fn force_history_preserves_thresholds_and_first_failure() {
    use flui_platform_api::EventTime;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    for failing in [false, true] {
        let arena = GestureArena::new();
        let log = Rc::new(RefCell::new(Vec::new()));
        let first_start = Rc::new(Cell::new(failing));
        let first_peak = Rc::new(Cell::new(failing));
        let (s, p, u, e) = (log.clone(), log.clone(), log.clone(), log.clone());
        let recognizer = ForcePressGestureRecognizer::builder(arena.clone())
            .on_start(move |_| {
                s.borrow_mut().push("start");
                assert!(
                    !first_start.replace(false),
                    "first historical pressure failure"
                );
            })
            .on_peak(move |_| {
                p.borrow_mut().push("peak");
                assert!(!first_peak.replace(false), "later peak failure");
            })
            .on_update(move |_| u.borrow_mut().push("update"))
            .on_end(move |_| e.borrow_mut().push("end"))
            .build();
        for attempt in 0..2 {
            admit(&*recognizer, &arena, &pressure(down(1, 0.0), 0.2), 1);
            let sample = |reading, time| {
                let PointerEvent::Move(movement) = pressure(
                    make_move_event_for_id(pointer(1), Offset::ZERO, PointerKind::Touch)
                        .expect("Move"),
                    reading,
                ) else {
                    unreachable!("Move fixture")
                };
                let mut sample = *movement.current();
                sample.time = EventTime::from_nanos(time);
                sample
            };
            let PointerEvent::Move(movement) = pressure(
                make_move_event_for_id(pointer(1), Offset::ZERO, PointerKind::Touch).expect("Move"),
                0.2,
            ) else {
                unreachable!("Move fixture")
            };
            let packet = PointerEvent::Move(
                flui_interaction::events::PointerMove::new(
                    movement.pointer,
                    movement.buttons,
                    sample(0.2, 3),
                )
                .with_coalesced(vec![sample(0.7, 1), sample(0.9, 2)])
                .with_predicted(vec![sample(1.0, 4)]),
            );
            let outcome = catch_unwind(AssertUnwindSafe(|| {
                recognizer.handle_event(PointerDispatch::at_root(&packet));
            }));
            if failing && attempt == 0 {
                let failure = outcome.expect_err("first historical observer failure must escape");
                assert_eq!(
                    failure.downcast_ref::<&str>(),
                    Some(&"first historical pressure failure")
                );
            } else {
                outcome.expect("healthy packet delivery");
            }
            assert_eq!(
                &*log.borrow(),
                &["start", "peak", "update", "end"],
                "measured thresholds progress in order, including after caught observer failures"
            );
            log.borrow_mut().clear();
            assert_eq!(
                recognizer.cancel(),
                CancelOutcome::Idle,
                "pressure release withdrew the exact contact"
            );
            assert!(arena.is_empty());
        }
    }
}

fn force_sample(
    reading: Option<f32>,
    x: f64,
    time: u64,
) -> flui_platform_api::pointer::PointerSample {
    let PointerEvent::Move(movement) =
        make_move_event_for_id(pointer(1), Offset::new(x, 0.0), PointerKind::Touch)
            .expect("valid measured position")
    else {
        unreachable!("Move fixture")
    };
    let mut sample = *movement.current();
    sample.time = flui_platform_api::EventTime::from_nanos(time);
    sample.pressure = reading.map(|reading| {
        flui_platform_api::pointer::Pressure::try_new(reading).expect("valid measured pressure")
    });
    sample
}

fn force_packet(samples: &[flui_platform_api::pointer::PointerSample]) -> PointerEvent {
    let PointerEvent::Move(movement) =
        make_move_event_for_id(pointer(1), Offset::ZERO, PointerKind::Touch).expect("Move")
    else {
        unreachable!("Move fixture")
    };
    PointerEvent::Move(
        flui_interaction::events::PointerMove::new(
            movement.pointer,
            movement.buttons,
            *samples.last().expect("nonempty history"),
        )
        .with_coalesced(samples[..samples.len() - 1].to_vec())
        .with_predicted(vec![force_sample(Some(1.0), 0.0, 10)]),
    )
}

fn force_history_geometry_keeps_chronological_order() {
    for excursion_first in [false, true] {
        for coalesced in [false, true] {
            let arena = GestureArena::new();
            let log = Rc::new(RefCell::new(Vec::new()));
            let (s, p, u, e) = (log.clone(), log.clone(), log.clone(), log.clone());
            let recognizer = ForcePressGestureRecognizer::builder(arena.clone())
                .on_start(move |_| s.borrow_mut().push("start"))
                .on_peak(move |_| p.borrow_mut().push("peak"))
                .on_update(move |_| u.borrow_mut().push("update"))
                .on_end(move |_| e.borrow_mut().push("end"))
                .build();
            admit(&*recognizer, &arena, &pressure(down(1, 0.0), 0.2), 1);
            let readings = if excursion_first {
                [
                    (None, 100.0),
                    (Some(0.7), 1.0),
                    (Some(0.9), 2.0),
                    (None, 0.0),
                ]
            } else {
                [
                    (Some(0.7), 1.0),
                    (Some(0.9), 2.0),
                    (None, 100.0),
                    (None, 0.0),
                ]
            };
            let samples: Vec<_> = readings
                .into_iter()
                .enumerate()
                .map(|(index, (reading, x))| force_sample(reading, x, index as u64 + 1))
                .collect();
            if coalesced {
                recognizer.handle_event(PointerDispatch::at_root(&force_packet(&samples)));
            } else {
                for sample in samples {
                    recognizer.handle_event(PointerDispatch::at_root(&force_packet(&[sample])));
                }
            }
            let expected: &[&str] = if excursion_first {
                &[]
            } else {
                &["start", "peak", "update", "end"]
            };
            assert_eq!(
                &*log.borrow(),
                expected,
                "excursion_first={excursion_first}, coalesced={coalesced}"
            );
            assert_eq!(recognizer.cancel(), CancelOutcome::Idle);
            assert!(arena.is_empty());
        }
    }
    let arena = GestureArena::new();
    let starts = Rc::new(Cell::new(0));
    let seen = starts.clone();
    let recognizer = ForcePressGestureRecognizer::builder(arena.clone())
        .on_start(move |_| seen.set(seen.get() + 1))
        .build();
    admit(&*recognizer, &arena, &pressure(down(1, 0.0), 0.2), 1);
    recognizer.handle_event(PointerDispatch::at_root(&force_packet(&[force_sample(
        None, 1.0, 1,
    )])));
    assert_eq!(starts.get(), 0, "predicted pressure is not measured input");
    recognizer.cancel();
}

fn force_history_replacement_revokes_tail() {
    let arena = GestureArena::new();
    let owner = Rc::new(RefCell::new(Weak::<ForcePressGestureRecognizer>::new()));
    let starts = Rc::new(Cell::new(0));
    let peaks = Rc::new(Cell::new(0));
    let (s, p, weak, reentrant_arena) =
        (starts.clone(), peaks.clone(), owner.clone(), arena.clone());
    let recognizer = ForcePressGestureRecognizer::builder(arena.clone())
        .on_start(move |_| {
            s.set(s.get() + 1);
            if s.get() == 1 {
                let owner = weak.borrow().upgrade().expect("live owner");
                owner.cancel();
                admit(&*owner, &reentrant_arena, &pressure(down(1, 0.0), 0.2), 1);
            }
        })
        .on_peak(move |_| p.set(p.get() + 1))
        .build();
    *owner.borrow_mut() = Rc::downgrade(&recognizer);
    admit(&*recognizer, &arena, &pressure(down(1, 0.0), 0.2), 1);
    recognizer.handle_event(PointerDispatch::at_root(&force_packet(&[
        force_sample(Some(0.7), 1.0, 1),
        force_sample(Some(0.9), 2.0, 2),
    ])));
    assert_eq!(
        (starts.get(), peaks.get()),
        (1, 0),
        "old tail cannot drive the replacement contact"
    );
    recognizer.handle_event(PointerDispatch::at_root(&force_packet(&[force_sample(
        Some(0.9),
        1.0,
        3,
    )])));
    assert_eq!(
        (starts.get(), peaks.get()),
        (2, 1),
        "new contact accepts its own fresh sample"
    );
    recognizer.cancel();
    assert!(arena.is_empty());
}

fn force_missing_pressure_preserves_claim_coordinates() {
    let arena = GestureArena::new();
    let starts = Rc::new(RefCell::new(Vec::new()));
    let observed = starts.clone();
    let recognizer = ForcePressGestureRecognizer::builder(arena.clone())
        .on_start(move |details| observed.borrow_mut().push(details))
        .build();
    let down = pressure(down(1, 0.0), 0.2);
    recognizer.add_pointer(PointerDispatch::at_root(&down));
    recognizer.handle_event(PointerDispatch::at_root(&force_packet(&[
        force_sample(Some(0.7), 1.0, 1),
        force_sample(None, 2.0, 2),
    ])));
    assert!(starts.borrow().is_empty(), "arena is still open");
    arena.close(pointer(1));
    arena.drain_deferred_resolutions();
    let details = starts.borrow();
    assert_eq!(details.len(), 1);
    assert_eq!(details[0].global_position, Offset::new(1.0, 0.0));
    assert_eq!(details[0].local_position, Offset::new(1.0, 0.0));
    assert!((details[0].pressure - 0.7).abs() < 1e-6);
    drop(details);
    recognizer.cancel();
    assert!(arena.is_empty());
}

fn carried_scale_does_not_underflow_its_intermediate_ratio() {
    let arena = GestureArena::new();
    let updates = Rc::new(RefCell::new(Vec::new()));
    let observed = Rc::clone(&updates);
    let recognizer = ScaleGestureRecognizer::builder(arena.clone())
        .on_update(move |details| {
            observed
                .borrow_mut()
                .push((details.scale, details.horizontal_scale));
        })
        .build();
    let motion = |raw, x| {
        recognizer.handle_event(PointerDispatch::at_root(
            &make_move_event_for_id(pointer(raw), Offset::new(x, 0.0), PointerKind::Touch)
                .expect("finite measured contact"),
        ));
    };
    admit(&*recognizer, &arena, &down(1, 0.0), 1);
    admit(&*recognizer, &arena, &down(2, 200.0), 2);
    motion(2, 2e202);
    let expanded = *updates.borrow().last().expect("expanded scale update");
    for factor in [expanded.0, expanded.1] {
        assert!(
            (factor / 1e200 - 1.0).abs() < 1e-12,
            "expanded factor: {factor}"
        );
    }
    // A contact change carries the expanded factor into a huge new baseline.
    // The next ratio is smaller than f64 can hold, but the carried result is
    // representable: 1e200 * (tiny span / huge span) = 1e-200. The spans
    // start past the span slop, which a baseline must exceed to divide by.
    admit(&*recognizer, &arena, &down(3, 0.0), 3);
    motion(2, 2e-198);
    let contracted = *updates.borrow().last().expect("contracted scale update");
    for factor in [contracted.0, contracted.1] {
        assert!(
            (factor / 1e-200 - 1.0).abs() < 1e-12,
            "a representable carried factor must not collapse to zero: {factor}"
        );
    }
    motion(2, 0.0);
    assert_eq!(
        *updates.borrow().last().expect("zero-span scale update"),
        (0.0, 0.0),
        "coincident contacts still publish an actual zero factor"
    );
    recognizer.cancel();
    assert!(arena.is_empty());
    admit(&*recognizer, &arena, &down(1, 0.0), 1);
    admit(&*recognizer, &arena, &down(2, 200.0), 2);
    motion(2, 400.0);
    let recovered = *updates.borrow().last().expect("fresh scale update");
    assert_eq!(recovered, (2.0, 2.0));
    recognizer.cancel();
    assert!(arena.is_empty());
}
