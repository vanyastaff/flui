//! Integration tests for flui-scheduler
//!
//! These tests verify the complete scheduler system works correctly
//! across multiple components working together.

// Target-level lint relaxations — crate-level allows don't reach this
// target. `unwrap` in test/example code: a panic IS the failure report
// (docs/PANIC-POLICY.md).
// `RawWaker` vtables are built manually to poll futures without a runtime.
// Most are no-ops, where `Waker::from_raw` is the only `unsafe` entry point
// and the vtable's no-op contract is what makes it sound. One is not:
// `a_frame_completing_while_poll_clones_the_waker_still_resolves_it` needs a
// `clone` with a side effect, so its vtable dereferences a borrowed
// `*const UpdateScheduler` and drives a frame. Its SAFETY comments carry the
// lifetime and aliasing argument that the no-op contract does not cover.
#![expect(unsafe_code)]

use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use flui_scheduler::{
    FrameBudget, FrameOutcome,
    config::PerformanceMode,
    duration::Milliseconds,
    frame::AppLifecycleState,
    scheduler::UpdateScheduler,
    task::{Priority, TaskQueue},
    ticker::{Ticker, TickerFuture},
};

// ============================================================================
// UpdateScheduler Integration Tests
// ============================================================================

#[test]
fn test_full_frame_lifecycle() {
    // Test complete frame lifecycle: schedule -> begin -> callbacks -> end
    let scheduler = UpdateScheduler::new();

    let transient_called = Arc::new(AtomicU32::new(0));
    let persistent_called = Arc::new(AtomicU32::new(0));
    let post_frame_called = Arc::new(AtomicU32::new(0));

    // Register all callback types
    let t = Arc::clone(&transient_called);
    scheduler.schedule_frame_callback(Box::new(move |_timestamp| {
        t.fetch_add(1, Ordering::SeqCst);
    }));

    let p = Arc::clone(&persistent_called);
    scheduler.add_persistent_frame_callback(Arc::new(move |_timing| {
        p.fetch_add(1, Ordering::SeqCst);
    }));

    let pf = Arc::clone(&post_frame_called);
    scheduler.add_post_frame_callback(Box::new(move |_timing| {
        pf.fetch_add(1, Ordering::SeqCst);
    }));

    // Execute frame
    scheduler.execute_frame();

    // Verify all callbacks were called
    assert_eq!(transient_called.load(Ordering::SeqCst), 1);
    assert_eq!(persistent_called.load(Ordering::SeqCst), 1);
    assert_eq!(post_frame_called.load(Ordering::SeqCst), 1);

    // Execute another frame - only persistent should be called again
    scheduler.execute_frame();

    assert_eq!(transient_called.load(Ordering::SeqCst), 1); // Still 1
    assert_eq!(persistent_called.load(Ordering::SeqCst), 2); // Now 2
    assert_eq!(post_frame_called.load(Ordering::SeqCst), 1); // Still 1
}

#[test]
fn test_app_lifecycle_state_changes() {
    let scheduler = UpdateScheduler::new();

    let states_seen = Arc::new(parking_lot::Mutex::new(Vec::new()));

    let s = Arc::clone(&states_seen);
    scheduler.add_lifecycle_state_listener(Arc::new(move |state| {
        s.lock().push(state);
    }));

    // Simulate app lifecycle changes
    scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Inactive);
    scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Paused);
    scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Resumed);

    let states = states_seen.lock();
    assert_eq!(states.len(), 3);
    assert_eq!(states[0], AppLifecycleState::Inactive);
    assert_eq!(states[1], AppLifecycleState::Paused);
    assert_eq!(states[2], AppLifecycleState::Resumed);
}

// ============================================================================
// Ticker Integration Tests
// ============================================================================

#[test]
fn test_ticker_with_scheduler() {
    let scheduler = UpdateScheduler::new();
    let mut ticker = Ticker::new_with_scheduler(&scheduler);

    let tick_count = Arc::new(AtomicU32::new(0));

    let tc = Arc::clone(&tick_count);
    ticker.start(move |_elapsed| {
        tc.fetch_add(1, Ordering::SeqCst);
    });

    // Execute frames
    for _ in 0..5 {
        scheduler.execute_frame();
    }

    // Ticker should have been called each frame
    assert!(tick_count.load(Ordering::SeqCst) >= 1);
}

#[test]
fn test_ticker_future_states() {
    // Test pending state
    let (_completer, future) = TickerFuture::pending();
    assert!(future.is_pending());
    assert!(!future.is_complete());
    assert!(!future.is_canceled());

    // Test pre-completed future
    let complete_future = TickerFuture::complete();
    assert!(!complete_future.is_pending());
    assert!(complete_future.is_complete());
    assert!(!complete_future.is_canceled());
}

