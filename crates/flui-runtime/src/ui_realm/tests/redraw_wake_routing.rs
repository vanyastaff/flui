use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};

use super::*;

fn counting_window(id: u64) -> (Arc<dyn PlatformWindow>, Arc<AtomicU32>) {
    let window = crate::testing::TestWindow::new().with_id(id);
    let redraw_calls = window.redraw_calls_handle();
    (Arc::new(window), redraw_calls)
}

/// A redraw request that dirties presentation A's own pipeline must
/// poke ONLY A's native window — never B's — even though both
/// presentations share this realm's platform wake capability.
///
/// If reverted (`PresentationState::new`'s `on_need_visual_update`
/// wiring calling only `capabilities.wake` with no per-window poke,
/// the pre-addressed-routing shape): this fails by observing A's own
/// counter stay at zero — `capabilities.wake` in this test is a
/// no-op closure, so nothing would poke ANY window at all.
#[test]
fn redraw_request_from_a_does_not_wake_bs_window() {
    let (window_a, calls_a) = counting_window(1);
    let (window_b, calls_b) = counting_window(2);

    // `PresentationState` keeps only a `Weak` ref to its window (the
    // platform owns the strong `Arc` in production, e.g. via its own
    // per-window callback closures) -- these two local bindings are
    // what keep each window alive for this test, exactly like
    // `presentation.rs`'s own `perform_haptic_feedback_*` tests do;
    // pass clones into the realm, never the only strong reference.
    let mut realm = UiRealm::new(
        noop_wake(),
        Arc::clone(&window_a),
        1.0,
        Arc::new(AtomicBool::new(false)),
        crate::presentation::test_clipboard(),
    )
    .expect("realm constructs");
    let a_id = realm.presentation_id();
    let presentation_b = realm.assemble_presentation(Arc::clone(&window_b));
    let b_id = realm.install_presentation(presentation_b);

    assert_eq!(calls_a.load(AtomicOrdering::Relaxed), 0);
    assert_eq!(calls_b.load(AtomicOrdering::Relaxed), 0);

    realm
        .presentations
        .get(a_id)
        .expect("A installed")
        .pipeline()
        .with(flui_rendering::pipeline::PipelineOwner::request_visual_update);

    assert_eq!(
        calls_a.load(AtomicOrdering::Relaxed),
        1,
        "dirtying A's own pipeline must poke A's own window"
    );
    assert_eq!(
        calls_b.load(AtomicOrdering::Relaxed),
        0,
        "dirtying A must never poke B's window"
    );

    // The reverse direction, for symmetry: dirtying B pokes B only.
    realm
        .presentations
        .get(b_id)
        .expect("B installed")
        .pipeline()
        .with(flui_rendering::pipeline::PipelineOwner::request_visual_update);

    assert_eq!(
        calls_a.load(AtomicOrdering::Relaxed),
        1,
        "dirtying B must never poke A's window"
    );
    assert_eq!(
        calls_b.load(AtomicOrdering::Relaxed),
        1,
        "dirtying B's own pipeline must poke B's own window"
    );
}

/// The same edge, fired from a foreign thread — the shape an async
/// task completing on a background executor actually takes.
///
/// The wake handed to a realm is `Send + Sync` precisely so this is
/// legal; this pins that the scheduler seam preserves it.
#[test]
fn a_cross_thread_frame_request_reaches_the_realms_platform_wake() {
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

/// The gating half of the unification: a pipeline visual update issued
/// while the realm's own scheduler is mid-frame on the driving thread
/// must NOT immediately poke the native window — the in-flight frame's
/// surplus-frame guard, not a second `request_redraw`, is what picks up
/// demand that lands after this frame's pipeline slot has opened.
///
/// If reverted (the closure poking `request_redraw` unconditionally,
/// as the pre-unified carrier did): this observes the window's redraw
/// counter advance from inside the frame itself.
#[test]
fn a_pipeline_visual_update_during_the_realm_frame_does_not_redraw_immediately() {
    let (window_a, calls_a) = counting_window(1);
    let realm = UiRealm::new(
        noop_wake(),
        Arc::clone(&window_a),
        1.0,
        Arc::new(AtomicBool::new(false)),
        crate::presentation::test_clipboard(),
    )
    .expect("realm constructs");
    let a_id = realm.presentation_id();

    // Drive a full frame whose pipeline slot issues a visual update on
    // the driving thread. The scheduler is in PersistentCallbacks here
    // (the pipeline's slot), so the phase gate must no-op the poke.
    realm.scheduler().drive_frame_with_lane(
        flui_scheduler::Instant::now(),
        flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
        || {
            assert_eq!(
                realm.scheduler().phase(),
                flui_scheduler::SchedulerPhase::PersistentCallbacks,
                "precondition: the pipeline slot runs mid-frame"
            );
            realm
                .presentations
                .get(a_id)
                .expect("primary installed")
                .pipeline()
                .with(flui_rendering::pipeline::PipelineOwner::request_visual_update);
        },
        realm.local_post_frame_lane(),
    );

    assert_eq!(
        calls_a.load(AtomicOrdering::Relaxed),
        0,
        "a mid-frame pipeline visual update must not reach \
         request_redraw — the phase gate drops it"
    );
}
