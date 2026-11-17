//! A delivery another thread raises for a Win32 window reaches its handler in
//! a turn of the window's own message queue, with nothing else to carry it.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use flui_platform::WindowOptions;
use flui_testing::window_commands::AdmitAndApply as _;
use flui_testing::window_handler::{HandlerCall, RecordingWindowHandler};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
};

use super::proc::DELIVER_QUEUED;
use crate::WindowsPlatform;
use crate::owner::windows::control::testing::FullQueue;
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
                .title("delivery from another thread")
                .visible(false),
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

/// The posts of `hwnd` waiting in this thread's queue for `message`, taken
/// out and dispatched: how many there were.
#[expect(
    unsafe_code,
    reason = "takes the test thread's own Win32 messages for one window"
)]
fn take_posts(hwnd: HWND, message: u32) -> usize {
    let mut taken = 0;
    let mut posted = MSG::default();
    // SAFETY: `posted` is a stack-local out-parameter; the messages are this
    // thread's own, for a window it created and still owns.
    unsafe {
        while PeekMessageW(&raw mut posted, Some(hwnd), message, message, PM_REMOVE).as_bool() {
            taken += 1;
            DispatchMessageW(&raw const posted);
        }
    }
    taken
}

/// The window's handle, read the way an embedder reads it.
fn hwnd_of(window: &dyn crate::HostWindow) -> HWND {
    let surface = flui_platform::PlatformWindow::surface_target(window);
    let raw_window_handle::RawWindowHandle::Win32(handle) =
        raw_window_handle::HasWindowHandle::window_handle(&*surface)
            .expect("a live Win32 window has a handle")
            .as_raw()
    else {
        panic!("not a Win32 window");
    };
    HWND(handle.hwnd.get() as *mut std::ffi::c_void)
}

/// A thousand deliveries another thread raises are one `DELIVER_QUEUED`
/// waiting in the queue, and, when the queue is full, one item for the
/// owner's turn.
#[test]
fn a_burst_of_deliveries_from_another_thread_is_one_post_and_one_debt() {
    let platform = WindowsPlatform::try_new().expect("native platform");
    let handler = RecordingWindowHandler::new();
    let window = platform
        .open_window(
            WindowOptions::default()
                .title("a burst of deliveries")
                .visible(false),
            handler.clone(),
        )
        .expect("a hidden window opens");
    pump_until(|| false);
    handler.take_calls();
    let hwnd = hwnd_of(window.as_ref());
    let raised = window.delivery();
    let burst = || {
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    for _ in 0..1000 {
                        raised.dispatch_state_changed();
                    }
                })
                .join()
                .expect("the worker");
        });
    };

    burst();
    assert_eq!(
        take_posts(hwnd, DELIVER_QUEUED),
        1,
        "one post for the burst"
    );
    assert!(
        handler.count(&HandlerCall::StateChanged) > 0,
        "the burst was delivered"
    );

    let signal = Arc::clone(&platform.owner().owner_control.signal);
    {
        let _full = FullQueue::fill();
        burst();
    }
    assert_eq!(
        signal.outside_turn_len_for_test(),
        1,
        "one item for the owner's turn for the refused burst"
    );
    assert_eq!(take_posts(hwnd, DELIVER_QUEUED), 0, "nothing was queued");
    window.close_now();
}

