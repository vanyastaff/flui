//! One Euro smoothing after recognition: positions published after acceptance
//! are smoothed, decisions before it read the raw samples, and every accepted
//! gesture starts from an unsmoothed position.

use flui_interaction::gesture::GestureContest;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use flui_foundation::geometry::Offset;
use flui_interaction::{
    DragAxis, DragGestureRecognizer, GestureArena, GestureArenaMember, GestureRecognizer,
    GestureSettings, ManualClock, PointerId,
    events::{
        PointerEvent, PointerKind, PointerMove, make_down_event_for_id, make_move_event_for_id,
        make_up_event_for_id,
    },
    gesture::{OneEuroTuning, ScaleGestureRecognizer, Velocity},
    pointer::PointerDispatch,
};
use flui_platform::EventTime;

/// Sampling period of the traces: a 125 Hz digitizer.
const PERIOD_NANOS: u64 = 8_000_000;

struct Rival;
impl GestureArenaMember for Rival {
    fn accept_gesture(&self, _: GestureContest) {}
    fn reject_gesture(&self, _: GestureContest) {}
}

fn pointer(raw: u64) -> PointerId {
    PointerId::try_from(raw).expect("nonzero contact")
}

/// Strong fixed-cutoff smoothing, so a leak into decisions would show.
fn tuning() -> OneEuroTuning {
    OneEuroTuning::try_new(0.5, 0.0, 1.0).expect("valid tuning")
}

fn settings(smoothing: Option<OneEuroTuning>) -> GestureSettings {
    GestureSettings::default().with_position_smoothing(smoothing)
}

/// Stamp `event` with a hardware time `step` sampling periods into the trace.
fn at(mut event: PointerEvent, step: usize) -> PointerEvent {
    let time = EventTime::from_nanos(step as u64 * PERIOD_NANOS);
    match &mut event {
        PointerEvent::Down(data) => data.sample.time = time,
        PointerEvent::Up(data) => data.sample.time = time,
        PointerEvent::Move(data) => {
            let mut sample = *data.current();
            sample.time = time;
            *data = PointerMove::new(data.pointer, data.buttons, sample);
        }
        _ => {}
    }
    event
}

/// Peak-to-peak spread of `values`.
fn spread(values: impl Iterator<Item = f64>) -> f64 {
    let (low, high) = values.fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), value| {
        (low.min(value), high.max(value))
    });
    high - low
}

/// What one drag published.
#[derive(Debug, Clone, Default, PartialEq)]
struct DragLog {
    /// Trace step whose sample the arena accepted on, and the start position.
    start: Option<(usize, Offset<f64>)>,
    /// Update positions and deltas, in order.
    updates: Vec<(Offset<f64>, Offset<f64>)>,
    /// Raw release velocity.
    velocity: Option<Velocity>,
}

struct DragRig {
    arena: GestureArena,
    drag: Rc<DragGestureRecognizer>,
    step: Rc<Cell<usize>>,
    log: Rc<RefCell<DragLog>>,
}

impl DragRig {
    fn new(smoothing: Option<OneEuroTuning>) -> Self {
        let arena = GestureArena::binding_driven(Arc::new(ManualClock::new()));
        let step = Rc::new(Cell::new(0));
        let log: Rc<RefCell<DragLog>> = Rc::default();
        let (start_step, start_log, update_log, end_log) = (
            Rc::clone(&step),
            Rc::clone(&log),
            Rc::clone(&log),
            Rc::clone(&log),
        );
        let drag = DragGestureRecognizer::builder(arena.clone(), DragAxis::Free)
            .settings(settings(smoothing))
            .on_start(move |details| {
                start_log.borrow_mut().start = Some((start_step.get(), details.local_position));
            })
            .on_update(move |details| {
                update_log
                    .borrow_mut()
                    .updates
                    .push((details.local_position, details.delta));
            })
            .on_end(move |details| end_log.borrow_mut().velocity = Some(details.velocity))
            .build();
        Self {
            arena,
            drag,
            step,
            log,
        }
    }

