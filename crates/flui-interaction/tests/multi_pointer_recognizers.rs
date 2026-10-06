//! Scale, force-press, tap-and-drag and eager recognizers driven through the
//! production `GestureBinding`: the binding closes each arena after Down,
//! sweeps it after Up, and the frame drains lone-member default wins, exactly
//! as a presentation drives them.
use std::{
    cell::{Cell, RefCell},
    f64::consts::PI,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use flui_foundation::geometry::Offset;
use flui_interaction::events::{
    PointerEvent, PointerType, make_cancel_event_for_id, make_down_event_for_id,
    make_move_event_for_id, make_up_event_for_id,
};
use flui_interaction::recognizers::scale::{ScaleEndDetails, ScaleUpdateDetails};
use flui_interaction::routing::PointerDispatch;
use flui_interaction::{
    DragAxis, DragGestureRecognizer, EagerGestureRecognizer, ForcePressGestureRecognizer,
    GestureBinding, GestureRecognizer, HitTestResult, ManualClock, PointerId,
    ScaleGestureRecognizer, TapAndDragGestureRecognizer, TapGestureRecognizer,
};

// ============================================================================
// Rig
// ============================================================================

type Route = (Option<u64>, Rc<Cell<bool>>, Rc<dyn Fn(&PointerEvent)>);

/// A binding whose global pointer handler forwards to attached recognizers
/// the way a detector does: `add_pointer` then the Down itself, and every
/// later event through `handle_event`.
struct Rig {
    binding: Rc<GestureBinding>,
    clock: ManualClock,
    routes: Rc<RefCell<Vec<Route>>>,
}

fn id(raw: u64) -> PointerId {
    PointerId::new(raw).expect("nonzero pointer id")
}

fn down_position(event: &PointerEvent) -> Offset<f64> {
    match event {
        PointerEvent::Down(data) => Offset::new(data.state.position.x, data.state.position.y),
        _ => Offset::ZERO,
    }
}

fn pointer_of(event: &PointerEvent) -> Option<u64> {
    let info = match event {
        PointerEvent::Down(data) | PointerEvent::Up(data) => &data.pointer,
        PointerEvent::Move(data) => &data.pointer,
        PointerEvent::Cancel(info) => info,
        _ => return None,
    };
    info.pointer_id.map(|p| p.get_inner().get())
}

impl Rig {
    fn new() -> Self {
        let clock = ManualClock::new();
        let binding = Rc::new(GestureBinding::with_clock(Arc::new(clock.clone())));
        let routes: Rc<RefCell<Vec<Route>>> = Rc::new(RefCell::new(Vec::new()));
        let dispatch = Rc::clone(&routes);
        binding
            .pointer_router()
            .add_global_handler(Rc::new(move |event: &PointerEvent| {
                // Snapshot, then release: a callback may attach or detach.
                let snapshot: Vec<Route> = dispatch.borrow().clone();
                for (only, live, route) in snapshot {
                    if live.get() && (only.is_none() || only == pointer_of(event)) {
                        route(event);
                    }
                }
            }));
        Self {
            binding,
            clock,
            routes,
        }
    }

    /// Route events to `recognizer`, optionally for one pointer only. The
    /// returned flag stops delivery (a detector unmounting it).
    fn attach<R: GestureRecognizer + 'static>(
        &self,
        recognizer: &Arc<R>,
        only: Option<u64>,
    ) -> Rc<Cell<bool>> {
        self.attach_with_global(recognizer, only, PointerEvent::clone)
    }

    /// [`attach`](Self::attach), delivering `global(event)` as the root-space
    /// event that accompanies each local one.
    fn attach_with_global<R: GestureRecognizer + 'static>(
        &self,
        recognizer: &Arc<R>,
        only: Option<u64>,
        global: impl Fn(&PointerEvent) -> PointerEvent + 'static,
    ) -> Rc<Cell<bool>> {
        let live = Rc::new(Cell::new(true));
        let recognizer = Arc::clone(recognizer);
        let alive = Rc::clone(&live);
        let route: Rc<dyn Fn(&PointerEvent)> = Rc::new(move |event| {
            if let PointerEvent::Down(_) = event {
                let position = down_position(event);
                let pointer = id(pointer_of(event).expect("down carries an id"));
                recognizer.add_pointer(pointer, position, position);
                if !alive.get() {
                    return;
                }
            }
            let global = global(event);
            recognizer.handle_event(PointerDispatch {
                local: event,
                global: &global,
            });
        });
        self.routes
            .borrow_mut()
            .push((only, Rc::clone(&live), route));
        live
    }

    fn send(&self, event: &PointerEvent) {
        self.binding
            .handle_pointer_event(event, |_| HitTestResult::new());
    }

    fn down_with(&self, pointer: u64, x: f64, y: f64, kind: PointerType, pressure: f32) {
        let mut event = make_down_event_for_id(id(pointer), Offset::new(x, y), kind);
        if let PointerEvent::Down(data) = &mut event {
            data.state.pressure = pressure;
        }
        self.send(&event);
    }

    fn down(&self, pointer: u64, x: f64, y: f64) {
        self.down_with(pointer, x, y, PointerType::Touch, 0.5);
    }

    fn move_with(&self, pointer: u64, x: f64, y: f64, kind: PointerType, pressure: f32) {
        let mut event = make_move_event_for_id(id(pointer), Offset::new(x, y), kind);
        if let PointerEvent::Move(data) = &mut event {
            data.current.pressure = pressure;
        }
        self.send(&event);
        self.frame();
    }

    fn move_to(&self, pointer: u64, x: f64, y: f64) {
        self.move_with(pointer, x, y, PointerType::Touch, 0.5);
    }

    fn up_with(&self, pointer: u64, x: f64, y: f64, kind: PointerType) {
        self.send(&make_up_event_for_id(id(pointer), Offset::new(x, y), kind));
    }

    fn up(&self, pointer: u64, x: f64, y: f64) {
        self.up_with(pointer, x, y, PointerType::Touch);
    }

    fn cancel(&self, pointer: u64) {
        self.send(&make_cancel_event_for_id(id(pointer), PointerType::Touch));
    }

    /// End of input/frame: flush coalesced moves, drain lone-member wins.
    fn frame(&self) {
        self.binding.flush_pending_moves();
        self.binding.drain_deferred_arena_resolutions();
    }

    fn advance(&self, ms: u64) {
        self.clock.advance(Duration::from_millis(ms));
    }
}

