//! A delivery another thread raises for a Win32 window reaches its handler in
//! a turn of the window's own message queue, with nothing else to carry it.

use std::time::{Duration, Instant};

use flui_platform::WindowOptions;
use flui_testing::window_commands::AdmitAndApply as _;
use flui_testing::window_handler::{HandlerCall, RecordingWindowHandler};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
};

use crate::WindowsPlatform;
use crate::platform::Platform;

/// Dispatch this thread's messages until `done`, for at most two seconds.
#[expect(
    unsafe_code,
    reason = "pumps the test thread's own Win32 message queue"
)]
fn pump_until(done: impl Fn() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        let mut message = MSG::default();
        // SAFETY: `message` is a stack-local out-parameter, and the messages
        // are this thread's own, for windows it created and still owns.
        unsafe {
            while PeekMessageW(&raw mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&raw const message);
                DispatchMessageW(&raw const message);
            }
        }
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    done()
}

#[test]
fn a_delivery_raised_on_another_thread_reaches_the_handler_with_no_other_input() {
    let platform = WindowsPlatform::try_new().expect("native platform");
    let handler = RecordingWindowHandler::new();
    let window = platform
        .open_window(
            WindowOptions::default()
                .with_title("delivery from another thread")
                .with_visible(false),
            handler.clone(),
        )
        .expect("a hidden window opens");
    pump_until(|| false);
    handler.take_calls();

    let raised = window.delivery();
    std::thread::scope(|scope| {
        scope
            .spawn(|| raised.dispatch_state_changed())
            .join()
            .expect("the worker");
    });
    assert!(
        handler.calls().is_empty(),
        "nothing ran on the worker: {:?}",
        handler.calls()
    );
    assert!(
        pump_until(|| handler.count(&HandlerCall::StateChanged) == 1),
        "the state change never reached the idle window's handler"
    );
    window.close_now();
}
