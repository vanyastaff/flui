//! Held presses and multi-contact taps through the immutable owner API.

use std::{
    cell::{Cell, RefCell},
    panic::{AssertUnwindSafe, catch_unwind},
    rc::{Rc, Weak},
    sync::Arc,
    time::Duration,
};

use flui_foundation::geometry::Offset;
use flui_interaction::{
    CancelOutcome, GestureArena, GestureArenaMember, GestureRecognizer, GestureSettings,
    LongPressEndDetails, LongPressGestureRecognizer, LongPressMoveUpdateDetails, ManualClock,
    MonotonicClock, MultiTapGestureRecognizer, PointerId,
    events::{
        PointerButton, PointerEvent, PointerKind, PointerMove, PointerPosition, PointerSample,
        make_down_event_for_id_with_button, make_up_event_for_id,
    },
    pointer::PointerDispatch,
};

fn pointer(raw: u64) -> PointerId {
    PointerId::new(std::num::NonZeroU64::new(raw).expect("nonzero fixture pointer"))
}

fn pen_kind() -> PointerKind {
    PointerKind::Pen {
        tool: flui_platform_api::PenTool::Tip,
    }
}

fn press_event(raw: u64, position: Offset<f64>, nanos: u64, up: bool) -> PointerEvent {
    let mut event = if up {
        make_up_event_for_id(pointer(raw), position, pen_kind()).expect("finite release")
    } else {
        make_down_event_for_id_with_button(
            pointer(raw),
            position,
            pen_kind(),
            PointerButton::PRIMARY,
        )
        .expect("finite contact")
    };
    match &mut event {
        PointerEvent::Down(press) => {
            press.sample.time = flui_platform_api::EventTime::from_nanos(nanos);
        }
        PointerEvent::Up(release) => {
            release.sample.time = flui_platform_api::EventTime::from_nanos(nanos);
        }
        _ => unreachable!("fixture creates button edges"),
    }
    event
}

fn press_move(raw: u64, readings: &[(u64, f64)], predictions: &[(u64, f64)]) -> PointerEvent {
    let sample = |(nanos, x): &(u64, f64)| {
        PointerSample::new(
            flui_platform_api::EventTime::from_nanos(*nanos),
            PointerPosition::try_new(flui_foundation::geometry::Point::new(*x, 0.0))
                .expect("finite reading"),
        )
    };
    let current = sample(readings.last().expect("current reading"));
    PointerEvent::Move(
        PointerMove::new(
            flui_interaction::events::PointerInfo::new(pointer(raw), pen_kind()),
            flui_interaction::events::PointerButtons::only(PointerButton::PRIMARY),
            current,
        )
        .with_coalesced(readings[..readings.len() - 1].iter().map(sample).collect())
        .with_predicted(predictions.iter().map(sample).collect()),
    )
}