/// Run `op`, which must panic with a callback's payload.
fn expect_panic(what: &str, op: impl FnOnce()) {
    let result = catch_unwind(AssertUnwindSafe(op));
    assert!(result.is_err(), "{what}: the callback panic must propagate");
}

/// A one-shot panic switch for a callback.
fn trip(flag: &Cell<bool>, message: &str) {
    assert!(!flag.replace(false), "{message}");
}

fn run_rows(family: &str, rows: &[(&str, fn())]) {
    let mut failures = Vec::new();
    for (name, row) in rows {
        if catch_unwind(*row).is_err() {
            failures.push(*name);
        }
    }
    assert!(failures.is_empty(), "{family} rows failed: {failures:?}");
}

// ============================================================================
// Scale
// ============================================================================

#[derive(Default)]
struct ScaleLog {
    starts: Cell<usize>,
    updates: RefCell<Vec<ScaleUpdateDetails>>,
    ends: RefCell<Vec<ScaleEndDetails>>,
    cancels: Cell<usize>,
    panic_start: Cell<bool>,
    panic_update: Cell<bool>,
    panic_end: Cell<bool>,
}

impl ScaleLog {
    fn last_update(&self) -> ScaleUpdateDetails {
        self.updates.borrow().last().cloned().expect("an update")
    }

    fn all_finite(&self) -> bool {
        self.updates.borrow().iter().all(|u| {
            u.scale.is_finite()
                && u.horizontal_scale.is_finite()
                && u.vertical_scale.is_finite()
                && u.rotation.is_finite()
                && u.focal_point.is_finite()
        })
    }
}

fn scale_on(rig: &Rig) -> (Arc<ScaleGestureRecognizer>, Rc<ScaleLog>) {
    let log = Rc::new(ScaleLog::default());
    let (s, u, e, c) = (log.clone(), log.clone(), log.clone(), log.clone());
    let recognizer = ScaleGestureRecognizer::new(rig.binding.arena().clone())
        .with_on_scale_start(move |_| {
            s.starts.set(s.starts.get() + 1);
            trip(&s.panic_start, "scale start callback panic");
        })
        .with_on_scale_update(move |d| {
            u.updates.borrow_mut().push(d);
            trip(&u.panic_update, "scale update callback panic");
        })
        .with_on_scale_end(move |d| {
            e.ends.borrow_mut().push(d);
            trip(&e.panic_end, "scale end callback panic");
        })
        .with_on_scale_cancel(move || c.cancels.set(c.cancels.get() + 1));
    rig.attach(&recognizer, None);
    (recognizer, log)
}

/// Two contacts 200 px apart on a horizontal line, spread to 300 px.
fn pinch_out(rig: &Rig, a: u64, b: u64) {
    rig.down(a, 100.0, 200.0);
    rig.down(b, 300.0, 200.0);
    rig.frame();
    rig.move_to(a, 50.0, 200.0);
    rig.move_to(b, 350.0, 200.0);
}

fn scale_axis_with_zero_baseline_holds_finite() {
    let rig = Rig::new();
    let (_scale, log) = scale_on(&rig);
    // Placed on one vertical line: the horizontal span starts at zero.
    rig.down(1, 200.0, 100.0);
    rig.down(2, 200.0, 300.0);
    rig.frame();
    rig.move_to(1, 200.0, 50.0);
    rig.move_to(2, 200.0, 350.0);
    rig.up(1, 200.0, 50.0);
    rig.up(2, 200.0, 350.0);
    assert!(log.starts.get() == 1 && log.all_finite());
    let last = log.last_update();
    assert_eq!(last.horizontal_scale, 1.0);
    assert!((last.vertical_scale - 1.5).abs() < 1e-9);
}

