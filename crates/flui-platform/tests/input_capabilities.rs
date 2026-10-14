//! Input capabilities (ADR-0213): what a backend implements, which devices
//! are present, the snapshot a window's input callback receives first, and
//! the order and recovery of the input held behind it, and input that arrives
//! while the callback runs.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::geometry::Point;
use flui_platform::traits::{DispatchEventResult, PlatformInput};
use flui_platform::{
    HeadlessInputDevices, HeadlessOwnerTurns, HeadlessPlatform, HostWindow, MockWindow,
    OwnerPlatform, Platform as _, WindowOptions,
};
use flui_platform_api::input_capabilities::{Availability, InputCapabilities, PointerKindSet};
use flui_platform_api::pointer::{
    PenTool, PointerButton, PointerButtons, PointerEvent, PointerId, PointerInfo, PointerKind,
    PointerPosition, PointerPress, PointerSample,
};
use flui_platform_api::{EventTime, ImeEvent};
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
}

fn seen(input: &PlatformInput) -> Seen {
    match input {
        PlatformInput::Capabilities(capabilities) => Seen::Capabilities(capabilities.present()),
        PlatformInput::Pointer(PointerEvent::DeviceAdded(_)) => Seen::Added,
        PlatformInput::Pointer(PointerEvent::DeviceRemoved(_)) => Seen::Removed,
        PlatformInput::Pointer(PointerEvent::Down(_)) => Seen::Down,
        PlatformInput::Ime(_) => Seen::Ime,
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
    assert_eq!(foreign.load(Ordering::SeqCst), 0, "the foreign registration was refused");

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

/// Each backend states what it implements, as the code stands; X11 has no
/// pen, trackpad gestures or outgoing drags (ADR-0213 §1); input regions reach
/// the OS on iOS, and on Android from API 29 (ADR-0217 §5).
fn every_backend_states_its_availability() {
    use Availability::{Native, Unavailable};
    use flui_platform::shared::input_availability::{self as table, WinitSession};
    let rows = [
        (
            "windows",
            table::windows(),
            [Native, Unavailable, Unavailable, Unavailable],
        ),
        (
            "macos",
            table::macos(),
            [Unavailable, Native, Native, Unavailable],
        ),
        (
            "ios",
            table::ios(),
            [Native, Unavailable, Unavailable, Native],
        ),
        (
            "android 29",
            table::android(29),
            [Native, Unavailable, Unavailable, Native],
        ),
        (
            "android 28",
            table::android(28),
            [Native, Unavailable, Unavailable, Unavailable],
        ),
        (
            "web",
            table::web(),
            [Native, Unavailable, Unavailable, Unavailable],
        ),
        (
            "winit x11",
            table::winit(WinitSession::X11),
            [Unavailable, Unavailable, Unavailable, Unavailable],
        ),
        (
            "winit wayland",
            table::winit(WinitSession::Wayland),
            [Unavailable, Native, Unavailable, Unavailable],
        ),
        (
            "winit macos",
            table::winit(WinitSession::MacOs),
            [Unavailable, Native, Native, Unavailable],
        ),
    ];
    for (backend, capabilities, [pen, precise_scroll, trackpad, input_regions]) in rows {
        assert_eq!(
            [
                capabilities.pen(),
                capabilities.precise_scroll(),
                capabilities.trackpad_gestures(),
                capabilities.input_regions(),
            ],
            [pen, precise_scroll, trackpad, input_regions],
            "{backend}"
        );
        assert_eq!(capabilities.outgoing_drag(), Unavailable, "{backend}");
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
                "reentrantly_admitted_input_waits_for_the_next_pass",
                reentrantly_admitted_input_waits_for_the_next_pass,
            ),
            (
                "a_native_event_during_a_callback_is_queued_not_dropped",
                a_native_event_during_a_callback_is_queued_not_dropped,
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
