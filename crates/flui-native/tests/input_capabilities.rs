//! Input capabilities (ADR-0213): what a backend implements, which devices
//! are present, the snapshot a window's input callback receives first, and
//! the order and recovery of the input held behind it, and input that arrives
//! while the callback runs.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::geometry::Point;
use flui_native::{
    HeadlessInputDevices, HeadlessOwnerTurns, HeadlessPlatform, HostWindow, MockWindow,
    OwnerPlatform, Platform as _,
};
use flui_platform::WindowOptions;
use flui_platform::input_capabilities::{Availability, InputCapabilities, PointerKindSet};
use flui_platform::pointer::{
    PenTool, PointerButton, PointerButtons, PointerEvent, PointerId, PointerInfo, PointerKind,
    PointerPosition, PointerPress, PointerSample,
};
use flui_platform::{DispatchEventResult, PlatformInput};
use flui_platform::{EventTime, ImeEvent};
use parking_lot::Mutex;

const PEN: PointerKind = PointerKind::Pen { tool: PenTool::Tip };

/// What a callback saw, in a comparable shape.
#[derive(Debug, Clone, PartialEq)]
enum Seen {
    Capabilities(PointerKindSet),
    Added,
    Removed,
    Down,
    Ime,
    Key,
}

fn seen(input: &PlatformInput) -> Seen {
    match input {
        PlatformInput::Capabilities(capabilities) => Seen::Capabilities(capabilities.present()),
        PlatformInput::Pointer(PointerEvent::DeviceAdded(_)) => Seen::Added,
        PlatformInput::Pointer(PointerEvent::DeviceRemoved(_)) => Seen::Removed,
        PlatformInput::Pointer(PointerEvent::Down(_)) => Seen::Down,
        PlatformInput::Ime(_) => Seen::Ime,
        PlatformInput::Keyboard(_) => Seen::Key,
        other => panic!("unexpected input {other:?}"),
    }
}

struct Session {
    owner: OwnerPlatform,
    turns: HeadlessOwnerTurns,
    devices: HeadlessInputDevices,
}

fn session(platform: HeadlessPlatform) -> Session {
    let turns = platform.owner_turns();
    let devices = platform.input_devices();
    let saved = std::rc::Rc::new(std::cell::RefCell::new(None));
    let installed = std::rc::Rc::clone(&saved);
    Box::new(platform)
        .run(Box::new(move |owner| {
            *installed.borrow_mut() = Some(owner);
            Ok(())
        }))
        .expect("headless owner");
    let owner = saved.borrow_mut().take().expect("started owner");
    Session {
        owner,
        turns,
        devices,
    }
}

impl Session {
    fn open(&self) -> Arc<dyn HostWindow> {
        self.owner
            .open_window(WindowOptions::default())
            .expect("open")
            .try_ready()
            .expect("ready")
    }
}

fn mock(window: &Arc<dyn HostWindow>) -> &MockWindow {
    window.as_any().downcast_ref::<MockWindow>().expect("mock")
}

/// Record every input; the returned log is shared with the callback.
fn recorder(window: &Arc<dyn HostWindow>) -> Arc<Mutex<Vec<Seen>>> {
    let log = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&log);
    window.on_input(Box::new(move |input| {
        sink.lock().push(seen(&input));
        DispatchEventResult::default()
    }));
    log
}

fn pen_down() -> PlatformInput {
    let pen = PointerInfo::new(PointerId::try_from(5_u64).expect("id"), PEN);
    let at = PointerSample::new(
        EventTime::from_nanos(10),
        PointerPosition::try_new(Point::new(1.0, 1.0)).expect("finite"),
    );
    PlatformInput::Pointer(PointerEvent::Down(PointerPress::new(
        pen,
        PointerButton::PRIMARY,
        PointerButtons::NONE,
        at,
    )))
}

fn ime() -> PlatformInput {
    PlatformInput::Ime(ImeEvent::Enabled)
}

