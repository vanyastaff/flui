//! Recognizer lifecycles driven the way a binding drives them, and a
//! property test of the arena itself.
//!
//! [`Lane`] stands in for the binding: every recognizer is admitted from the
//! `Down`, other events reach every recognizer, the arena lifecycle
//! (`run_pointer_lifecycle`) runs after routing even when a callback panicked,
//! and deferred resolutions drain at input end. Time is a [`ManualClock`];
//! `frames` pumps deadline frames.

use std::{
    cell::{Cell, RefCell},
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

use flui_foundation::geometry::Offset;
use flui_interaction::arena::{
    GestureArena, GestureArenaEntry, GestureArenaMember, GestureCompetition, GestureDisposition,
    run_pointer_lifecycle,
};
use flui_interaction::events::{
    PointerButton, PointerEvent, PointerKind, make_down_event_for_id_with_button,
    make_move_event_for_id, make_up_event_for_id, make_up_event_for_id_with_button,
};
use flui_interaction::routing::PointerDispatch;
use flui_interaction::{
    DoubleTapGestureRecognizer, DragAxis, DragGestureRecognizer, GestureEndReason,
    GestureRecognizer, LongPressGestureRecognizer, ManualClock, MultiDragAxis, MultiDragEndDetails,
    MultiDragGestureRecognizer, MultiDragHandle, MultiDragUpdateDetails, MultiTapGestureRecognizer,
    PointerId, ScaleGestureRecognizer, TapAndDragGestureRecognizer, TapGestureRecognizer,
};
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Binding stand-in
// ---------------------------------------------------------------------------

type Route = Box<dyn Fn(PointerDispatch<'_>)>;

struct Lane {
    arena: GestureArena,
    clock: ManualClock,
    admit: Vec<Route>,
    forward: Vec<Route>,
}

impl Lane {
    fn new() -> Self {
        let clock = ManualClock::new();
        Self {
            arena: GestureArena::binding_driven(Arc::new(clock.clone())),
            clock,
            admit: Vec::new(),
            forward: Vec::new(),
        }
    }

    /// Route this recognizer the way `GestureDetector` does: admitted from
    /// the `Down`, every other event forwarded.
    fn join<R: GestureRecognizer + 'static>(&mut self, recognizer: &Rc<R>) {
        let admit = Rc::clone(recognizer);
        self.admit
            .push(Box::new(move |dispatch| admit.add_pointer(dispatch)));
        let forward = Rc::clone(recognizer);
        self.forward
            .push(Box::new(move |dispatch| forward.handle_event(dispatch)));
    }

    /// Dispatch one event, then run the arena lifecycle and drain, even when
    /// a callback panicked; the first panic resumes afterwards.
    fn send(&self, event: &PointerEvent) {
        let dispatch = PointerDispatch::at_root(event);
        let routes = if matches!(event, PointerEvent::Down(_)) {
            &self.admit
        } else {
            &self.forward
        };
        let mut failure = None;
        for route in routes {
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| route(dispatch))) {
                failure.get_or_insert(payload);
            }
        }
        if let Err(payload) = catch_unwind(AssertUnwindSafe(|| {
            run_pointer_lifecycle(&self.arena, event);
            self.arena.drain_deferred_resolutions();
        })) {
            failure.get_or_insert(payload);
        }
        if let Some(payload) = failure {
            resume_unwind(payload);
        }
    }

    /// Pump ~16 ms frames covering `ms` of virtual time.
    fn frames(&self, ms: u64) {
        for _ in 0..=ms / 16 {
            self.clock.advance(Duration::from_millis(16));
            self.arena.poll_deadlines();
            self.arena.drain_deferred_resolutions();
        }
    }
}

fn id(raw: u64) -> PointerId {
    PointerId::new(std::num::NonZeroU64::new(raw).expect("nonzero pointer id"))
}

fn at(x: f64, y: f64) -> Offset<f64> {
    Offset::new(x, y)
}

fn down(pointer: PointerId, position: Offset<f64>, kind: PointerKind) -> PointerEvent {
    make_down_event_for_id_with_button(pointer, position, kind, PointerButton::PRIMARY)
        .expect("valid fixture sample")
}

fn up(pointer: PointerId, position: Offset<f64>, kind: PointerKind) -> PointerEvent {
    make_up_event_for_id_with_button(pointer, position, kind, PointerButton::PRIMARY)
        .expect("valid fixture sample")
}

fn motion(pointer: PointerId, position: Offset<f64>, kind: PointerKind) -> PointerEvent {
    make_move_event_for_id(pointer, position, kind).expect("valid fixture sample")
}

/// Stamp an event with its hardware time, as a platform does.
fn stamped(mut event: PointerEvent, nanos: u64) -> PointerEvent {
    match &mut event {
        PointerEvent::Down(data) => {
            data.sample.time = flui_platform_api::EventTime::from_nanos(nanos)
        }
        PointerEvent::Up(data) => {
            data.sample.time = flui_platform_api::EventTime::from_nanos(nanos)
        }
        PointerEvent::Move(data) => {
            let mut sample = *data.current();
            sample.time = flui_platform_api::EventTime::from_nanos(nanos);
            *data = flui_interaction::events::PointerMove::new(data.pointer, data.buttons, sample)
                .with_modifiers(data.modifiers)
                .with_coalesced(data.coalesced().to_vec())
                .with_predicted(data.predicted().to_vec());
        }
        _ => {}
    }
    event
}

fn click(lane: &Lane, pointer: PointerId, position: Offset<f64>, kind: PointerKind) {
    lane.send(&down(pointer, position, kind));
    lane.send(&up(pointer, position, kind));
}

fn counter() -> Rc<Cell<u32>> {
    Rc::new(Cell::new(0))
}

type TapLog = Rc<RefCell<Vec<Offset<f64>>>>;

/// A tap and a double tap on one detector, recording where taps fired.
fn tap_and_double_tap(lane: &mut Lane) -> (TapLog, Rc<Cell<u32>>) {
    let taps = Rc::new(RefCell::new(Vec::new()));
    let doubles = counter();
    let tap_log = Rc::clone(&taps);
    let tap = TapGestureRecognizer::builder(lane.arena.clone())
        .on_tap(move |details| tap_log.borrow_mut().push(details.local_position))
        .build();
    let double_log = Rc::clone(&doubles);
    let double_tap = DoubleTapGestureRecognizer::builder(lane.arena.clone())
        .on_double_tap(move |_| double_log.set(double_log.get() + 1))
        .build();
    lane.join(&tap);
    lane.join(&double_tap);
    (taps, doubles)
}

// ---------------------------------------------------------------------------
// A held first tap and the next contact
// ---------------------------------------------------------------------------

fn far_second_click_delivers_the_held_first_tap() {
    for (kind, first, second) in [
        (
            PointerKind::Mouse,
            PointerId::new(std::num::NonZeroU64::MIN),
            PointerId::new(std::num::NonZeroU64::MIN),
        ),
        (PointerKind::Touch, id(2), id(3)),
    ] {
        let mut lane = Lane::new();
        let (taps, doubles) = tap_and_double_tap(&mut lane);
        click(&lane, first, at(10.0, 10.0), kind);
        assert!(
            taps.borrow().is_empty(),
            "the first tap waits for a double tap"
        );
        lane.frames(50);
        lane.send(&down(second, at(300.0, 10.0), kind));
        assert_eq!(
            *taps.borrow(),
            [at(10.0, 10.0)],
            "{kind:?}: a far second contact ends the first tap's window"
        );
        lane.send(&up(second, at(300.0, 10.0), kind));
        lane.frames(400);
        assert_eq!(*taps.borrow(), [at(10.0, 10.0), at(300.0, 10.0)]);
        assert_eq!(doubles.get(), 0);
        assert!(lane.arena.is_empty());
    }
}

fn late_first_tap_verdict_lands_on_its_own_click() {
    let mut lane = Lane::new();
    let (taps, doubles) = tap_and_double_tap(&mut lane);
    let mouse = PointerKind::Mouse;
    click(
        &lane,
        PointerId::new(std::num::NonZeroU64::MIN),
        at(10.0, 10.0),
        mouse,
    );
    // The window runs out with no frame to notice it; the next click does.
    lane.clock.advance(Duration::from_millis(400));
    lane.send(&down(
        PointerId::new(std::num::NonZeroU64::MIN),
        at(12.0, 10.0),
        mouse,
    ));
    assert_eq!(
        *taps.borrow(),
        [at(10.0, 10.0)],
        "the expired first tap fires"
    );
    lane.send(&up(
        PointerId::new(std::num::NonZeroU64::MIN),
        at(12.0, 10.0),
        mouse,
    ));
    assert_eq!(
        *taps.borrow(),
        [at(10.0, 10.0)],
        "the second click waits for its own window"
    );
    lane.frames(400);
    assert_eq!(*taps.borrow(), [at(10.0, 10.0), at(12.0, 10.0)]);
    assert_eq!(doubles.get(), 0);
    assert!(lane.arena.is_empty());
}

fn near_second_click_is_a_double_tap() {
    for kind in [PointerKind::Mouse, PointerKind::Touch] {
        let mut lane = Lane::new();
        let (taps, doubles) = tap_and_double_tap(&mut lane);
        let second = if kind == PointerKind::Mouse {
            PointerId::new(std::num::NonZeroU64::MIN)
        } else {
            id(3)
        };
        click(
            &lane,
            PointerId::new(std::num::NonZeroU64::MIN),
            at(10.0, 10.0),
            kind,
        );
        lane.frames(50);
        click(&lane, second, at(14.0, 10.0), kind);
        lane.frames(400);
        assert_eq!(doubles.get(), 1, "{kind:?}");
        assert!(
            taps.borrow().is_empty(),
            "{kind:?}: neither single tap fires"
        );
        assert!(lane.arena.is_empty());
    }
}

// ---------------------------------------------------------------------------
// A second finger
// ---------------------------------------------------------------------------