fn scale_from_coincident_contacts_stays_finite() {
    let rig = Rig::new();
    let (_scale, log) = scale_on(&rig);
    rig.down(1, 200.0, 200.0);
    rig.down(2, 200.0, 200.0);
    rig.frame();
    rig.move_to(1, 150.0, 200.0);
    rig.move_to(2, 250.0, 200.0);
    rig.move_to(2, 300.0, 200.0);
    assert!(log.all_finite(), "{:?}", log.updates.borrow());
    assert_eq!(log.updates.borrow()[0].scale, 1.0);
    assert!(log.last_update().scale > 1.0);
}

fn scale_claims_the_second_contacts_arena() {
    let rig = Rig::new();
    let (_scale, log) = scale_on(&rig);
    // A pan that only sees the second contact competes for its arena.
    let pans = Rc::new(Cell::new(0));
    let counted = pans.clone();
    let pan = DragGestureRecognizer::new(rig.binding.arena().clone(), DragAxis::Free)
        .with_on_start(move |_| counted.set(counted.get() + 1));
    rig.attach(&pan, Some(2));
    rig.down(1, 100.0, 200.0);
    rig.down(2, 300.0, 200.0);
    rig.frame();
    rig.move_to(2, 400.0, 200.0);
    rig.move_to(2, 450.0, 200.0);
    rig.up(2, 450.0, 200.0);
    rig.up(1, 100.0, 200.0);
    assert_eq!(log.starts.get(), 1, "the scale starts");
    assert_eq!(pans.get(), 0, "the pinch's second contact is the scale's");
}

fn scale_that_won_its_first_contact_starts_on_the_second() {
    let rig = Rig::new();
    let (_scale, log) = scale_on(&rig);
    rig.down(1, 100.0, 200.0);
    rig.frame(); // a lone member wins the first contact by default
    rig.down(2, 300.0, 200.0);
    rig.frame();
    rig.move_to(1, 50.0, 200.0);
    rig.move_to(2, 350.0, 200.0);
    assert_eq!(log.starts.get(), 1);
    assert!((log.last_update().scale - 1.5).abs() < 1e-9);
}

fn scale_is_continuous_across_added_and_lifted_contacts() {
    let rig = Rig::new();
    let (_scale, log) = scale_on(&rig);
    pinch_out(&rig, 1, 2);
    assert!((log.last_update().scale - 1.5).abs() < 1e-9);
    // A third contact at the focal point halves nothing on its own.
    rig.down(3, 200.0, 200.0);
    rig.frame();
    rig.move_to(3, 200.5, 200.0);
    let with_three = log.last_update();
    assert_eq!(with_three.pointer_count, 3);
    assert!(
        (with_three.scale - 1.5).abs() < 0.01,
        "added contact jumped the scale to {}",
        with_three.scale
    );
    rig.up(3, 200.5, 200.0);
    rig.move_to(1, 50.5, 200.0);
    assert!(
        (log.last_update().scale - 1.5).abs() < 0.01,
        "lifted contact jumped the scale to {}",
        log.last_update().scale
    );
    rig.up(1, 50.5, 200.0);
    assert_eq!(
        log.ends.borrow().len(),
        1,
        "below two contacts the scale ends"
    );
}

fn scale_rotation_keeps_counting_past_half_a_turn() {
    let rig = Rig::new();
    let (_scale, log) = scale_on(&rig);
    let at = |angle_deg: f64, radius: f64| {
        let a = angle_deg.to_radians();
        (200.0 + radius * a.cos(), 200.0 + radius * a.sin())
    };
    rig.down(1, 300.0, 200.0);
    rig.down(2, 100.0, 200.0);
    rig.frame();
    // Pinch out to start, then turn the pair 220 degrees in 20-degree steps.
    rig.move_to(1, 360.0, 200.0);
    rig.move_to(2, 40.0, 200.0);
    for step in 1..=11 {
        let angle = f64::from(step) * 20.0;
        let (x1, y1) = at(angle, 160.0);
        let (x2, y2) = at(angle + 180.0, 160.0);
        rig.move_to(1, x1, y1);
        rig.move_to(2, x2, y2);
    }
    let rotation = log.last_update().rotation;
    assert!(
        (rotation - 220.0_f64.to_radians()).abs() < 1e-6,
        "rotation {rotation} rad, expected {}",
        220.0_f64.to_radians()
    );
    assert!(rotation > PI);
}