    /// One contact over `trace` against a rival: `trace[0]` is the Down, the
    /// rest are moves, and the contact lifts at the last position.
    fn run(&self, raw: u64, trace: &[Offset<f64>]) -> DragLog {
        *self.log.borrow_mut() = DragLog::default();
        let id = pointer(raw);
        let rival = Rc::new(Rival);
        let send = |step: usize, event: PointerEvent| {
            self.step.set(step);
            let event = at(event, step);
            let dispatch = PointerDispatch::at_root(&event);
            match event {
                PointerEvent::Down(_) => {
                    self.drag.add_pointer(dispatch);
                    self.arena.add(id, &rival);
                    self.arena.close(id);
                }
                PointerEvent::Up(_) => {
                    self.drag.handle_event(dispatch);
                    self.arena.sweep(id);
                }
                _ => self.drag.handle_event(dispatch),
            }
            self.arena.drain_deferred_resolutions();
        };
        let (first, moves) = trace.split_first().expect("a trace has a Down");
        send(
            0,
            make_down_event_for_id(id, *first, PointerKind::Touch).expect("finite fixture"),
        );
        for (step, position) in (1..).zip(moves) {
            send(
                step,
                make_move_event_for_id(id, *position, PointerKind::Touch).expect("finite fixture"),
            );
        }
        let last = *trace.last().expect("nonempty trace");
        send(
            trace.len(),
            make_up_event_for_id(id, last, PointerKind::Touch).expect("finite fixture"),
        );
        self.log.borrow().clone()
    }
}

/// A finger creeping right from `origin` with ±`amplitude` vertical jitter.
fn jittery_approach(origin: Offset<f64>, amplitude: f64, samples: u32) -> Vec<Offset<f64>> {
    (0..samples)
        .map(|step| {
            let sign = if step % 2 == 0 { 1.0 } else { -1.0 };
            origin + Offset::new(f64::from(step) * 1.5, sign * amplitude)
        })
        .collect()
}

/// A jittery approach, then ±4 px vertical jitter about where it stopped,
/// then the finger at rest there for 2.4 s.
fn approach_jitter_rest() -> (Vec<Offset<f64>>, usize, Offset<f64>) {
    let mut trace = jittery_approach(Offset::ZERO, 1.0, 40);
    let rest = Offset::new(60.0, 0.0);
    let jitter_from = trace.len();
    trace.extend(
        (0..60).map(|step| rest + Offset::new(0.0, if step % 2 == 0 { 4.0 } else { -4.0 })),
    );
    trace.extend(std::iter::repeat_n(rest, 300));
    (trace, jitter_from, rest)
}

/// The arena accepts on the same raw sample, at the same position, and the
/// release velocity is the same, whether or not smoothing is selected.
fn smoothing_never_moves_the_slop_crossing() {
    let trace = jittery_approach(Offset::ZERO, 6.0, 40);
    let raw = DragRig::new(None).run(7, &trace);
    let smoothed = DragRig::new(Some(tuning())).run(7, &trace);
    assert!(raw.start.is_some(), "the trace crosses the slop");
    assert_eq!(smoothed.start, raw.start, "acceptance sample and position");
    assert_eq!(
        smoothed.velocity, raw.velocity,
        "velocity reads raw samples"
    );
}

/// After acceptance the published position carries a fraction of the sensor
/// jitter, its deltas add up to it, and at rest it converges to the raw position.
fn smoothing_reduces_jitter_and_converges_at_rest() {
    let (trace, jitter_from, rest) = approach_jitter_rest();
    let jitter = |log: &DragLog| {
        // The updates for the jitter samples, after 20 to settle.
        let skip = log.updates.len() - (trace.len() - jitter_from) + 20;
        spread(
            log.updates
                .iter()
                .skip(skip)
                .take(40)
                .map(|(position, _)| position.dy),
        )
    };
    let raw = DragRig::new(None).run(7, &trace);
    let smoothed = DragRig::new(Some(tuning())).run(7, &trace);
    assert_eq!(jitter(&raw), 8.0, "raw updates carry the full jitter");
    assert!(
        jitter(&smoothed) < 1.0,
        "smoothed jitter {} of 8 px",
        jitter(&smoothed)
    );
    let (start, last) = (
        smoothed.start.expect("accepted").1,
        smoothed.updates.last().expect("updates").0,
    );
    let travelled = smoothed
        .updates
        .iter()
        .fold(Offset::ZERO, |sum, (_, delta)| sum + *delta);
    assert!(
        ((start + travelled) - last).distance() < 1e-9,
        "deltas add up to the published position"
    );
    assert!(
        (last - rest).distance() < 0.01,
        "at rest the smoothed position {last:?} converges to {rest:?}"
    );
}