/// With the thread's queue full, every post a window makes to itself is
/// refused: a burst of frame requests on a hidden window from another thread,
/// a delivery another thread raises for it, a close posted for it, and, once
/// the last window closes, the exit policy's request. Each still runs, from
/// the owner's turn, and the burst is one frame.
fn refused_work_runs_from_the_owners_turn_and_the_loop_still_ends(close: Close) {
    let frames = Arc::new(AtomicUsize::new(0));
    let delivered = Arc::new(AtomicBool::new(false));
    let watchdog_fired = Arc::new(AtomicBool::new(false));
    let close_asked = Arc::new(AtomicBool::new(false));
    let (counted, heard, fired, asked, asking) = (
        Arc::clone(&frames),
        Arc::clone(&delivered),
        Arc::clone(&watchdog_fired),
        Arc::clone(&close_asked),
        Arc::clone(&close_asked),
    );
    let (finish, finished) = std::sync::mpsc::channel::<()>();
    let (worker_tx, worker_rx) = std::sync::mpsc::channel();
    Box::new(WindowsPlatform::try_new().expect("native platform"))
        .run(crate::testing::app::with_log(move |log, owner| {
            // The host's own policy: the loop ends with its last window.
            log.answer_exit_policy(crate::platform::ExitAnswer::HostDecides);
            let handler = RecordingWindowHandler::new();
            handler.react_to_frame(move |_| {
                counted.fetch_add(1, Ordering::SeqCst);
            });
            let window = owner
                .open_window(
                    WindowOptions::default()
                        .title("refused posts")
                        .visible(false),
                    handler,
                )
                .expect("the window opens")
                .try_ready()
                .expect("the window opens at once");
            let requester = window.redraw_requester();
            {
                let _full = FullQueue::fill();
                let raised = window.delivery();
                std::thread::scope(|scope| {
                    scope
                        .spawn(move || {
                            for _ in 0..1000 {
                                requester.request_redraw();
                            }
                            raised.diagnose(move || heard.store(true, Ordering::SeqCst));
                        })
                        .join()
                        .expect("the requesting thread");
                });
            }
            let proxy = owner.proxy();
            let watchdog = proxy.clone();
            let window = RefCell::new(Some(window));
            owner.on_wake(Box::new(move || {
                // Once the worker asks, the last window closes while the
                // queue is full again.
                if asked.load(Ordering::SeqCst)
                    && let Some(window) = window.borrow_mut().take()
                {
                    let _full = FullQueue::fill();
                    match close {
                        Close::Command => window.close_now(),
                        Close::Posted => {
                            let hwnd = hwnd_of(window.as_ref());
                            let (queue, identity) =
                                super::proc::with_window_context(hwnd, "close", |context| {
                                    (context.queue.clone(), context.identity)
                                })
                                .expect("the window's context");
                            super::proc::post_close(&queue, hwnd, identity)
                                .expect("the close is owed to the owner's turn");
                        }
                    }
                }
            }))?;
            worker_tx
                .send(std::thread::spawn(move || {
                    // Time for what the owner's turn runs, and for any frame
                    // past the one the burst asked for.
                    std::thread::sleep(Duration::from_millis(300));
                    asking.store(true, Ordering::SeqCst);
                    proxy.wake().expect("the owner takes a wake");
                    if finished.recv_timeout(Duration::from_secs(3)).is_err() {
                        fired.store(true, Ordering::SeqCst);
                        let _ = watchdog.request_quit();
                    }
                }))
                .expect("worker handle");
            Ok(())
        }))
        .expect("the Win32 loop");
    let _ = finish.send(());
    worker_rx
        .recv()
        .expect("worker")
        .join()
        .expect("worker completed");
    assert_eq!(
        frames.load(Ordering::SeqCst),
        1,
        "a thousand refused frame requests on a hidden window are one frame"
    );
    assert!(
        delivered.load(Ordering::SeqCst),
        "a delivery raised on another thread was lost with its post"
    );
    assert!(
        !watchdog_fired.load(Ordering::SeqCst),
        "the last window was to close and the loop did not end: the close or the exit policy was lost with its post"
    );
}

/// How the last window is closed under the full queue.
#[derive(Clone, Copy)]
enum Close {
    /// `WindowCommand::Close` on the owner thread, whose exit-policy post is
    /// refused.
    Command,
    /// The posted close a close the handler granted behind a running method
    /// takes.
    Posted,
}

#[test]
fn window_posts_under_a_full_queue() {
    crate::table_test::run_table(
        "window_posts_under_a_full_queue",
        &[
            ("the_last_window_closed_by_command", || {
                refused_work_runs_from_the_owners_turn_and_the_loop_still_ends(Close::Command);
            }),
            ("the_last_window_closed_by_a_posted_close", || {
                refused_work_runs_from_the_owners_turn_and_the_loop_still_ends(Close::Posted);
            }),
        ],
    );
}