fn second_finger_leaves_a_running_drag_alone() {
    let mut lane = Lane::new();
    let (updates, ends, cancels) = (counter(), counter(), counter());
    let (u, e, c) = (Rc::clone(&updates), Rc::clone(&ends), Rc::clone(&cancels));
    let drag = DragGestureRecognizer::builder(lane.arena.clone(), DragAxis::Free)
        .on_update(move |_| u.set(u.get() + 1))
        .on_end(move |_| e.set(e.get() + 1))
        .on_cancel(move || c.set(c.get() + 1))
        .build();
    lane.join(&drag);
    let touch = PointerKind::Touch;
    lane.send(&down(id(2), at(0.0, 0.0), touch));
    lane.send(&motion(id(2), at(40.0, 0.0), touch));
    let before = updates.get();
    assert!(before > 0, "the lone drag runs");
    lane.send(&down(id(3), at(200.0, 200.0), touch));
    lane.send(&motion(id(2), at(60.0, 0.0), touch));
    assert_eq!(updates.get(), before + 1, "the first finger keeps dragging");
    lane.send(&up(id(3), at(200.0, 200.0), touch));
    assert_eq!(
        (ends.get(), cancels.get()),
        (0, 0),
        "the other finger ends nothing"
    );
    lane.send(&up(id(2), at(60.0, 0.0), touch));
    assert_eq!((ends.get(), cancels.get()), (1, 0), "one end for one drag");
}

fn second_finger_leaves_a_long_press_alone() {
    let mut lane = Lane::new();
    let (starts, ends, cancels) = (counter(), counter(), counter());
    let (s, e, c) = (Rc::clone(&starts), Rc::clone(&ends), Rc::clone(&cancels));
    let long_press = LongPressGestureRecognizer::builder(lane.arena.clone())
        .on_long_press_start(move |_| s.set(s.get() + 1))
        .on_long_press_end(move |_| e.set(e.get() + 1))
        .on_long_press_cancel(move |_| c.set(c.get() + 1))
        .build();
    lane.join(&long_press);
    let touch = PointerKind::Touch;
    lane.send(&down(id(2), at(10.0, 10.0), touch));
    lane.frames(600);
    assert_eq!(starts.get(), 1);
    click(&lane, id(3), at(200.0, 200.0), touch);
    assert_eq!(
        (ends.get(), cancels.get()),
        (0, 0),
        "the other finger ends nothing"
    );
    lane.send(&up(id(2), at(10.0, 10.0), touch));
    assert_eq!((ends.get(), cancels.get()), (1, 0));
}

fn drag_cancel_callback_admits_the_next_contact_once() {
    assert_drag_terminal_callback_admits_the_next_contact_once(false);
}

fn drag_cancelled_end_callback_admits_the_next_contact_once() {
    assert_drag_terminal_callback_admits_the_next_contact_once(true);
}

fn assert_drag_terminal_callback_admits_the_next_contact_once(started: bool) {
    let mut lane = Lane::new();
    let pointer = id(2);
    let touch = PointerKind::Touch;
    let downs = Rc::new(RefCell::new(Vec::new()));
    let starts = Rc::new(RefCell::new(Vec::new()));
    let ends = Rc::new(RefCell::new(Vec::new()));
    let cancels = counter();
    let slot: Rc<RefCell<std::rc::Weak<DragGestureRecognizer>>> = Rc::default();
    let readmit_slot = Rc::clone(&slot);
    let readmit = Rc::new(move || {
        let recognizer = readmit_slot
            .borrow()
            .upgrade()
            .expect("recognizer is routed");
        let next_down = down(pointer, at(100.0, 10.0), touch);
        recognizer.add_pointer(PointerDispatch::at_root(&next_down));
    });
    let (d, s, e, c) = (
        Rc::clone(&downs),
        Rc::clone(&starts),
        Rc::clone(&ends),
        Rc::clone(&cancels),
    );
    let readmit_cancel = Rc::clone(&readmit);
    let drag = DragGestureRecognizer::builder(lane.arena.clone(), DragAxis::Free)
        .on_down(move |details| {
            d.borrow_mut()
                .push((details.local_position, details.global_position));
        })
        .on_start(move |details| {
            s.borrow_mut()
                .push((details.local_position, details.global_position));
        })
        .on_end(move |details| {
            e.borrow_mut()
                .push((details.reason, details.local_position));
            if details.reason == GestureEndReason::Cancelled {
                readmit();
            }
        })
        .on_cancel(move || {
            c.set(c.get() + 1);
            readmit_cancel();
        })
        .build();
    *slot.borrow_mut() = Rc::downgrade(&drag);
    lane.join(&drag);
    let first_down = down(pointer, at(0.0, 0.0), touch);
    if started {
        lane.send(&first_down);
        lane.send(&motion(pointer, at(40.0, 0.0), touch));
    } else {
        // A closed arena defers its lone winner until the input-end drain.
        // Restart while the first contact still awaits that verdict.
        drag.add_pointer(PointerDispatch::at_root(&first_down));
        lane.arena.close(pointer);
    }

    lane.send(&down(pointer, at(200.0, 20.0), touch));
    assert_eq!(
        &*downs.borrow(),
        &[
            (at(0.0, 0.0), at(0.0, 0.0)),
            (at(100.0, 10.0), at(100.0, 10.0))
        ],
        "the retiring callback's admission supersedes the outer Down"
    );
    lane.send(&motion(pointer, at(140.0, 10.0), touch));
    lane.send(&up(pointer, at(140.0, 10.0), touch));
    let expected_starts = if started {
        vec![
            (at(0.0, 0.0), at(0.0, 0.0)),
            (at(100.0, 10.0), at(100.0, 10.0)),
        ]
    } else {
        vec![(at(100.0, 10.0), at(100.0, 10.0))]
    };
    assert_eq!(
        &*starts.borrow(),
        &expected_starts,
        "one start per accepted contact"
    );
    let expected_ends = if started {
        vec![
            (GestureEndReason::Cancelled, at(40.0, 0.0)),
            (GestureEndReason::Completed, at(140.0, 10.0)),
        ]
    } else {
        vec![(GestureEndReason::Completed, at(140.0, 10.0))]
    };
    assert_eq!(
        &*ends.borrow(),
        &expected_ends,
        "the inner contact completes once"
    );
    assert_eq!(cancels.get(), u32::from(!started));
    assert!(
        lane.arena.is_empty(),
        "the reused pointer's contest has settled"
    );

    lane.send(&down(pointer, at(300.0, 30.0), touch));
    lane.send(&motion(pointer, at(340.0, 30.0), touch));
    lane.send(&up(pointer, at(340.0, 30.0), touch));
    assert_eq!(
        downs.borrow().len(),
        3,
        "the same pointer serves the next gesture"
    );
    assert_eq!(starts.borrow().len(), expected_starts.len() + 1);
    assert_eq!(ends.borrow().len(), expected_ends.len() + 1);
    assert_eq!(
        ends.borrow().last(),
        Some(&(GestureEndReason::Completed, at(340.0, 30.0)))
    );
    assert!(lane.arena.is_empty());
}

fn other_finger_does_not_complete_a_double_tap_contact() {
    let mut lane = Lane::new();
    let doubles = counter();
    let d = Rc::clone(&doubles);
    let double_tap = DoubleTapGestureRecognizer::builder(lane.arena.clone())
        .on_double_tap(move |_| d.set(d.get() + 1))
        .build();
    lane.join(&double_tap);
    let touch = PointerKind::Touch;
    lane.send(&down(id(2), at(10.0, 10.0), touch));
    click(&lane, id(3), at(300.0, 300.0), touch);
    lane.send(&up(id(2), at(10.0, 10.0), touch));
    lane.frames(50);
    click(&lane, id(4), at(12.0, 10.0), touch);
    assert_eq!(doubles.get(), 1, "the double tap follows the first finger");
    lane.frames(400);
    assert!(lane.arena.is_empty());
}

// ---------------------------------------------------------------------------
// Callbacks that dispose their recognizer, and callbacks that panic
// ---------------------------------------------------------------------------

fn long_press_callback_can_dispose_its_recognizer() {
    let mut lane = Lane::new();
    let slot: Rc<RefCell<Option<Rc<LongPressGestureRecognizer>>>> = Rc::default();
    let inner = Rc::clone(&slot);
    let long_press = LongPressGestureRecognizer::builder(lane.arena.clone())
        .on_long_press(move || {
            let recognizer = inner.borrow().clone();
            if let Some(recognizer) = recognizer {
                recognizer.cancel();
            }
        })
        .build();
    *slot.borrow_mut() = Some(Rc::clone(&long_press));
    lane.join(&long_press);
    lane.send(&down(id(2), at(10.0, 10.0), PointerKind::Touch));
    let pumped = catch_unwind(AssertUnwindSafe(|| lane.frames(600)));
    assert!(
        pumped.is_ok(),
        "disposing from on_long_press must not panic"
    );
    // A disposed recognizer is no longer routed (its detector unmounted).
    assert!(lane.arena.is_empty());
}

fn double_tap_callback_can_dispose_its_recognizer() {
    let mut lane = Lane::new();
    let slot: Rc<RefCell<Option<Rc<DoubleTapGestureRecognizer>>>> = Rc::default();
    let inner = Rc::clone(&slot);
    let double_tap = DoubleTapGestureRecognizer::builder(lane.arena.clone())
        .on_double_tap(move |_| {
            let recognizer = inner.borrow().clone();
            if let Some(recognizer) = recognizer {
                recognizer.cancel();
            }
        })
        .build();
    *slot.borrow_mut() = Some(Rc::clone(&double_tap));
    lane.join(&double_tap);
    let touch = PointerKind::Touch;
    click(&lane, id(2), at(10.0, 10.0), touch);
    lane.frames(50);
    lane.send(&down(id(3), at(12.0, 10.0), touch));
    let released = catch_unwind(AssertUnwindSafe(|| {
        lane.send(&up(id(3), at(12.0, 10.0), touch));
    }));
    assert!(
        released.is_ok(),
        "disposing from on_double_tap must not panic"
    );
    assert!(lane.arena.is_empty());
}

fn tap_move_callback_can_dispose_its_recognizer() {
    let mut lane = Lane::new();
    let slot: Rc<RefCell<Option<Rc<TapGestureRecognizer>>>> = Rc::default();
    let inner = Rc::clone(&slot);
    let tap = TapGestureRecognizer::builder(lane.arena.clone())
        .on_tap_move(move |_| {
            let recognizer = inner.borrow().clone();
            if let Some(recognizer) = recognizer {
                recognizer.cancel();
            }
        })
        .build();
    *slot.borrow_mut() = Some(Rc::clone(&tap));
    lane.join(&tap);
    let touch = PointerKind::Touch;
    lane.send(&down(id(2), at(10.0, 10.0), touch));
    let moved = catch_unwind(AssertUnwindSafe(|| {
        lane.send(&motion(id(2), at(11.0, 10.0), touch));
    }));
    assert!(moved.is_ok(), "disposing from on_tap_move must not panic");
    // A disposed recognizer is no longer routed (its detector unmounted).
    assert!(lane.arena.is_empty());
}