/// A backend implementing pen with no pen attached reports `Native` and an
/// empty presence; attaching and removing the pen change presence only, each
/// change in order with the input around it.
fn input_capabilities_report_implemented_and_present_separately() {
    let session = session(HeadlessPlatform::new());
    let window = session.open();
    let first = Arc::new(Mutex::new(None));
    let kept = Arc::clone(&first);
    window.on_input(Box::new(move |input| {
        if let PlatformInput::Capabilities(capabilities) = input {
            kept.lock().get_or_insert(capabilities);
        }
        DispatchEventResult::default()
    }));
    let snapshot = first.lock().expect("snapshot delivered on registration");
    assert_eq!(snapshot.pen(), Availability::Native, "implemented");
    assert!(
        !snapshot.present().contains(PointerKindSet::PEN),
        "but absent"
    );

    let log = recorder(&window);
    let pen = session.devices.attach(PEN).expect("attached");
    mock(&window).inject_event(pen_down());
    assert!(session.devices.remove(pen));
    assert_eq!(
        *log.lock(),
        [
            Seen::Capabilities(PointerKindSet::empty()),
            Seen::Added,
            Seen::Capabilities(PointerKindSet::PEN),
            Seen::Down,
            Seen::Removed,
            Seen::Capabilities(PointerKindSet::empty()),
        ]
    );
}

/// A capability change after input was queued is delivered after that input
/// and before the input that followed it.
fn capability_change_is_ordered_with_input() {
    let session = session(HeadlessPlatform::new());
    let window = session.open();
    mock(&window).inject_event(ime());
    session.devices.attach(PEN).expect("attached");
    mock(&window).inject_event(pen_down());
    let log = recorder(&window);
    assert_eq!(
        *log.lock(),
        [
            Seen::Capabilities(PointerKindSet::empty()),
            Seen::Ime,
            Seen::Added,
            Seen::Capabilities(PointerKindSet::PEN),
            Seen::Down,
        ]
    );
}

/// A device attached before the window opens is present in its first
/// snapshot.
fn devices_attached_before_the_window_opens_are_in_the_initial_snapshot() {
    let platform = HeadlessPlatform::new();
    platform
        .input_devices()
        .attach(PointerKind::Touch)
        .expect("attached");
    let session = session(platform);
    let window = session.open();
    let log = recorder(&window);
    assert_eq!(*log.lock(), [Seen::Capabilities(PointerKindSet::TOUCH)]);
}

/// The availability a backend states is what the snapshot carries.
fn the_snapshot_carries_the_backend_availability() {
    let unavailable = InputCapabilities::builder()
        .outgoing_drag(Availability::Unavailable)
        .trackpad_gestures(Availability::Emulated)
        .build();
    let session = session(HeadlessPlatform::new().with_input_availability(unavailable));
    let window = session.open();
    let first = Arc::new(Mutex::new(None));
    let kept = Arc::clone(&first);
    window.on_input(Box::new(move |input| {
        if let PlatformInput::Capabilities(capabilities) = input {
            kept.lock().get_or_insert(capabilities);
        }
        DispatchEventResult::default()
    }));
    let snapshot = first.lock().expect("snapshot");
    assert_eq!(snapshot.trackpad_gestures(), Availability::Emulated);
    assert_eq!(snapshot.pen(), Availability::Unavailable);
}

/// The first snapshot panics with an otherwise idle application: the queued
/// tail stays in order and the backend's owner turn resumes it.
fn a_panic_on_the_initial_snapshot_keeps_the_queued_tail_in_order() {
    let session = session(HeadlessPlatform::new());
    let window = session.open();
    mock(&window).inject_event(ime());
    mock(&window).inject_event(pen_down());
    let log = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&log);
    let calls = AtomicUsize::new(0);
    window.on_input(Box::new(move |input| {
        let call = calls.fetch_add(1, Ordering::SeqCst);
        assert_ne!(call, 0, "the first snapshot fails");
        sink.lock().push(seen(&input));
        DispatchEventResult::default()
    }));
    assert!(log.lock().is_empty(), "no input before its baseline");
    session.turns.drive();
    assert_eq!(
        *log.lock(),
        [
            Seen::Capabilities(PointerKindSet::empty()),
            Seen::Ime,
            Seen::Down,
        ]
    );
}

