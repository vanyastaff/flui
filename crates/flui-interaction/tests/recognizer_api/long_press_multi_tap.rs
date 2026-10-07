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
    LongPressGestureRecognizer, ManualClock, MultiTapGestureRecognizer, PointerId,
    events::{
        PointerButton, PointerKind, make_down_event_for_id_with_button, make_up_event_for_id,
    },
    routing::PointerDispatch,
};

fn pointer(raw: u64) -> PointerId {
    PointerId::new(std::num::NonZeroU64::new(raw).expect("nonzero fixture pointer"))
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
        .settings(GestureSettings::touch_defaults().with_long_press_timeout(Duration::MAX))
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
