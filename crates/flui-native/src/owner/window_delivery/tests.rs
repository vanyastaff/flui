//! What a window delivers to its handler, one row per obligation
//! (ADR-0254 §4): the capability snapshot comes first, a delivery raised
//! inside a method waits for it in order, a panicking method keeps the queue
//! behind it and its own payload, a close can be vetoed, `closed` is the last
//! call, and the window lets go of its handler when it is cleared.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::{Arc, Weak};

use flui_platform::input_capabilities::InputCapabilities;
use flui_platform::keyboard::{Code, Key, KeyEvent, KeyState, NamedKey};
use flui_platform::{
    Availability, CloseDecision, DispatchEventResult, EventTime, FrameRequest, ImeEvent,
    PlatformInput, SurfaceChange,
};
use flui_testing::window_handler::{HandlerCall, RecordingWindowHandler};

use super::WindowDelivery;
use crate::owner::handlers::frame_request_now;

fn ime() -> PlatformInput {
    PlatformInput::Ime(ImeEvent::Enabled)
}

fn key() -> PlatformInput {
    PlatformInput::Keyboard(KeyEvent::new(
        KeyState::Down,
        Key::Named(NamedKey::Enter),
        Code::Enter,
        EventTime::from_nanos(0),
    ))
}

fn snapshot() -> HandlerCall {
    HandlerCall::Input(PlatformInput::Capabilities(InputCapabilities::UNAVAILABLE))
}

fn input() -> HandlerCall {
    HandlerCall::Input(ime())
}

/// A window's delivery to a recording handler.
fn window() -> (Arc<WindowDelivery>, Rc<RecordingWindowHandler>) {
    let handler = RecordingWindowHandler::new();
    let delivery = Arc::new(WindowDelivery::new(
        handler.clone(),
        InputCapabilities::UNAVAILABLE,
    ));
    (delivery, handler)
}

/// The handler's first input is the capability snapshot, whether its window
/// delivers it at open or an input arrives first; it is delivered once.
fn the_capability_snapshot_is_the_first_input() {
    let (delivery, handler) = window();
    delivery.deliver_capability_snapshot();
    delivery.deliver_capability_snapshot();
    delivery.dispatch_input(ime());
    assert_eq!(handler.take_calls(), [snapshot(), input()]);

    let (delivery, handler) = window();
    delivery.dispatch_state_changed();
    delivery.dispatch_input(ime());
    delivery.deliver_capability_snapshot();
    assert_eq!(
        handler.take_calls(),
        [HandlerCall::StateChanged, snapshot(), input()],
        "an input that arrives before the open delivered the snapshot carries it"
    );
}

/// Capabilities set before the snapshot is delivered become the snapshot;
/// a change after it is an input in order with the input around it.
fn a_capability_change_is_ordered_with_the_input() {
    let (delivery, handler) = window();
    let touch = InputCapabilities::builder()
        .pen(Availability::Native)
        .build();
    delivery.set_input_capabilities(touch);
    delivery.deliver_capability_snapshot();
    delivery.dispatch_input(ime());
    delivery.set_input_capabilities(InputCapabilities::UNAVAILABLE);
    let inputs = handler.inputs();
    assert_eq!(inputs.len(), 3, "{inputs:?}");
    assert!(matches!(inputs[0], PlatformInput::Capabilities(c) if c == touch));
    assert!(matches!(inputs[1], PlatformInput::Ime(_)));
    assert!(
        matches!(inputs[2], PlatformInput::Capabilities(c) if c == InputCapabilities::UNAVAILABLE)
    );
}