fn long_press_payloads_preserve_spaces_history_and_terminal_time() {
    for release_time in [530_000_000, 600_000_000] {
        let clock = ManualClock::new();
        let arena = GestureArena::with_clock(Arc::new(clock.clone()));
        let updates = Rc::new(RefCell::new(Vec::new()));
        let ends = Rc::new(RefCell::new(Vec::new()));
        let update_log = Rc::clone(&updates);
        let end_log = Rc::clone(&ends);
        let recognizer = LongPressGestureRecognizer::builder(arena)
            .on_long_press_move_update(move |details: LongPressMoveUpdateDetails| {
                update_log.borrow_mut().push(details);
            })
            .on_long_press_end(move |details: LongPressEndDetails| {
                end_log.borrow_mut().push(details);
            })
            .build();
        let local_down = press_event(71, Offset::new(10.0, 0.0), 0, false);
        let root_down = press_event(71, Offset::new(100.0, 0.0), 0, false);
        recognizer.add_pointer(PointerDispatch::new(&local_down, &root_down));
        clock.advance(Duration::from_millis(500));
        recognizer.poll_deadline(super::at(&clock));
        let local_move = press_move(
            71,
            &[
                (510_000_000, 20.0),
                (520_000_000, 30.0),
                (530_000_000, 40.0),
            ],
            &[(540_000_000, 4000.0)],
        );
        let root_move = press_move(
            71,
            &[
                (510_000_000, 120.0),
                (520_000_000, 140.0),
                (530_000_000, 160.0),
            ],
            &[(540_000_000, 8000.0)],
        );
        recognizer.handle_event(PointerDispatch::new(&local_move, &root_move));
        let details = &updates.borrow()[0];
        assert_eq!(details.local_position, Offset::new(40.0, 0.0));
        assert_eq!(details.global_position, Offset::new(160.0, 0.0));
        assert_eq!(details.local_offset_from_origin, Offset::new(30.0, 0.0));
        assert_eq!(details.offset_from_origin, Offset::new(60.0, 0.0));
        assert_eq!(details.kind, pen_kind());
        let local_up = press_event(71, Offset::new(45.0, 0.0), release_time, true);
        let root_up = press_event(71, Offset::new(170.0, 0.0), release_time, true);
        recognizer.handle_event(PointerDispatch::new(&local_up, &root_up));
        let ends = ends.borrow();
        assert_eq!(ends.len(), 1);
        assert_eq!(ends[0].local_position, Offset::new(45.0, 0.0));
        assert_eq!(ends[0].global_position, Offset::new(170.0, 0.0));
        assert_eq!(ends[0].kind, pen_kind());
        let expected = if release_time == 530_000_000 {
            1000.0
        } else {
            0.0
        };
        assert!(
            (ends[0].velocity.dx() - expected).abs() < 1.0e-6,
            "measured velocity at release: {:?}",
            ends[0]
        );
        assert_eq!(ends[0].velocity.dy(), 0.0);
        assert_eq!(recognizer.cancel(), CancelOutcome::Idle);
    }
}

fn long_press_overflow_and_callback_failure_allow_fresh_terminal_delivery() {
    let clock = ManualClock::new();
    let arena = GestureArena::with_clock(Arc::new(clock.clone()));
    let updates = Rc::new(Cell::new(0));
    let cancellations = Rc::new(Cell::new(0));
    let ends = Rc::new(Cell::new(0));
    let fail = Rc::new(Cell::new(true));
    let update_count = Rc::clone(&updates);
    let cancel_count = Rc::clone(&cancellations);
    let end_count = Rc::clone(&ends);
    let callback_fail = Rc::clone(&fail);
    let recognizer = LongPressGestureRecognizer::builder(arena)
        .on_long_press_move_update(move |_| update_count.set(update_count.get() + 1))
        .on_long_press_cancel(move |_| cancel_count.set(cancel_count.get() + 1))
        .on_long_press_end(move |details: LongPressEndDetails| {
            assert!(details.velocity.dx().is_finite() && details.velocity.dy().is_finite());
            end_count.set(end_count.get() + 1);
            assert!(!callback_fail.get(), "first end callback failure");
        })
        .build();
    let extreme = press_event(72, Offset::new(-f64::MAX, 0.0), 0, false);
    recognizer.add_pointer(PointerDispatch::at_root(&extreme));
    clock.advance(Duration::from_millis(500));
    recognizer.poll_deadline(super::at(&clock));
    let overflowing = press_move(72, &[(510_000_000, f64::MAX)], &[]);
    recognizer.handle_event(PointerDispatch::at_root(&overflowing));
    assert_eq!(
        updates.get(),
        0,
        "unrepresentable displacement is not published"
    );
    assert_eq!(cancellations.get(), 1);
    for attempt in 0..2 {
        let down = press_event(72, Offset::ZERO, 0, false);
        recognizer.add_pointer(PointerDispatch::at_root(&down));
        clock.advance(Duration::from_millis(500));
        recognizer.poll_deadline(super::at(&clock));
        let up = press_event(72, Offset::ZERO, 500_000_000, true);
        let result = catch_unwind(AssertUnwindSafe(|| {
            recognizer.handle_event(PointerDispatch::at_root(&up));
        }));
        if attempt == 0 {
            let failure = result.expect_err("end callback fails once");
            assert_eq!(
                failure.downcast_ref::<&str>(),
                Some(&"first end callback failure")
            );
            fail.set(false);
        } else {
            result.expect("fresh terminal delivery recovers");
        }
        assert_eq!(recognizer.cancel(), CancelOutcome::Idle);
    }
    assert_eq!(ends.get(), 2);
}