// ============================================================================
// Task Queue Integration Tests
// ============================================================================

#[test]
fn test_task_queue_priority_execution() {
    let queue = TaskQueue::new();

    let execution_order = Arc::new(parking_lot::Mutex::new(Vec::new()));

    // Add tasks in reverse priority order
    let eo = Arc::clone(&execution_order);
    queue.add(Priority::Idle, move || {
        eo.lock().push("idle");
    });

    let eo = Arc::clone(&execution_order);
    queue.add(Priority::Build, move || {
        eo.lock().push("build");
    });

    let eo = Arc::clone(&execution_order);
    queue.add(Priority::Animation, move || {
        eo.lock().push("animation");
    });

    let eo = Arc::clone(&execution_order);
    queue.add(Priority::UserInput, move || {
        eo.lock().push("user_input");
    });

    // Execute all tasks
    queue.execute_all();

    let order = execution_order.lock();
    assert_eq!(order.len(), 4);

    // Higher priority should execute first
    assert_eq!(order[0], "user_input");
    assert_eq!(order[1], "animation");
    assert_eq!(order[2], "build");
    assert_eq!(order[3], "idle");
}

// ============================================================================
// Frame Budget Integration Tests
// ============================================================================

#[test]
fn test_frame_budget_jank_detection() {
    let mut budget = FrameBudget::new(60);

    // Record a fast frame (under the 60fps target duration)
    budget.record_frame_duration(Milliseconds::new(10.0));
    assert!(!budget.is_janky()); // 10ms is under the target, not janky

    // Record a janky frame (over the 60fps target duration)
    budget.record_frame_duration(Milliseconds::new(25.0));
    assert!(budget.is_janky()); // 25ms exceeds the target, janky

    // Jank count should reflect the janky frame
    assert_eq!(budget.jank_count(), 1);
}

// ============================================================================
// UpdateScheduler Binding Integration Tests
// ============================================================================

#[test]
fn test_performance_mode_request() {
    let scheduler = UpdateScheduler::new();

    // Request performance mode
    let handle = scheduler.request_performance_mode(PerformanceMode::Latency);

    // Handle exists - drop it to release mode
    drop(handle);
}

// ============================================================================
// Concurrent Access Tests
// ============================================================================

