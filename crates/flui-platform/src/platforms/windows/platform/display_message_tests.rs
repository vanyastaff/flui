//! `WM_DISPLAYCHANGE` on the hidden settings receiver: the cached snapshot is
//! sampled again, on the owner's turn and not inside the message.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn display_change_message_invalidates_the_snapshot_and_wakes_the_owner() {
    let platform = WindowsPlatform::new().expect("native platform");
    let displays = Arc::clone(&platform.displays);
    let source = platform
        .owner_control
        .gate()
        .shares("display message fixture")
        .expect("owner")
        .preferences;
    platform.display_snapshot().expect("first sample");
    let sampled = displays.sample_count();
    platform.display_snapshot().expect("cached answer");
    assert_eq!(
        displays.sample_count(),
        sampled,
        "an unchanged snapshot was sampled again"
    );

    let samples_in_message = Arc::new(AtomicUsize::new(usize::MAX));
    let samples_on_turn = Arc::new(AtomicUsize::new(usize::MAX));
    let (finish, finished) = std::sync::mpsc::channel();
    let (watchdog_tx, watchdog_rx) = std::sync::mpsc::channel();
    let (message, turn) = (
        Arc::clone(&samples_in_message),
        Arc::clone(&samples_on_turn),
    );
    let counted = Arc::clone(&displays);
    Box::new(platform)
        .run(Box::new(move |owner| {
            let quit = owner.proxy();
            let timeout = quit.clone();
            let observer = Arc::clone(&counted);
            owner.on_wake(Box::new(move || {
                observer.snapshot().expect("sample on the owner turn");
                turn.store(observer.sample_count(), Ordering::Release);
                quit.request_quit().expect("observed the wake");
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
            source.send_display_change_for_test();
            // SendMessage returned: the handler ran to completion.
            message.store(counted.sample_count(), Ordering::Release);
            Ok(())
        }))
        .expect("display message loop");
    let _ = finish.send(());
    watchdog_rx
        .recv()
        .expect("watchdog")
        .join()
        .expect("watchdog finished");
    assert_eq!(
        samples_in_message.load(Ordering::Acquire),
        sampled,
        "the message handler sampled the displays"
    );
    assert_eq!(
        samples_on_turn.load(Ordering::Acquire),
        sampled + 1,
        "the owner's turn did not sample the invalidated snapshot"
    );
}