thread_local! {
    #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
    static CLOCK_PRESS: RefCell<Option<Weak<LongPressGestureRecognizer>>> = const { RefCell::new(None) };
}

fn long_press_clock_failure_preserves_first_failure_and_replacement() {
    use std::sync::atomic::{AtomicBool, Ordering};
    #[derive(Debug)]
    struct Clock {
        now: web_time::Instant,
        armed: AtomicBool,
        replace: bool,
    }
    impl MonotonicClock for Clock {
        fn now(&self) -> web_time::Instant {
            if self.armed.swap(false, Ordering::Relaxed) {
                if self.replace {
                    let owner = CLOCK_PRESS
                        .with(|slot| slot.borrow().as_ref().and_then(Weak::upgrade))
                        .expect("live clock owner");
                    owner.cancel();
                    let down = press_event(73, Offset::new(7.0, 0.0), 0, false);
                    owner.add_pointer(PointerDispatch::at_root(&down));
                }
                std::panic::panic_any("first press clock failure");
            }
            self.now
        }
    }
    for replace in [false, true] {
        let clock = Arc::new(Clock {
            now: web_time::Instant::now(),
            armed: AtomicBool::new(false),
            replace,
        });
        let arena = GestureArena::with_clock(clock.clone());
        let fail_cancel = Rc::new(Cell::new(!replace));
        let callback_fail = Rc::clone(&fail_cancel);
        let ends = Rc::new(Cell::new(0));
        let end_count = Rc::clone(&ends);
        let recognizer = LongPressGestureRecognizer::builder(arena)
            .on_long_press_cancel(move |_| {
                assert!(!callback_fail.get(), "competing press cancellation failure");
            })
            .on_long_press_end(move |_| end_count.set(end_count.get() + 1))
            .build();
        CLOCK_PRESS.with(|slot| *slot.borrow_mut() = Some(Rc::downgrade(&recognizer)));
        let down = press_event(73, Offset::ZERO, 0, false);
        recognizer.add_pointer(PointerDispatch::at_root(&down));
        recognizer.poll_deadline(recognizer.deadline().expect("initial deadline"));
        clock.armed.store(true, Ordering::Relaxed);
        let moved = press_move(73, &[(510_000_000, 1.0)], &[]);
        let failure = catch_unwind(AssertUnwindSafe(|| {
            recognizer.handle_event(PointerDispatch::at_root(&moved));
        }))
        .expect_err("clock fails");
        assert_eq!(
            failure.downcast_ref::<&str>(),
            Some(&"first press clock failure")
        );
        fail_cancel.set(false);
        if !replace {
            assert_eq!(recognizer.cancel(), CancelOutcome::Idle);
            recognizer.add_pointer(PointerDispatch::at_root(&down));
        }
        recognizer.poll_deadline(
            recognizer
                .deadline()
                .expect("replacement contact still armed"),
        );
        let up = press_event(73, Offset::new(7.0, 0.0), 520_000_000, true);
        recognizer.handle_event(PointerDispatch::at_root(&up));
        assert_eq!(
            ends.get(),
            1,
            "fresh or clock-admitted replacement completes"
        );
        assert_eq!(recognizer.cancel(), CancelOutcome::Idle);
        CLOCK_PRESS.with(|slot| slot.borrow_mut().take());
    }
}

fn down(raw: u64) -> flui_interaction::events::PointerEvent {
    make_down_event_for_id_with_button(
        pointer(raw),
        Offset::new(raw as f64, 4.0),
        PointerKind::Touch,
        PointerButton::PRIMARY,
    )
    .expect("valid fixture sample")
}

fn up(recognizer: &dyn GestureRecognizer, raw: u64) {
    let event = make_up_event_for_id(
        pointer(raw),
        Offset::new(raw as f64, 4.0),
        PointerKind::Touch,
    )
    .expect("valid fixture sample");
    recognizer.handle_event(PointerDispatch::at_root(&event));
}