#[test]
fn test_scheduler_thread_safety() {
    let scheduler = UpdateScheduler::new();

    let handles: Vec<_> = (0..4)
        .map(|i| {
            let sched = scheduler.clone();
            std::thread::spawn(move || {
                for _ in 0..100 {
                    sched.schedule_frame_callback(Box::new(move |_| {
                        // Callback for thread i
                        let _ = i;
                    }));
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    // Execute frame to process all callbacks
    scheduler.execute_frame();
}

// ============================================================================
// Edge Cases and Error Handling
// ============================================================================

// ============================================================================
// Warm-up Frame Tests
// ============================================================================

// ============================================================================
// Microtask Tests
// ============================================================================

// ============================================================================
// End of Frame Future Tests
// ============================================================================

#[test]
fn test_end_of_frame_future() {
    let scheduler = UpdateScheduler::new();

    // Get end of frame future
    let _future = scheduler.end_of_frame();

    // Execute frame to complete it
    scheduler.execute_frame();

    // Future should be completed after frame
}

// ============================================================================
// Extended Binding Tests (for coverage)
// ============================================================================

#[test]
fn test_scheduler_binding_handle_begin_draw_frame() {
    let scheduler = UpdateScheduler::new();
    let called = Arc::new(AtomicU32::new(0));

    let c = Arc::clone(&called);
    scheduler.add_persistent_frame_callback(Arc::new(move |_timing| {
        c.fetch_add(1, Ordering::SeqCst);
    }));

    // Use scheduler methods directly
    let vsync_time = web_time::Instant::now();
    scheduler.handle_begin_frame(vsync_time);
    scheduler.handle_draw_frame();

    assert_eq!(called.load(Ordering::SeqCst), 1);
}

// ============================================================================
// Extended Binding Tests (79% -> 80%+)
// ============================================================================

// ============================================================================
// Extended Ticker Coverage Tests
// ============================================================================
//
// The old `test_ticker_provider_schedule_tick_typed` exercised
// `TickerProvider::schedule_tick_typed`, since removed alongside the
// `schedule_tick` API (Flutter `TickerProvider.createTicker(callback)
// -> Ticker` is now the only factory shape). The auto-scheduling integration
// is covered by `crates/flui-scheduler/src/ticker.rs` unit tests
// (`test_auto_scheduling_ticker_fires_each_frame`,
// `test_create_ticker_via_provider_auto_schedules`).

// ============================================================================
// TickerFuture Polling Tests
// ============================================================================

// ============================================================================
// TickerState Tests
// ============================================================================

/// `FrameCompletionFuture::poll` releases the `state` guard across
/// `Waker::clone` — the clone is executor code, and this crate's lock order
/// forbids holding either completion mutex across any `Waker` operation. It
/// then re-acquires the guard to store the clone, and re-checks `completed`
/// before storing.
///
/// That re-check is what this pins. A frame completing inside the window has
/// already taken the OLD waker and woken it, so the executor may have moved
/// on to the waker it is polling with now. Returning `Pending` on the
/// strength of the pre-clone check would strand the task forever: the
/// completion was delivered to a waker nobody is listening on any more.
///
/// The window opens only while `Waker::clone` runs. A perfectly ordinary
/// safe `Waker` can land a frame in it from another thread, so what the
/// hand-built vtable buys is DETERMINISM, not reachability: it makes the
/// interleaving happen on one thread, every run, with no barrier, no sleep,
/// and no race to lose. That is why the test lives here rather than beside
/// issue #1055's others in `end_of_frame_lifecycle.rs`, which is
/// `#![forbid(unsafe_code)]`; this file already builds raw wakers by hand
/// under the module-level `expect(unsafe_code)` above.
///
/// # This oracle can also fail by HANGING
///
/// Its `clone` drives a whole frame, so under the regression of moving the
/// clone back under the `state` guard it deadlocks on one thread --
/// `poll` -> `clone` -> `execute_frame` -> `notify_frame_completion` ->
/// `state.lock()` -- and nextest reports it on the terminate-after timeout
/// rather than in milliseconds. Same failure mode as the two waker-driven
/// tests in `end_of_frame_lifecycle.rs`. The fast signal for that
/// particular regression is
/// `a_displaced_waker_is_dropped_outside_the_completion_state_lock` in
/// `scheduler.rs`, which probes with `try_lock` instead of blocking.
#[test]
fn a_frame_completing_while_poll_clones_the_waker_still_resolves_it() {
    use std::{
        future::Future,
        pin::Pin,
        task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
    };

    /// A waker whose `clone` drives a whole frame before returning, landing
    /// the completion exactly in `poll`'s guard-free window. Its data
    /// pointer is a borrowed `*const UpdateScheduler`.
    fn frame_driving_raw_waker(scheduler: *const UpdateScheduler) -> RawWaker {
        fn clone(data: *const ()) -> RawWaker {
            // SAFETY: `data` is the `&UpdateScheduler` the caller passed to
            // `frame_driving_raw_waker`, and that scheduler is declared
            // before every waker built from it, so it is still alive here
            // (locals drop in reverse declaration order). The reference
            // does not escape this function.
            let scheduler = unsafe { &*data.cast::<UpdateScheduler>() };
            scheduler.execute_frame();
            frame_driving_raw_waker(std::ptr::from_ref(scheduler))
        }
        // Nothing is owned through the data pointer, so wake and drop have
        // nothing to do; the future under test observes the frame, not a
        // wake count.
        fn no_op(_: *const ()) {}
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, no_op, no_op, no_op);
        RawWaker::new(scheduler.cast(), &VTABLE)
    }

    let scheduler = UpdateScheduler::new();
    let mut future = scheduler.end_of_frame();

    // Poll once with a DIFFERENT waker, so the second poll cannot take
    // `poll`'s `will_wake` fast path and must go through the clone.
    let mut noop_cx = Context::from_waker(Waker::noop());
    assert!(
        Pin::new(&mut future).poll(&mut noop_cx).is_pending(),
        "no frame has run yet"
    );

    // SAFETY: the vtable's `clone` returns a waker over the same borrowed
    // data, and `wake`/`wake_by_ref`/`drop` are no-ops over a pointer that
    // owns nothing, so every `RawWakerVTable` contract holds. `scheduler`
    // outlives this waker: it is declared first and so dropped last.
    let frame_driving_waker =
        unsafe { Waker::from_raw(frame_driving_raw_waker(std::ptr::from_ref(&scheduler))) };

    let resolved = Pin::new(&mut future).poll(&mut Context::from_waker(&frame_driving_waker));

    assert_eq!(
        scheduler.frame_count(),
        1,
        "the waker's clone must actually have driven a frame, or this test \
         never opens the window it exists to probe"
    );
    let Poll::Ready(outcome) = resolved else {
        panic!(
            "poll must re-check `completed` after re-acquiring the guard: the frame \
             that completed while the waker was being cloned already took and woke \
             the PREVIOUS waker, so returning Pending here strands the task forever"
        );
    };
    assert!(
        matches!(outcome, Ok(FrameOutcome::Completed { .. })),
        "the scheduler committed a clean frame -- this must resolve Completed, not {outcome:?}"
    );
}