/// Each method of the handler raises a delivery of every kind from inside
/// itself: none runs inside it, each runs after it returns, in the order
/// raised; a nested input is answered with its deferred form, a key with the
/// one that lets the platform default run.
fn a_delivery_raised_inside_a_method_waits_for_it() {
    type Raise = fn(&RecordingWindowHandler, Weak<WindowDelivery>, Rc<Cell<Option<bool>>>);
    type Trigger = fn(&WindowDelivery);
    fn raise_all(delivery: &WindowDelivery, answers: &Cell<Option<bool>>) {
        let nested = delivery.dispatch_input(ime());
        let nested_key = delivery.dispatch_input(key());
        answers.set(Some(
            nested == DispatchEventResult::DEFERRED
                && nested_key == DispatchEventResult::DEFERRED_UNHANDLED,
        ));
        delivery.dispatch_request_frame();
        delivery.dispatch_state_changed();
        delivery.dispatch_surface_change(SurfaceChange::Rebuild);
    }
    let rows: [(&str, Raise, Trigger); 4] = [
        (
            "input",
            |handler, delivery, answers| {
                let first = Cell::new(true);
                handler.react_to_input(move |event| {
                    if matches!(event, PlatformInput::Ime(_)) && first.replace(false) {
                        let delivery = delivery.upgrade().expect("live delivery");
                        raise_all(&delivery, &answers);
                    }
                    DispatchEventResult::default()
                });
            },
            |delivery| {
                delivery.dispatch_input(ime());
            },
        ),
        (
            "state changed",
            |handler, delivery, answers| {
                let first = Cell::new(true);
                handler.react_to_state_change(move || {
                    if first.replace(false) {
                        raise_all(&delivery.upgrade().expect("live delivery"), &answers);
                    }
                });
            },
            WindowDelivery::dispatch_state_changed,
        ),
        (
            "frame requested",
            |handler, delivery, answers| {
                let first = Cell::new(true);
                handler.react_to_frame(move |_| {
                    if first.replace(false) {
                        raise_all(&delivery.upgrade().expect("live delivery"), &answers);
                    }
                });
            },
            WindowDelivery::dispatch_request_frame,
        ),
        (
            "surface changed",
            |handler, delivery, answers| {
                let first = Cell::new(true);
                handler.react_to_surface_change(move |_| {
                    if first.replace(false) {
                        raise_all(&delivery.upgrade().expect("live delivery"), &answers);
                    }
                });
            },
            |delivery| delivery.dispatch_surface_change(SurfaceChange::Release),
        ),
    ];
    for (method, raise, deliver) in rows {
        let (delivery, handler) = window();
        delivery.deliver_capability_snapshot();
        handler.take_calls();
        let answers = Rc::new(Cell::new(None));
        raise(&handler, Arc::downgrade(&delivery), Rc::clone(&answers));
        deliver(&delivery);
        let calls = handler.take_calls();
        let tail = &calls[1..];
        assert!(
            matches!(
                tail,
                [
                    HandlerCall::Input(PlatformInput::Ime(_)),
                    HandlerCall::Input(PlatformInput::Keyboard(_)),
                    HandlerCall::FrameRequested(_),
                    HandlerCall::StateChanged,
                    HandlerCall::SurfaceChanged(SurfaceChange::Rebuild),
                ]
            ),
            "{method}: {calls:?}"
        );
        assert_eq!(answers.get(), Some(true), "{method}: the nested answers");
    }
}

/// A method that panics is contained at the call point: every delivery
/// queued behind it still reaches the handler, the first panic resumes
/// afterwards, and a second one does not replace it.
fn a_panicking_method_keeps_the_queue_and_its_payload() {
    let (delivery, handler) = window();
    delivery.deliver_capability_snapshot();
    handler.take_calls();
    let weak = Arc::downgrade(&delivery);
    handler.react_to_input(move |_| {
        let delivery = weak.upgrade().expect("live delivery");
        delivery.dispatch_state_changed();
        delivery.dispatch_frame(frame_request_now());
        delivery.dispatch_close();
        panic!("the first failure");
    });
    handler.react_to_state_change(|| panic!("a later failure"));
    let failure = catch_unwind(AssertUnwindSafe(|| delivery.dispatch_input(ime())))
        .expect_err("the input's panic resumes");
    assert_eq!(
        flui_foundation::panic::payload_text(failure.as_ref()),
        Some("the first failure")
    );
    let calls = handler.take_calls();
    assert!(
        matches!(
            calls.as_slice(),
            [
                HandlerCall::Input(_),
                HandlerCall::StateChanged,
                HandlerCall::FrameRequested(_),
                HandlerCall::Closed,
            ]
        ),
        "the queued tail was lost: {calls:?}"
    );
}