fn scale_start_panic_then_next_gesture() {
    let rig = Rig::new();
    let (_scale, log) = scale_on(&rig);
    log.panic_start.set(true);
    rig.down(1, 100.0, 200.0);
    rig.down(2, 300.0, 200.0);
    expect_panic("on_start", || rig.frame());
    rig.move_to(1, 50.0, 200.0);
    rig.up(1, 50.0, 200.0);
    rig.up(2, 300.0, 200.0);
    let ends = log.ends.borrow().len();
    pinch_out(&rig, 3, 4);
    rig.up(3, 50.0, 200.0);
    rig.up(4, 350.0, 200.0);
    assert_eq!(log.starts.get(), 2);
    assert_eq!(log.ends.borrow().len(), ends + 1);
}

fn scale_update_and_end_panics_then_next_gesture() {
    let rig = Rig::new();
    let (_scale, log) = scale_on(&rig);
    rig.down(1, 100.0, 200.0);
    rig.down(2, 300.0, 200.0);
    rig.frame();
    log.panic_update.set(true);
    expect_panic("on_update", || rig.move_to(1, 50.0, 200.0));
    rig.move_to(2, 350.0, 200.0);
    assert!((log.last_update().scale - 1.5).abs() < 1e-9);
    log.panic_end.set(true);
    expect_panic("on_end", || rig.up(1, 50.0, 200.0));
    rig.up(2, 350.0, 200.0);
    pinch_out(&rig, 3, 4);
    rig.up(3, 50.0, 200.0);
    assert_eq!(log.starts.get(), 2);
    assert_eq!(log.ends.borrow().len(), 2);
}

fn scale_cancel_mid_gesture_then_next_gesture() {
    let rig = Rig::new();
    let (_scale, log) = scale_on(&rig);
    pinch_out(&rig, 1, 2);
    rig.cancel(1);
    rig.move_to(2, 400.0, 200.0);
    rig.up(2, 400.0, 200.0);
    assert_eq!(log.cancels.get(), 1);
    assert!(log.ends.borrow().is_empty());
    let updates = log.updates.borrow().len();
    pinch_out(&rig, 3, 4);
    assert_eq!(log.starts.get(), 2);
    assert!(log.updates.borrow().len() > updates);
}

fn scale_contact_lost_to_a_competitor_cancels_the_scale() {
    let rig = Rig::new();
    // An eager member that sees only the third contact claims its arena first.
    let eager = EagerGestureRecognizer::new(rig.binding.arena().clone());
    rig.attach(&eager, Some(3));
    let (_scale, log) = scale_on(&rig);
    pinch_out(&rig, 1, 2);
    assert_eq!(log.starts.get(), 1);
    rig.down(3, 200.0, 250.0);
    rig.frame();
    assert_eq!(log.cancels.get(), 1, "losing a contact's arena cancels");
    let updates = log.updates.borrow().len();
    rig.move_to(1, 20.0, 200.0);
    rig.up(1, 20.0, 200.0);
    rig.up(2, 350.0, 200.0);
    rig.up(3, 200.0, 250.0);
    assert_eq!(
        log.updates.borrow().len(),
        updates,
        "a cancelled scale is over"
    );
    assert!(log.ends.borrow().is_empty());
    pinch_out(&rig, 4, 5);
    assert_eq!(log.starts.get(), 2, "the next gesture starts clean");
}

fn scale_disposed_from_its_update() {
    let rig = Rig::new();
    let slot: Rc<RefCell<Option<Arc<ScaleGestureRecognizer>>>> = Rc::default();
    let live_slot: Rc<RefCell<Option<Rc<Cell<bool>>>>> = Rc::default();
    let updates = Rc::new(Cell::new(0));
    let (s, l, n) = (slot.clone(), live_slot.clone(), updates.clone());
    let recognizer =
        ScaleGestureRecognizer::new(rig.binding.arena().clone()).with_on_scale_update(move |_| {
            n.set(n.get() + 1);
            if let Some(live) = l.borrow().as_ref() {
                live.set(false);
            }
            if let Some(recognizer) = s.borrow_mut().take() {
                recognizer.dispose();
            }
        });
    *live_slot.borrow_mut() = Some(rig.attach(&recognizer, None));
    *slot.borrow_mut() = Some(recognizer);
    pinch_out(&rig, 1, 2);
    rig.up(1, 50.0, 200.0);
    rig.up(2, 350.0, 200.0);
    assert_eq!(updates.get(), 1, "a disposed scale publishes nothing more");
}

#[test]
fn scale_publishes_finite_continuous_values_and_owns_its_contacts() {
    run_rows(
        "scale",
        &[
            (
                "zero horizontal baseline",
                scale_axis_with_zero_baseline_holds_finite,
            ),
            (
                "coincident contacts",
                scale_from_coincident_contacts_stays_finite,
            ),
            (
                "second contact's arena",
                scale_claims_the_second_contacts_arena,
            ),
            (
                "won first contact by default",
                scale_that_won_its_first_contact_starts_on_the_second,
            ),
            (
                "added and lifted contacts",
                scale_is_continuous_across_added_and_lifted_contacts,
            ),
            (
                "rotation past half a turn",
                scale_rotation_keeps_counting_past_half_a_turn,
            ),
            ("start panic", scale_start_panic_then_next_gesture),
            (
                "update and end panics",
                scale_update_and_end_panics_then_next_gesture,
            ),
            ("cancel", scale_cancel_mid_gesture_then_next_gesture),
            (
                "contact lost to a competitor",
                scale_contact_lost_to_a_competitor_cancels_the_scale,
            ),
            ("dispose from update", scale_disposed_from_its_update),
        ],
    );
}