fn panicking_double_tap_callback_leaves_the_next_double_tap_working() {
    let mut lane = Lane::new();
    let doubles = counter();
    let d = Rc::clone(&doubles);
    let double_tap = DoubleTapGestureRecognizer::builder(lane.arena.clone())
        .on_double_tap(move |_| {
            d.set(d.get() + 1);
            assert!(d.get() > 1, "first double tap panics");
        })
        .build();
    lane.join(&double_tap);
    let touch = PointerKind::Touch;
    for (first, second, expect_panic) in [(2, 3, true), (4, 5, false)] {
        click(&lane, id(first), at(10.0, 10.0), touch);
        lane.frames(50);
        let completed = catch_unwind(AssertUnwindSafe(|| {
            click(&lane, id(second), at(12.0, 10.0), touch);
        }));
        assert_eq!(completed.is_err(), expect_panic);
        lane.frames(400);
        assert!(lane.arena.is_empty(), "no arena stays held after the panic");
    }
    assert_eq!(
        doubles.get(),
        2,
        "the double tap after the panic is recognized"
    );
}

fn verdict_by_pointer_cannot_pick_a_tap_sequence() {
    // A verdict that names only a pointer cannot say which click it decides:
    // with a mouse, the held first click and the current one share the ID.
    let mut lane = Lane::new();
    let taps = counter();
    let doubles = counter();
    let t = Rc::clone(&taps);
    let tap = TapGestureRecognizer::builder(lane.arena.clone())
        .on_tap(move |_| t.set(t.get() + 1))
        .build();
    let d = Rc::clone(&doubles);
    let double_tap = DoubleTapGestureRecognizer::builder(lane.arena.clone())
        .on_double_tap(move |_| d.set(d.get() + 1))
        .build();
    lane.join(&tap);
    lane.join(&double_tap);
    let mouse = PointerKind::Mouse;
    click(
        &lane,
        PointerId::new(std::num::NonZeroU64::MIN),
        at(10.0, 10.0),
        mouse,
    );
    lane.frames(50);
    lane.send(&down(
        PointerId::new(std::num::NonZeroU64::MIN),
        at(14.0, 10.0),
        mouse,
    ));
    tap.accept_gesture(PointerId::new(std::num::NonZeroU64::MIN));
    lane.send(&up(
        PointerId::new(std::num::NonZeroU64::MIN),
        at(14.0, 10.0),
        mouse,
    ));
    lane.frames(400);
    assert_eq!(doubles.get(), 1, "the arena's own verdicts stand");
    assert_eq!(taps.get(), 0, "a pointer-keyed verdict fired a single tap");
    assert!(lane.arena.is_empty());
}

fn panicking_first_callback_still_runs_the_rest() {
    // Long press: `on_long_press` then `on_long_press_start` fire together.
    let mut lane = Lane::new();
    let starts = counter();
    let s = Rc::clone(&starts);
    let long_press = LongPressGestureRecognizer::builder(lane.arena.clone())
        .on_long_press(|| panic!("on_long_press panics"))
        .on_long_press_start(move |_| s.set(s.get() + 1))
        .build();
    lane.join(&long_press);
    lane.send(&down(id(2), at(10.0, 10.0), PointerKind::Touch));
    let pumped = catch_unwind(AssertUnwindSafe(|| lane.frames(600)));
    assert!(pumped.is_err(), "the first panic resumes");
    assert_eq!(starts.get(), 1, "on_long_press_start still fires");

    // Tap: `on_tap_up` then `on_tap` fire together.
    let mut lane = Lane::new();
    let taps = counter();
    let t = Rc::clone(&taps);
    let tap = TapGestureRecognizer::builder(lane.arena.clone())
        .on_tap_up(|_| panic!("on_tap_up panics"))
        .on_tap(move |_| t.set(t.get() + 1))
        .build();
    lane.join(&tap);
    let released = catch_unwind(AssertUnwindSafe(|| {
        click(&lane, id(2), at(10.0, 10.0), PointerKind::Touch);
    }));
    assert!(released.is_err(), "the first panic resumes");
    assert_eq!(taps.get(), 1, "on_tap still fires");
    assert!(lane.arena.is_empty());
}

fn panicking_multi_tap_callback_leaves_the_next_pair_working() {
    let taps = counter();
    let t = Rc::clone(&taps);
    let recognizer = MultiTapGestureRecognizer::builder(GestureArena::new(), 2)
        .on_multi_tap(move |_| {
            t.set(t.get() + 1);
            assert!(t.get() > 1, "first multi tap panics");
        })
        .build();
    for (a, b, expect_panic) in [(2, 3, true), (4, 5, false)] {
        let first = down(id(a), at(10.0, 10.0), PointerKind::Touch);
        let second = down(id(b), at(90.0, 10.0), PointerKind::Touch);
        recognizer.add_pointer(PointerDispatch::at_root(&first));
        recognizer.add_pointer(PointerDispatch::at_root(&second));
        recognizer.handle_event(PointerDispatch::at_root(
            &make_up_event_for_id(id(a), at(10.0, 10.0), PointerKind::Touch)
                .expect("valid fixture sample"),
        ));
        let completed = catch_unwind(AssertUnwindSafe(|| {
            recognizer.handle_event(PointerDispatch::at_root(
                &make_up_event_for_id(id(b), at(90.0, 10.0), PointerKind::Touch)
                    .expect("valid fixture sample"),
            ));
        }));
        assert_eq!(completed.is_err(), expect_panic);
    }
    assert_eq!(taps.get(), 2, "the pair after the panic is recognized");
}

#[derive(Default)]
struct Verdicts {
    accepted: AtomicU32,
    rejected: AtomicU32,
}

impl GestureArenaMember for Verdicts {
    fn accept_gesture(&self, _pointer: PointerId) {
        self.accepted.fetch_add(1, Ordering::SeqCst);
    }

    fn reject_gesture(&self, _pointer: PointerId) {
        self.rejected.fetch_add(1, Ordering::SeqCst);
    }
}

impl Verdicts {
    fn get(&self) -> (u32, u32) {
        (
            self.accepted.load(Ordering::SeqCst),
            self.rejected.load(Ordering::SeqCst),
        )
    }
}

// ---------------------------------------------------------------------------
// Arena resolution
// ---------------------------------------------------------------------------

fn accept_from_a_withdrawn_member_cannot_end_the_arena() {
    let arena = GestureArena::new();
    let pointer = id(2);
    let members: Vec<_> = (0..3).map(|_| Rc::new(Verdicts::default())).collect();
    let entries: Vec<_> = members
        .iter()
        .map(|member| arena.add(pointer, member))
        .collect();
    arena.close(pointer);
    entries[0].resolve(GestureDisposition::Rejected);
    entries[0].resolve(GestureDisposition::Accepted);
    assert_eq!(members[1].get(), (0, 0), "a withdrawn member cannot win");
    assert_eq!(members[2].get(), (0, 0));
    assert!(arena.contains(pointer));
    entries[1].resolve(GestureDisposition::Accepted);
    assert_eq!(members[1].get(), (1, 0));
    assert_eq!(members[2].get(), (0, 1));
    assert!(arena.is_empty());
}

fn held_arena_swept_on_up_leaves_room_for_the_next_contact() {
    let arena = GestureArena::new();
    let pointer = PointerId::new(std::num::NonZeroU64::MIN);
    let first = Rc::new(Verdicts::default());
    let second = Rc::new(Verdicts::default());
    let first_entry = arena.add(pointer, &first);
    arena.add(pointer, &second);
    arena.close(pointer);
    first_entry.hold();
    run_pointer_lifecycle(&arena, &up(pointer, at(0.0, 0.0), PointerKind::Mouse));
    let next = Rc::new(Verdicts::default());
    arena.add(pointer, &next);
    arena.close(pointer);
    arena.drain_deferred_resolutions();
    assert_eq!(next.get(), (1, 0), "the next contact's lone member wins");
    first_entry.release();
    assert_eq!(
        first.get(),
        (1, 0),
        "the released sweep still settles the held arena"
    );
    assert_eq!(second.get(), (0, 1));
    assert!(arena.is_empty());
}

// ---------------------------------------------------------------------------
// Device kind and buttons
// ---------------------------------------------------------------------------

fn mouse_drift_beyond_the_precise_slop_cancels_taps_and_presses() {
    // 5 px is far past a mouse's slop and well inside a finger's.
    let mouse = PointerKind::Mouse;
    let start = at(10.0, 10.0);
    let drifted = at(15.0, 10.0);

    let mut lane = Lane::new();
    let (taps, cancels) = (counter(), counter());
    let (t, c) = (Rc::clone(&taps), Rc::clone(&cancels));
    let tap = TapGestureRecognizer::builder(lane.arena.clone())
        .on_tap(move |_| t.set(t.get() + 1))
        .on_tap_cancel(move |_| c.set(c.get() + 1))
        .build();
    lane.join(&tap);
    lane.send(&down(
        PointerId::new(std::num::NonZeroU64::MIN),
        start,
        mouse,
    ));
    lane.send(&motion(
        PointerId::new(std::num::NonZeroU64::MIN),
        drifted,
        mouse,
    ));
    lane.send(&up(
        PointerId::new(std::num::NonZeroU64::MIN),
        drifted,
        mouse,
    ));
    assert_eq!((taps.get(), cancels.get()), (0, 1), "tap");

    let mut lane = Lane::new();
    let (starts, cancels) = (counter(), counter());
    let (s, c) = (Rc::clone(&starts), Rc::clone(&cancels));
    let long_press = LongPressGestureRecognizer::builder(lane.arena.clone())
        .on_long_press_start(move |_| s.set(s.get() + 1))
        .on_long_press_cancel(move |_| c.set(c.get() + 1))
        .build();
    lane.join(&long_press);
    lane.send(&down(
        PointerId::new(std::num::NonZeroU64::MIN),
        start,
        mouse,
    ));
    lane.send(&motion(
        PointerId::new(std::num::NonZeroU64::MIN),
        drifted,
        mouse,
    ));
    lane.frames(600);
    assert_eq!((starts.get(), cancels.get()), (0, 1), "long press");

    let mut lane = Lane::new();
    let cancels = counter();
    let c = Rc::clone(&cancels);
    let double_tap = DoubleTapGestureRecognizer::builder(lane.arena.clone())
        .on_double_tap_cancel(move |_| c.set(c.get() + 1))
        .build();
    lane.join(&double_tap);
    lane.send(&down(
        PointerId::new(std::num::NonZeroU64::MIN),
        start,
        mouse,
    ));
    lane.send(&motion(
        PointerId::new(std::num::NonZeroU64::MIN),
        drifted,
        mouse,
    ));
    assert_eq!(cancels.get(), 1, "double tap");
}

