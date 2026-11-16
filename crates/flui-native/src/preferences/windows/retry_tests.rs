//! Native idle-loop proof with a private failing getter; no OS settings mutation.

use super::*;
use crate::WindowsPlatform;
use crate::{PlatformError, platform::Platform};
use parking_lot::Mutex;
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::WM_QUIT;

#[test]
fn native_setting_message_replaces_the_cached_observation() {
    let platform = WindowsPlatform::try_new().expect("native platform");
    let gate = platform.owner().owner_control.gate();
    let shares = gate.shares("notification fixture").expect("owner");
    let (source, receiver) = (kept(shares.preferences), shares.broadcast_receiver);
    let actual = source.sample().expect("OS observation");
    let expected = actual.text_scale().expect("Windows scale");
    let stale = actual
        .with_text_scale(expected + 1.0)
        .expect("distinct scale");
    source.invalidate_for_retry_test();
    source
        .sample_with(|| Ok(stale))
        .expect("seed distinguishable cache");
    assert_eq!(
        source.sample().expect("cached observation").text_scale(),
        Some(expected + 1.0)
    );
    let observed = Arc::new(Mutex::new(None));
    let output = Arc::clone(&observed);
    let (finish, finished) = std::sync::mpsc::channel();
    let (watchdog_tx, watchdog_rx) = std::sync::mpsc::channel();
    Box::new(platform)
        .run(crate::owner::app_delivery::testing::on_ready(
            move |owner| {
                let quit = owner.proxy();
                let timeout = quit.clone();
                owner.on_wake(Box::new(move || {
                    let source = gate
                        .shares("notification observation")
                        .expect("owner")
                        .preferences;
                    let value = source.sample().expect("native refresh").text_scale();
                    *output.lock() = value;
                    quit.request_quit().expect("observed notification");
                }))?;
                watchdog_tx
                    .send(std::thread::spawn(move || {
                        if finished
                            .recv_timeout(std::time::Duration::from_secs(2))
                            .is_err()
                        {
                            let _ = timeout.request_quit();
                        }
                    }))
                    .expect("watchdog handle");
                // Hook registration saw a clean source. Only the receiver message can
                // invalidate the distinguishing cache and arrange an owner callback.
                crate::owner::windows::broadcast::send_for_test(
                    receiver,
                    windows::Win32::UI::WindowsAndMessaging::WM_SETTINGCHANGE,
                    0,
                    0,
                );
                Ok(())
            },
        ))
        .expect("notification loop");
    let _ = finish.send(());
    watchdog_rx
        .recv()
        .expect("watchdog")
        .join()
        .expect("watchdog finished");
    assert_eq!(
        *observed.lock(),
        Some(expected),
        "native message failed to refresh the cached observation"
    );
}

#[test]
fn failed_native_posts_still_deliver_owner_turns_and_quit() {
    crate::table_test::run_table(
        "failed_native_posts_still_deliver_owner_turns_and_quit",
        &[
            (
                "a_refused_wake_falls_back_on_the_owner_event",
                a_refused_wake_falls_back_on_the_owner_event as fn(),
            ),
            (
                "a_full_queue_loses_no_window_work_and_no_exit",
                a_full_queue_loses_no_window_work_and_no_exit as fn(),
            ),
        ],
    );
}

/// With the thread's queue full, every post a window makes to itself is
/// refused: a burst of frame requests on a hidden window from another thread,
/// a delivery another thread raises for it, and, once the last window
/// closes, the exit policy's request. Each still runs, from the owner's turn,
/// and the burst is one frame.
fn a_full_queue_loses_no_window_work_and_no_exit() {
    use crate::owner::windows::control::testing::FullQueue;
    use flui_testing::window_commands::AdmitAndApply as _;
    use flui_testing::window_handler::RecordingWindowHandler;

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
                    flui_platform::WindowOptions::default()
                        .title("refused posts")
                        .visible(false),
                    handler,
                )
                .expect("the window opens")
                .try_ready()
                .expect("the window opens at once");
            let surface = flui_platform::PlatformWindow::surface_target(window.as_ref());
            let raw_window_handle::RawWindowHandle::Win32(handle) =
                raw_window_handle::HasWindowHandle::window_handle(&*surface)
                    .expect("a live Win32 window has a handle")
                    .as_raw()
            else {
                panic!("not a Win32 window");
            };
            let hwnd = HWND(handle.hwnd.get() as *mut std::ffi::c_void);
            let delivery = crate::window::windows::proc::with_window_context(
                hwnd,
                "refused posts",
                |context| Arc::clone(&context.delivery),
            )
            .expect("the window's context");
            let requester = window.redraw_requester();
            let full = FullQueue::fill();
            std::thread::spawn(move || {
                for _ in 0..1000 {
                    requester.request_redraw();
                }
                delivery.diagnose(move || heard.store(true, Ordering::SeqCst));
            })
            .join()
            .expect("the requesting thread");
            drop(full);
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
                    window.close_now();
                }
            }))?;
            worker_tx
                .send(std::thread::spawn(move || {
                    // Time for what the owner's turn runs, and for any frame
                    // past the one the burst asked for.
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    asking.store(true, Ordering::SeqCst);
                    proxy.wake().expect("the owner takes a wake");
                    if finished
                        .recv_timeout(std::time::Duration::from_secs(3))
                        .is_err()
                    {
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
        "the last window closed and the loop did not end: the exit policy was lost with its post"
    );
}

fn a_refused_wake_falls_back_on_the_owner_event() {
    let platform = WindowsPlatform::try_new().expect("native platform");
    platform
        .preferences()
        .expect("clean source before registration");
    platform
        .owner()
        .owner_control
        .wake
        .fail_posts
        .store(true, Ordering::Release);
    let turns = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&turns);
    let (finish, finished) = std::sync::mpsc::channel();
    let (worker_tx, worker_rx) = std::sync::mpsc::channel();
    Box::new(platform)
        .run(crate::owner::app_delivery::testing::on_ready(
            move |owner| {
                let proxy = owner.proxy();
                let next = proxy.clone();
                owner.on_wake(Box::new(move || {
                    if observed.fetch_add(1, Ordering::AcqRel) == 0 {
                        // Reentrant admission must survive the active callback and
                        // another failed post when its continuation is scheduled.
                        next.wake().expect("reentrant fallback admission");
                    } else {
                        next.request_quit().expect("quit through fallback");
                    }
                }))?;
                // SAFETY: value-only identity of this native message-loop thread.
                let thread = unsafe { windows::Win32::System::Threading::GetCurrentThreadId() };
                worker_tx
                    .send(std::thread::spawn(move || {
                        proxy.wake().expect("worker fallback admission");
                        if finished
                            .recv_timeout(std::time::Duration::from_secs(2))
                            .is_err()
                        {
                            // SAFETY: this test-created loop has an initialized message
                            // queue. The pointer-free watchdog message only ends that loop.
                            unsafe {
                                windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                                    thread,
                                    WM_QUIT,
                                    WPARAM(0),
                                    LPARAM(0),
                                )
                                .expect("watchdog quit");
                            }
                        }
                    }))
                    .expect("worker handle");
                Ok(())
            },
        ))
        .expect("native fallback loop");
    let _ = finish.send(());
    worker_rx
        .recv()
        .expect("worker")
        .join()
        .expect("worker completed");
    assert_eq!(
        turns.load(Ordering::Acquire),
        2,
        "accepted owner work was stranded after native post failure"
    );
}