/// A second contact on the same recognizer publishes exactly what a fresh
/// recognizer publishes for it: no history leaks from the previous gesture.
fn a_new_sequence_starts_unsmoothed() {
    let (first, _, _) = approach_jitter_rest();
    let second = jittery_approach(Offset::new(300.0, 300.0), 4.0, 60);
    let reused = DragRig::new(Some(tuning()));
    reused.run(7, &first);
    let after = reused.run(8, &second);
    let fresh = DragRig::new(Some(tuning())).run(8, &second);
    assert_eq!(after, fresh);
    let (step, position) = after.start.expect("accepted");
    assert_eq!(
        position, second[step],
        "the start position is the raw sample"
    );
}

/// Published focal points, tagged with the frame that published them.
type FramedFocal = Vec<(usize, Offset<f64>)>;

/// Two fingers panning together with the same vertical jitter, moved at one
/// timestamp per frame. Returns the start step and the published focal points.
fn scale_trace(smoothing: Option<OneEuroTuning>) -> (Option<usize>, Vec<Offset<f64>>) {
    let arena = GestureArena::binding_driven(Arc::new(ManualClock::new()));
    let step = Rc::new(Cell::new(0));
    let start = Rc::new(Cell::new(None));
    // The focal point each frame last published.
    let focal: Rc<RefCell<FramedFocal>> = Rc::default();
    let (start_step, update_step, started, published) = (
        Rc::clone(&step),
        Rc::clone(&step),
        Rc::clone(&start),
        Rc::clone(&focal),
    );
    let scale = ScaleGestureRecognizer::builder(arena.clone())
        .settings(settings(smoothing))
        .on_start(move |_| started.set(Some(start_step.get())))
        .on_update(move |details| {
            let mut published = published.borrow_mut();
            let frame = update_step.get();
            if published.last().is_some_and(|(last, _)| *last == frame) {
                published.pop();
            }
            published.push((frame, details.local_focal_point));
        })
        .build();
    let rival = Rc::new(Rival);
    let fingers = [(pointer(1), 0.0), (pointer(2), 100.0)];
    for (id, x) in fingers {
        let down = at(
            make_down_event_for_id(id, Offset::new(x, 0.0), PointerKind::Touch)
                .expect("finite fixture"),
            0,
        );
        scale.add_pointer(PointerDispatch::at_root(&down));
        arena.add(id, &rival);
        arena.close(id);
        arena.drain_deferred_resolutions();
    }
    for frame in 1..400 {
        step.set(frame);
        let travel = (frame.min(40) as f64) * 1.5;
        let jitter = if frame % 2 == 0 { 4.0 } else { -4.0 };
        for (id, x) in fingers {
            let event = at(
                make_move_event_for_id(id, Offset::new(x + travel, jitter), PointerKind::Touch)
                    .expect("finite fixture"),
                frame,
            );
            scale.handle_event(PointerDispatch::at_root(&event));
            arena.drain_deferred_resolutions();
        }
    }
    let focal = focal.borrow().iter().map(|(_, focal)| *focal).collect();
    (start.get(), focal)
}

/// Scale accepts on the same frame with or without smoothing; afterwards its
/// focal point carries a fraction of the jitter, each frame's last contact
/// sample replacing the earlier one at the same timestamp.
fn scale_smooths_the_focal_point_after_acceptance() {
    let (raw_start, raw) = scale_trace(None);
    let (smoothed_start, smoothed) = scale_trace(Some(tuning()));
    assert!(raw_start.is_some(), "the fingers cross the pan slop");
    assert_eq!(smoothed_start, raw_start);
    let tail = |focal: &[Offset<f64>]| spread(focal[focal.len() - 60..].iter().map(|f| f.dy));
    assert_eq!(tail(&raw), 8.0, "raw focal carries the full jitter");
    assert!(
        tail(&smoothed) < 1.0,
        "smoothed focal jitter {}",
        tail(&smoothed)
    );
    // A cutoff too high to smooth publishes each frame's raw focal point:
    // the second contact's sample at a timestamp replaces the first's.
    let transparent = OneEuroTuning::try_new(f64::MAX, 0.0, 1.0).expect("valid tuning");
    let transparent = scale_trace(Some(transparent)).1;
    let differs = transparent
        .iter()
        .zip(&raw)
        .position(|(left, right)| left != right);
    assert_eq!(differs, None, "first frame not published raw");
    assert_eq!(transparent.len(), raw.len());
    let (raw_last, smoothed_last) = (raw[raw.len() - 1], smoothed[smoothed.len() - 1]);
    assert!(
        (smoothed_last.dx - raw_last.dx).abs() < 0.01,
        "the completed pan converges: {smoothed_last:?} against {raw_last:?}"
    );
}