fn secondary_button_starts_no_drag_and_no_long_press() {
    let mouse = PointerKind::Mouse;
    let right = |position| {
        make_down_event_for_id_with_button(
            PointerId::new(std::num::NonZeroU64::MIN),
            position,
            mouse,
            PointerButton::SECONDARY,
        )
        .expect("valid fixture sample")
    };

    let mut lane = Lane::new();
    let starts = counter();
    let s = Rc::clone(&starts);
    let drag = DragGestureRecognizer::builder(lane.arena.clone(), DragAxis::Free)
        .on_start(move |_| s.set(s.get() + 1))
        .build();
    lane.join(&drag);
    lane.send(&right(at(0.0, 0.0)));
    lane.send(&motion(
        PointerId::new(std::num::NonZeroU64::MIN),
        at(60.0, 0.0),
        mouse,
    ));
    lane.send(
        &make_up_event_for_id_with_button(
            PointerId::new(std::num::NonZeroU64::MIN),
            at(60.0, 0.0),
            mouse,
            PointerButton::SECONDARY,
        )
        .expect("valid fixture sample"),
    );
    assert_eq!(starts.get(), 0, "a right-button drag does not pan");

    let mut lane = Lane::new();
    let presses = counter();
    let p = Rc::clone(&presses);
    let long_press = LongPressGestureRecognizer::builder(lane.arena.clone())
        .on_long_press(move || p.set(p.get() + 1))
        .build();
    lane.join(&long_press);
    lane.send(&right(at(0.0, 0.0)));
    lane.frames(600);
    assert_eq!(presses.get(), 0, "a right-button hold is not a long press");
}

fn drag_reports_the_device_kind_from_its_down() {
    let mut lane = Lane::new();
    let kinds = Rc::new(RefCell::new(Vec::new()));
    let k = Rc::clone(&kinds);
    let drag = DragGestureRecognizer::builder(lane.arena.clone(), DragAxis::Horizontal)
        .on_down(move |details| k.borrow_mut().push(details.kind))
        .build();
    lane.join(&drag);
    lane.send(&down(
        PointerId::new(std::num::NonZeroU64::MIN),
        at(0.0, 0.0),
        PointerKind::Mouse,
    ));
    assert_eq!(*kinds.borrow(), [PointerKind::Mouse]);
}

// ---------------------------------------------------------------------------
// Velocity
// ---------------------------------------------------------------------------

fn fling_velocity_follows_event_timestamps_not_dispatch_time() {
    let mut lane = Lane::new();
    let velocity = Rc::new(Cell::new(None));
    let v = Rc::clone(&velocity);
    let drag = DragGestureRecognizer::builder(lane.arena.clone(), DragAxis::Horizontal)
        .on_end(move |details| v.set(Some(details.velocity.pixels_per_second.dx)))
        .build();
    lane.join(&drag);
    let touch = PointerKind::Touch;
    // Every event of the stroke is dispatched in one frame (the arena clock
    // never moves), but the device stamped them 10 ms apart: 10 px per 10 ms.
    let base = 1_000_000_000_u64;
    let step = 10_000_000_u64;
    lane.send(&stamped(down(id(2), at(0.0, 0.0), touch), base));
    for k in 1..=6_u32 {
        let position = at(f64::from(k) * 10.0, 0.0);
        lane.send(&stamped(
            motion(id(2), position, touch),
            base + u64::from(k) * step,
        ));
    }
    lane.send(&stamped(up(id(2), at(60.0, 0.0), touch), base + 6 * step));
    let dx = velocity.get().expect("the drag ended");
    assert!(
        (dx - 1000.0).abs() < 100.0,
        "expected ~1000 px/s from the event timestamps, got {dx}"
    );
}

/// Dispatch the terminal event in the same frame as the last move, even
/// though the device observed a stationary gap before release.
fn terminal_velocity_cases(lane: &Lane, velocities: &RefCell<Vec<f64>>, scale: bool) {
    for (sequence, gap_ms, queued) in [
        (0_u64, 0_u64, false),
        (1, 100, false),
        (2, 0, false),
        (3, 0, true),
    ] {
        let base = 1_000_000_000 + sequence * 1_000_000_000;
        let touch = PointerKind::Touch;
        lane.send(&stamped(down(id(2), at(0.0, 0.0), touch), base));
        if scale {
            lane.send(&stamped(down(id(3), at(100.0, 0.0), touch), base));
        }
        for k in 1..=6_u32 {
            if !queued {
                lane.clock.advance(Duration::from_millis(10));
            }
            lane.send(&stamped(
                motion(id(2), at(-f64::from(k) * 20.0, 0.0), touch),
                base + u64::from(k) * 10_000_000,
            ));
        }
        let terminal = base + (60 + gap_ms) * 1_000_000;
        lane.send(&stamped(up(id(2), at(-120.0, 0.0), touch), terminal));
        if scale {
            lane.send(&stamped(up(id(3), at(100.0, 0.0), touch), terminal));
        }
        let values = velocities.borrow();
        assert_eq!(
            values.len(),
            usize::try_from(sequence + 1).expect("small sequence")
        );
        let velocity = *values.last().expect("accepted gesture ended");
        if gap_ms == 0 {
            let expected = if scale { 20.0_f64 } else { -2000.0_f64 };
            assert!(
                (velocity - expected).abs() < expected.abs() * 0.1,
                "healthy/recovery velocity (queued={queued}): {velocity}, expected {expected}"
            );
        } else {
            assert_eq!(
                velocity, 0.0,
                "a stationary gap before Up must stop the fling"
            );
        }
        assert!(lane.arena.is_empty());
    }
}

fn drag_release_uses_terminal_event_time() {
    let mut lane = Lane::new();
    let velocities = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&velocities);
    let drag = DragGestureRecognizer::builder(lane.arena.clone(), DragAxis::Horizontal)
        .on_end(move |details| log.borrow_mut().push(details.velocity.pixels_per_second.dx))
        .build();
    lane.join(&drag);
    terminal_velocity_cases(&lane, &velocities, false);
}

struct VelocityHandle(Rc<RefCell<Vec<f64>>>);

impl MultiDragHandle for VelocityHandle {
    fn update(&self, _: MultiDragUpdateDetails) {}
    fn end(&self, details: MultiDragEndDetails) {
        self.0
            .borrow_mut()
            .push(details.velocity.pixels_per_second.dx);
    }
    fn cancel(&self) {}
}

fn multidrag_release_uses_terminal_event_time() {
    let mut lane = Lane::new();
    let velocities = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&velocities);
    let drag = MultiDragGestureRecognizer::builder(lane.arena.clone(), MultiDragAxis::Horizontal)
        .on_start(move |_, _| {
            Some(Rc::new(VelocityHandle(Rc::clone(&log))) as Rc<dyn MultiDragHandle>)
        })
        .build();
    lane.join(&drag);
    terminal_velocity_cases(&lane, &velocities, false);
}

fn scale_release_uses_terminal_event_time() {
    let mut lane = Lane::new();
    let velocities = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&velocities);
    let scale = ScaleGestureRecognizer::builder(lane.arena.clone())
        .on_end(move |details| log.borrow_mut().push(details.velocity))
        .build();
    lane.join(&scale);
    terminal_velocity_cases(&lane, &velocities, true);
}

fn tap_and_drag_release_uses_terminal_event_time() {
    let mut lane = Lane::new();
    let velocities = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&velocities);
    let drag = TapAndDragGestureRecognizer::builder(lane.arena.clone())
        .on_drag_end(move |details| log.borrow_mut().push(details.velocity.pixels_per_second.dx))
        .build();
    lane.join(&drag);
    terminal_velocity_cases(&lane, &velocities, false);
}

fn selected_velocity_cases(family: &str) {
    use flui_interaction::{GestureSettings, processing::VelocityEstimator};

    for (estimator, expected) in [
        (VelocityEstimator::LeastSquares, 500.0),
        (VelocityEstimator::Impulse, 1589.9257985831982),
        (VelocityEstimator::Ios, 2550.0),
        (VelocityEstimator::Macos, 1950.0),
    ] {
        let mut lane = Lane::new();
        let velocities = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&velocities);
        let settings = GestureSettings::default().with_velocity_estimator(estimator);
        match family {
            "drag" => lane.join(
                &DragGestureRecognizer::builder(lane.arena.clone(), DragAxis::Horizontal)
                    .settings(settings)
                    .on_end(move |details| log.borrow_mut().push(details.velocity.dx()))
                    .build(),
            ),
            "multi-drag" => lane.join(
                &MultiDragGestureRecognizer::builder(lane.arena.clone(), MultiDragAxis::Horizontal)
                    .settings(settings)
                    .on_start(move |_, _| {
                        Some(Rc::new(VelocityHandle(Rc::clone(&log))) as Rc<dyn MultiDragHandle>)
                    })
                    .build(),
            ),
            "tap-and-drag" => lane.join(
                &TapAndDragGestureRecognizer::builder(lane.arena.clone())
                    .settings(settings)
                    .on_drag_end(move |details| log.borrow_mut().push(details.velocity.dx()))
                    .build(),
            ),
            "scale" => lane.join(
                &ScaleGestureRecognizer::builder(lane.arena.clone())
                    .settings(settings)
                    .on_end(move |details| log.borrow_mut().push(details.velocity))
                    .build(),
            ),
            _ => unreachable!("known fixture producer"),
        }
        for (sequence, gap) in [(0_u64, 0_u64), (1, 100), (2, 0)] {
            let base = sequence * 1_000_000_000;
            let scale = family == "scale";
            let start = if scale { 0.0 } else { 10.0 };
            lane.send(&stamped(
                down(id(2), at(start, 0.0), PointerKind::Touch),
                base,
            ));
            if scale {
                lane.send(&stamped(
                    down(id(3), at(100.0, 0.0), PointerKind::Touch),
                    base,
                ));
            }
            let samples: &[(u64, f64)] = if scale {
                &[(0, -10.0), (10, -40.0), (20, -60.0), (30, -70.0)]
            } else {
                &[(10, 40.0), (20, 60.0), (30, 70.0)]
            };
            // Deliver historical samples in one current Move, while the host
            // clock stays fixed. Scale must fit ratios from that history;
            // the other producers fit admitted positions.
            let sample = |millis: u64, x| {
                flui_platform_api::pointer::PointerSample::new(
                    flui_platform_api::EventTime::from_nanos(base + millis * 1_000_000),
                    flui_platform_api::pointer::PointerPosition::try_new(
                        flui_foundation::geometry::Point::new(x, 0.0),
                    )
                    .expect("finite position"),
                )
            };
            let last = samples.last().expect("authored history");
            let movement = flui_platform_api::pointer::PointerMove::new(
                flui_platform_api::pointer::PointerInfo::new(id(2), PointerKind::Touch),
                flui_platform_api::pointer::PointerButtons::NONE.with(PointerButton::PRIMARY),
                sample(last.0, last.1),
            )
            .with_coalesced(
                samples[..samples.len() - 1]
                    .iter()
                    .map(|&(time, x)| sample(time, x))
                    .collect(),
            );
            lane.send(&PointerEvent::Move(movement));
            let terminal = base + (30 + gap) * 1_000_000;
            lane.send(&stamped(
                up(id(2), at(last.1, 0.0), PointerKind::Touch),
                terminal,
            ));
            if scale {
                lane.send(&stamped(
                    up(id(3), at(100.0, 0.0), PointerKind::Touch),
                    terminal,
                ));
            }
            let delivered = velocities.borrow();
            assert_eq!(
                delivered.len(),
                usize::try_from(sequence + 1).expect("small sequence")
            );
            let actual = *delivered.last().expect("completed gesture");
            let expected = if gap != 0 {
                0.0
            } else if scale {
                expected / 100.0
            } else {
                expected
            };
            assert!(
                (actual - expected).abs() < 1e-6,
                "{family}, {estimator:?}, sequence={sequence}: got {actual}, expected {expected}"
            );
            assert!(lane.arena.is_empty());
        }
    }
}