/// A failed snapshot is retried before any queued input, and at most three
/// failed passes are made per registration; a new registration starts over.
fn the_snapshot_is_retried_before_any_queued_input() {
    let session = session(HeadlessPlatform::new());
    let window = session.open();
    mock(&window).inject_event(ime());
    let log = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&log);
    window.on_input(Box::new(move |input| {
        assert!(
            !matches!(input, PlatformInput::Capabilities(_)),
            "the snapshot keeps failing"
        );
        sink.lock().push(seen(&input));
        DispatchEventResult::default()
    }));
    for _ in 0..5 {
        session.turns.drive();
    }
    assert!(log.lock().is_empty(), "held after three failed passes");
    let log = recorder(&window);
    assert_eq!(
        *log.lock(),
        [Seen::Capabilities(PointerKindSet::empty()), Seen::Ime]
    );
}

/// Input admitted during a pass waits for the next pass, which the same wake
/// schedules: a callback admitting input on every call cannot monopolize the
/// owner thread.
fn reentrantly_admitted_input_waits_for_the_next_pass() {
    let session = session(HeadlessPlatform::new());
    let window = session.open();
    mock(&window).inject_event(ime());
    let log = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&log);
    let reentry = Arc::downgrade(&window);
    window.on_input(Box::new(move |input| {
        sink.lock().push(seen(&input));
        if let Some(window) = reentry.upgrade() {
            // Admitted during the pass that delivers the snapshot and tail.
            mock(&window).inject_event(ime());
        }
        DispatchEventResult::default()
    }));
    assert_eq!(
        *log.lock(),
        [Seen::Capabilities(PointerKindSet::empty()), Seen::Ime],
        "one pass delivers only what it started with"
    );
    session.turns.drive();
    assert_eq!(
        log.lock().len(),
        4,
        "the next pass takes the admitted input"
    );
}

/// A native event that arrives while the input callback runs is queued
/// behind it and delivered once it returns, in order, never dropped and never
/// nested inside the running callback (ADR-0207 §7); its own dispatch reports
/// the delivery deferred.
fn a_native_event_during_a_callback_is_queued_not_dropped() {
    let session = session(HeadlessPlatform::new());
    let window = session.open();
    let log = Arc::new(Mutex::new(Vec::new()));
    let nested = Arc::new(Mutex::new(None));
    let (sink, answer) = (Arc::clone(&log), Arc::clone(&nested));
    let reentry = Arc::downgrade(&window);
    window.on_input(Box::new(move |input| {
        let kind = seen(&input);
        sink.lock().push(format!("enter {kind:?}"));
        if kind == Seen::Down
            && let Some(window) = reentry.upgrade()
        {
            *answer.lock() = Some(mock(&window).inject_event(ime()));
        }
        sink.lock().push(format!("exit {kind:?}"));
        DispatchEventResult::default()
    }));
    log.lock().clear();
    mock(&window).inject_event(pen_down());
    session.turns.drive();
    assert!(
        nested.lock().is_some_and(DispatchEventResult::is_deferred),
        "the reentrant event's own answer is deferred"
    );
    assert_eq!(
        *log.lock(),
        ["enter Down", "exit Down", "enter Ime", "exit Ime"],
        "queued behind the running callback, delivered once, in order"
    );
}