#[test]
fn preference_failure_retries_without_a_user_window() {
    crate::table_test::run_table(
        "preference_failure_retries_without_a_user_window",
        &[
            ("repeated_errors_recover_and_stop_retrying", || {
                recovery(false, true);
            }),
            ("panic_recovers_and_stops_retrying", || {
                recovery(true, false);
            }),
        ],
    );
}

fn recovery(panics: bool, repeat_failure: bool) {
    let platform = WindowsPlatform::try_new().expect("native platform");
    let gate = platform.owner().owner_control.gate();
    let source = kept(gate.shares("retry fixture").expect("owner").preferences);
    source.sample().expect("initial observation");
    let recovered = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&recovered);
    let calls = Arc::new(AtomicUsize::new(0));
    let called = Arc::clone(&calls);
    let (finished, finish) = std::sync::mpsc::channel();
    let (healthy, health) = std::sync::mpsc::channel();
    let (watchdog_tx, watchdog_rx) = std::sync::mpsc::channel();
    Box::new(platform)
        .run(crate::owner::app_delivery::testing::on_ready(
            move |owner| {
                let watchdog_quit = owner.proxy();
                watchdog_tx
                    .send(std::thread::spawn(move || {
                        if health
                            .recv_timeout(std::time::Duration::from_secs(2))
                            .is_ok()
                        {
                            // Leave more than two retry intervals idle after success.
                            // A source that forgot to disarm produces extra callbacks.
                            let _ = finish.recv_timeout(std::time::Duration::from_millis(250));
                        }
                        let _ = watchdog_quit.request_quit();
                    }))
                    .expect("watchdog handle");
                owner.on_wake(Box::new(move || {
                    let source = kept(
                        gate.shares("retry callback")
                            .expect("live owner")
                            .preferences,
                    );
                    let call = called.fetch_add(1, Ordering::AcqRel);
                    if repeat_failure && call == 0 {
                        assert!(
                            source
                                .sample_with(|| Err(PlatformError::Preferences {
                                    message: "repeated getter failure".into(),
                                }))
                                .is_err()
                        );
                        return;
                    }
                    source.sample().expect("native read recovers");
                    observed.store(true, Ordering::Release);
                    let _ = healthy.send(());
                }))?;
                // Registration saw a clean source. Only a failed later native read can
                // make this owner wake; no input, user window, or redraw source exists.
                // Inject a pending read without the notification wake being tested
                // elsewhere, so only this read's failure can arrange a retry.
                source.invalidate_for_retry_test();
                if panics {
                    assert!(
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            source.sample_with(|| panic!("injected native getter panic"))
                        }))
                        .is_err()
                    );
                } else {
                    assert!(
                        source
                            .sample_with(|| Err(PlatformError::Preferences {
                                message: "injected getter failure".into(),
                            }))
                            .is_err()
                    );
                }
                Ok(())
            },
        ))
        .expect("native loop");
    let _ = finished.send(());
    watchdog_rx
        .recv()
        .expect("watchdog")
        .join()
        .expect("watchdog completed");
    assert!(
        recovered.load(Ordering::Acquire),
        "failed native read remained stranded without a user window"
    );
    assert_eq!(
        calls.load(Ordering::Acquire),
        if repeat_failure { 2 } else { 1 },
        "healthy source continued waking the idle owner"
    );
}
