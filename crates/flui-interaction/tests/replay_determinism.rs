//! One timeline (ADR-0207 §2, §3): every arena deadline is an `EventTime` read
//! from the interaction clock, so a recorded stream of input and frames, with
//! its arrival times, replays to the same arena decisions — as long as the
//! application state the input lands on is the same.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_foundation::geometry::Point;
use flui_interaction::events::{
    PointerButton, PointerButtons, PointerEvent, PointerInfo, PointerKind, PointerMove,
    PointerPosition, PointerPress, PointerRelease, PointerRole, PointerSample,
};
use flui_interaction::gesture::GestureSettings;
use flui_interaction::pointer::PointerDispatch;
use flui_interaction::{
    DragAxis, DragGestureRecognizer, GestureBinding, GestureRecognizer, HitTestResult,
    LongPressGestureRecognizer, ManualClock, PointerId, PointerRouteHandler, TapGestureRecognizer,
};
use flui_platform_api::EventTime;

/// One recorded host input: when it arrived, and what it was.
#[derive(Clone)]
enum Recorded {
    Pointer(PointerEvent),
    /// A frame whose vsync is its arrival time.
    Frame,
}

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

fn at(time: Duration) -> EventTime {
    EventTime::from_nanos(u64::try_from(time.as_nanos()).expect("test time fits"))
}

fn sample(time: Duration, x: f64, y: f64) -> PointerSample {
    PointerSample::new(
        at(time),
        PointerPosition::try_new(Point::new(x, y)).expect("finite fixture position"),
    )
}

fn info(id: u64) -> PointerInfo {
    PointerInfo::new(
        PointerId::try_from(id).expect("nonzero pointer"),
        PointerKind::Touch,
    )
    .with_role(PointerRole::Primary)
}

fn down(time: Duration, id: u64, x: f64, y: f64) -> (Duration, Recorded) {
    let press = PointerPress::new(
        info(id),
        PointerButton::PRIMARY,
        PointerButtons::NONE,
        sample(time, x, y),
    );
    (time, Recorded::Pointer(PointerEvent::Down(press)))
}

fn moved(time: Duration, id: u64, x: f64, y: f64) -> (Duration, Recorded) {
    let motion = PointerMove::new(
        info(id),
        PointerButtons::only(PointerButton::PRIMARY),
        sample(time, x, y),
    );
    (time, Recorded::Pointer(PointerEvent::Move(motion)))
}

fn up(time: Duration, id: u64, x: f64, y: f64) -> (Duration, Recorded) {
    let release = PointerRelease::new(
        info(id),
        PointerButton::PRIMARY,
        PointerButtons::only(PointerButton::PRIMARY),
        sample(time, x, y),
    );
    (time, Recorded::Pointer(PointerEvent::Up(release)))
}

fn frames(from: u64, to: u64) -> impl Iterator<Item = (Duration, Recorded)> {
    (from..=to)
        .step_by(16)
        .map(|time| (ms(time), Recorded::Frame))
}

/// A held press that becomes a long press, a drag, and a tap, with frames
/// throughout; motion is resampled, so where each drag sample lands depends
/// on the frames' timeline too.
fn recording() -> Vec<(Duration, Recorded)> {
    let mut entries = vec![
        down(ms(0), 1, 10.0, 10.0),
        moved(ms(40), 1, 11.0, 10.0),
        up(ms(620), 1, 11.0, 10.0),
        down(ms(700), 2, 100.0, 100.0),
        moved(ms(716), 2, 112.0, 100.0),
        moved(ms(732), 2, 130.0, 100.0),
        moved(ms(748), 2, 150.0, 100.0),
        up(ms(800), 2, 150.0, 100.0),
        down(ms(900), 3, 50.0, 50.0),
        up(ms(950), 3, 50.0, 50.0),
    ];
    entries.extend(frames(0, 1000));
    // Stable: a frame and an input at one instant keep input first.
    entries.sort_by_key(|(time, entry)| (*time, matches!(entry, Recorded::Frame)));
    entries
}