fn drag_captures_the_selected_estimator() {
    selected_velocity_cases("drag");
}
fn multidrag_captures_the_selected_estimator() {
    selected_velocity_cases("multi-drag");
}
fn tap_and_drag_captures_the_selected_estimator() {
    selected_velocity_cases("tap-and-drag");
}
fn scale_captures_the_selected_estimator() {
    selected_velocity_cases("scale");
}

#[test]
fn gesture_lifecycle_matrix() {
    let cases: &[(&str, fn())] = &[
        (
            "fallback_acceptance_waits_for_preferred_failure",
            fallback_acceptance_waits_for_preferred_failure,
        ),
        (
            "sweep_cannot_grant_a_dependency_blocked_fallback",
            sweep_cannot_grant_a_dependency_blocked_fallback,
        ),
        (
            "preferred_death_releases_accepted_fallback_debt",
            preferred_death_releases_accepted_fallback_debt,
        ),
        (
            "preferred_deadline_releases_fallback_without_input",
            preferred_deadline_releases_fallback_without_input,
        ),
        (
            "composition_checks_preferred_members_after_close",
            composition_checks_preferred_members_after_close,
        ),
        (
            "preferred_acceptance_rejects_a_pending_fallback",
            preferred_acceptance_rejects_a_pending_fallback,
        ),
        (
            "composition_preserves_first_callback_failure_and_recovers",
            composition_preserves_first_callback_failure_and_recovers,
        ),
        (
            "an_aliased_branch_join_cannot_change_the_contact_relationship",
            an_aliased_branch_join_cannot_change_the_contact_relationship,
        ),
        (
            "arena_polls_pointer_deadlines_in_identity_order",
            arena_polls_pointer_deadlines_in_identity_order,
        ),
        (
            "nonmember_resolution_candidate_retires_after_detachment",
            nonmember_resolution_candidate_retires_after_detachment,
        ),
        (
            "arena_retirement_preserves_the_first_failure_and_recovers",
            arena_retirement_preserves_the_first_failure_and_recovers,
        ),
        (
            "ignored_accept_candidate_drops_outside_the_slot_lock",
            ignored_accept_candidate_drops_outside_the_slot_lock,
        ),
        (
            "panicking_window_end_still_admits_the_far_contact",
            panicking_window_end_still_admits_the_far_contact,
        ),
        (
            "dispose_retires_two_panicking_captures_one_at_a_time",
            dispose_retires_two_panicking_captures_one_at_a_time,
        ),
        (
            "far_second_click_delivers_the_held_first_tap",
            far_second_click_delivers_the_held_first_tap,
        ),
        (
            "late_first_tap_verdict_lands_on_its_own_click",
            late_first_tap_verdict_lands_on_its_own_click,
        ),
        (
            "near_second_click_is_a_double_tap",
            near_second_click_is_a_double_tap,
        ),
        (
            "second_finger_leaves_a_running_drag_alone",
            second_finger_leaves_a_running_drag_alone,
        ),
        (
            "drag_cancel_callback_admits_the_next_contact_once",
            drag_cancel_callback_admits_the_next_contact_once,
        ),
        (
            "drag_cancelled_end_callback_admits_the_next_contact_once",
            drag_cancelled_end_callback_admits_the_next_contact_once,
        ),
        (
            "second_finger_leaves_a_long_press_alone",
            second_finger_leaves_a_long_press_alone,
        ),
        (
            "other_finger_does_not_complete_a_double_tap_contact",
            other_finger_does_not_complete_a_double_tap_contact,
        ),
        (
            "long_press_callback_can_dispose_its_recognizer",
            long_press_callback_can_dispose_its_recognizer,
        ),
        (
            "double_tap_callback_can_dispose_its_recognizer",
            double_tap_callback_can_dispose_its_recognizer,
        ),
        (
            "tap_move_callback_can_dispose_its_recognizer",
            tap_move_callback_can_dispose_its_recognizer,
        ),
        (
            "panicking_double_tap_callback_leaves_the_next_double_tap_working",
            panicking_double_tap_callback_leaves_the_next_double_tap_working,
        ),
        (
            "panicking_multi_tap_callback_leaves_the_next_pair_working",
            panicking_multi_tap_callback_leaves_the_next_pair_working,
        ),
        (
            "accept_from_a_withdrawn_member_cannot_end_the_arena",
            accept_from_a_withdrawn_member_cannot_end_the_arena,
        ),
        (
            "held_arena_swept_on_up_leaves_room_for_the_next_contact",
            held_arena_swept_on_up_leaves_room_for_the_next_contact,
        ),
        (
            "mouse_drift_beyond_the_precise_slop_cancels_taps_and_presses",
            mouse_drift_beyond_the_precise_slop_cancels_taps_and_presses,
        ),
        (
            "secondary_button_starts_no_drag_and_no_long_press",
            secondary_button_starts_no_drag_and_no_long_press,
        ),
        (
            "drag_reports_the_device_kind_from_its_down",
            drag_reports_the_device_kind_from_its_down,
        ),
        (
            "fling_velocity_follows_event_timestamps_not_dispatch_time",
            fling_velocity_follows_event_timestamps_not_dispatch_time,
        ),
        (
            "drag_release_uses_terminal_event_time",
            drag_release_uses_terminal_event_time,
        ),
        (
            "multidrag_release_uses_terminal_event_time",
            multidrag_release_uses_terminal_event_time,
        ),
        (
            "scale_release_uses_terminal_event_time",
            scale_release_uses_terminal_event_time,
        ),
        (
            "tap_and_drag_release_uses_terminal_event_time",
            tap_and_drag_release_uses_terminal_event_time,
        ),
        (
            "drag_captures_the_selected_estimator",
            drag_captures_the_selected_estimator,
        ),
        (
            "multidrag_captures_the_selected_estimator",
            multidrag_captures_the_selected_estimator,
        ),
        (
            "tap_and_drag_captures_the_selected_estimator",
            tap_and_drag_captures_the_selected_estimator,
        ),
        (
            "scale_captures_the_selected_estimator",
            scale_captures_the_selected_estimator,
        ),
        (
            "verdict_by_pointer_cannot_pick_a_tap_sequence",
            verdict_by_pointer_cannot_pick_a_tap_sequence,
        ),
        (
            "panicking_first_callback_still_runs_the_rest",
            panicking_first_callback_still_runs_the_rest,
        ),
    ];
    let mut failures = Vec::new();
    for &(name, case) in cases {
        if let Err(payload) = catch_unwind(case) {
            let message = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(ToString::to_string))
                .unwrap_or_default();
            failures.push(format!("{name}: {message}"));
        }
    }
    assert!(failures.is_empty(), "failed rows:\n{}", failures.join("\n"));
}

// ---------------------------------------------------------------------------
// Arena property test
// ---------------------------------------------------------------------------

