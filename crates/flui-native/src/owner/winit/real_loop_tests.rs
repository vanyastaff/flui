//! A test that drives a REAL winit `EventLoop` over the quiet platform
//! (`testing::quiet_platform`): the loop's own behaviour, with no window.
//! The real-loop tests that open windows are the window feature's
//! (`window::winit`), and those of the Windows preference source over the
//! loop are the preferences' (`preferences::windows`).
//!
//! Declared from `owner/winit.rs` as a sibling of its `tests` module, so this
//! test reads as `owner::winit::real_loop_tests::*`.

use std::{
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use super::WinitApp;
use super::testing::{
    build_test_event_loop, install_control_lane, install_test_owner_signal, quiet_platform,
    wait_for_running,
};

/// A wake that lands while a turn reads what wakes asked of it (an X11
/// window's properties, host notices, the portal's appearance) gets a turn of
/// its own, which reads again. Were it folded into the turn under way, that
/// turn's drive would settle it with the change unread, and the loop would
/// sleep until some unrelated wake.
#[test]
#[cfg_attr(
    target_os = "macos",
    ignore = "winit requires AppKit's event loop on the real main thread; \
              the test harness runs this on an ordinary test thread"
)]
fn a_wake_while_the_loop_reads_gets_a_turn_that_reads_again() {
    let host = Arc::new(quiet_platform());
    let platform = host.shared_owner();
    let event_loop = build_test_event_loop();
    install_test_owner_signal(&platform, &event_loop);
    let signal = platform
        .owner_signal
        .lock()
        .clone()
        .expect("the test installed an owner signal");
    // An application's turn, as a running app has one.
    signal
        .register(Box::new(|| {}))
        .expect("the owner registers its turn");
    let (control, lane) = install_control_lane(&platform, &event_loop);

    // Set by the worker once the loop runs: the next read wakes the owner.
    let armed = Arc::new(AtomicBool::new(false));
    // The owner was woken while a turn read.
    let woke_while_reading = Arc::new(AtomicBool::new(false));
    // A later turn read again.
    let read_again = Arc::new(AtomicBool::new(false));
    *platform.turn_read_hook_for_test.lock() = Some(Box::new({
        let armed = Arc::clone(&armed);
        let woke_while_reading = Arc::clone(&woke_while_reading);
        let read_again = Arc::clone(&read_again);
        let signal = Arc::downgrade(&signal);
        move || {
            if woke_while_reading.load(Ordering::SeqCst) {
                read_again.store(true, Ordering::SeqCst);
            } else if armed.load(Ordering::SeqCst)
                && let Some(signal) = signal.upgrade()
            {
                woke_while_reading.store(true, Ordering::SeqCst);
                signal.wake().expect("the owner accepts a wake");
            }
        }
    }));

    let platform_for_worker = Arc::clone(&platform);
    let worker_signal = Arc::clone(&signal);
    let worker = thread::spawn(move || {
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            wait_for_running(&platform_for_worker);
            armed.store(true, Ordering::SeqCst);
            worker_signal.wake().expect("the owner accepts a wake");
            // Bounded: without a turn of its own the wake is never read.
            let deadline = Instant::now() + Duration::from_secs(5);
            while !read_again.load(Ordering::SeqCst) && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(5));
            }
            assert!(
                woke_while_reading.load(Ordering::SeqCst),
                "the turn the worker woke for never read"
            );
            assert!(
                read_again.load(Ordering::SeqCst),
                "the wake that landed while the loop read got no turn of its own"
            );
        }));
        control.request_quit();
        if let Err(payload) = outcome {
            resume_unwind(payload);
        }
    });

    let mut app = WinitApp::for_test(Arc::clone(&platform), host, lane, None);
    event_loop
        .run_app(&mut app)
        .expect("event loop runs to completion");
    worker.join().expect("worker thread does not panic");
}