/// A key that arrives while the input callback runs needs its answer now: it
/// is answered "not handled" so the platform default runs, queued behind the
/// running callback, and delivered once — the platform's follow-up for the
/// unhandled key (AppKit resends it as `keyDown:`) is recognized, whether it
/// arrives still inside the callback or after (ADR-0207 §7).
fn a_reentrant_event_needing_an_answer_is_unhandled_and_delivered_once() {
    use flui_platform::keyboard::{Code, Key, KeyEvent, KeyState};
    let key = |at| {
        PlatformInput::Keyboard(KeyEvent::new(
            KeyState::Down,
            Key::character("k"),
            Code::KeyK,
            EventTime::from_nanos(at),
        ))
    };
    for follow_up_inside in [true, false] {
        let session = session(HeadlessPlatform::new());
        let window = session.open();
        let log = Arc::new(Mutex::new(Vec::new()));
        let answers = Arc::new(Mutex::new(Vec::new()));
        let (sink, answered) = (Arc::clone(&log), Arc::clone(&answers));
        let reentry = Arc::downgrade(&window);
        window.on_input(Box::new(move |input| {
            let kind = seen(&input);
            sink.lock().push(kind.clone());
            if kind == Seen::Down
                && let Some(window) = reentry.upgrade()
            {
                answered.lock().push(mock(&window).inject_event(key(20)));
                if follow_up_inside {
                    answered.lock().push(mock(&window).inject_event(key(20)));
                }
            }
            DispatchEventResult::default()
        }));
        log.lock().clear();
        mock(&window).inject_event(pen_down());
        if !follow_up_inside {
            answers.lock().push(mock(&window).inject_event(key(20)));
        }
        session.turns.drive();
        let answers = answers.lock().clone();
        assert_eq!(
            answers[0],
            DispatchEventResult::DEFERRED_UNHANDLED,
            "the reentrant key is queued and answered not handled"
        );
        assert!(
            !answers[1].default_prevented && !answers[1].is_deferred(),
            "the follow-up is answered not handled and not queued again"
        );
        assert_eq!(
            *log.lock(),
            [Seen::Down, Seen::Key],
            "inside={follow_up_inside}: the key is delivered once, after the callback"
        );
        // A later key with its own timestamp is a new key, not a follow-up.
        mock(&window).inject_event(key(30));
        assert_eq!(log.lock().last(), Some(&Seen::Key));
        assert_eq!(log.lock().len(), 3);
    }
}