// ============================================================================
// Force press
// ============================================================================

#[derive(Default)]
struct PressLog {
    starts: Cell<usize>,
    peaks: Cell<usize>,
    ends: Cell<usize>,
    panic_start: Cell<bool>,
}

fn press_on(rig: &Rig) -> (Arc<ForcePressGestureRecognizer>, Rc<PressLog>) {
    let log = Rc::new(PressLog::default());
    let (s, p, e) = (log.clone(), log.clone(), log.clone());
    let recognizer = ForcePressGestureRecognizer::new(rig.binding.arena().clone())
        .with_on_start(move |_| {
            s.starts.set(s.starts.get() + 1);
            trip(&s.panic_start, "force press start callback panic");
        })
        .with_on_peak(move |_| p.peaks.set(p.peaks.get() + 1))
        .with_on_end(move |_| e.ends.set(e.ends.get() + 1));
    (recognizer, log)
}

/// A pen with a real sensor pressing at `pressures`, in place.
fn press(rig: &Rig, pointer: u64, pressures: &[f32]) {
    rig.down_with(pointer, 100.0, 100.0, PointerType::Pen, 0.2);
    rig.frame();
    for &pressure in pressures {
        rig.move_with(pointer, 100.0, 100.0, PointerType::Pen, pressure);
    }
    rig.up_with(pointer, 100.0, 100.0, PointerType::Pen);
}

fn sensorless_contacts_never_force_press() {
    for kind in [PointerType::Mouse, PointerType::Touch] {
        let rig = Rig::new();
        let (press, log) = press_on(&rig);
        rig.attach(&press, None);
        rig.down_with(1, 100.0, 100.0, kind, 0.5);
        rig.frame();
        rig.move_with(1, 100.0, 100.0, kind, 0.5);
        rig.up_with(1, 100.0, 100.0, kind);
        assert_eq!(
            log.starts.get(),
            0,
            "{kind:?} at the W3C 0.5 is not a sensor"
        );
    }
}

fn force_press_claims_the_arena_before_starting() {
    let rig = Rig::new();
    let taps = Rc::new(Cell::new(0));
    let counted = taps.clone();
    let tap = TapGestureRecognizer::new(rig.binding.arena().clone())
        .with_on_tap(move |_| counted.set(counted.get() + 1));
    rig.attach(&tap, None);
    let (press_rec, log) = press_on(&rig);
    rig.attach(&press_rec, None);
    press(&rig, 1, &[0.3, 0.7, 0.9]);
    assert_eq!(
        (log.starts.get(), log.peaks.get(), log.ends.get()),
        (1, 1, 1)
    );
    assert_eq!(taps.get(), 0, "a force press that started owns the contact");
}

fn force_press_start_panic_then_next_press() {
    let rig = Rig::new();
    let (press_rec, log) = press_on(&rig);
    rig.attach(&press_rec, None);
    log.panic_start.set(true);
    rig.down_with(1, 100.0, 100.0, PointerType::Pen, 0.2);
    rig.frame();
    expect_panic("on_start", || {
        rig.move_with(1, 100.0, 100.0, PointerType::Pen, 0.7);
    });
    rig.up_with(1, 100.0, 100.0, PointerType::Pen);
    press(&rig, 2, &[0.7]);
    assert_eq!(log.starts.get(), 2);
    assert_eq!(log.ends.get(), 2);
}

fn force_press_start_panic_still_delivers_peak_and_end() {
    let rig = Rig::new();
    let (press_rec, log) = press_on(&rig);
    rig.attach(&press_rec, None);
    log.panic_start.set(true);
    rig.down_with(1, 100.0, 100.0, PointerType::Pen, 0.2);
    rig.frame(); // the lone member wins by default
    // Start and peak are one transition; the start's panic must not drop the peak.
    expect_panic("on_start", || {
        rig.move_with(1, 100.0, 100.0, PointerType::Pen, 0.9);
    });
    rig.up_with(1, 100.0, 100.0, PointerType::Pen);
    assert_eq!(
        (log.starts.get(), log.peaks.get(), log.ends.get()),
        (1, 1, 1)
    );
}