/// Each frame's published `(frame, focal point, focal delta)`.
type FramedFocalDelta = Vec<(usize, Offset<f64>, Offset<f64>)>;

/// Three fingers on one scale: two pan it past the slop and rest, a third
/// lands mid-gesture, and the frames after it publish the new centroid.
/// Returns each frame's last `(focal point, focal delta)`.
fn scale_rebaseline_trace(smoothing: Option<OneEuroTuning>) -> FramedFocalDelta {
    let arena = GestureArena::binding_driven(Arc::new(ManualClock::new()));
    let step = Rc::new(Cell::new(0));
    let log: Rc<RefCell<FramedFocalDelta>> = Rc::default();
    let (frame, published) = (Rc::clone(&step), Rc::clone(&log));
    let scale = ScaleGestureRecognizer::builder(arena.clone())
        .settings(settings(smoothing))
        .on_update(move |details| {
            let mut published = published.borrow_mut();
            let frame = frame.get();
            if published.last().is_some_and(|(last, ..)| *last == frame) {
                published.pop();
            }
            published.push((frame, details.local_focal_point, details.focal_point_delta));
        })
        .build();
    let rival = Rc::new(Rival);
    let land = |id: PointerId, position: Offset<f64>, frame: usize| {
        let down = at(
            make_down_event_for_id(id, position, PointerKind::Touch).expect("finite fixture"),
            frame,
        );
        scale.add_pointer(PointerDispatch::at_root(&down));
        arena.add(id, &rival);
        arena.close(id);
        arena.drain_deferred_resolutions();
    };
    land(pointer(1), Offset::ZERO, 0);
    land(pointer(2), Offset::new(100.0, 0.0), 0);
    for frame in 1..160 {
        step.set(frame);
        if frame == 100 {
            land(pointer(3), Offset::new(110.0, 300.0), frame);
        }
        let travel = (frame.min(40) as f64) * 1.5;
        let mut fingers = vec![(pointer(1), Offset::new(travel, 0.0))];
        fingers.push((pointer(2), Offset::new(100.0 + travel, 0.0)));
        if frame > 100 {
            fingers.push((pointer(3), Offset::new(110.0, 300.0)));
        }
        for (id, position) in fingers {
            let event = at(
                make_move_event_for_id(id, position, PointerKind::Touch).expect("finite fixture"),
                frame,
            );
            scale.handle_event(PointerDispatch::at_root(&event));
            arena.drain_deferred_resolutions();
        }
    }
    log.borrow().clone()
}

/// A contact joining mid-gesture re-baselines the focal point without a
/// published jump; smoothing restarts there, so the first frames after the
/// change publish exactly the raw centroid and delta instead of gliding to it.
fn scale_restarts_smoothing_when_the_contact_set_changes() {
    let raw = scale_rebaseline_trace(None);
    let smoothed = scale_rebaseline_trace(Some(tuning()));
    let at_frame = |log: &[(usize, Offset<f64>, Offset<f64>)], frame: usize| {
        log.iter()
            .find(|(published, ..)| *published == frame)
            .copied()
            .expect("the scale publishes every frame")
    };
    let joined = at_frame(&raw, 101);
    assert!(
        (joined.1 - Offset::new(110.0, 100.0)).distance() < 1e-9,
        "the third finger moved the raw centroid: {joined:?}"
    );
    assert_eq!(at_frame(&smoothed, 101), joined);
}

/// One contact, held near its origin or dragged, for the recognizers that
/// publish a position after acceptance.
#[derive(Debug, Clone, Copy)]
enum Contact {
    MultiDrag,
    TapAndDrag,
    LongPress,
    ForcePress,
}

/// Steps the recognizer accepted on and the local positions it published
/// after acceptance, tagged with their trace step.
#[derive(Debug, Default)]
struct ContactLog {
    start: Option<usize>,
    published: Vec<(usize, Offset<f64>)>,
}