/// Replay `recording` against a fresh binding on a virtual clock driven to
/// each entry's arrival, and return the arena's decisions with their times.
///
/// `unmount_drag_at`, when set, is application state the recording does not
/// hold: from that time on the drag detector is gone, as a rebuild driven by
/// a worker result would remove it.
fn replay(recording: &[(Duration, Recorded)], unmount_drag_at: Option<Duration>) -> Vec<String> {
    let clock = ManualClock::new();
    let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
    binding
        .set_resampling_enabled(true)
        .expect("no contact is active yet");
    let log = Rc::new(RefCell::new(Vec::new()));
    let arena = binding.arena().clone();
    let now = {
        let arena = arena.clone();
        move || arena.now().as_nanos() / 1_000_000
    };
    let entry = |name: &'static str| {
        let log = Rc::clone(&log);
        let now = now.clone();
        move || log.borrow_mut().push(format!("{name}@{}ms", now()))
    };
    let settings = GestureSettings::default().with_long_press_timeout(ms(500));
    let long_press = {
        let fired = entry("long_press");
        LongPressGestureRecognizer::builder(arena.clone())
            .settings(settings.clone())
            .on_long_press_start(move |_| fired())
            .build()
    };
    let drag = {
        let started = entry("drag_start");
        let ended = entry("drag_end");
        DragGestureRecognizer::builder(arena.clone(), DragAxis::Free)
            .settings(settings.clone())
            .on_start(move |_| started())
            .on_end(move |_| ended())
            .build()
    };
    let tap = {
        let tapped = entry("tap");
        TapGestureRecognizer::builder(arena)
            .settings(settings)
            .on_tap(move |_| tapped())
            .build()
    };
    let mounted: Rc<RefCell<Vec<Rc<dyn GestureRecognizer>>>> =
        Rc::new(RefCell::new(vec![tap, long_press, drag.clone()]));
    let routed = Rc::clone(&mounted);
    let route: PointerRouteHandler = Rc::new(move |event| {
        let dispatch = PointerDispatch::at_root(event);
        let recognizers = routed.borrow().clone();
        for recognizer in &recognizers {
            if matches!(event, PointerEvent::Down(_)) {
                recognizer.add_pointer(dispatch);
            } else {
                recognizer.handle_event(dispatch);
            }
        }
    });
    binding.pointer_router().add_global_handler(route);

    let mut elapsed = Duration::ZERO;
    for (arrival, input) in recording {
        clock.advance(arrival.saturating_sub(elapsed));
        elapsed = *arrival;
        if unmount_drag_at.is_some_and(|unmount| *arrival >= unmount) && mounted.borrow().len() == 3
        {
            let drag: Rc<dyn GestureRecognizer> = drag.clone();
            mounted
                .borrow_mut()
                .retain(|mounted| !Rc::ptr_eq(mounted, &drag));
            let _ = drag.cancel();
        }
        match input {
            Recorded::Pointer(event) => {
                let _ = binding.dispatch_pointer_on(
                    flui_interaction::InputSurface::Window,
                    event,
                    |_| HitTestResult::new(),
                );
            }
            Recorded::Frame => {
                // The frame tick: resampled motion is flushed on the binding's
                // clock, then due deadlines fire on the event timeline.
                let _ = binding.flush_pending_moves();
                binding.arena().poll_deadlines();
                binding.arena().drain_deferred_resolutions();
            }
        }
    }
    log.take()
}

#[test]
fn replaying_a_recording_twice_gives_identical_arena_decisions() {
    let recording = recording();
    let first = replay(&recording, None);
    assert_eq!(
        first,
        // The 500 ms press fires at the first frame past it (512 ms); the drag
        // crosses its 18 px slop with the 732 ms sample, flushed at 736 ms.
        [
            "long_press@512ms",
            "drag_start@736ms",
            "drag_end@800ms",
            "tap@950ms"
        ],
        "the recording's decisions"
    );
    assert_eq!(
        replay(&recording, None),
        first,
        "a second replay decided differently"
    );
}

#[test]
fn deadlines_fire_on_event_time_without_wall_clock_reads() {
    let clock = ManualClock::new();
    let binding = GestureBinding::with_clock(Arc::new(clock.clone()));
    let fired = Rc::new(RefCell::new(None));
    let seen = Rc::clone(&fired);
    let arena = binding.arena().clone();
    let reader = arena.clone();
    let long_press = LongPressGestureRecognizer::builder(arena.clone())
        .settings(GestureSettings::default().with_long_press_timeout(ms(500)))
        .on_long_press_start(move |_| *seen.borrow_mut() = Some(reader.now()))
        .build();
    let route: PointerRouteHandler = Rc::new(move |event| {
        if matches!(event, PointerEvent::Down(_)) {
            long_press.add_pointer(PointerDispatch::at_root(event));
        }
    });
    binding.pointer_router().add_global_handler(route);

    clock.advance(ms(250));
    let Recorded::Pointer(press) = down(ms(250), 1, 10.0, 10.0).1 else {
        unreachable!("a Down is pointer input")
    };
    binding.handle_pointer_event(&press, |_| HitTestResult::new());
    // The deadline is a point on the event timeline: the Down's arrival plus
    // the timeout, comparable with the event times input carries.
    assert_eq!(binding.next_deadline(), Some(at(ms(750))));
    clock.advance(ms(499));
    arena.poll_deadlines();
    assert_eq!(*fired.borrow(), None, "1 ms early");
    clock.advance(ms(1));
    arena.poll_deadlines();
    assert_eq!(*fired.borrow(), Some(at(ms(750))), "fires at the deadline");
}

/// The determinism claim stops at the recording: application state changed by
/// something other than input (a worker result rebuilding the tree between
/// Down and Move) is not in it, and the same recording then decides
/// differently.
#[test]
fn replay_determinism_holds_for_a_fixed_application_state() {
    let recording = recording();
    let fixed = replay(&recording, None);
    assert_eq!(replay(&recording, None), fixed);
    // Between the drag's Down (700 ms) and its first Move (716 ms) a
    // non-input source unmounts the drag detector.
    let changed = replay(&recording, Some(ms(704)));
    assert_eq!(
        changed,
        ["long_press@512ms", "tap@950ms"],
        "the same recording over another application state decides differently"
    );
}
