//! Input capabilities (ADR-0213) and a window's input as its handler hears
//! it (ADR-0254 §4): what a backend implements, which devices are present,
//! the snapshot that is a handler's first input, its order with the input
//! around it, and input that arrives while the handler runs.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use flui_foundation::geometry::Point;
use flui_native::{
    HeadlessInputDevices, HeadlessOwnerTurns, HeadlessPlatform, HostWindow, MockWindow,
    OwnerPlatform, Platform as _,
};
use flui_platform::Availability;
use flui_platform::WindowOptions;
use flui_platform::input_capabilities::{InputCapabilities, PointerKindSet};
use flui_platform::pointer::{
    PenTool, PointerButton, PointerButtons, PointerEvent, PointerId, PointerInfo, PointerKind,
    PointerPosition, PointerPress, PointerSample,
};
use flui_platform::{DispatchEventResult, PlatformInput};
use flui_platform::{EventTime, ImeEvent};
use flui_testing::window_handler::RecordingWindowHandler;
use parking_lot::Mutex;

const PEN: PointerKind = PointerKind::Pen { tool: PenTool::Tip };

/// What a handler heard, in a comparable shape.
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
        .run(crate::app::on_ready(move |owner| {
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
    /// A window delivering to `handler`.
    fn open(&self, handler: &Rc<RecordingWindowHandler>) -> Rc<dyn HostWindow> {
        self.owner
            .open_window(WindowOptions::default(), handler.clone())
            .expect("open")
            .try_ready()
            .expect("ready")
    }
}

fn mock(window: &Rc<dyn HostWindow>) -> &MockWindow {
    window.as_any().downcast_ref::<MockWindow>().expect("mock")
}

/// Every input `handler` heard, in a comparable shape.
fn heard(handler: &RecordingWindowHandler) -> Vec<Seen> {
    handler.inputs().iter().map(seen).collect()
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

fn key_at(at: u64) -> PlatformInput {
    use flui_platform::keyboard::{Code, Key, KeyEvent, KeyState};
    PlatformInput::Keyboard(KeyEvent::new(
        KeyState::Down,
        Key::character("k"),
        Code::KeyK,
        EventTime::from_nanos(at),
    ))
}

/// The first event after a window opens reaches its handler: the handler
/// exists before the window does, so its capability snapshot arrives before
/// the open returns, and the first input after it is delivered at once, not
/// held for a registration.
fn the_first_event_after_open_reaches_the_handler() {
    let session = session(HeadlessPlatform::new());
    let handler = RecordingWindowHandler::new();
    let window = session.open(&handler);
    assert_eq!(
        heard(&handler),
        [Seen::Capabilities(PointerKindSet::empty())],
        "the snapshot arrived with the open"
    );
    let answer = mock(&window).inject_event(ime());
    assert!(!answer.is_deferred(), "the input was delivered, not held");
    assert_eq!(
        heard(&handler),
        [Seen::Capabilities(PointerKindSet::empty()), Seen::Ime]
    );
}

/// A backend implementing pen with no pen attached reports `Native` and an
/// empty presence; attaching and removing the pen change presence only, each
/// change in order with the input around it.
fn input_capabilities_report_implemented_and_present_separately() {
    let session = session(HeadlessPlatform::new());
    let handler = RecordingWindowHandler::new();
    let window = session.open(&handler);
    let PlatformInput::Capabilities(snapshot) = handler.inputs().remove(0) else {
        panic!("the first input is the snapshot");
    };
    assert_eq!(snapshot.pen(), Availability::Native, "implemented");
    assert!(
        !snapshot.present().contains(PointerKindSet::PEN),
        "but absent"
    );

    let pen = session.devices.attach(PEN).expect("attached");
    mock(&window).inject_event(pen_down());
    assert!(session.devices.remove(pen));
    assert_eq!(
        heard(&handler),
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

/// A capability change between two inputs is delivered after the first and
/// before the second.
fn capability_change_is_ordered_with_input() {
    let session = session(HeadlessPlatform::new());
    let handler = RecordingWindowHandler::new();
    let window = session.open(&handler);
    mock(&window).inject_event(ime());
    session.devices.attach(PEN).expect("attached");
    mock(&window).inject_event(pen_down());
    assert_eq!(
        heard(&handler),
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
    let handler = RecordingWindowHandler::new();
    let _window = session.open(&handler);
    assert_eq!(heard(&handler), [Seen::Capabilities(PointerKindSet::TOUCH)]);
}

/// The availability a backend states is what the snapshot carries.
fn the_snapshot_carries_the_backend_availability() {
    let unavailable = InputCapabilities::builder()
        .outgoing_drag(Availability::Unavailable)
        .trackpad_gestures(Availability::Emulated)
        .build();
    let session = session(HeadlessPlatform::new().with_input_availability(unavailable));
    let handler = RecordingWindowHandler::new();
    let _window = session.open(&handler);
    let PlatformInput::Capabilities(snapshot) = handler.inputs().remove(0) else {
        panic!("the first input is the snapshot");
    };
    assert_eq!(snapshot.trackpad_gestures(), Availability::Emulated);
    assert_eq!(snapshot.pen(), Availability::Unavailable);
}

/// A handler that panics on its capability snapshot is not silenced: the
/// open that delivered the snapshot resumes the handler's own panic, and the
/// window it opened keeps delivering to the handler.
fn a_panic_on_the_snapshot_is_resumed() {
    let session = session(HeadlessPlatform::new());
    let handler = RecordingWindowHandler::new();
    handler.react_to_input(|input| {
        assert!(
            !matches!(input, PlatformInput::Capabilities(_)),
            "the snapshot fails"
        );
        DispatchEventResult::default()
    });
    let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = session.open(&handler);
    }))
    .expect_err("the snapshot's panic reaches the caller of the open");
    assert_eq!(
        flui_foundation::panic::payload_text(&*payload),
        Some("the snapshot fails"),
        "the handler's own panic, not one of the platform's"
    );
    assert_eq!(
        heard(&handler),
        [Seen::Capabilities(PointerKindSet::empty())]
    );
}

/// A native event that arrives while the handler's `input` runs is queued
/// behind it and delivered once it returns, in order, never dropped and never
/// nested inside the running method (ADR-0207 §7); its own dispatch reports
/// the delivery deferred.
fn a_native_event_during_a_callback_is_queued_not_dropped() {
    let session = session(HeadlessPlatform::new());
    let handler = RecordingWindowHandler::new();
    let window = session.open(&handler);
    let log = Arc::new(Mutex::new(Vec::new()));
    let nested = Arc::new(Mutex::new(None));
    let (sink, answer) = (Arc::clone(&log), Arc::clone(&nested));
    let reentry = Rc::downgrade(&window);
    handler.react_to_input(move |input| {
        let kind = seen(&input);
        sink.lock().push(format!("enter {kind:?}"));
        if kind == Seen::Down
            && let Some(window) = reentry.upgrade()
        {
            *answer.lock() = Some(mock(&window).inject_event(ime()));
        }
        sink.lock().push(format!("exit {kind:?}"));
        DispatchEventResult::default()
    });
    mock(&window).inject_event(pen_down());
    session.turns.drive();
    assert!(
        nested.lock().is_some_and(DispatchEventResult::is_deferred),
        "the reentrant event's own answer is deferred"
    );
    assert_eq!(
        *log.lock(),
        ["enter Down", "exit Down", "enter Ime", "exit Ime"],
        "queued behind the running method, delivered once, in order"
    );
}

/// A key that arrives while the handler's `input` runs needs its answer now:
/// it is answered "not handled" so the platform default runs, queued behind
/// the running method, and delivered once — the platform's follow-up for the
/// unhandled key (AppKit resends it as `keyDown:`) is recognized, whether it
/// arrives still inside the method or after (ADR-0207 §7).
fn a_reentrant_event_needing_an_answer_is_unhandled_and_delivered_once() {
    for follow_up_inside in [true, false] {
        let session = session(HeadlessPlatform::new());
        let handler = RecordingWindowHandler::new();
        let window = session.open(&handler);
        let log = Arc::new(Mutex::new(Vec::new()));
        let answers = Arc::new(Mutex::new(Vec::new()));
        let (sink, answered) = (Arc::clone(&log), Arc::clone(&answers));
        let reentry = Rc::downgrade(&window);
        handler.react_to_input(move |input| {
            let kind = seen(&input);
            sink.lock().push(kind.clone());
            if kind == Seen::Down
                && let Some(window) = reentry.upgrade()
            {
                answered.lock().push(mock(&window).inject_event(key_at(20)));
                if follow_up_inside {
                    answered.lock().push(mock(&window).inject_event(key_at(20)));
                }
            }
            DispatchEventResult::default()
        });
        mock(&window).inject_event(pen_down());
        if !follow_up_inside {
            answers.lock().push(mock(&window).inject_event(key_at(20)));
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
            "inside={follow_up_inside}: the key is delivered once, after the method"
        );
        // A later key with its own timestamp is a new key, not a follow-up.
        mock(&window).inject_event(key_at(30));
        assert_eq!(log.lock().last(), Some(&Seen::Key));
        assert_eq!(log.lock().len(), 3);
    }
}

/// An `input` that keeps raising input from inside itself is delivered each
/// one after the method that raised it returns, never nested, and the window
/// keeps answering.
fn input_raised_from_input_is_delivered_after_it() {
    let session = session(HeadlessPlatform::new());
    let handler = RecordingWindowHandler::new();
    let window = session.open(&handler);
    let depth = Rc::new(Cell::new(0_u32));
    let deepest = Rc::new(Cell::new(0_u32));
    let raised = Rc::new(Cell::new(0_u32));
    let (inside, max, count) = (Rc::clone(&depth), Rc::clone(&deepest), Rc::clone(&raised));
    let reentry = Rc::downgrade(&window);
    handler.react_to_input(move |input| {
        if matches!(input, PlatformInput::Ime(_)) {
            inside.set(inside.get() + 1);
            max.set(max.get().max(inside.get()));
            if count.get() < 3
                && let Some(window) = reentry.upgrade()
            {
                count.set(count.get() + 1);
                mock(&window).inject_event(ime());
            }
            inside.set(inside.get() - 1);
        }
        DispatchEventResult::default()
    });
    mock(&window).inject_event(ime());
    assert_eq!(deepest.get(), 1, "an input ran inside another");
    assert_eq!(
        heard(&handler),
        [
            Seen::Capabilities(PointerKindSet::empty()),
            Seen::Ime,
            Seen::Ime,
            Seen::Ime,
            Seen::Ime,
        ]
    );
}

#[test]
fn input_capabilities_contracts() {
    super::run_table(
        "input_capabilities",
        &[
            (
                "the_first_event_after_open_reaches_the_handler",
                the_first_event_after_open_reaches_the_handler,
            ),
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
                "the_snapshot_carries_the_backend_availability",
                the_snapshot_carries_the_backend_availability,
            ),
            (
                "a_panic_on_the_snapshot_is_resumed",
                a_panic_on_the_snapshot_is_resumed,
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
                "input_raised_from_input_is_delivered_after_it",
                input_raised_from_input_is_delivered_after_it,
            ),
        ],
    );
}