struct CompositionMember {
    name: &'static str,
    log: Rc<RefCell<Vec<(&'static str, GestureDisposition)>>>,
}

impl GestureArenaMember for CompositionMember {
    fn accept_gesture(&self, _: PointerId) {
        self.log
            .borrow_mut()
            .push((self.name, GestureDisposition::Accepted));
    }
    fn reject_gesture(&self, _: PointerId) {
        self.log
            .borrow_mut()
            .push((self.name, GestureDisposition::Rejected));
    }
}

fn fallback_acceptance_waits_for_preferred_failure() {
    let arena = GestureArena::new();
    let (preferred_arena, fallback_arena) = arena
        .compose(GestureCompetition::RequireFirstFailure)
        .expect("root arena can compose")
        .into_branches();
    let log = Rc::new(RefCell::new(Vec::new()));
    let preferred = Rc::new(CompositionMember {
        name: "preferred",
        log: Rc::clone(&log),
    });
    let fallback = Rc::new(CompositionMember {
        name: "fallback",
        log: Rc::clone(&log),
    });
    let pointer = PointerId::new(core::num::NonZeroU64::MIN);
    let fallback_entry = fallback_arena.add(pointer, &fallback);
    let preferred_entry = preferred_arena.add(pointer, &preferred);
    arena.close(pointer);
    fallback_entry.resolve(GestureDisposition::Accepted);
    arena.drain_deferred_resolutions();
    assert!(
        log.borrow().is_empty(),
        "fallback acceptance is pending while preferred remains viable"
    );
    preferred_entry.resolve(GestureDisposition::Rejected);
    arena.drain_deferred_resolutions();
    assert_eq!(
        log.borrow().as_slice(),
        [
            ("preferred", GestureDisposition::Rejected),
            ("fallback", GestureDisposition::Accepted)
        ]
    );
}

fn sweep_cannot_grant_a_dependency_blocked_fallback() {
    let arena = GestureArena::new();
    let (preferred_arena, fallback_arena) = arena
        .compose(GestureCompetition::RequireFirstFailure)
        .expect("root arena can compose")
        .into_branches();
    let log = Rc::new(RefCell::new(Vec::new()));
    let preferred = Rc::new(CompositionMember {
        name: "preferred",
        log: Rc::clone(&log),
    });
    let fallback = Rc::new(CompositionMember {
        name: "fallback",
        log: Rc::clone(&log),
    });
    let pointer = PointerId::new(core::num::NonZeroU64::MIN);
    let fallback_entry = fallback_arena.add(pointer, &fallback);
    let preferred_entry = preferred_arena.add(pointer, &preferred);
    arena.close(pointer);
    arena.sweep(pointer);
    arena.drain_deferred_resolutions();
    assert!(
        log.borrow().is_empty(),
        "sweep must retain a blocked fallback without granting either branch"
    );
    let fresh = Rc::new(CompositionMember {
        name: "fresh",
        log: Rc::clone(&log),
    });
    let fresh_entry = arena.add(pointer, &fresh);
    preferred_entry.resolve(GestureDisposition::Rejected);
    fallback_entry.resolve(GestureDisposition::Accepted);
    arena.drain_deferred_resolutions();
    assert_eq!(
        log.borrow().as_slice(),
        [
            ("preferred", GestureDisposition::Rejected),
            ("fallback", GestureDisposition::Accepted)
        ],
        "the retained old verdict cannot close or resolve the fresh open generation"
    );
    arena.close(pointer);
    fresh_entry.resolve(GestureDisposition::Accepted);
    assert_eq!(
        log.borrow().as_slice(),
        [
            ("preferred", GestureDisposition::Rejected),
            ("fallback", GestureDisposition::Accepted),
            ("fresh", GestureDisposition::Accepted)
        ]
    );
}

fn preferred_death_releases_accepted_fallback_debt() {
    for sweep in [false, true] {
        let arena = GestureArena::new();
        let (first, second) = arena
            .compose(GestureCompetition::RequireFirstFailure)
            .expect("root composition")
            .into_branches();
        let log = Rc::new(RefCell::new(Vec::new()));
        let pointer = id(2);
        let preferred = Rc::new(CompositionMember {
            name: "preferred",
            log: Rc::clone(&log),
        });
        let fallback = Rc::new(CompositionMember {
            name: "fallback",
            log: Rc::clone(&log),
        });
        first.add(pointer, &preferred);
        let fallback_entry = second.add(pointer, &fallback);
        arena.close(pointer);
        fallback_entry.resolve(GestureDisposition::Accepted);
        if sweep {
            arena.sweep(pointer);
        }
        assert!(log.borrow().is_empty());
        drop(preferred);
        arena.drain_deferred_resolutions();
        assert_eq!(
            log.borrow().as_slice(),
            [("fallback", GestureDisposition::Accepted)]
        );
        assert!(arena.is_empty());
    }
}

struct DeadlineFailure {
    due: Cell<Option<web_time::Instant>>,
    entry: RefCell<Option<GestureArenaEntry>>,
    member: CompositionMember,
}

impl GestureArenaMember for DeadlineFailure {
    fn accept_gesture(&self, pointer: PointerId) {
        self.member.accept_gesture(pointer);
    }
    fn reject_gesture(&self, pointer: PointerId) {
        self.due.set(None);
        self.member.reject_gesture(pointer);
    }
    fn deadline(&self) -> Option<web_time::Instant> {
        self.due.get()
    }
    fn poll_deadline(&self, now: web_time::Instant) {
        if self.due.get().is_some_and(|due| due <= now) {
            self.due.set(None);
            let entry = self.entry.borrow_mut().take();
            if let Some(entry) = entry {
                entry.resolve(GestureDisposition::Rejected);
            }
        }
    }
}

fn preferred_deadline_releases_fallback_without_input() {
    let clock = ManualClock::new();
    let arena = GestureArena::binding_driven(Arc::new(clock.clone()));
    let (first, second) = arena
        .compose(GestureCompetition::RequireFirstFailure)
        .expect("root composition")
        .into_branches();
    let log = Rc::new(RefCell::new(Vec::new()));
    let pointer = id(2);
    let preferred = Rc::new(DeadlineFailure {
        due: Cell::new(Some(arena.now() + Duration::from_millis(30))),
        entry: RefCell::new(None),
        member: CompositionMember {
            name: "preferred",
            log: Rc::clone(&log),
        },
    });
    let fallback = Rc::new(CompositionMember {
        name: "fallback",
        log: Rc::clone(&log),
    });
    *preferred.entry.borrow_mut() = Some(first.add(pointer, &preferred));
    let fallback_entry = second.add(pointer, &fallback);
    arena.close(pointer);
    fallback_entry.resolve(GestureDisposition::Accepted);
    arena.sweep(pointer);
    clock.advance(Duration::from_millis(29));
    arena.poll_deadlines();
    arena.drain_deferred_resolutions();
    assert!(log.borrow().is_empty());
    clock.advance(Duration::from_millis(1));
    arena.poll_deadlines();
    arena.drain_deferred_resolutions();
    assert_eq!(
        log.borrow().as_slice(),
        [
            ("preferred", GestureDisposition::Rejected),
            ("fallback", GestureDisposition::Accepted)
        ]
    );
    assert!(!arena.has_pending_deadlines());
}

fn composition_checks_preferred_members_after_close() {
    for admit_preferred in [false, true] {
        let arena = GestureArena::new();
        let (first, second) = arena
            .compose(GestureCompetition::RequireFirstFailure)
            .expect("root composition")
            .into_branches();
        let log = Rc::new(RefCell::new(Vec::new()));
        let pointer = id(2);
        let preferred = Rc::new(CompositionMember {
            name: "preferred",
            log: Rc::clone(&log),
        });
        let fallback = Rc::new(CompositionMember {
            name: "fallback",
            log: Rc::clone(&log),
        });
        let fallback_entry = second.add(pointer, &fallback);
        fallback_entry.resolve(GestureDisposition::Accepted);
        let preferred_entry = admit_preferred.then(|| first.add(pointer, &preferred));
        arena.close(pointer);
        arena.drain_deferred_resolutions();
        if let Some(preferred_entry) = preferred_entry {
            assert!(
                log.borrow().is_empty(),
                "a later preferred admission blocks an earlier fallback vote"
            );
            preferred_entry.resolve(GestureDisposition::Rejected);
            arena.drain_deferred_resolutions();
            assert_eq!(
                log.borrow().as_slice(),
                [
                    ("preferred", GestureDisposition::Rejected),
                    ("fallback", GestureDisposition::Accepted)
                ]
            );
        } else {
            assert_eq!(
                log.borrow().as_slice(),
                [("fallback", GestureDisposition::Accepted)]
            );
        }
    }
}

fn preferred_acceptance_rejects_a_pending_fallback() {
    let arena = GestureArena::new();
    let (first, second) = arena
        .compose(GestureCompetition::RequireFirstFailure)
        .expect("root composition")
        .into_branches();
    let log = Rc::new(RefCell::new(Vec::new()));
    let pointer = id(2);
    let preferred = Rc::new(CompositionMember {
        name: "preferred",
        log: Rc::clone(&log),
    });
    let fallback = Rc::new(CompositionMember {
        name: "fallback",
        log: Rc::clone(&log),
    });
    let fallback_entry = second.add(pointer, &fallback);
    let preferred_entry = first.add(pointer, &preferred);
    arena.close(pointer);
    fallback_entry.resolve(GestureDisposition::Accepted);
    preferred_entry.resolve(GestureDisposition::Accepted);
    fallback_entry.resolve(GestureDisposition::Accepted);
    arena.drain_deferred_resolutions();
    assert_eq!(
        log.borrow().as_slice(),
        [
            ("fallback", GestureDisposition::Rejected),
            ("preferred", GestureDisposition::Accepted)
        ]
    );
}

struct FailingCompositionMember {
    member: CompositionMember,
    failure: &'static str,
}

impl GestureArenaMember for FailingCompositionMember {
    fn accept_gesture(&self, pointer: PointerId) {
        self.member.accept_gesture(pointer);
    }
    fn reject_gesture(&self, pointer: PointerId) {
        self.member.reject_gesture(pointer);
        panic!("{}", self.failure);
    }
}

fn composition_preserves_first_callback_failure_and_recovers() {
    let arena = GestureArena::new();
    let (first, second) = arena
        .compose(GestureCompetition::RequireFirstFailure)
        .expect("root composition")
        .into_branches();
    let log = Rc::new(RefCell::new(Vec::new()));
    let pointer = id(2);
    let preferred = Rc::new(FailingCompositionMember {
        member: CompositionMember {
            name: "preferred",
            log: Rc::clone(&log),
        },
        failure: "preferred rejection failure",
    });
    let other = Rc::new(FailingCompositionMember {
        member: CompositionMember {
            name: "other",
            log: Rc::clone(&log),
        },
        failure: "competing rejection failure",
    });
    let fallback = Rc::new(CompositionMember {
        name: "fallback",
        log: Rc::clone(&log),
    });
    let preferred_entry = first.add(pointer, &preferred);
    arena.add(pointer, &other);
    let fallback_entry = second.add(pointer, &fallback);
    arena.close(pointer);
    fallback_entry.resolve(GestureDisposition::Accepted);
    let failure = catch_unwind(AssertUnwindSafe(|| {
        preferred_entry.resolve(GestureDisposition::Rejected)
    }))
    .expect_err("preferred callback fails");
    assert_eq!(
        failure.downcast_ref::<String>().map(String::as_str),
        Some("preferred rejection failure")
    );
    assert_eq!(
        log.borrow().as_slice(),
        [
            ("preferred", GestureDisposition::Rejected),
            ("other", GestureDisposition::Rejected),
            ("fallback", GestureDisposition::Accepted),
        ]
    );
    assert!(arena.is_empty());
    let fresh = Rc::new(CompositionMember {
        name: "fresh",
        log: Rc::clone(&log),
    });
    let fresh_entry = second.add(pointer, &fresh);
    arena.close(pointer);
    preferred_entry.resolve(GestureDisposition::Accepted);
    fallback_entry.resolve(GestureDisposition::Accepted);
    arena.drain_deferred_resolutions();
    fresh_entry.resolve(GestureDisposition::Accepted);
    assert_eq!(
        log.borrow().last(),
        Some(&("fresh", GestureDisposition::Accepted))
    );
}

const POINTERS: u64 = 3;

fn an_aliased_branch_join_cannot_change_the_contact_relationship() {
    let arena = GestureArena::new();
    let (first, second) = arena
        .compose(GestureCompetition::RequireFirstFailure)
        .expect("root composition")
        .into_branches();
    assert!(matches!(
        first.compose(GestureCompetition::Exclusive),
        Err(flui_interaction::arena::CompositionError::AlreadyComposed)
    ));
    let log = Rc::new(RefCell::new(Vec::new()));
    let pointer = id(2);
    let preferred = Rc::new(CompositionMember {
        name: "preferred",
        log: Rc::clone(&log),
    });
    let fallback = Rc::new(CompositionMember {
        name: "fallback",
        log: Rc::clone(&log),
    });
    let preferred_entry = first.add(pointer, &preferred);
    let refused_alias = second.add(pointer, &preferred);
    let fallback_entry = second.add(pointer, &fallback);
    refused_alias.resolve(GestureDisposition::Accepted);
    arena.close(pointer);
    fallback_entry.resolve(GestureDisposition::Accepted);
    assert!(
        log.borrow().is_empty(),
        "a refused aliased join cannot become a preferred vote"
    );
    preferred_entry.resolve(GestureDisposition::Rejected);
    arena.drain_deferred_resolutions();
    assert_eq!(
        log.borrow().as_slice(),
        [
            ("preferred", GestureDisposition::Rejected),
            ("fallback", GestureDisposition::Accepted)
        ]
    );
}

#[derive(Debug, Clone, Copy)]
enum MemberKind {
    Plain,
    PanicOnAccept,
    PanicOnReject,
    /// Re-enters the arena from its own callbacks: resolves its own entry
    /// again, releases it, and drains the deferred queue.
    Reentrant,
}

#[derive(Debug, Clone)]
enum Op {
    Add(u64, MemberKind),
    Close(u64),
    Accept(u64, usize),
    Reject(u64, usize),
    Hold(u64, usize),
    Release(u64, usize),
    Sweep(u64),
    Abandon(u64),
    Drain,
}

fn op_strategy() -> impl Strategy<Value = Op> {
    let pointer = 1..=POINTERS;
    let kind = prop_oneof![
        3 => Just(MemberKind::Plain),
        1 => Just(MemberKind::PanicOnAccept),
        1 => Just(MemberKind::PanicOnReject),
        1 => Just(MemberKind::Reentrant),
    ];
    prop_oneof![
        4 => (pointer.clone(), kind).prop_map(|(p, k)| Op::Add(p, k)),
        2 => pointer.clone().prop_map(Op::Close),
        2 => (pointer.clone(), 0..6_usize).prop_map(|(p, i)| Op::Accept(p, i)),
        2 => (pointer.clone(), 0..6_usize).prop_map(|(p, i)| Op::Reject(p, i)),
        1 => (pointer.clone(), 0..6_usize).prop_map(|(p, i)| Op::Hold(p, i)),
        1 => (pointer.clone(), 0..6_usize).prop_map(|(p, i)| Op::Release(p, i)),
        2 => pointer.clone().prop_map(Op::Sweep),
        1 => pointer.prop_map(Op::Abandon),
        1 => Just(Op::Drain),
    ]
}

/// One arena member; its verdicts land in the shared log.
struct ModelMember {
    index: usize,
    kind: MemberKind,
    log: Arc<std::sync::Mutex<Vec<(usize, GestureDisposition)>>>,
    arena: GestureArena,
    entry: std::sync::Mutex<Option<GestureArenaEntry>>,
}

impl ModelMember {
    fn record(&self, disposition: GestureDisposition) {
        self.log
            .lock()
            .expect("verdict log")
            .push((self.index, disposition));
        if let MemberKind::Reentrant = self.kind {
            let entry = self.entry.lock().expect("member entry").clone();
            if let Some(entry) = entry {
                entry.resolve(GestureDisposition::Accepted);
                entry.release();
            }
            self.arena.drain_deferred_resolutions();
        }
    }
}

impl GestureArenaMember for ModelMember {
    fn accept_gesture(&self, _pointer: PointerId) {
        self.record(GestureDisposition::Accepted);
        assert!(
            !matches!(self.kind, MemberKind::PanicOnAccept),
            "member accept panics"
        );
    }