fn force_press_cancel_ends_once() {
    let rig = Rig::new();
    let (press_rec, log) = press_on(&rig);
    rig.attach(&press_rec, None);
    rig.down_with(1, 100.0, 100.0, PointerType::Pen, 0.2);
    rig.frame();
    rig.move_with(1, 100.0, 100.0, PointerType::Pen, 0.7);
    rig.cancel(1);
    assert_eq!((log.starts.get(), log.ends.get()), (1, 1));
    press(&rig, 2, &[0.7]);
    assert_eq!((log.starts.get(), log.ends.get()), (2, 2));
}

fn force_press_disposed_from_its_start() {
    let rig = Rig::new();
    let slot: Rc<RefCell<Option<Arc<ForcePressGestureRecognizer>>>> = Rc::default();
    let live_slot: Rc<RefCell<Option<Rc<Cell<bool>>>>> = Rc::default();
    let ends = Rc::new(Cell::new(0));
    let (s, l, e) = (slot.clone(), live_slot.clone(), ends.clone());
    let recognizer = ForcePressGestureRecognizer::new(rig.binding.arena().clone())
        .with_on_start(move |_| {
            if let Some(live) = l.borrow().as_ref() {
                live.set(false);
            }
            if let Some(recognizer) = s.borrow_mut().take() {
                recognizer.dispose();
            }
        })
        .with_on_end(move |_| e.set(e.get() + 1));
    *live_slot.borrow_mut() = Some(rig.attach(&recognizer, None));
    *slot.borrow_mut() = Some(recognizer);
    press(&rig, 1, &[0.7, 0.9]);
    assert_eq!(ends.get(), 0, "a disposed force press reports nothing more");
}

#[test]
fn force_press_needs_a_sensor_and_the_arena() {
    run_rows(
        "force press",
        &[
            ("sensor-less 0.5", sensorless_contacts_never_force_press),
            (
                "claims the arena",
                force_press_claims_the_arena_before_starting,
            ),
            ("start panic", force_press_start_panic_then_next_press),
            (
                "start panic keeps peak and end",
                force_press_start_panic_still_delivers_peak_and_end,
            ),
            ("cancel", force_press_cancel_ends_once),
            ("dispose from start", force_press_disposed_from_its_start),
        ],
    );
}

// ============================================================================
// Tap and drag
// ============================================================================

#[derive(Default)]
struct TapDragLog {
    events: RefCell<Vec<String>>,
    counts: RefCell<Vec<u32>>,
    panic_down: Cell<bool>,
    panic_up: Cell<bool>,
    panic_update: Cell<bool>,
}

impl TapDragLog {
    fn count(&self, name: &str) -> usize {
        self.events.borrow().iter().filter(|e| *e == name).count()
    }
}

fn tap_drag_on(rig: &Rig) -> (Arc<TapAndDragGestureRecognizer>, Rc<TapDragLog>) {
    let log = Rc::new(TapDragLog::default());
    let push = |log: &Rc<TapDragLog>, name: &'static str| {
        let log = log.clone();
        move || log.events.borrow_mut().push(name.to_owned())
    };
    let (start, end, cancel) = (push(&log, "start"), push(&log, "end"), push(&log, "cancel"));
    let (down_log, up_log, update_log) = (log.clone(), log.clone(), log.clone());
    let recognizer = TapAndDragGestureRecognizer::new(rig.binding.arena().clone())
        .with_on_tap_down(move |_| {
            down_log.events.borrow_mut().push("down".to_owned());
            trip(&down_log.panic_down, "tap-and-drag down callback panic");
        })
        .with_on_tap_up(move |d| {
            up_log.events.borrow_mut().push("up".to_owned());
            up_log.counts.borrow_mut().push(d.consecutive_tap_count);
            trip(&up_log.panic_up, "tap-and-drag up callback panic");
        })
        .with_on_drag_start(move |_| start())
        .with_on_drag_update(move |_| {
            update_log.events.borrow_mut().push("update".to_owned());
            trip(
                &update_log.panic_update,
                "tap-and-drag update callback panic",
            );
        })
        .with_on_drag_end(move |_| end())
        .with_on_cancel(cancel);
    (recognizer, log)
}

fn click(rig: &Rig, x: f64, y: f64) {
    rig.down_with(1, x, y, PointerType::Mouse, 0.5);
    rig.frame();
    rig.up_with(1, x, y, PointerType::Mouse);
    rig.frame();
}

fn tap_wins_against_a_later_tap_recognizer() {
    let rig = Rig::new();
    let (tad, log) = tap_drag_on(&rig);
    rig.attach(&tad, None);
    let taps = Rc::new(Cell::new(0));
    let counted = taps.clone();
    let tap = TapGestureRecognizer::new(rig.binding.arena().clone())
        .with_on_tap(move |_| counted.set(counted.get() + 1));
    rig.attach(&tap, None);
    rig.down(1, 100.0, 100.0);
    rig.frame();
    rig.up(1, 100.0, 100.0);
    rig.frame();
    assert_eq!(*log.events.borrow(), ["down", "up"]);
    assert_eq!(taps.get(), 0);
}

