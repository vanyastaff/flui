use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};

use super::*;

fn counting_window(id: u64) -> (Arc<dyn PlatformWindow>, Arc<AtomicU32>) {
    let window = crate::testing::TestWindow::new().with_id(id);
    let redraw_calls = window.redraw_calls_handle();
    (Arc::new(window), redraw_calls)
}

/// The same edge, fired from a foreign thread — the shape an async
/// task completing on a background executor actually takes.
///
/// The wake handed to a realm is `Send + Sync` precisely so this is
/// legal; this pins that the scheduler seam preserves it.
pub(crate) fn a_cross_thread_frame_request_reaches_the_realms_platform_wake() {
    let wakes = Arc::new(AtomicU32::new(0));
    let wake_counter = Arc::clone(&wakes);
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        wake_counter.fetch_add(1, AtomicOrdering::Relaxed);
    });

    let (window, _calls) = counting_window(1);
    let realm = UiRealm::new(
        wake,
        window,
        1.0,
        Arc::new(AtomicBool::new(false)),
        crate::presentation::test_clipboard(),
        &flui_painting::FontCollection::new(),
        flui_scheduler::ClockSource::Platform,
    )
    .expect("realm constructs");
    // Clear the `frame_scheduled` latch so the request below is a real
    // false->true transition — the only edge the hook fires on.
    realm.scheduler().finish_async_pump();
    let before = wakes.load(AtomicOrdering::Relaxed);

    let scheduler = realm.scheduler().clone();
    std::thread::spawn(move || scheduler.request_frame())
        .join()
        .expect("waker thread completes");

    assert!(
        wakes.load(AtomicOrdering::Relaxed) > before,
        "a frame request raised off the owner thread must still reach the wake"
    );
}