    fn reject_gesture(&self, _pointer: PointerId) {
        self.record(GestureDisposition::Rejected);
        assert!(
            !matches!(self.kind, MemberKind::PanicOnReject),
            "member reject panics"
        );
    }
}

/// One pointer's arena, from its first member to its sweep.
#[derive(Default)]
struct Sequence {
    members: Vec<usize>,
    closed: bool,
    abandoned: bool,
}

struct Model {
    arena: GestureArena,
    log: Arc<std::sync::Mutex<Vec<(usize, GestureDisposition)>>>,
    members: Vec<Rc<ModelMember>>,
    entries: Vec<GestureArenaEntry>,
    /// Members withdrawn by their own rejection before any verdict.
    withdrawn: Vec<bool>,
    sequences: Vec<Sequence>,
    current: [Option<usize>; POINTERS as usize],
}

fn quietly(run: impl FnOnce()) {
    // Panicking members are part of the property; their panics resume out of
    // the arena by design.
    let _ = catch_unwind(AssertUnwindSafe(run));
}

impl Model {
    fn new() -> Self {
        Self {
            arena: GestureArena::new(),
            log: Arc::default(),
            members: Vec::new(),
            entries: Vec::new(),
            withdrawn: Vec::new(),
            sequences: Vec::new(),
            current: [None; POINTERS as usize],
        }
    }

    fn verdicts(&self, member: usize) -> Vec<GestureDisposition> {
        self.log
            .lock()
            .expect("verdict log")
            .iter()
            .filter(|(index, _)| *index == member)
            .map(|(_, disposition)| *disposition)
            .collect()
    }

    fn slot(pointer: u64) -> usize {
        usize::try_from(pointer - 1).expect("small pointer index")
    }

    fn member_of(&self, pointer: u64, choice: usize) -> Option<usize> {
        let sequence = &self.sequences[self.current[Self::slot(pointer)]?];
        (!sequence.members.is_empty()).then(|| sequence.members[choice % sequence.members.len()])
    }

    fn add(&mut self, pointer: u64, kind: MemberKind) {
        let slot = Self::slot(pointer);
        let sequence = *self.current[slot].get_or_insert_with(|| {
            self.sequences.push(Sequence::default());
            self.sequences.len() - 1
        });
        if self.sequences[sequence].closed {
            return;
        }
        let index = self.members.len();
        let member = Rc::new(ModelMember {
            index,
            kind,
            log: Arc::clone(&self.log),
            arena: self.arena.clone(),
            entry: std::sync::Mutex::new(None),
        });
        let entry = self.arena.add(id(pointer), &member);
        *member.entry.lock().expect("member entry") = Some(entry.clone());
        self.members.push(member);
        self.entries.push(entry);
        self.withdrawn.push(false);
        self.sequences[sequence].members.push(index);
    }

    fn apply(&mut self, op: &Op) {
        match *op {
            Op::Add(pointer, kind) => self.add(pointer, kind),
            Op::Close(pointer) => {
                if let Some(sequence) = self.current[Self::slot(pointer)] {
                    self.sequences[sequence].closed = true;
                    quietly(|| self.arena.close(id(pointer)));
                }
            }
            Op::Accept(pointer, choice) => {
                if let Some(member) = self.member_of(pointer, choice) {
                    quietly(|| self.entries[member].resolve(GestureDisposition::Accepted));
                }
            }
            Op::Reject(pointer, choice) => {
                if let Some(member) = self.member_of(pointer, choice) {
                    if self.verdicts(member).is_empty() {
                        self.withdrawn[member] = true;
                    }
                    quietly(|| self.entries[member].resolve(GestureDisposition::Rejected));
                }
            }
            Op::Hold(pointer, choice) => {
                if let Some(member) = self.member_of(pointer, choice) {
                    self.entries[member].hold();
                }
            }
            Op::Release(pointer, choice) => {
                if let Some(member) = self.member_of(pointer, choice) {
                    quietly(|| self.entries[member].release());
                }
            }
            Op::Sweep(pointer) => {
                if self.current[Self::slot(pointer)].take().is_some() {
                    let event = up(id(pointer), at(0.0, 0.0), PointerKind::Touch);
                    quietly(|| run_pointer_lifecycle(&self.arena, &event));
                }
            }
            Op::Abandon(pointer) => {
                // Abandon tears down every generation of the pointer, the
                // current one and any held earlier ones.
                for sequence in &mut self.sequences {
                    let on_pointer = sequence
                        .members
                        .first()
                        .is_some_and(|&member| self.entries[member].pointer() == id(pointer));
                    if on_pointer {
                        sequence.abandoned = true;
                    }
                }
                self.current[Self::slot(pointer)] = None;
                quietly(|| self.arena.abandon(id(pointer)));
            }
            Op::Drain => quietly(|| {
                self.arena.drain_deferred_resolutions();
            }),
        }
    }

    /// End every contact the way a binding would: close, lift, release every
    /// hold, drain.
    fn finish(&mut self) {
        for pointer in 1..=POINTERS {
            if self.current[Self::slot(pointer)].is_some() {
                self.apply(&Op::Close(pointer));
                self.apply(&Op::Sweep(pointer));
            }
        }
        for entry in &self.entries {
            quietly(|| entry.release());
        }
        quietly(|| {
            self.arena.drain_deferred_resolutions();
        });
    }

    fn check(&self) -> Result<(), TestCaseError> {
        prop_assert!(
            self.arena.is_empty(),
            "arena keeps state after every contact ended"
        );
        for (index, sequence) in self.sequences.iter().enumerate() {
            let mut accepted = 0;
            let mut contested = false;
            for &member in &sequence.members {
                let verdicts = self.verdicts(member);
                prop_assert_eq!(
                    verdicts.len(),
                    1,
                    "member {} of sequence {} got {:?}",
                    member,
                    index,
                    verdicts
                );
                if verdicts[0] == GestureDisposition::Accepted {
                    accepted += 1;
                }
                contested |= !self.withdrawn[member];
            }
            prop_assert!(accepted <= 1, "sequence {} has {} winners", index, accepted);
            if contested && !sequence.abandoned {
                prop_assert_eq!(
                    accepted,
                    1,
                    "sequence {} ended without a winner although a member stayed in",
                    index
                );
            }
        }
        Ok(())
    }

