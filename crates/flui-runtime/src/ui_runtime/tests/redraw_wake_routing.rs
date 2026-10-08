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
/// The wake handed to a UI runtime is `Send + Sync` precisely so this is
/// legal; this pins that the scheduler seam preserves it.
pub(crate) fn a_cross_thread_frame_request_reaches_the_ui_runtimes_platform_wake() {
    let wakes = Arc::new(AtomicU32::new(0));
    let wake_counter = Arc::clone(&wakes);
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        wake_counter.fetch_add(1, AtomicOrdering::Relaxed);
    });

    let (window, _calls) = counting_window(1);
    let ui_runtime = UiRuntime::new(
        window,
        1.0,
        crate::runtime_services::RuntimeHostServices::new(
            wake,
            Arc::new(AtomicBool::new(false)),
            crate::presentation::test_clipboard(),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Platform,
        ),
    )
    .expect("ui_runtime constructs");
    // Clear the `frame_scheduled` latch so the request below is a real
    // false->true transition — the only edge the hook fires on.
    ui_runtime.scheduler().finish_async_pump();
    let before = wakes.load(AtomicOrdering::Relaxed);

    let scheduler = ui_runtime.scheduler().clone();
    std::thread::spawn(move || scheduler.request_frame())
        .join()
        .expect("waker thread completes");

    assert!(
        wakes.load(AtomicOrdering::Relaxed) > before,
        "a frame request raised off the owner thread must still reach the wake"
    );
}

fn counting_ui_runtime(id: u64) -> (UiRuntime, Arc<AtomicU32>) {
    let wakes = Arc::new(AtomicU32::new(0));
    let wake_counter = Arc::clone(&wakes);
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        wake_counter.fetch_add(1, AtomicOrdering::Relaxed);
    });
    let (window, _calls) = counting_window(id);
    let ui_runtime = UiRuntime::new(
        window,
        1.0,
        crate::runtime_services::RuntimeHostServices::new(
            wake,
            Arc::new(AtomicBool::new(false)),
            crate::presentation::test_clipboard(),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Platform,
        ),
    )
    .expect("ui_runtime constructs");
    ui_runtime.scheduler().finish_async_pump();
    (ui_runtime, wakes)
}

/// The cross-thread half of a UI runtime's scheduler: a `FrameWaker` sent to a
/// worker wakes its own UI runtime once per frame demand, never a sibling, and
/// does nothing once its UI runtime is gone.
pub(crate) fn frame_waker_wakes_the_ui_runtime_from_a_worker() {
    let (ui_runtime, wakes) = counting_ui_runtime(1);
    let (sibling, sibling_wakes) = counting_ui_runtime(2);
    let sibling_before = sibling_wakes.load(AtomicOrdering::Relaxed);

    let before = wakes.load(AtomicOrdering::Relaxed);

    let waker = ui_runtime.scheduler().frame_waker();
    let worker_waker = waker.clone();
    std::thread::spawn(move || {
        worker_waker.request_frame();
        worker_waker.request_frame();
    })
    .join()
    .expect("worker completes");
    assert_eq!(
        wakes.load(AtomicOrdering::Relaxed),
        before + 1,
        "two requests before a frame are one demand, one wake"
    );
    assert!(ui_runtime.scheduler().is_frame_scheduled());

    ui_runtime.scheduler().finish_async_pump();
    let worker_waker = waker.clone();
    std::thread::spawn(move || worker_waker.request_frame())
        .join()
        .expect("worker completes");
    assert_eq!(
        wakes.load(AtomicOrdering::Relaxed),
        before + 2,
        "a cleared latch takes the next demand"
    );

    // Cleared, so a waker that kept the scheduler alive would wake it again.
    ui_runtime.scheduler().finish_async_pump();
    drop(ui_runtime);
    std::thread::spawn(move || waker.request_frame())
        .join()
        .expect("a waker outliving its ui_runtime is inert, not a panic");
    assert_eq!(wakes.load(AtomicOrdering::Relaxed), before + 2);
    assert_eq!(
        sibling_wakes.load(AtomicOrdering::Relaxed),
        sibling_before,
        "another ui_runtime's waker never wakes this one"
    );
    drop(sibling);
}