fn huge_timeout_arms_no_deadline() {
    let recognizer = LongPressGestureRecognizer::builder(GestureArena::new())
        .settings(GestureSettings::default().with_long_press_timeout(Duration::MAX))
        .build();
    recognizer.add_pointer(PointerDispatch::at_root(&down(31)));
    assert!(recognizer.deadline().is_none());
    assert_eq!(recognizer.cancel(), CancelOutcome::Cancelled);
    assert_eq!(recognizer.cancel(), CancelOutcome::Idle);
}

fn cancelling_from_start_suppresses_the_stale_start_notice() {
    let owner: Rc<RefCell<Weak<LongPressGestureRecognizer>>> = Rc::default();
    let cancel_owner = Rc::clone(&owner);
    let log = Rc::new(RefCell::new(Vec::new()));
    let simple_log = Rc::clone(&log);
    let start_log = Rc::clone(&log);
    let recognizer = LongPressGestureRecognizer::builder(GestureArena::new())
        .on_long_press(move || {
            simple_log.borrow_mut().push("simple");
            cancel_owner
                .borrow()
                .upgrade()
                .expect("live owner")
                .cancel();
        })
        .on_long_press_start(move |_| start_log.borrow_mut().push("start"))
        .build();
    *owner.borrow_mut() = Rc::downgrade(&recognizer);
    recognizer.add_pointer(PointerDispatch::at_root(&down(32)));
    recognizer.poll_deadline(recognizer.deadline().expect("armed deadline"));
    assert_eq!(*log.borrow(), ["simple"]);
    assert_eq!(recognizer.cancel(), CancelOutcome::Idle);
}

fn cancelling_from_up_suppresses_the_stale_end_notice() {
    let owner: Rc<RefCell<Weak<LongPressGestureRecognizer>>> = Rc::default();
    let cancel_owner = Rc::clone(&owner);
    let log = Rc::new(RefCell::new(Vec::new()));
    let up_log = Rc::clone(&log);
    let end_log = Rc::clone(&log);
    let recognizer = LongPressGestureRecognizer::builder(GestureArena::new())
        .on_long_press_up(move |_| {
            up_log.borrow_mut().push("up");
            cancel_owner
                .borrow()
                .upgrade()
                .expect("live owner")
                .cancel();
        })
        .on_long_press_end(move |_| end_log.borrow_mut().push("end"))
        .build();
    *owner.borrow_mut() = Rc::downgrade(&recognizer);
    for raw in [33, 34] {
        recognizer.add_pointer(PointerDispatch::at_root(&down(raw)));
        recognizer.poll_deadline(recognizer.deadline().expect("armed deadline"));
        up(recognizer.as_ref(), raw);
    }
    assert_eq!(*log.borrow(), ["up", "up"]);
    assert_eq!(recognizer.cancel(), CancelOutcome::Idle);
}

fn unsupported_long_press_button_is_not_admitted() {
    let recognizer = LongPressGestureRecognizer::builder(GestureArena::new()).build();
    let secondary = make_down_event_for_id_with_button(
        pointer(35),
        Offset::ZERO,
        PointerKind::Mouse,
        PointerButton::SECONDARY,
    )
    .expect("valid fixture sample");
    recognizer.add_pointer(PointerDispatch::at_root(&secondary));
    assert!(recognizer.deadline().is_none());
    assert_eq!(recognizer.cancel(), CancelOutcome::Idle);
}

fn next_multi_tap_after_a_callback_panic() {
    let fail = Rc::new(Cell::new(true));
    let callback_fail = Rc::clone(&fail);
    let taps = Rc::new(Cell::new(0));
    let callback_taps = Rc::clone(&taps);
    let recognizer = MultiTapGestureRecognizer::builder(GestureArena::new(), 2)
        .on_multi_tap(move |_| {
            callback_taps.set(callback_taps.get() + 1);
            if callback_fail.get() {
                std::panic::panic_any("multi tap failure");
            }
        })
        .build();
    for raw in [41, 42] {
        recognizer.add_pointer(PointerDispatch::at_root(&down(raw)));
    }
    up(recognizer.as_ref(), 41);
    let failure = catch_unwind(AssertUnwindSafe(|| up(recognizer.as_ref(), 42)))
        .expect_err("first multi tap callback fails");
    assert_eq!(failure.downcast_ref::<&str>(), Some(&"multi tap failure"));
    fail.set(false);
    for raw in [43, 44] {
        recognizer.add_pointer(PointerDispatch::at_root(&down(raw)));
    }
    up(recognizer.as_ref(), 44);
    up(recognizer.as_ref(), 43);
    assert_eq!(taps.get(), 2);
    assert_eq!(recognizer.cancel(), CancelOutcome::Idle);
}