/// Stamp `event` at `step` with a sensor `pressure`.
fn pressed(mut event: PointerEvent, step: usize, pressure: f32) -> PointerEvent {
    let pressure = flui_platform::pointer::Pressure::try_new(pressure).expect("unit pressure");
    match &mut event {
        PointerEvent::Down(data) => data.sample.pressure = Some(pressure),
        PointerEvent::Up(data) => data.sample.pressure = Some(pressure),
        PointerEvent::Move(data) => {
            let sample = data.current().with_pressure(pressure);
            *data = PointerMove::new(data.pointer, data.buttons, sample);
        }
        _ => {}
    }
    at(event, step)
}

struct LoggedHandle(Rc<Cell<usize>>, Rc<RefCell<ContactLog>>);
impl flui_interaction::gesture::MultiDragHandle for LoggedHandle {
    fn update(&self, details: flui_interaction::gesture::MultiDragUpdateDetails) {
        self.1
            .borrow_mut()
            .published
            .push((self.0.get(), details.local_position));
    }
    fn end(&self, _: flui_interaction::gesture::MultiDragEndDetails) {}
    fn cancel(&self) {}
}

fn contact_recognizer(
    contact: Contact,
    arena: &GestureArena,
    smoothing: Option<OneEuroTuning>,
    step: &Rc<Cell<usize>>,
    log: &Rc<RefCell<ContactLog>>,
) -> Rc<dyn GestureRecognizer> {
    use flui_interaction::gesture::{
        ForcePressGestureRecognizer, LongPressGestureRecognizer, MultiDragAxis,
        MultiDragGestureRecognizer, MultiDragHandle, TapAndDragGestureRecognizer,
    };
    let (started, start_log) = (Rc::clone(step), Rc::clone(log));
    let start = move || start_log.borrow_mut().start = Some(started.get());
    let (updated, update_log) = (Rc::clone(step), Rc::clone(log));
    let update = move |position: Offset<f64>| {
        update_log
            .borrow_mut()
            .published
            .push((updated.get(), position));
    };
    let settings = settings(smoothing);
    match contact {
        Contact::MultiDrag => {
            let (step, log) = (Rc::clone(step), Rc::clone(log));
            MultiDragGestureRecognizer::builder(arena.clone(), MultiDragAxis::Free)
                .settings(settings)
                .on_start(move |_, _| {
                    start();
                    Some(Rc::new(LoggedHandle(step.clone(), log.clone())) as Rc<dyn MultiDragHandle>)
                })
                .build()
        }
        Contact::TapAndDrag => TapAndDragGestureRecognizer::builder(arena.clone())
            .settings(settings)
            .on_drag_start(move |_| start())
            .on_drag_update(move |details| update(details.local_position))
            .build(),
        Contact::LongPress => LongPressGestureRecognizer::builder(arena.clone())
            .settings(settings)
            .on_long_press_start(move |_| start())
            .on_long_press_move_update(move |details| update(details.local_position))
            .build(),
        Contact::ForcePress => ForcePressGestureRecognizer::builder(arena.clone())
            .settings(settings)
            .on_start(move |_| start())
            .on_update(move |details| update(details.local_position))
            .build(),
    }
}

/// One pressed contact over `trace` against a rival, the clock advancing one
/// sampling period per step.
fn contact_trace(
    contact: Contact,
    smoothing: Option<OneEuroTuning>,
    trace: &[Offset<f64>],
) -> ContactLog {
    let clock = ManualClock::new();
    let arena = GestureArena::binding_driven(Arc::new(clock.clone()));
    let step = Rc::new(Cell::new(0));
    let log: Rc<RefCell<ContactLog>> = Rc::default();
    let recognizer = contact_recognizer(contact, &arena, smoothing, &step, &log);
    let id = pointer(9);
    let rival = Rc::new(Rival);
    let (first, moves) = trace.split_first().expect("a trace has a Down");
    let down = pressed(
        make_down_event_for_id(id, *first, PointerKind::Touch).expect("finite fixture"),
        0,
        0.6,
    );
    recognizer.add_pointer(PointerDispatch::at_root(&down));
    arena.add(id, &rival);
    arena.close(id);
    arena.drain_deferred_resolutions();
    for (index, position) in (1..).zip(moves) {
        step.set(index);
        clock.advance(std::time::Duration::from_nanos(PERIOD_NANOS));
        let event = pressed(
            make_move_event_for_id(id, *position, PointerKind::Touch).expect("finite fixture"),
            index,
            0.6,
        );
        recognizer.handle_event(PointerDispatch::at_root(&event));
        arena.drain_deferred_resolutions();
    }
    let up = at(
        make_up_event_for_id(id, *trace.last().expect("nonempty"), PointerKind::Touch)
            .expect("finite fixture"),
        trace.len(),
    );
    recognizer.handle_event(PointerDispatch::at_root(&up));
    arena.sweep(id);
    arena.drain_deferred_resolutions();
    std::mem::take(&mut *log.borrow_mut())
}