fn drag_claims_the_arena_before_starting() {
    let rig = Rig::new();
    let (tad, log) = tap_drag_on(&rig);
    rig.attach(&tad, None);
    let pans = Rc::new(Cell::new(0));
    let counted = pans.clone();
    let pan = DragGestureRecognizer::new(rig.binding.arena().clone(), DragAxis::Free)
        .with_on_start(move |_| counted.set(counted.get() + 1));
    rig.attach(&pan, None);
    rig.down(1, 100.0, 100.0);
    rig.frame();
    rig.move_to(1, 200.0, 100.0);
    rig.move_to(1, 220.0, 100.0);
    rig.up(1, 220.0, 100.0);
    assert_eq!(
        *log.events.borrow(),
        ["down", "start", "update", "update", "end"]
    );
    assert_eq!(pans.get(), 0, "the competing pan lost the claimed contact");
}

fn consecutive_clicks_count_up_and_reset() {
    let rig = Rig::new();
    let (tad, log) = tap_drag_on(&rig);
    rig.attach(&tad, None);
    click(&rig, 100.0, 100.0);
    rig.advance(100);
    click(&rig, 102.0, 100.0);
    rig.advance(100);
    click(&rig, 101.0, 101.0);
    rig.advance(1000);
    click(&rig, 100.0, 100.0);
    rig.advance(100);
    click(&rig, 400.0, 100.0);
    assert_eq!(*log.counts.borrow(), [1, 2, 3, 1, 1]);
}

fn tap_up_panic_then_next_tap() {
    let rig = Rig::new();
    let (tad, log) = tap_drag_on(&rig);
    rig.attach(&tad, None);
    rig.down(1, 100.0, 100.0);
    rig.frame();
    log.panic_up.set(true);
    expect_panic("on_tap_up", || rig.up(1, 100.0, 100.0));
    rig.down(2, 100.0, 100.0);
    rig.frame();
    rig.up(2, 100.0, 100.0);
    assert_eq!(log.count("up"), 2);
}

fn drag_update_panic_then_drag_continues() {
    let rig = Rig::new();
    let (tad, log) = tap_drag_on(&rig);
    rig.attach(&tad, None);
    rig.down(1, 100.0, 100.0);
    rig.frame();
    log.panic_update.set(true);
    expect_panic("on_drag_update", || rig.move_to(1, 200.0, 100.0));
    rig.move_to(1, 220.0, 100.0);
    rig.up(1, 220.0, 100.0);
    assert_eq!((log.count("start"), log.count("end")), (1, 1));
    click(&rig, 100.0, 100.0);
    assert_eq!(log.count("up"), 1);
}

fn tap_down_panic_still_starts_and_ends_the_drag() {
    let rig = Rig::new();
    let (tad, log) = tap_drag_on(&rig);
    rig.attach(&tad, None);
    // A competing pan keeps the arena open, so the claim delivers tap-down,
    // drag-start and the first update as one transition.
    let pan = DragGestureRecognizer::new(rig.binding.arena().clone(), DragAxis::Free);
    rig.attach(&pan, None);
    rig.down(1, 100.0, 100.0);
    rig.frame();
    log.panic_down.set(true);
    expect_panic("on_tap_down", || rig.move_to(1, 200.0, 100.0));
    rig.move_to(1, 220.0, 100.0);
    rig.up(1, 220.0, 100.0);
    assert_eq!(
        *log.events.borrow(),
        ["down", "start", "update", "update", "end"]
    );
}

fn tap_down_panic_still_delivers_the_tap_up() {
    let rig = Rig::new();
    let (tad, log) = tap_drag_on(&rig);
    rig.attach(&tad, None);
    // A competing tap keeps the arena open until the sweep on up, which
    // delivers tap-down and tap-up together.
    let tap = TapGestureRecognizer::new(rig.binding.arena().clone());
    rig.attach(&tap, None);
    rig.down(1, 100.0, 100.0);
    rig.frame();
    log.panic_down.set(true);
    expect_panic("on_tap_down", || rig.up(1, 100.0, 100.0));
    assert_eq!(*log.events.borrow(), ["down", "up"]);
}

/// The same event with its position replaced by NaN, for every event after
/// the down.
fn nan_after_down(event: &PointerEvent) -> PointerEvent {
    let pointer = id(pointer_of(event).expect("event carries an id"));
    let nan = Offset::new(f64::NAN, f64::NAN);
    match event {
        PointerEvent::Move(_) => make_move_event_for_id(pointer, nan, PointerType::Touch),
        PointerEvent::Up(_) => make_up_event_for_id(pointer, nan, PointerType::Touch),
        _ => event.clone(),
    }
}