fn incomplete_multi_tap_times_out_on_the_owner_frame() {
    let clock = ManualClock::new();
    let arena = GestureArena::with_clock(Arc::new(clock.clone()));
    let cancellations = Rc::new(Cell::new(0));
    let callback_cancellations = Rc::clone(&cancellations);
    let recognizer = MultiTapGestureRecognizer::builder(arena.clone(), 2)
        .on_multi_tap_cancel(move |_| callback_cancellations.set(callback_cancellations.get() + 1))
        .build();
    recognizer.add_pointer(PointerDispatch::at_root(&down(51)));
    assert!(arena.has_pending_deadlines());
    clock.advance(Duration::from_millis(101));
    arena.poll_deadlines();
    assert_eq!(cancellations.get(), 1);
    assert_eq!(recognizer.cancel(), CancelOutcome::Idle);
    for raw in [52, 53] {
        recognizer.add_pointer(PointerDispatch::at_root(&down(raw)));
    }
    up(recognizer.as_ref(), 52);
    up(recognizer.as_ref(), 53);
    assert_eq!(cancellations.get(), 1);
}

fn multi_tap_center_stays_finite_at_admitted_coordinate_extremes() {
    for coordinates in [[f64::MAX, f64::MAX], [f64::MAX, -f64::MAX]] {
        let details = Rc::new(RefCell::new(None));
        let captured = Rc::clone(&details);
        let recognizer = MultiTapGestureRecognizer::builder(GestureArena::new(), 2)
            .on_multi_tap(move |tap| *captured.borrow_mut() = Some(tap))
            .build();
        for (raw, x) in [(61, coordinates[0]), (62, coordinates[1])] {
            let event = make_down_event_for_id_with_button(
                pointer(raw),
                Offset::new(x, x),
                PointerKind::Touch,
                PointerButton::PRIMARY,
            )
            .expect("valid fixture sample");
            recognizer.add_pointer(PointerDispatch::at_root(&event));
        }
        up(recognizer.as_ref(), 61);
        up(recognizer.as_ref(), 62);
        let details = details.borrow();
        let center = details.as_ref().expect("completed pair").center;
        let expected = if coordinates[0] == coordinates[1] {
            f64::MAX
        } else {
            0.0
        };
        assert_eq!(center, Offset::new(expected, expected));
        assert!(center.dx.is_finite() && center.dy.is_finite());
    }
}

#[test]
fn held_press_and_multi_tap_owner_contracts() {
    let cases: &[(&str, fn())] = &[
        (
            "clock failure, cancellation competition and exact replacement",
            long_press_clock_failure_preserves_first_failure_and_replacement,
        ),
        (
            "phase payload geometry and terminal event time",
            long_press_payloads_preserve_spaces_history_and_terminal_time,
        ),
        (
            "overflow, end callback failure and same-pointer recovery",
            long_press_overflow_and_callback_failure_allow_fresh_terminal_delivery,
        ),
        ("huge timeout", huge_timeout_arms_no_deadline),
        (
            "finite center at extreme positions",
            multi_tap_center_stays_finite_at_admitted_coordinate_extremes,
        ),
        (
            "cancel during start",
            cancelling_from_start_suppresses_the_stale_start_notice,
        ),
        (
            "cancel during up",
            cancelling_from_up_suppresses_the_stale_end_notice,
        ),
        (
            "unsupported button",
            unsupported_long_press_button_is_not_admitted,
        ),
        (
            "next multi tap after failure",
            next_multi_tap_after_a_callback_panic,
        ),
        (
            "owner-frame multi tap timeout",
            incomplete_multi_tap_times_out_on_the_owner_frame,
        ),
    ];
    let mut failed = Vec::new();
    for &(name, case) in cases {
        if let Err(payload) = catch_unwind(case) {
            failed.push(name);
            flui_foundation::panic::retain_opaque_payload(payload);
        }
    }
    assert!(failed.is_empty(), "failed cases: {failed:?}");
}