/// The handler decides a close request; asked again while it answers, the
/// window keeps itself open without asking.
fn a_close_request_is_the_handlers_to_decide() {
    let (delivery, handler) = window();
    handler.react_to_close_request(|| CloseDecision::KeepOpen);
    assert!(!delivery.dispatch_should_close(), "a veto keeps the window");
    handler.react_to_close_request(|| CloseDecision::Close);
    assert!(delivery.dispatch_should_close(), "the handler closes it");

    let nested = Rc::new(Cell::new(None));
    let weak = Arc::downgrade(&delivery);
    let heard = Rc::clone(&nested);
    handler.react_to_close_request(move || {
        heard.set(Some(
            weak.upgrade()
                .expect("live delivery")
                .dispatch_should_close(),
        ));
        CloseDecision::Close
    });
    assert!(delivery.dispatch_should_close());
    assert_eq!(
        nested.get(),
        Some(false),
        "a nested request keeps the window"
    );
    assert_eq!(handler.count(&HandlerCall::CloseRequested), 3);
}

/// `closed` is called once and is the last call; a window cleared lets go of
/// its handler, after the deliveries queued ahead of the clear, a clear made
/// inside a method included.
fn closed_is_the_last_call_and_the_window_lets_go() {
    let (delivery, handler) = window();
    delivery.dispatch_close();
    delivery.dispatch_close();
    delivery.dispatch_state_changed();
    delivery.dispatch_input(ime());
    assert_eq!(handler.take_calls(), [HandlerCall::Closed]);
    delivery.clear();
    assert_eq!(Rc::strong_count(&handler), 1, "the window kept its handler");

    let (delivery, handler) = window();
    let weak = Arc::downgrade(&delivery);
    let released = Rc::new(RefCell::new(None));
    let notice = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let retired = Arc::clone(&notice);
    let seen = Rc::clone(&released);
    let held = Rc::downgrade(&handler);
    handler.react_to_frame(move |_| {
        let delivery = weak.upgrade().expect("live delivery");
        delivery.dispatch_close();
        let retired = Arc::clone(&retired);
        delivery.clear_then(move || retired.store(true, std::sync::atomic::Ordering::SeqCst));
        *seen.borrow_mut() = Some(held.strong_count());
    });
    let handler_weak = Rc::downgrade(&handler);
    drop(handler);
    delivery.dispatch_request_frame();
    assert!(
        released.borrow().is_some_and(|count| count > 0),
        "the handler was dropped inside its own method"
    );
    assert!(
        notice.load(std::sync::atomic::Ordering::SeqCst),
        "the retirement notice ran"
    );
    assert_eq!(
        handler_weak.strong_count(),
        0,
        "a cleared window still holds its handler"
    );
    // A delivery after the clear reaches nothing.
    assert_eq!(
        delivery.dispatch_input(ime()),
        DispatchEventResult::default()
    );
    assert!(
        delivery.dispatch_should_close(),
        "with no handler the close goes ahead"
    );
}

/// A frame is counted when the handler's `frame_requested` runs, not when no
/// handler takes it.
fn a_frame_counts_when_the_handler_runs() {
    let handler = RecordingWindowHandler::new();
    let frames = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let delivery = WindowDelivery::counting_frames(
        handler,
        InputCapabilities::UNAVAILABLE,
        Arc::clone(&frames),
    );
    delivery.dispatch_frame(FrameRequest::new(EventTime::from_nanos(1)));
    delivery.clear();
    delivery.dispatch_request_frame();
    assert_eq!(frames.load(std::sync::atomic::Ordering::Relaxed), 1);
}

/// One row per obligation of a window's handler, as its window delivers
/// them.
#[test]
fn window_handler_obligation_table() {
    let rows: &[(&str, fn())] = &[
        (
            "the capability snapshot is the first input",
            the_capability_snapshot_is_the_first_input,
        ),
        (
            "a capability change is ordered with the input",
            a_capability_change_is_ordered_with_the_input,
        ),
        (
            "a delivery raised inside a method waits for it",
            a_delivery_raised_inside_a_method_waits_for_it,
        ),
        (
            "a panicking method keeps the queue and its payload",
            a_panicking_method_keeps_the_queue_and_its_payload,
        ),
        (
            "a close request is the handler's to decide",
            a_close_request_is_the_handlers_to_decide,
        ),
        (
            "closed is the last call and the window lets go",
            closed_is_the_last_call_and_the_window_lets_go,
        ),
        (
            "a frame counts when the handler runs",
            a_frame_counts_when_the_handler_runs,
        ),
    ];
    let mut failures = Vec::new();
    for &(name, row) in rows {
        if let Err(payload) = catch_unwind(row) {
            failures.push(format!(
                "{name}: {}",
                flui_foundation::panic::payload_text(payload.as_ref()).unwrap_or("?")
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