    /// After teardown: stale handles change nothing, and each pointer ID
    /// serves a fresh contest.
    fn check_reuse(&mut self) -> Result<(), TestCaseError> {
        let before = self.log.lock().expect("verdict log").len();
        for entry in &self.entries {
            quietly(|| entry.resolve(GestureDisposition::Accepted));
            quietly(|| entry.sweep());
        }
        prop_assert_eq!(
            self.log.lock().expect("verdict log").len(),
            before,
            "a stale handle produced a verdict"
        );
        for pointer in 1..=POINTERS {
            let first = self.members.len();
            self.add(pointer, MemberKind::Plain);
            self.add(pointer, MemberKind::Plain);
            self.apply(&Op::Close(pointer));
            quietly(|| self.entries[first].resolve(GestureDisposition::Accepted));
            prop_assert_eq!(self.verdicts(first), [GestureDisposition::Accepted]);
            prop_assert_eq!(self.verdicts(first + 1), [GestureDisposition::Rejected]);
            self.apply(&Op::Sweep(pointer));
        }
        prop_assert!(self.arena.is_empty());
        Ok(())
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    #[test]
    fn arena_settles_every_member_exactly_once(ops in proptest::collection::vec(op_strategy(), 1..60)) {
        let mut model = Model::new();
        for op in &ops {
            model.apply(op);
        }
        model.finish();
        model.check()?;
        model.check_reuse()?;
    }
}

/// A capture whose destructor panics: a stand-in for user state that fails
/// while being released.
struct PanicsOnDrop(&'static str);

impl Drop for PanicsOnDrop {
    fn drop(&mut self) {
        panic!("{} capture failed to release", self.0);
    }
}

/// Two callback captures that are their last owners and both panic while
/// dropped: disposal releases them one at a time, so the process survives,
/// the first failure surfaces, and the second capture is retained instead of
/// panicking during the first unwind (an abort).
fn dispose_retires_two_panicking_captures_one_at_a_time() {
    let lane = Lane::new();
    let first = PanicsOnDrop("on_tap");
    let second = PanicsOnDrop("on_tap_up");
    let tap = TapGestureRecognizer::builder(lane.arena)
        .on_tap(move |_| {
            let _ = &first;
        })
        .on_tap_up(move |_| {
            let _ = &second;
        })
        .build();
    let failure = catch_unwind(AssertUnwindSafe(|| drop(tap)))
        .expect_err("the first capture's panic surfaces from final owner drop");
    let message = failure
        .downcast_ref::<String>()
        .map(String::as_str)
        .unwrap_or_default();
    assert!(
        message.contains("capture failed to release"),
        "the first failure is the one reported, got {message:?}"
    );
}

/// A far second click ends the first double-tap window; when the cancel
/// callback that ending runs panics, the far contact is still admitted as
/// the next first tap, so a click near it completes a double tap.
fn panicking_window_end_still_admits_the_far_contact() {
    let mut lane = Lane::new();
    let doubles = counter();
    let double_log = Rc::clone(&doubles);
    let armed = Rc::new(Cell::new(true));
    let trip = Rc::clone(&armed);
    let double_tap = DoubleTapGestureRecognizer::builder(lane.arena.clone())
        .on_double_tap(move |_| double_log.set(double_log.get() + 1))
        .on_double_tap_cancel(move |_| {
            assert!(!trip.replace(false), "cancel callback failed");
        })
        .build();
    lane.join(&double_tap);
    let kind = PointerKind::Touch;
    click(&lane, id(2), at(10.0, 10.0), kind);
    lane.frames(50);
    let failure = catch_unwind(AssertUnwindSafe(|| {
        lane.send(&down(id(3), at(300.0, 10.0), kind));
    }));
    assert!(failure.is_err(), "premise: the cancel callback panicked");
    lane.send(&up(id(3), at(300.0, 10.0), kind));
    lane.frames(50);
    click(&lane, id(4), at(302.0, 10.0), kind);
    assert_eq!(
        doubles.get(),
        1,
        "the far contact became the next first tap despite the panic"
    );
}

/// A candidate that is not a member and whose destructor reaches back into
/// the arena.
struct ClosesOnDrop {
    arena: GestureArena,
    pointer: PointerId,
    closed: Rc<Cell<bool>>,
}

impl GestureArenaMember for ClosesOnDrop {
    fn accept_gesture(&self, _pointer: PointerId) {}
    fn reject_gesture(&self, _pointer: PointerId) {}
}

impl Drop for ClosesOnDrop {
    fn drop(&mut self) {
        self.arena.close(self.pointer);
        self.closed.set(true);
    }
}

/// Borrowed acceptance leaves the candidate with its caller. Its eventual
/// destruction can reenter the arena after the operation returns.
fn ignored_accept_candidate_drops_outside_the_slot_lock() {
    // On a worker with a deadline: a regression deadlocks instead of failing,
    // and must not hang the whole test binary.
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let arena = GestureArena::new();
        let pointer = id(2);
        let closed = counter_flag();
        let member = Rc::new(Verdicts::default());
        arena.add(pointer, &member);
        let candidate: Rc<dyn GestureArenaMember> = Rc::new(ClosesOnDrop {
            arena: arena.clone(),
            pointer,
            closed: Rc::clone(&closed),
        });
        arena.accept(pointer, &candidate);
        assert!(
            !closed.get(),
            "borrowed acceptance keeps the caller's owner"
        );
        drop(candidate);
        assert!(
            closed.get(),
            "the destructor ran and its call into the arena returned"
        );
        let _ = done.send(());
    });
    finished
        .recv_timeout(Duration::from_secs(5))
        .expect("accept returned: the candidate was not dropped under the slot lock");
}

fn counter_flag() -> Rc<Cell<bool>> {
    Rc::new(Cell::new(false))
}

fn nonmember_resolution_candidate_retires_after_detachment() {
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let arena = GestureArena::new();
        let pointer = id(2);
        let closed = counter_flag();
        let member = Rc::new(Verdicts::default());
        arena.add(pointer, &member);
        let candidate: Rc<dyn GestureArenaMember> = Rc::new(ClosesOnDrop {
            arena: arena.clone(),
            pointer,
            closed: Rc::clone(&closed),
        });
        arena.resolve(pointer, Some(&candidate));
        assert!(
            !closed.get(),
            "resolution does not consume the borrowed candidate"
        );
        assert!(arena.is_empty(), "detachment precedes caller retirement");
        drop(candidate);
        assert!(closed.get(), "candidate destructor reentered the arena");
        assert!(arena.is_empty());
        let _ = done.send(());
    });
    finished.recv_timeout(Duration::from_secs(5)).expect(
        "resolution candidate must retire after the slot borrow and map entry are released",
    );
}

type RetirementOwner = RefCell<Option<Rc<RetirementMember>>>;

struct RetirementMember {
    dropped: Rc<Cell<u32>>,
    rejected: Rc<Cell<u32>>,
    panic_on_reject: bool,
    panic_on_drop: bool,
    owner: std::rc::Weak<RetirementOwner>,
}

impl GestureArenaMember for RetirementMember {
    fn accept_gesture(&self, _: PointerId) {}

    fn reject_gesture(&self, _: PointerId) {
        self.rejected.set(self.rejected.get() + 1);
        if let Some(owner) = self.owner.upgrade() {
            let retired = owner.borrow_mut().take();
            drop(retired);
        }
        assert!(!self.panic_on_reject, "member rejection failed");
    }
}

impl Drop for RetirementMember {
    fn drop(&mut self) {
        self.dropped.set(self.dropped.get() + 1);
        assert!(!self.panic_on_drop, "candidate retirement failed");
    }
}

fn arena_retirement_preserves_the_first_failure_and_recovers() {
    for (retirement_panics, rejection_panics, expected_failure) in [
        (false, false, None),
        (true, false, Some("candidate retirement failed")),
        (false, true, Some("member rejection failed")),
        (true, true, Some("member rejection failed")),
    ] {
        let arena = GestureArena::new();
        let pointer = id(2);
        let retired = counter();
        let rejected = counter();
        let owner: Rc<RetirementOwner> = Rc::default();
        let member = Rc::new(RetirementMember {
            dropped: Rc::clone(&retired),
            rejected: Rc::clone(&rejected),
            panic_on_reject: rejection_panics,
            panic_on_drop: retirement_panics,
            owner: Rc::downgrade(&owner),
        });
        arena.add(pointer, &member);
        // Only the external holder owns the participant until its callback
        // releases that owner. Delivery then owns the last upgraded Rc.
        *owner.borrow_mut() = Some(member);
        // Borrowed candidates remain owned by their caller throughout a
        // resolve call. Exercise framework retirement instead: rejection
        // releases the external owner, leaving delivery's last Rc snapshot.
        let result = catch_unwind(AssertUnwindSafe(|| {
            arena.resolve(pointer, None);
        }));
        if let Some(expected_failure) = expected_failure {
            let failure = result.expect_err("a failing retirement or rejection must propagate");
            assert_eq!(
                flui_foundation::panic::payload_text(failure.as_ref()),
                Some(expected_failure),
            );
        } else {
            result.expect("healthy notification retirement and rejection complete");
        }
        assert_eq!(
            rejected.get(),
            1,
            "accepted notifications remain deliverable"
        );
        assert_eq!(
            retired.get(),
            u32::from(!rejection_panics),
            "successful callback retires its last owner, even if Drop fails; failed callback retains it"
        );
        assert!(
            owner.borrow().is_none(),
            "the callback released its external owner"
        );
        assert!(arena.is_empty(), "failed contest was detached");

        let fresh = Rc::new(Verdicts::default());
        arena.add(pointer, &fresh);
        arena.close(pointer);
        assert_eq!(arena.drain_deferred_resolutions(), 1);
        assert_eq!(fresh.get(), (1, 0));
        assert!(arena.is_empty());
    }
}

fn arena_polls_pointer_deadlines_in_identity_order() {
    let clock = ManualClock::new();
    let arena = GestureArena::binding_driven(Arc::new(clock.clone()));
    let fired = Rc::new(RefCell::new(Vec::new()));
    let mut recognizers = Vec::new();
    let mut rivals = Vec::new();
    for raw in (1..=8).rev() {
        let pointer = id(raw);
        let log = Rc::clone(&fired);
        let recognizer = LongPressGestureRecognizer::builder(arena.clone())
            .on_long_press(move || log.borrow_mut().push(raw))
            .build();
        let event = down(pointer, at(0.0, 0.0), PointerKind::Touch);
        recognizer.add_pointer(PointerDispatch::at_root(&event));
        let rival = Rc::new(Verdicts::default());
        arena.add(pointer, &rival);
        arena.close(pointer);
        recognizers.push(recognizer);
        rivals.push(rival);
    }

    clock.advance(Duration::from_secs(1));
    arena.poll_deadlines();
    assert_eq!(
        *fired.borrow(),
        [1, 2, 3, 4, 5, 6, 7, 8],
        "simultaneously due active pointers are polled in identity order",
    );
    arena.poll_deadlines();
    assert_eq!(fired.borrow().len(), 8, "each deadline fires once");
    for (raw, recognizer) in (1..=8).rev().zip(recognizers) {
        let event = up(id(raw), at(0.0, 0.0), PointerKind::Touch);
        recognizer.handle_event(PointerDispatch::at_root(&event));
    }
    assert!(arena.is_empty());
    assert!(!arena.has_pending_deadlines());
}