fn tap_drag_publishes_no_non_finite_global_position() {
    let rig = Rig::new();
    let globals: Rc<RefCell<Vec<Offset<f64>>>> = Rc::default();
    let (s, u, e, t) = (
        globals.clone(),
        globals.clone(),
        globals.clone(),
        globals.clone(),
    );
    let tad = TapAndDragGestureRecognizer::new(rig.binding.arena().clone())
        .with_on_drag_start(move |d| s.borrow_mut().push(d.global_position))
        .with_on_drag_update(move |d| u.borrow_mut().push(d.global_position))
        .with_on_drag_end(move |d| e.borrow_mut().push(d.global_position))
        .with_on_tap_up(move |d| t.borrow_mut().push(d.global_position));
    rig.attach_with_global(&tad, None, nan_after_down);
    rig.down(1, 100.0, 100.0);
    rig.frame();
    rig.move_to(1, 200.0, 100.0);
    rig.move_to(1, 220.0, 100.0);
    rig.up(1, 220.0, 100.0);
    rig.down(2, 100.0, 100.0);
    rig.frame();
    rig.up(2, 100.0, 100.0);
    let globals = globals.borrow();
    assert_eq!(globals.len(), 5, "start, two updates, end, tap up");
    assert!(globals.iter().all(|g| g.is_finite()), "{globals:?}");
}

fn cancel_mid_drag_cancels_once() {
    let rig = Rig::new();
    let (tad, log) = tap_drag_on(&rig);
    rig.attach(&tad, None);
    rig.down(1, 100.0, 100.0);
    rig.frame();
    rig.move_to(1, 200.0, 100.0);
    rig.cancel(1);
    assert_eq!((log.count("cancel"), log.count("end")), (1, 0));
    click(&rig, 100.0, 100.0);
    assert_eq!(log.count("up"), 1);
}

fn tap_drag_disposed_from_its_drag_start() {
    let rig = Rig::new();
    let slot: Rc<RefCell<Option<Arc<TapAndDragGestureRecognizer>>>> = Rc::default();
    let live_slot: Rc<RefCell<Option<Rc<Cell<bool>>>>> = Rc::default();
    let later = Rc::new(Cell::new(0));
    let (s, l, n, m) = (
        slot.clone(),
        live_slot.clone(),
        later.clone(),
        later.clone(),
    );
    let recognizer = TapAndDragGestureRecognizer::new(rig.binding.arena().clone())
        .with_on_drag_start(move |_| {
            if let Some(live) = l.borrow().as_ref() {
                live.set(false);
            }
            if let Some(recognizer) = s.borrow_mut().take() {
                recognizer.dispose();
            }
        })
        .with_on_drag_update(move |_| n.set(n.get() + 1))
        .with_on_drag_end(move |_| m.set(m.get() + 1));
    *live_slot.borrow_mut() = Some(rig.attach(&recognizer, None));
    *slot.borrow_mut() = Some(recognizer);
    rig.down(1, 100.0, 100.0);
    rig.frame();
    rig.move_to(1, 200.0, 100.0);
    rig.up(1, 200.0, 100.0);
    assert_eq!(later.get(), 0, "a disposed recogniser reports nothing more");
}

#[test]
fn tap_and_drag_resolves_through_the_shared_arena() {
    run_rows(
        "tap and drag",
        &[
            ("tap against a tap", tap_wins_against_a_later_tap_recognizer),
            ("drag against a pan", drag_claims_the_arena_before_starting),
            ("consecutive clicks", consecutive_clicks_count_up_and_reset),
            ("tap up panic", tap_up_panic_then_next_tap),
            ("drag update panic", drag_update_panic_then_drag_continues),
            (
                "tap down panic in a drag",
                tap_down_panic_still_starts_and_ends_the_drag,
            ),
            (
                "tap down panic in a tap",
                tap_down_panic_still_delivers_the_tap_up,
            ),
            (
                "non-finite global position",
                tap_drag_publishes_no_non_finite_global_position,
            ),
            ("cancel mid drag", cancel_mid_drag_cancels_once),
            (
                "dispose from drag start",
                tap_drag_disposed_from_its_drag_start,
            ),
        ],
    );
}

// ============================================================================
// Eager
// ============================================================================

#[test]
fn eager_wins_at_close_and_forgets_a_finished_contact() {
    let rig = Rig::new();
    let eager = EagerGestureRecognizer::new(rig.binding.arena().clone());
    rig.attach(&eager, None);
    let taps = Rc::new(Cell::new(0));
    let counted = taps.clone();
    let tap = TapGestureRecognizer::new(rig.binding.arena().clone())
        .with_on_tap(move |_| counted.set(counted.get() + 1));
    rig.attach(&tap, None);
    for pointer in [1, 2] {
        rig.down(pointer, 100.0, 100.0);
        assert_eq!(eager.primary_pointer(), Some(id(pointer)));
        rig.frame();
        rig.up(pointer, 100.0, 100.0);
        rig.frame();
        assert_eq!(eager.primary_pointer(), None, "the lifted contact is over");
    }
    rig.down(3, 100.0, 100.0);
    rig.cancel(3);
    assert_eq!(
        eager.primary_pointer(),
        None,
        "the cancelled contact is over"
    );
    assert_eq!(taps.get(), 0, "the eager member won every contact");
}
