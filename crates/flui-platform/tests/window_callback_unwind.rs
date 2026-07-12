//! Integration coverage for panic-safe platform callback dispatch.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use flui_platform::{
    WindowCallbacks,
    traits::{DispatchEventResult, Key, PlatformInput},
};
use flui_types::{Size, geometry::px};

fn keyboard_event(repeat: bool) -> PlatformInput {
    PlatformInput::Keyboard(ui_events::keyboard::KeyboardEvent {
        state: ui_events::keyboard::KeyState::Down,
        key: Key::Named(keyboard_types::NamedKey::Enter),
        code: ui_events::keyboard::Code::Unidentified,
        location: ui_events::keyboard::Location::Standard,
        modifiers: keyboard_types::Modifiers::empty(),
        repeat,
        is_composing: false,
    })
}

#[test]
fn frame_callback_is_restored_after_real_dispatch_panics() {
    let callbacks = Arc::new(WindowCallbacks::new());
    let weak_callbacks = Arc::downgrade(&callbacks);
    let calls = Arc::new(AtomicUsize::new(0));
    let calls_in_callback = Arc::clone(&calls);
    *callbacks.on_request_frame.lock() = Some(Box::new(move || {
        let call = calls_in_callback.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            weak_callbacks
                .upgrade()
                .expect("callbacks alive")
                .dispatch_request_frame();
        }
        assert_ne!(call, 0, "first dispatch panics");
    }));

    let first = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        callbacks.dispatch_request_frame();
    }));
    assert!(first.is_err());

    callbacks.dispatch_request_frame();
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "nested work from the aborted dispatch must not leak into the next call"
    );
}

#[test]
fn nested_frame_dispatch_is_drained_fifo_not_dropped() {
    let callbacks = Arc::new(WindowCallbacks::new());
    let weak_callbacks = Arc::downgrade(&callbacks);
    let order = Arc::new(Mutex::new(Vec::new()));
    let callback_order = Arc::clone(&order);
    let calls = Arc::new(AtomicUsize::new(0));
    let callback_calls = Arc::clone(&calls);
    *callbacks.on_request_frame.lock() = Some(Box::new(move || {
        let call = callback_calls.fetch_add(1, Ordering::SeqCst);
        if call == 0 {
            callback_order.lock().expect("order lock").push(1);
            weak_callbacks
                .upgrade()
                .expect("callbacks alive")
                .dispatch_request_frame();
            callback_order.lock().expect("order lock").push(2);
        } else {
            callback_order.lock().expect("order lock").push(3);
        }
    }));

    callbacks.dispatch_request_frame();
    assert_eq!(*order.lock().expect("order lock"), vec![1, 2, 3]);
}

#[test]
fn nested_input_is_queued_and_returns_deferred_outcome() {
    let callbacks = Arc::new(WindowCallbacks::new());
    let weak_callbacks = Arc::downgrade(&callbacks);
    let repeats = Arc::new(Mutex::new(Vec::new()));
    let callback_repeats = Arc::clone(&repeats);
    *callbacks.on_input.lock() = Some(Box::new(move |event| {
        let repeat = event.as_keyboard().expect("keyboard input").repeat;
        callback_repeats.lock().expect("repeat lock").push(repeat);
        if !repeat {
            let nested = weak_callbacks
                .upgrade()
                .expect("callbacks alive")
                .dispatch_input(keyboard_event(true));
            assert_eq!(nested, DispatchEventResult::DEFERRED);
            assert!(nested.is_deferred());
        }
        DispatchEventResult::resolved(false, true)
    }));

    let outer = callbacks.dispatch_input(keyboard_event(false));
    assert_eq!(outer, DispatchEventResult::resolved(false, true));
    assert_eq!(*repeats.lock().expect("repeat lock"), vec![false, true]);
}

#[test]
fn nested_cross_kind_events_keep_one_window_causal_order() {
    let callbacks = Arc::new(WindowCallbacks::new());
    let weak_for_input = Arc::downgrade(&callbacks);
    let weak_for_frame = Arc::downgrade(&callbacks);
    let order = Arc::new(Mutex::new(Vec::new()));

    let input_order = Arc::clone(&order);
    *callbacks.on_input.lock() = Some(Box::new(move |_| {
        input_order.lock().expect("order lock").push("input:start");
        let callbacks = weak_for_input.upgrade().expect("callbacks alive");
        callbacks.dispatch_resize(Size::new(px(200.0), px(80.0)), 2.0);
        callbacks.dispatch_request_frame();
        input_order.lock().expect("order lock").push("input:end");
        DispatchEventResult::default()
    }));

    let resize_order = Arc::clone(&order);
    *callbacks.on_resize.lock() = Some(Box::new(move |_, _| {
        resize_order.lock().expect("order lock").push("resize");
        weak_for_frame
            .upgrade()
            .expect("callbacks alive")
            .dispatch_request_frame();
    }));

    let frame_order = Arc::clone(&order);
    *callbacks.on_request_frame.lock() = Some(Box::new(move || {
        frame_order.lock().expect("order lock").push("frame");
    }));

    callbacks.dispatch_input(keyboard_event(false));
    assert_eq!(
        *order.lock().expect("order lock"),
        vec!["input:start", "input:end", "resize", "frame", "frame"]
    );
}

#[test]
fn nested_resize_is_drained_after_outer_callback_returns() {
    let callbacks = Arc::new(WindowCallbacks::new());
    let weak_callbacks = Arc::downgrade(&callbacks);
    let widths = Arc::new(Mutex::new(Vec::new()));
    let callback_widths = Arc::clone(&widths);
    *callbacks.on_resize.lock() = Some(Box::new(move |size, scale| {
        callback_widths
            .lock()
            .expect("width lock")
            .push(size.width.0);
        if size.width.0 == 100.0 {
            weak_callbacks
                .upgrade()
                .expect("callbacks alive")
                .dispatch_resize(Size::new(px(200.0), px(80.0)), scale);
            callback_widths.lock().expect("width lock").push(150.0);
        }
    }));

    callbacks.dispatch_resize(Size::new(px(100.0), px(80.0)), 2.0);
    assert_eq!(
        *widths.lock().expect("width lock"),
        vec![100.0, 150.0, 200.0]
    );
}

#[test]
fn nested_should_close_is_conservative_veto_without_recursion() {
    let callbacks = Arc::new(WindowCallbacks::new());
    let weak_callbacks = Arc::downgrade(&callbacks);
    let calls = Arc::new(AtomicUsize::new(0));
    let callback_calls = Arc::clone(&calls);
    *callbacks.on_should_close.lock() = Some(Box::new(move || {
        callback_calls.fetch_add(1, Ordering::SeqCst);
        let nested = weak_callbacks
            .upgrade()
            .expect("callbacks alive")
            .dispatch_should_close();
        assert!(!nested, "nested close query must conservatively veto");
        true
    }));

    assert!(callbacks.dispatch_should_close());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