/// A held contact: 100 samples of ±1 px tremor (a long press starts in
/// them), 100 of ±4 px jitter, then 2.4 s at rest at the origin.
fn hold_jitter_rest() -> (Vec<Offset<f64>>, usize) {
    let tremor = |amplitude: f64| {
        move |step: usize| {
            Offset::new(
                0.0,
                if step.is_multiple_of(2) {
                    amplitude
                } else {
                    -amplitude
                },
            )
        }
    };
    let mut trace: Vec<_> = (0..100).map(tremor(1.0)).collect();
    let jitter_from = trace.len();
    trace.extend((0..100).map(tremor(4.0)));
    trace.extend(std::iter::repeat_n(Offset::ZERO, 300));
    (trace, jitter_from)
}

/// A recognizer that publishes a position after acceptance accepts on the
/// same step with or without smoothing, its published position then carries
/// a fraction of the jitter, and at rest it converges to the raw position.
fn contact_smooths_after_acceptance(contact: Contact) {
    let (trace, jitter_from) = match contact {
        Contact::MultiDrag | Contact::TapAndDrag => {
            let (trace, jitter_from, _) = approach_jitter_rest();
            (trace, jitter_from)
        }
        Contact::LongPress | Contact::ForcePress => hold_jitter_rest(),
    };
    let raw = contact_trace(contact, None, &trace);
    let smoothed = contact_trace(contact, Some(tuning()), &trace);
    assert!(raw.start.is_some(), "{contact:?} accepts the trace");
    assert!(
        raw.start < Some(jitter_from),
        "{contact:?} starts before the jitter"
    );
    assert_eq!(smoothed.start, raw.start, "{contact:?} acceptance step");
    let jitter = |log: &ContactLog| {
        spread(
            log.published
                .iter()
                .filter(|(step, _)| (jitter_from + 20..jitter_from + 60).contains(step))
                .map(|(_, position)| position.dy),
        )
    };
    assert_eq!(jitter(&raw), 8.0, "{contact:?} raw jitter");
    assert!(
        jitter(&smoothed) < 1.0,
        "{contact:?} smoothed jitter {}",
        jitter(&smoothed)
    );
    let rest = *trace.last().expect("nonempty");
    let last = smoothed.published.last().expect("published").1;
    assert!(
        (last - rest).distance() < 0.01,
        "{contact:?} converges: {last:?} against {rest:?}"
    );
}

#[test]
fn position_smoothing_after_recognition() {
    for (name, case) in [
        (
            "smoothing_never_moves_the_slop_crossing",
            smoothing_never_moves_the_slop_crossing as fn(),
        ),
        (
            "smoothing_reduces_jitter_and_converges_at_rest",
            smoothing_reduces_jitter_and_converges_at_rest,
        ),
        (
            "a_new_sequence_starts_unsmoothed",
            a_new_sequence_starts_unsmoothed,
        ),
        (
            "scale_smooths_the_focal_point_after_acceptance",
            scale_smooths_the_focal_point_after_acceptance,
        ),
        (
            "scale_restarts_smoothing_when_the_contact_set_changes",
            scale_restarts_smoothing_when_the_contact_set_changes,
        ),
        ("multidrag_smooths_after_acceptance", || {
            contact_smooths_after_acceptance(Contact::MultiDrag);
        }),
        ("tap_and_drag_smooths_after_acceptance", || {
            contact_smooths_after_acceptance(Contact::TapAndDrag);
        }),
        ("long_press_move_smooths_after_acceptance", || {
            contact_smooths_after_acceptance(Contact::LongPress);
        }),
        ("force_press_smooths_after_acceptance", || {
            contact_smooths_after_acceptance(Contact::ForcePress);
        }),
    ] {
        if let Err(payload) = std::panic::catch_unwind(case) {
            eprintln!("smoothing contract {name} failed");
            std::panic::resume_unwind(payload);
        }
    }
}