/// The input callback is held for the window's owner thread (ADR-0082 §4): a
/// registration from another thread is refused, so a callback that need not
/// be `Send` can never run or drop anywhere else.
fn an_input_callback_registered_off_the_owner_thread_is_refused() {
    let session = session(HeadlessPlatform::new());
    let window = session.open();
    struct Dropped(Arc<std::sync::atomic::AtomicBool>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let foreign = Arc::new(AtomicUsize::new(0));
    let (sink, registrar) = (Arc::clone(&foreign), Arc::clone(&window));
    std::thread::spawn(move || {
        let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let guard = Dropped(Arc::clone(&dropped));
        registrar.on_input(Box::new(move |_| {
            let _ = &guard;
            sink.fetch_add(1, Ordering::SeqCst);
            DispatchEventResult::default()
        }));
        assert!(
            dropped.load(Ordering::SeqCst),
            "the refused callback is dropped by the registration, on its own thread"
        );
    })
    .join()
    .expect("registering thread");
    mock(&window).inject_event(ime());
    session.turns.drive();
    assert_eq!(
        foreign.load(Ordering::SeqCst),
        0,
        "the foreign registration was refused"
    );

    let log = recorder(&window);
    session.turns.drive();
    assert_eq!(
        *log.lock(),
        [Seen::Capabilities(PointerKindSet::empty()), Seen::Ime],
        "the owner's registration receives the held input"
    );
}

/// A callback replaced during the first delivery whose captures panic on
/// drop: the replacement gets its own snapshot and the tail.
fn a_replaced_input_callback_whose_drop_panics_still_delivers_the_tail() {
    struct Hostile;
    impl Drop for Hostile {
        fn drop(&mut self) {
            panic!("outgoing capture");
        }
    }
    let session = session(HeadlessPlatform::new());
    let window = session.open();
    mock(&window).inject_event(ime());
    let log = Arc::new(Mutex::new(Vec::new()));
    let replacement_log = Arc::clone(&log);
    let reentry = Arc::downgrade(&window);
    let hostile = Hostile;
    window.on_input(Box::new(move |_| {
        let _ = &hostile;
        let sink = Arc::clone(&replacement_log);
        if let Some(window) = reentry.upgrade() {
            window.on_input(Box::new(move |input| {
                sink.lock().push(seen(&input));
                DispatchEventResult::default()
            }));
        }
        DispatchEventResult::default()
    }));
    session.turns.drive();
    assert_eq!(
        *log.lock(),
        [Seen::Capabilities(PointerKindSet::empty()), Seen::Ime]
    );
}

/// The most input a window holds behind its snapshot, as documented in
/// `ARCHITECTURE.md` ("A window's input waits behind its capability
/// snapshot").
const HELD_INPUT: u64 = 256;

fn key_at(at: u64) -> PlatformInput {
    use flui_platform::keyboard::{Code, Key, KeyEvent, KeyState};
    PlatformInput::Keyboard(KeyEvent::new(
        KeyState::Down,
        Key::character("k"),
        Code::KeyK,
        EventTime::from_nanos(at),
    ))
}

/// Record the time of every key and count the snapshots.
fn key_times(window: &Arc<dyn HostWindow>) -> Arc<Mutex<(usize, Vec<u64>)>> {
    let log = Arc::new(Mutex::new((0, Vec::new())));
    let sink = Arc::clone(&log);
    window.on_input(Box::new(move |input| {
        let mut log = sink.lock();
        match input {
            PlatformInput::Capabilities(_) => log.0 += 1,
            PlatformInput::Keyboard(key) => log.1.push(key.time.as_nanos()),
            other => panic!("unexpected input {other:?}"),
        }
        DispatchEventResult::default()
    }));
    log
}

/// A window whose input callback is never registered holds only the newest
/// input, so neither its memory nor the first registration's delivery grows
/// with how long it was open.
fn input_held_before_any_registration_is_bounded() {
    let session = session(HeadlessPlatform::new());
    let window = session.open();
    for at in 0..10_000 {
        mock(&window).inject_event(key_at(at));
    }
    let log = key_times(&window);
    session.turns.drive();
    let (snapshots, keys) = log.lock().clone();
    assert_eq!(snapshots, 1);
    assert_eq!(keys.len() as u64, HELD_INPUT, "held input is bounded");
    assert_eq!(
        keys,
        (10_000 - HELD_INPUT..10_000).collect::<Vec<_>>(),
        "the newest input is held, in order"
    );
}

/// Once a registration's snapshot has failed its three passes, nothing can
/// deliver new input until the callback is registered again: it is refused,
/// answered with the platform default rather than held, and a later
/// registration receives only what was held before.
fn input_after_the_snapshot_attempts_run_out_is_refused() {
    let session = session(HeadlessPlatform::new());
    let window = session.open();
    mock(&window).inject_event(key_at(1));
    window.on_input(Box::new(move |input| {
        assert!(
            !matches!(input, PlatformInput::Capabilities(_)),
            "the snapshot keeps failing"
        );
        DispatchEventResult::default()
    }));
    for _ in 0..5 {
        session.turns.drive();
    }
    for at in 2..10_000 {
        let answer = mock(&window).inject_event(key_at(at));
        assert!(
            !answer.is_deferred() && !answer.default_prevented,
            "refused input keeps its platform default"
        );
    }
    let log = key_times(&window);
    session.turns.drive();
    let (snapshots, keys) = log.lock().clone();
    assert_eq!(
        (snapshots, keys.len()),
        (1, 1),
        "only the input held before"
    );
    assert_eq!(keys, [1]);
}

/// Each backend states what it implements, as the code stands; X11 has no
/// pen, trackpad gestures or outgoing drags (ADR-0213 §1); input regions reach
/// the OS on iOS, and on Android from API 29 (ADR-0217 §5); an Android
/// touchpad's scroll and pinch arrive from API 34; Win32 starts OS drags
/// through OLE and the web through HTML drags (ADR-0211); the web emulates
/// trackpad pinch from Control-held wheel events unless the browser has
/// WebKit gesture events. Columns: pen, precise scroll, trackpad
/// gestures, input regions, outgoing drags.
fn every_backend_states_its_availability() {
    use Availability::{Emulated, Native, Unavailable};
    use flui_native::shared::input_availability::{self as table, AndroidInputApis, WinitSession};
    let android = |gesture_exclusion, touchpad_gestures, drag_source| {
        table::android(AndroidInputApis {
            gesture_exclusion,
            touchpad_gestures,
            drag_source,
        })
    };
    let rows = [
        (
            "windows with DirectManipulation",
            table::windows(true),
            [Native, Native, Native, Unavailable, Native],
        ),
        (
            "windows without DirectManipulation",
            table::windows(false),
            [Native, Unavailable, Unavailable, Unavailable, Native],
        ),
        (
            "macos",
            table::macos(),
            [Unavailable, Native, Native, Unavailable, Unavailable],
        ),
        (
            "ios",
            table::ios(),
            [Native, Unavailable, Unavailable, Native, Unavailable],
        ),
        (
            "android with every input API",
            android(true, true, true),
            [Native, Native, Native, Native, Native],
        ),
        (
            "android without FluiView",
            android(true, true, false),
            [Native, Native, Native, Native, Unavailable],
        ),
        (
            "android before touchpad gestures",
            android(true, false, true),
            [Native, Unavailable, Unavailable, Native, Native],
        ),
        (
            "android before gesture exclusion",
            android(false, false, true),
            [Native, Unavailable, Unavailable, Unavailable, Native],
        ),
        (
            "android before startDragAndDrop",
            android(false, false, false),
            [Native, Unavailable, Unavailable, Unavailable, Unavailable],
        ),
        (
            "web with ctrl+wheel pinch",
            table::web(false),
            [Native, Unavailable, Emulated, Unavailable, Native],
        ),
        (
            "web with gesture events",
            table::web(true),
            [Native, Unavailable, Native, Unavailable, Native],
        ),
        (
            "winit x11",
            table::winit(WinitSession::X11),
            [
                Unavailable,
                Unavailable,
                Unavailable,
                Unavailable,
                Unavailable,
            ],
        ),
        (
            "winit wayland",
            table::winit(WinitSession::Wayland),
            [Unavailable, Native, Unavailable, Unavailable, Unavailable],
        ),
        (
            "winit macos",
            table::winit(WinitSession::MacOs),
            [Unavailable, Native, Native, Unavailable, Unavailable],
        ),
    ];
    for (backend, capabilities, expected) in rows {
        assert_eq!(
            [
                capabilities.pen(),
                capabilities.precise_scroll(),
                capabilities.trackpad_gestures(),
                capabilities.input_regions(),
                capabilities.outgoing_drag(),
            ],
            expected,
            "{backend}"
        );
        assert!(
            capabilities.present().is_empty(),
            "{backend}: presence is the registry's"
        );
    }
}

#[test]
fn input_capabilities_contracts() {
    super::run_table(
        "input_capabilities",
        &[
            (
                "input_capabilities_report_implemented_and_present_separately",
                input_capabilities_report_implemented_and_present_separately,
            ),
            (
                "capability_change_is_ordered_with_input",
                capability_change_is_ordered_with_input,
            ),
            (
                "devices_attached_before_the_window_opens_are_in_the_initial_snapshot",
                devices_attached_before_the_window_opens_are_in_the_initial_snapshot,
            ),
            (
                "every_backend_states_its_availability",
                every_backend_states_its_availability,
            ),
            (
                "the_snapshot_carries_the_backend_availability",
                the_snapshot_carries_the_backend_availability,
            ),
            (
                "a_panic_on_the_initial_snapshot_keeps_the_queued_tail_in_order",
                a_panic_on_the_initial_snapshot_keeps_the_queued_tail_in_order,
            ),
            (
                "the_snapshot_is_retried_before_any_queued_input",
                the_snapshot_is_retried_before_any_queued_input,
            ),
            (
                "input_held_before_any_registration_is_bounded",
                input_held_before_any_registration_is_bounded,
            ),
            (
                "input_after_the_snapshot_attempts_run_out_is_refused",
                input_after_the_snapshot_attempts_run_out_is_refused,
            ),
            (
                "reentrantly_admitted_input_waits_for_the_next_pass",
                reentrantly_admitted_input_waits_for_the_next_pass,
            ),
            (
                "a_native_event_during_a_callback_is_queued_not_dropped",
                a_native_event_during_a_callback_is_queued_not_dropped,
            ),
            (
                "a_reentrant_event_needing_an_answer_is_unhandled_and_delivered_once",
                a_reentrant_event_needing_an_answer_is_unhandled_and_delivered_once,
            ),
            (
                "an_input_callback_registered_off_the_owner_thread_is_refused",
                an_input_callback_registered_off_the_owner_thread_is_refused,
            ),
            (
                "a_replaced_input_callback_whose_drop_panics_still_delivers_the_tail",
                a_replaced_input_callback_whose_drop_panics_still_delivers_the_tail,
            ),
        ],
    );
}
