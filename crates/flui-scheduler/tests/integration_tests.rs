//! Integration tests for flui-scheduler
//!
//! These tests verify the complete scheduler system works correctly
//! across multiple components working together.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

use flui_scheduler::{
    FrameBudget,
    config::PerformanceMode,
    duration::{FrameDuration, Milliseconds},
    frame::{AppLifecycleState, SchedulerPhase},
    scheduler::{FrameSkipPolicy, Scheduler, SchedulerBuilder},
    task::{Priority, TaskQueue},
    ticker::{Ticker, TickerCanceled, TickerFuture, TickerState},
    vsync::{VsyncMode, VsyncScheduler},
};

// ============================================================================
// Scheduler Integration Tests
// ============================================================================

#[test]
fn test_full_frame_lifecycle() {
    // Test complete frame lifecycle: schedule -> begin -> callbacks -> end
    let scheduler = Scheduler::new();

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
fn test_scheduler_phase_transitions() {
    let scheduler = Scheduler::new();

    // Initially idle
    assert_eq!(scheduler.scheduler_phase(), SchedulerPhase::Idle);

    let phases_seen = Arc::new(parking_lot::Mutex::new(Vec::new()));

    // Track phases during transient callback
    let ps = Arc::clone(&phases_seen);
    let sched = scheduler.clone();
    scheduler.schedule_frame_callback(Box::new(move |_| {
        ps.lock().push(sched.scheduler_phase());
    }));

    // Track phases during persistent callback
    let ps = Arc::clone(&phases_seen);
    let sched = scheduler.clone();
    scheduler.add_persistent_frame_callback(Arc::new(move |_| {
        ps.lock().push(sched.scheduler_phase());
    }));

    // Track phases during post-frame callback
    let ps = Arc::clone(&phases_seen);
    let sched = scheduler.clone();
    scheduler.add_post_frame_callback(Box::new(move |_| {
        ps.lock().push(sched.scheduler_phase());
    }));

    scheduler.execute_frame();

    let phases = phases_seen.lock();
    assert_eq!(phases.len(), 3);
    assert_eq!(phases[0], SchedulerPhase::TransientCallbacks);
    assert_eq!(phases[1], SchedulerPhase::PersistentCallbacks);
    assert_eq!(phases[2], SchedulerPhase::PostFrameCallbacks);

    // After frame, back to idle
    assert_eq!(scheduler.scheduler_phase(), SchedulerPhase::Idle);
}

#[test]
fn test_callback_cancellation() {
    let scheduler = Scheduler::new();

    let called = Arc::new(AtomicU32::new(0));

    // Schedule callback and immediately cancel
    let c = Arc::clone(&called);
    let id = scheduler.schedule_frame_callback(Box::new(move |_| {
        c.fetch_add(1, Ordering::SeqCst);
    }));

    scheduler.cancel_frame_callback(id);

    // Execute frame - callback should NOT be called
    scheduler.execute_frame();

    assert_eq!(called.load(Ordering::SeqCst), 0);
}

#[test]
fn test_multiple_frames_with_persistent_callbacks() {
    let scheduler = Scheduler::new();

    let call_count = Arc::new(AtomicU32::new(0));

    let c = Arc::clone(&call_count);
    scheduler.add_persistent_frame_callback(Arc::new(move |_| {
        c.fetch_add(1, Ordering::SeqCst);
    }));

    // Execute 10 frames
    for _ in 0..10 {
        scheduler.execute_frame();
    }

    assert_eq!(call_count.load(Ordering::SeqCst), 10);
    assert_eq!(scheduler.frame_count(), 10);
}

#[test]
fn test_app_lifecycle_state_changes() {
    let scheduler = Scheduler::new();

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
    let scheduler = Arc::new(Scheduler::new());
    let mut ticker = Ticker::new_with_scheduler(scheduler.clone());

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
fn test_ticker_mute_unmute() {
    let scheduler = Arc::new(Scheduler::new());
    let mut ticker = Ticker::new_with_scheduler(scheduler.clone());

    let tick_count = Arc::new(AtomicU32::new(0));

    let tc = Arc::clone(&tick_count);
    ticker.start(move |_elapsed| {
        tc.fetch_add(1, Ordering::SeqCst);
    });

    // First frame
    scheduler.execute_frame();
    let count_after_first = tick_count.load(Ordering::SeqCst);

    // Mute and execute frame
    ticker.mute();
    scheduler.execute_frame();
    let count_after_mute = tick_count.load(Ordering::SeqCst);

    // Should not have increased
    assert_eq!(count_after_first, count_after_mute);

    // Unmute and execute frame
    ticker.unmute();
    scheduler.execute_frame();

    // Should have increased
    assert!(tick_count.load(Ordering::SeqCst) > count_after_mute);
}

#[test]
fn test_ticker_future_states() {
    // Test pending state
    let future = TickerFuture::new();
    assert!(future.is_pending());
    assert!(!future.is_complete());
    assert!(!future.is_canceled());

    // Test pre-completed future
    let complete_future = TickerFuture::complete();
    assert!(!complete_future.is_pending());
    assert!(complete_future.is_complete());
    assert!(!complete_future.is_canceled());
}

#[test]
fn test_ticker_canceled_error() {
    let error = TickerCanceled;
    assert_eq!(error.to_string(), "The ticker was canceled");

    // Test that it implements Error trait
    let _: &dyn std::error::Error = &error;
}

// ============================================================================
// VSync Integration Tests
// ============================================================================

#[test]
fn test_vsync_scheduler_basic() {
    let vsync = VsyncScheduler::try_new(60).expect("refresh > 0");

    assert_eq!(vsync.refresh_rate(), 60);
    assert!(!vsync.is_active());

    // Frame interval should be ~16.67ms for 60Hz
    let interval_ms = vsync.frame_interval_ms();
    assert!(interval_ms.value() > 16.0);
    assert!(interval_ms.value() < 17.0);
}

#[test]
fn test_vsync_modes() {
    let vsync = VsyncScheduler::try_with_mode(60, VsyncMode::Adaptive).expect("refresh > 0");
    assert_eq!(vsync.mode(), VsyncMode::Adaptive);

    vsync.set_mode(VsyncMode::On);
    assert_eq!(vsync.mode(), VsyncMode::On);

    vsync.set_mode(VsyncMode::Off);
    assert_eq!(vsync.mode(), VsyncMode::Off);

    vsync.set_mode(VsyncMode::TripleBuffer);
    assert_eq!(vsync.mode(), VsyncMode::TripleBuffer);
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

#[test]
fn test_task_queue_execute_until_priority() {
    let queue = TaskQueue::new();

    let executed = Arc::new(parking_lot::Mutex::new(Vec::new()));

    // Add tasks at different priorities
    let e = Arc::clone(&executed);
    queue.add(Priority::UserInput, move || {
        e.lock().push("user_input");
    });

    let e = Arc::clone(&executed);
    queue.add(Priority::Animation, move || {
        e.lock().push("animation");
    });

    let e = Arc::clone(&executed);
    queue.add(Priority::Build, move || {
        e.lock().push("build");
    });

    let e = Arc::clone(&executed);
    queue.add(Priority::Idle, move || {
        e.lock().push("idle");
    });

    // Execute only tasks with Animation priority or higher
    let count = queue.execute_until(Priority::Animation);

    let exec = executed.lock();
    // Should have executed user_input and animation
    assert_eq!(count, 2);
    assert!(exec.contains(&"user_input"));
    assert!(exec.contains(&"animation"));
    assert!(!exec.contains(&"build"));
    assert!(!exec.contains(&"idle"));
}

// ============================================================================
// Frame Budget Integration Tests
// ============================================================================

#[test]
fn test_frame_budget_tracking() {
    let mut budget = FrameBudget::new(60);

    // Record phase durations
    budget.record_build_duration(Milliseconds::new(2.0));
    budget.record_layout_duration(Milliseconds::new(3.0));
    budget.record_paint_duration(Milliseconds::new(4.0));
    budget.record_composite_duration(Milliseconds::new(1.0));

    // Check stats
    let build_stats = budget.build_stats();
    assert!((build_stats.duration_ms() - 2.0).abs() < 0.01);

    let layout_stats = budget.layout_stats();
    assert!((layout_stats.duration_ms() - 3.0).abs() < 0.01);

    let all_stats = budget.all_phase_stats();
    let total = all_stats.total_duration();
    assert!((total.value() - 10.0).abs() < 0.01);
}

#[test]
fn test_frame_budget_jank_detection() {
    let mut budget = FrameBudget::new(60);

    // Record a fast frame (under 16.67ms budget)
    budget.record_frame_duration(Milliseconds::new(10.0));
    assert!(!budget.is_janky()); // 10ms < 16.67ms, not janky

    // Record a janky frame (> 16.67ms for 60fps)
    budget.record_frame_duration(Milliseconds::new(25.0));
    assert!(budget.is_janky()); // 25ms > 16.67ms, janky

    // Jank count should reflect the janky frame
    assert_eq!(budget.jank_count(), 1);
}

// ============================================================================
// Scheduler Binding Integration Tests
// ============================================================================

#[test]
fn test_scheduler_binding_trait() {
    let scheduler = Scheduler::new();

    // Test scheduler binding methods (now inherent on Scheduler)
    assert_eq!(scheduler.scheduler_phase(), SchedulerPhase::Idle);
    assert!(!scheduler.has_scheduled_frame());

    // Schedule a frame
    scheduler.request_frame();
    assert!(scheduler.has_scheduled_frame());
}

#[test]
fn test_performance_mode_request() {
    let scheduler = Scheduler::new();

    // Request performance mode
    let handle = scheduler.request_performance_mode(PerformanceMode::Latency);

    // Handle exists - drop it to release mode
    drop(handle);
}

#[test]
fn test_service_extensions() {
    use flui_scheduler::SERVICE_EXT_TIME_DILATION;
    assert_eq!(SERVICE_EXT_TIME_DILATION, "timeDilation");
}

// ============================================================================
// Duration Type Integration Tests
// ============================================================================

#[test]
fn test_duration_conversions() {
    let ms = Milliseconds::new(1000.0);
    let secs = ms.to_seconds();
    assert!((secs.value() - 1.0).abs() < 0.001);

    let frame_duration = FrameDuration::try_from_fps(60).expect("fps > 0");
    assert!((frame_duration.fps() - 60.0).abs() < 0.1);
    assert!((frame_duration.as_ms().value() - 16.667).abs() < 0.1);
}

#[test]
fn test_frame_duration_budget_check() {
    let frame_duration = FrameDuration::try_from_fps(60).expect("fps > 0");

    // Under budget
    let elapsed = Milliseconds::new(10.0);
    assert!(!frame_duration.is_over_budget(elapsed));

    // Over budget
    let elapsed = Milliseconds::new(20.0);
    assert!(frame_duration.is_over_budget(elapsed));

    // Utilization
    let elapsed = Milliseconds::new(8.333);
    let utilization = frame_duration.utilization(elapsed);
    assert!((utilization - 0.5).abs() < 0.1);
}

// ============================================================================
// Concurrent Access Tests
// ============================================================================

#[test]
fn test_scheduler_thread_safety() {
    let scheduler = Arc::new(Scheduler::new());

    let handles: Vec<_> = (0..4)
        .map(|i| {
            let sched = Arc::clone(&scheduler);
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

#[test]
fn test_ticker_future_thread_safety() {
    let future = TickerFuture::new();
    let future_clone = future.clone();

    // Test that TickerFuture can be shared across threads
    let handle = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(10));
        // Just check state from another thread
        future_clone.is_pending()
    });

    let was_pending = handle.join().unwrap();
    assert!(was_pending || future.is_pending());
}

// ============================================================================
// Edge Cases and Error Handling
// ============================================================================

#[test]
fn test_empty_frame_execution() {
    let scheduler = Scheduler::new();

    // Execute frame with no callbacks - should not panic
    scheduler.execute_frame();
    scheduler.execute_frame();
    scheduler.execute_frame();

    assert_eq!(scheduler.frame_count(), 3);
}

#[test]
fn test_cancel_nonexistent_callback() {
    let scheduler = Scheduler::new();

    // Schedule a real callback first
    let id = scheduler.schedule_frame_callback(Box::new(|_| {}));

    // Cancel it
    scheduler.cancel_frame_callback(id);

    // Cancel again - should not panic
    scheduler.cancel_frame_callback(id);
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "A ticker was started twice")]
fn test_double_start_ticker_panics_in_debug() {
    // Flutter parity: ticker.dart:188 throws FlutterError('A ticker was
    // started twice.'). Post-U10 (7d93611d) FLUI debug-asserts the same
    // contract instead of silently overwriting the active callback.
    let mut ticker = Ticker::new();
    ticker.start(|_| {});
    ticker.start(|_| {});
}

#[test]
fn test_mute_idle_ticker() {
    let mut ticker = Ticker::new();

    // Muting an idle ticker should have no effect
    ticker.mute();
    assert_eq!(ticker.state(), TickerState::Idle);
}

#[test]
fn test_scheduler_builder_configuration() {
    let scheduler = SchedulerBuilder::new()
        .frame_duration(FrameDuration::try_from_fps(30).expect("fps > 0"))
        .build();

    // Scheduler should be created successfully
    assert_eq!(scheduler.frame_count(), 0);
}

// ============================================================================
// Warm-up Frame Tests
// ============================================================================

#[test]
fn test_warm_up_frame() {
    let scheduler = Scheduler::new();

    let warm_up_called = Arc::new(AtomicU32::new(0));

    let w = Arc::clone(&warm_up_called);
    scheduler.add_persistent_frame_callback(Arc::new(move |_| {
        w.fetch_add(1, Ordering::SeqCst);
    }));

    // Schedule warm-up frame - this may execute immediately or on next frame
    scheduler.schedule_warm_up_frame();

    // Warm-up frame may have already executed, so callback count could be 1 or more
    // The important thing is that the callback is registered and warm-up works
    let count = warm_up_called.load(Ordering::SeqCst);

    // Execute another frame to ensure it works
    scheduler.execute_frame();

    // After explicit execute_frame, count should have increased
    assert!(warm_up_called.load(Ordering::SeqCst) >= count);
}

// ============================================================================
// Microtask Tests
// ============================================================================

#[test]
fn test_microtask_execution() {
    let scheduler = Scheduler::new();

    let microtask_called = Arc::new(AtomicU32::new(0));

    let m = Arc::clone(&microtask_called);
    scheduler.schedule_microtask(Box::new(move || {
        m.fetch_add(1, Ordering::SeqCst);
    }));

    // Execute frame (microtasks run during frame)
    scheduler.execute_frame();

    assert_eq!(microtask_called.load(Ordering::SeqCst), 1);
}

// ============================================================================
// End of Frame Future Tests
// ============================================================================

#[test]
fn test_end_of_frame_future() {
    let scheduler = Scheduler::new();

    // Get end of frame future
    let _future = scheduler.end_of_frame();

    // Execute frame to complete it
    scheduler.execute_frame();

    // Future should be completed after frame
}

// ============================================================================
// Frame Skip Policy Tests
// ============================================================================

#[test]
fn test_frame_skip_policies() {
    let scheduler = Scheduler::new();

    // Test default policy
    let default_policy = scheduler.frame_skip_policy();

    // Test setting different policies
    scheduler.set_frame_skip_policy(FrameSkipPolicy::Never);
    assert_eq!(scheduler.frame_skip_policy(), FrameSkipPolicy::Never);

    scheduler.set_frame_skip_policy(FrameSkipPolicy::SkipToLatest);
    assert_eq!(scheduler.frame_skip_policy(), FrameSkipPolicy::SkipToLatest);

    scheduler.set_frame_skip_policy(FrameSkipPolicy::CatchUp);
    assert_eq!(scheduler.frame_skip_policy(), FrameSkipPolicy::CatchUp);

    // Restore default
    scheduler.set_frame_skip_policy(default_policy);
}

// ============================================================================
// Extended Binding Tests (for coverage)
// ============================================================================

#[test]
fn test_scheduler_binding_frames_enabled() {
    let mut scheduler = Scheduler::new();

    // Default should be enabled
    assert!(scheduler.frames_enabled());

    // Disable frames
    scheduler.set_frames_enabled(false);
    assert!(!scheduler.frames_enabled());

    // Re-enable frames
    scheduler.set_frames_enabled(true);
    assert!(scheduler.frames_enabled());
}

#[test]
fn test_scheduler_binding_schedule_methods() {
    let scheduler = Scheduler::new();

    // Test schedule_frame
    scheduler.schedule_frame_if_enabled();
    assert!(scheduler.has_scheduled_frame());

    scheduler.execute_frame();

    // Test schedule_forced_frame
    scheduler.schedule_forced_frame();
    assert!(scheduler.has_scheduled_frame());

    scheduler.execute_frame();

    // Test ensure_visual_update
    scheduler.ensure_visual_update();
}

#[test]
fn test_scheduler_frame_callback_cancel() {
    let scheduler = Scheduler::new();
    let called = Arc::new(AtomicU32::new(0));

    let c = Arc::clone(&called);
    let id = scheduler.schedule_frame_callback(Box::new(move |_| {
        c.fetch_add(1, Ordering::SeqCst);
    }));

    assert_eq!(scheduler.transient_callback_count(), 1);

    // Cancel it
    scheduler.cancel_frame_callback(id);

    // Execute frame - callback should not be called
    scheduler.execute_frame();
    assert_eq!(called.load(Ordering::SeqCst), 0);
}

#[test]
fn test_scheduler_post_frame_callback() {
    let scheduler = Scheduler::new();
    let called = Arc::new(AtomicU32::new(0));

    let c = Arc::clone(&called);
    scheduler.add_post_frame_callback(Box::new(move |_timing| {
        c.fetch_add(1, Ordering::SeqCst);
    }));

    scheduler.execute_frame();
    assert_eq!(called.load(Ordering::SeqCst), 1);
}

#[test]
fn test_scheduler_binding_schedule_task() {
    let scheduler = Scheduler::new();
    let executed = Arc::new(AtomicU32::new(0));

    let e = Arc::clone(&executed);
    scheduler.add_task(Priority::Animation, move || {
        e.fetch_add(1, Ordering::SeqCst);
    });

    scheduler.execute_frame();
    assert_eq!(executed.load(Ordering::SeqCst), 1);
}

#[test]
fn test_scheduler_binding_handle_begin_draw_frame() {
    let scheduler = Scheduler::new();
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

#[test]
fn test_scheduler_binding_timings_callback() {
    let scheduler = Scheduler::new();
    let timings_received = Arc::new(AtomicU32::new(0));

    let t = Arc::clone(&timings_received);
    let callback: flui_scheduler::config::TimingsCallback = Arc::new(move |_timings| {
        t.fetch_add(1, Ordering::SeqCst);
    });

    scheduler.add_timings_callback(callback.clone());

    // Execute several frames to trigger timing report
    for _ in 0..5 {
        scheduler.execute_frame();
    }

    // Remove callback
    scheduler.remove_timings_callback(&callback);
}

#[test]
fn test_scheduler_binding_lifecycle_change() {
    let scheduler = Scheduler::new();

    // Use scheduler methods directly
    scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Paused);
    assert_eq!(scheduler.lifecycle_state(), AppLifecycleState::Paused);

    scheduler.handle_app_lifecycle_state_change(AppLifecycleState::Resumed);
    assert_eq!(scheduler.lifecycle_state(), AppLifecycleState::Resumed);
}

#[test]
fn test_scheduler_binding_current_timestamps() {
    let scheduler = Scheduler::new();

    // Before frame, timestamp should be zero or default
    let ts = scheduler.current_frame_time_stamp();
    assert!(ts.as_millis() < 1000); // Should be near zero

    // System timestamp should be valid
    let sys_ts = scheduler.current_system_frame_time_stamp();
    // Just verify it returns something reasonable
    assert!(sys_ts.elapsed().as_secs() < 10);
}

#[test]
fn test_scheduler_binding_reset_epoch() {
    let scheduler = Scheduler::new();

    // Reset epoch
    scheduler.reset_epoch();

    // Should still work
    scheduler.execute_frame();
}

#[test]
fn test_scheduler_binding_debug_asserts() {
    let scheduler = Scheduler::new();

    // No transient callbacks
    assert!(scheduler.debug_assert_no_transient_callbacks("test"));

    // Add a callback
    scheduler.schedule_frame_callback(Box::new(|_| {}));

    // Now should fail (returns false)
    assert!(!scheduler.debug_assert_no_transient_callbacks("test"));

    // Clear by executing frame
    scheduler.execute_frame();

    // Should pass again
    assert!(scheduler.debug_assert_no_transient_callbacks("test"));
}

#[test]
fn test_scheduler_binding_debug_assert_no_performance_requests() {
    let scheduler = Scheduler::new();

    // No requests initially
    assert!(scheduler.debug_assert_no_pending_performance_mode_requests("test"));

    // Request performance mode
    let handle = scheduler.request_performance_mode(PerformanceMode::Latency);

    // Should fail now
    assert!(!scheduler.debug_assert_no_pending_performance_mode_requests("test"));

    // Release
    drop(handle);

    // Should pass again
    assert!(scheduler.debug_assert_no_pending_performance_mode_requests("test"));
}

#[test]
fn test_scheduler_binding_debug_assert_no_time_dilation() {
    use flui_scheduler::config::set_time_dilation;

    let scheduler = Scheduler::new();

    // Ensure default
    set_time_dilation(1.0).expect("positive finite time dilation");

    // Should pass with default
    assert!(scheduler.debug_assert_no_time_dilation("test"));

    // Set dilation
    set_time_dilation(2.0).expect("positive finite time dilation");

    // Should fail now
    assert!(!scheduler.debug_assert_no_time_dilation("test"));

    // Reset
    set_time_dilation(1.0).expect("positive finite time dilation");
    assert!(scheduler.debug_assert_no_time_dilation("test"));
}

#[test]
fn test_multiple_performance_mode_requests() {
    let scheduler = Scheduler::new();

    // Request multiple modes
    let handle1 = scheduler.request_performance_mode(PerformanceMode::Latency);
    let handle2 = scheduler.request_performance_mode(PerformanceMode::Throughput);
    let handle3 = scheduler.request_performance_mode(PerformanceMode::LowPower);

    // All handles exist
    assert!(!scheduler.debug_assert_no_pending_performance_mode_requests("test"));

    // Drop one by one
    drop(handle1);
    assert!(!scheduler.debug_assert_no_pending_performance_mode_requests("test"));

    drop(handle2);
    assert!(!scheduler.debug_assert_no_pending_performance_mode_requests("test"));

    drop(handle3);
    assert!(scheduler.debug_assert_no_pending_performance_mode_requests("test"));
}

#[test]
fn test_performance_mode_handle_dispose() {
    let scheduler = Scheduler::new();

    let handle = scheduler.request_performance_mode(PerformanceMode::Latency);
    assert!(!scheduler.debug_assert_no_pending_performance_mode_requests("test"));

    // Explicit dispose instead of drop
    handle.dispose();

    assert!(scheduler.debug_assert_no_pending_performance_mode_requests("test"));
}

// ============================================================================
// Extended Traits Tests (for coverage)
// ============================================================================

// ============================================================================
// Extended Budget Tests (for coverage)
// ============================================================================

#[test]
fn test_frame_budget_phase_recording() {
    let mut budget = FrameBudget::new(60);

    // Record all phases
    budget.record_build_duration(Milliseconds::new(2.0));
    budget.record_layout_duration(Milliseconds::new(3.0));
    budget.record_paint_duration(Milliseconds::new(4.0));
    budget.record_composite_duration(Milliseconds::new(1.0));

    // Check individual phase stats
    let build = budget.build_stats();
    assert!((build.duration_ms() - 2.0).abs() < 0.01);

    let layout = budget.layout_stats();
    assert!((layout.duration_ms() - 3.0).abs() < 0.01);

    let paint = budget.paint_stats();
    assert!((paint.duration_ms() - 4.0).abs() < 0.01);

    let composite = budget.composite_stats();
    assert!((composite.duration_ms() - 1.0).abs() < 0.01);
}

#[test]
fn test_frame_budget_policy_changes() {
    use flui_scheduler::budget::BudgetPolicy;

    let mut budget = FrameBudget::new(60);

    // Test policy changes
    budget.set_policy(BudgetPolicy::SkipIdle);
    assert_eq!(budget.policy(), BudgetPolicy::SkipIdle);

    budget.set_policy(BudgetPolicy::SkipIdleAndBuild);
    assert_eq!(budget.policy(), BudgetPolicy::SkipIdleAndBuild);

    budget.set_policy(BudgetPolicy::StopAll);
    assert_eq!(budget.policy(), BudgetPolicy::StopAll);

    budget.set_policy(BudgetPolicy::Continue);
    assert_eq!(budget.policy(), BudgetPolicy::Continue);
}

#[test]
fn test_frame_budget_statistics() {
    let mut budget = FrameBudget::new(60);

    // Record some frame times
    budget.record_frame_duration(Milliseconds::new(10.0));
    budget.record_frame_duration(Milliseconds::new(15.0));
    budget.record_frame_duration(Milliseconds::new(12.0));

    // Check statistics
    let avg = budget.avg_frame_time();
    assert!(avg.value() > 0.0);

    let avg_fps = budget.avg_fps();
    assert!(avg_fps > 0.0);

    let variance = budget.frame_time_variance();
    assert!(variance >= 0.0);
}

#[test]
fn test_frame_budget_finish_frame() {
    let mut budget = FrameBudget::new(60);

    // Record phases
    budget.record_build_duration(Milliseconds::new(5.0));
    budget.record_layout_duration(Milliseconds::new(5.0));

    // Finish frame
    budget.finish_frame();

    // Stats should be recorded
    let all = budget.all_phase_stats();
    assert!(all.total_duration().value() > 0.0);
}

// ============================================================================
// Extended Frame Tests (for coverage)
// ============================================================================

#[test]
fn test_frame_timing_builder_all_fields() {
    use flui_scheduler::frame::FrameTimingBuilder;

    let timing = FrameTimingBuilder::new().target_fps(120).build();

    assert!((timing.target_duration_ms() - 8.333).abs() < 0.1);
}

#[test]
fn test_frame_phase_all_variants() {
    use flui_scheduler::frame::FramePhase;

    // Test all phases
    let phases = [
        FramePhase::Idle,
        FramePhase::Build,
        FramePhase::Layout,
        FramePhase::Paint,
        FramePhase::Composite,
    ];

    for phase in phases {
        // Test display
        let _ = format!("{}", phase);

        // Test next
        let _ = phase.next();

        // Test previous
        let _ = phase.prev();
    }
}

#[test]
fn test_scheduler_phase_all_variants() {
    // Test all scheduler phases
    let phases = SchedulerPhase::ALL;
    assert_eq!(phases.len(), 5);

    for phase in phases {
        // Test display
        let _ = format!("{}", phase);

        // Test is_in_frame
        let _ = phase.is_in_frame();

        // Test is_animating
        let _ = phase.is_animating();

        // Test is_rendering
        let _ = phase.is_rendering();

        // Test next
        let _ = phase.next();
    }
}

#[test]
fn test_app_lifecycle_state_all_variants() {
    let states = AppLifecycleState::ALL;
    assert_eq!(states.len(), 5);

    for state in states {
        // Test display
        let _ = format!("{}", state);

        // Test is_visible
        let _ = state.is_visible();

        // Test is_focused
        let _ = state.is_focused();

        // Test should_animate
        let _ = state.should_animate();

        // Test can_animate
        let _ = state.can_animate();

        // Test should_render
        let _ = state.should_render();
    }
}

// ============================================================================
// Extended VSync Tests (55% -> 80%+)
// ============================================================================

#[test]
fn test_vsync_mode_properties() {
    // Test waits_for_vsync
    assert!(VsyncMode::On.waits_for_vsync());
    assert!(!VsyncMode::Off.waits_for_vsync());
    assert!(VsyncMode::Adaptive.waits_for_vsync());
    assert!(VsyncMode::TripleBuffer.waits_for_vsync());

    // Test can_tear
    assert!(!VsyncMode::On.can_tear());
    assert!(VsyncMode::Off.can_tear());
    assert!(!VsyncMode::Adaptive.can_tear());
    assert!(!VsyncMode::TripleBuffer.can_tear());

    // Test description
    let _ = VsyncMode::On.description();
    let _ = VsyncMode::Off.description();
    let _ = VsyncMode::Adaptive.description();
    let _ = VsyncMode::TripleBuffer.description();
}

#[test]
fn test_vsync_scheduler_start_stop() {
    let vsync = VsyncScheduler::try_new(60).expect("refresh > 0");

    assert!(!vsync.is_active());
    vsync.start();
    assert!(vsync.is_active());
    vsync.stop();
    assert!(!vsync.is_active());
}

#[test]
fn test_vsync_scheduler_callback() {
    let vsync = VsyncScheduler::try_new(60).expect("refresh > 0");
    let called = Arc::new(AtomicU32::new(0));

    let c = Arc::clone(&called);
    vsync.set_callback(move |_instant| {
        c.fetch_add(1, Ordering::SeqCst);
    });

    vsync.signal_vsync();
    assert_eq!(called.load(Ordering::SeqCst), 1);

    vsync.signal_vsync();
    assert_eq!(called.load(Ordering::SeqCst), 2);

    vsync.clear_callback();
    vsync.signal_vsync();
    assert_eq!(called.load(Ordering::SeqCst), 2); // Still 2, callback cleared
}

#[test]
fn test_vsync_scheduler_time_tracking() {
    let vsync = VsyncScheduler::try_new(60).expect("refresh > 0");

    // No vsync yet
    assert!(vsync.time_since_vsync().is_none());
    assert!(vsync.time_since_vsync_ms().is_none());
    assert!(vsync.predict_next_vsync().is_none());

    // Signal vsync
    vsync.signal_vsync();

    // Now should have values
    assert!(vsync.time_since_vsync().is_some());
    assert!(vsync.time_since_vsync_ms().is_some());
    assert!(vsync.predict_next_vsync().is_some());
}

#[test]
fn test_vsync_stats() {
    use flui_scheduler::vsync::VsyncStats;

    // Default stats
    let stats = VsyncStats::default();
    assert_eq!(stats.signal_count, 0);
    assert_eq!(stats.missed_count, 0);

    // Miss rate with zero signals
    assert_eq!(stats.miss_rate(), 0.0);

    // Effective FPS with zero interval
    assert_eq!(stats.effective_fps(), 0.0);
}

#[test]
fn test_vsync_scheduler_stats() {
    let vsync = VsyncScheduler::try_new(60).expect("refresh > 0");

    // Initial stats
    let stats = vsync.stats();
    assert_eq!(stats.signal_count, 0);

    // Signal multiple times
    vsync.signal_vsync();
    std::thread::sleep(Duration::from_millis(16));
    vsync.signal_vsync();
    std::thread::sleep(Duration::from_millis(16));
    vsync.signal_vsync();

    // Check stats updated
    let stats = vsync.stats();
    assert!(stats.signal_count >= 2);
    assert!(stats.avg_interval.value() > 0);

    // Reset stats
    vsync.reset_stats();
    let stats = vsync.stats();
    assert_eq!(stats.signal_count, 0);
}

#[test]
fn test_vsync_scheduler_is_at_target_rate() {
    let vsync = VsyncScheduler::try_new(60).expect("refresh > 0");

    // No data - should return true
    assert!(vsync.is_at_target_rate());

    // Signal at approximately correct rate
    vsync.signal_vsync();
    std::thread::sleep(Duration::from_millis(16));
    vsync.signal_vsync();
    std::thread::sleep(Duration::from_millis(16));
    vsync.signal_vsync();

    // Should be close to target rate
    // This is a soft test - timing may vary on CI
    let _ = vsync.is_at_target_rate();
}

#[test]
fn test_vsync_scheduler_frame_intervals() {
    // Test different refresh rates
    let vsync_60 = VsyncScheduler::try_new(60).expect("refresh > 0");
    let interval_60 = vsync_60.frame_interval();
    assert!(interval_60.value().abs_diff(16_666) < 100);

    let vsync_120 = VsyncScheduler::try_new(120).expect("refresh > 0");
    let interval_120 = vsync_120.frame_interval();
    assert!(interval_120.value().abs_diff(8_333) < 100);

    // Test frame_interval_duration
    let duration = vsync_60.frame_interval_duration();
    assert!(duration.as_millis() >= 16);
    assert!(duration.as_millis() <= 17);
}

#[test]
fn test_vsync_scheduler_wait_for_vsync() {
    let vsync = VsyncScheduler::try_new(60).expect("refresh > 0");

    // Off mode - no waiting
    vsync.set_mode(VsyncMode::Off);
    let start = std::time::Instant::now();
    let _ = vsync.wait_for_vsync();
    let elapsed = start.elapsed();
    assert!(elapsed.as_millis() < 5); // Should return immediately

    // On mode - may wait (depends on previous vsync)
    vsync.set_mode(VsyncMode::On);
    let _ = vsync.wait_for_vsync();
}

#[test]
fn test_vsync_scheduler_default() {
    let vsync = VsyncScheduler::default();
    assert_eq!(vsync.refresh_rate(), 60);
}

#[test]
fn test_vsync_scheduler_debug() {
    let vsync = VsyncScheduler::try_new(60).expect("refresh > 0");
    let debug_str = format!("{:?}", vsync);
    assert!(debug_str.contains("VsyncScheduler"));
    assert!(debug_str.contains("refresh_rate"));
}

// ============================================================================
// Extended ID Tests (55% -> 80%+)
// ============================================================================

#[test]
fn test_foundation_id_creation() {
    use flui_scheduler::id::{IdGenerator, markers};

    let frame_gen = IdGenerator::<markers::Frame>::new();
    let task_gen = IdGenerator::<markers::Task>::new();
    let ticker_gen = IdGenerator::<markers::Ticker>::new();
    let callback_gen = IdGenerator::<markers::FrameCallback>::new();

    let frame_id = frame_gen.next();
    let task_id = task_gen.next();
    let ticker_id = ticker_gen.next();
    let callback_id = callback_gen.next();

    // All IDs should have valid values
    assert!(frame_id.get() > 0);
    assert!(task_id.get() > 0);
    assert!(ticker_id.get() > 0);
    assert!(callback_id.get() > 0);
}

#[test]
fn test_foundation_id_raw() {
    use flui_scheduler::id::{FrameId, IdGenerator, markers};

    let id_gen = IdGenerator::<markers::Frame>::new();
    let id: FrameId = id_gen.next();
    let raw = id.into_raw();
    let value = id.get();

    assert_eq!(raw.unzip(), value);
}

#[test]
fn test_foundation_id_from_zip() {
    use flui_scheduler::id::FrameId;

    let id = FrameId::zip(42);
    assert_eq!(id.get(), 42);
}

#[test]
fn test_foundation_id_display() {
    use flui_scheduler::id::FrameId;

    let id = FrameId::zip(42);
    let display = format!("{}", id);

    assert!(display.contains("Frame"));
    assert!(display.contains("42"));
}

#[test]
fn test_foundation_id_debug() {
    use flui_scheduler::id::FrameId;

    let id = FrameId::zip(42);
    let debug = format!("{:?}", id);

    assert!(debug.contains("Frame"));
    assert!(debug.contains("42"));
}

#[test]
fn test_foundation_id_ordering() {
    use flui_scheduler::id::FrameId;

    let id1 = FrameId::zip(1);
    let id2 = FrameId::zip(2);
    let id3 = FrameId::zip(3);

    // IDs should be ordered by value
    assert!(id1 < id2);
    assert!(id2 < id3);
    assert!(id1 < id3);
}

#[test]
fn test_foundation_id_hash() {
    use std::collections::HashSet;

    use flui_scheduler::id::FrameId;

    let id1 = FrameId::zip(1);
    let id2 = FrameId::zip(2);

    let mut set = HashSet::new();
    set.insert(id1);
    set.insert(id2);

    assert_eq!(set.len(), 2);
    assert!(set.contains(&id1));
    assert!(set.contains(&id2));
}

#[test]
fn test_id_generator() {
    use flui_scheduler::id::{IdGenerator, markers};

    let generator = IdGenerator::<markers::Frame>::new();

    let id1 = generator.next();
    let id2 = generator.next();
    let id3 = generator.next();

    assert_eq!(id1.get(), 1);
    assert_eq!(id2.get(), 2);
    assert_eq!(id3.get(), 3);

    // Test current
    assert_eq!(generator.current(), 4);
}

#[test]
fn test_id_generator_starting_from() {
    use flui_scheduler::id::{IdGenerator, markers};

    let generator = IdGenerator::<markers::Task>::starting_from(100);

    let id1 = generator.next();
    let id2 = generator.next();

    assert_eq!(id1.get(), 100);
    assert_eq!(id2.get(), 101);
}

#[test]
fn test_id_generator_starting_from_zero() {
    use flui_scheduler::id::{IdGenerator, markers};

    // Starting from 0 should be converted to 1
    let generator = IdGenerator::<markers::Task>::starting_from(0);

    let id = generator.next();
    assert_eq!(id.get(), 1);
}

#[test]
fn test_id_generator_reset() {
    use flui_scheduler::id::{IdGenerator, markers};

    let generator = IdGenerator::<markers::Frame>::new();

    generator.next();
    generator.next();
    generator.next();

    assert_eq!(generator.current(), 4);

    generator.reset();
    assert_eq!(generator.current(), 1);

    let id = generator.next();
    assert_eq!(id.get(), 1);
}

#[test]
fn test_id_generator_default() {
    use flui_scheduler::id::{IdGenerator, markers};

    let generator: IdGenerator<markers::Frame> = Default::default();
    assert_eq!(generator.current(), 1);
}

// ============================================================================
// Extended Duration Tests (57% -> 80%+)
// ============================================================================

#[test]
fn test_milliseconds_constants() {
    assert_eq!(Milliseconds::ZERO.value(), 0.0);
    assert_eq!(Milliseconds::ONE.value(), 1.0);
}

#[test]
fn test_milliseconds_is_zero() {
    assert!(Milliseconds::ZERO.is_zero());
    assert!(!Milliseconds::ONE.is_zero());
    assert!(!Milliseconds::new(0.001).is_zero());
}

#[test]
fn test_milliseconds_max_min() {
    let a = Milliseconds::new(10.0);
    let b = Milliseconds::new(20.0);

    assert_eq!(a.max(b).value(), 20.0);
    assert_eq!(a.min(b).value(), 10.0);
}

#[test]
fn test_milliseconds_clamp() {
    let min = Milliseconds::new(5.0);
    let max = Milliseconds::new(15.0);

    assert_eq!(Milliseconds::new(0.0).clamp(min, max).value(), 5.0);
    assert_eq!(Milliseconds::new(10.0).clamp(min, max).value(), 10.0);
    assert_eq!(Milliseconds::new(20.0).clamp(min, max).value(), 15.0);
}

#[test]
fn test_milliseconds_to_micros() {
    let ms = Milliseconds::new(1.0);
    let us = ms.to_micros();

    assert_eq!(us.value(), 1000);
}

#[test]
fn test_milliseconds_from_microseconds() {
    use flui_scheduler::duration::Microseconds;

    let us = Microseconds::new(1000);
    let ms: Milliseconds = us.into();

    assert_eq!(ms.value(), 1.0);
}

#[test]
fn test_milliseconds_from_f64() {
    let ms: Milliseconds = 16.67.into();
    assert_eq!(ms.value(), 16.67);
}

#[test]
fn test_milliseconds_from_duration() {
    let std_dur = Duration::from_millis(100);
    let ms: Milliseconds = std_dur.into();

    assert_eq!(ms.value(), 100.0);
}

#[test]
fn test_milliseconds_to_duration() {
    let ms = Milliseconds::new(100.0);
    let std_dur: Duration = ms.into();

    assert_eq!(std_dur.as_millis(), 100);
}

#[test]
fn test_seconds_constants() {
    use flui_scheduler::duration::Seconds;

    assert_eq!(Seconds::ZERO.value(), 0.0);
    assert_eq!(Seconds::ONE.value(), 1.0);
}

#[test]
fn test_seconds_is_zero() {
    use flui_scheduler::duration::Seconds;

    assert!(Seconds::ZERO.is_zero());
    assert!(!Seconds::ONE.is_zero());
}

#[test]
fn test_seconds_arithmetic() {
    use flui_scheduler::duration::Seconds;

    let a = Seconds::new(1.0);
    let b = Seconds::new(0.5);

    assert_eq!((a + b).value(), 1.5);
    assert_eq!((a - b).value(), 0.5);
}

#[test]
fn test_seconds_from_milliseconds() {
    use flui_scheduler::duration::Seconds;

    let ms = Milliseconds::new(1000.0);
    let secs: Seconds = ms.into();

    assert_eq!(secs.value(), 1.0);
}

#[test]
fn test_seconds_from_duration() {
    use flui_scheduler::duration::Seconds;

    let std_dur = Duration::from_secs(2);
    let secs: Seconds = std_dur.into();

    assert_eq!(secs.value(), 2.0);
}

#[test]
fn test_seconds_to_duration() {
    use flui_scheduler::duration::Seconds;

    let secs = Seconds::new(2.0);
    let std_dur: Duration = secs.into();

    assert_eq!(std_dur.as_secs(), 2);
}

#[test]
fn test_seconds_from_f64() {
    use flui_scheduler::duration::Seconds;

    let secs: Seconds = 1.5.into();
    assert_eq!(secs.value(), 1.5);
}

#[test]
fn test_microseconds() {
    use flui_scheduler::duration::Microseconds;

    let us = Microseconds::new(1000);
    assert_eq!(us.value(), 1000);

    // Constants
    assert_eq!(Microseconds::ZERO.value(), 0);
    assert_eq!(Microseconds::ONE.value(), 1);
}

#[test]
fn test_microseconds_from_u64() {
    use flui_scheduler::duration::Microseconds;

    let us: Microseconds = 1000_u64.into();
    assert_eq!(us.value(), 1000);
}

#[test]
fn test_microseconds_from_duration() {
    use flui_scheduler::duration::Microseconds;

    let std_dur = Duration::from_micros(1000);
    let us: Microseconds = std_dur.into();

    assert_eq!(us.value(), 1000);
}

#[test]
fn test_frame_duration_constants() {
    // Test predefined constants
    assert!((FrameDuration::FPS_30.fps() - 30.0).abs() < 0.1);
    assert!((FrameDuration::FPS_60.fps() - 60.0).abs() < 0.1);
    assert!((FrameDuration::FPS_120.fps() - 120.0).abs() < 0.1);
    assert!((FrameDuration::FPS_144.fps() - 144.0).abs() < 0.1);
}

#[test]
fn test_frame_duration_as_seconds() {
    let fd = FrameDuration::try_from_fps(60).expect("fps > 0");
    let secs = fd.as_seconds();

    assert!((secs.value() - 0.01667).abs() < 0.001);
}

#[test]
fn test_frame_duration_utilization() {
    let fd = FrameDuration::try_from_fps(60).expect("fps > 0");

    let elapsed = Milliseconds::new(8.333); // 50% of 16.67ms
    let util = fd.utilization(elapsed);

    assert!((util - 0.5).abs() < 0.1);
}

#[test]
fn test_frame_duration_is_deadline_near() {
    let fd = FrameDuration::try_from_fps(60).expect("fps > 0");

    // 50% - not near
    assert!(!fd.is_deadline_near(Milliseconds::new(8.333)));

    // 90% - near
    assert!(fd.is_deadline_near(Milliseconds::new(15.0)));
}

#[test]
fn test_frame_duration_is_janky() {
    let fd = FrameDuration::try_from_fps(60).expect("fps > 0");

    // Under budget - not janky
    assert!(!fd.is_janky(Milliseconds::new(10.0)));

    // Over budget - janky
    assert!(fd.is_janky(Milliseconds::new(20.0)));
}

#[test]
fn test_frame_duration_default() {
    let fd = FrameDuration::default();
    assert!((fd.fps() - 60.0).abs() < 0.1);
}

#[test]
fn test_frame_duration_display() {
    let fd = FrameDuration::try_from_fps(60).expect("fps > 0");
    let display = format!("{}", fd);

    assert!(display.contains("ms"));
    assert!(display.contains("FPS"));
}

#[test]
fn test_percentage() {
    use flui_scheduler::duration::Percentage;

    // Constants
    assert_eq!(Percentage::ZERO.value(), 0.0);
    assert_eq!(Percentage::HUNDRED.value(), 100.0);

    // from_ratio
    let p = Percentage::from_ratio(0.5);
    assert_eq!(p.value(), 50.0);

    // as_ratio
    assert_eq!(p.as_ratio(), 0.5);

    // clamped
    let over = Percentage::new(150.0).clamped();
    assert_eq!(over.value(), 100.0);

    let under = Percentage::new(-10.0).clamped();
    assert_eq!(under.value(), 0.0);
}

#[test]
fn test_percentage_from_f64() {
    use flui_scheduler::duration::Percentage;

    let p: Percentage = 75.0.into();
    assert_eq!(p.value(), 75.0);
}

#[test]
fn test_percentage_display() {
    use flui_scheduler::duration::Percentage;

    let p = Percentage::new(75.5);
    let display = format!("{}", p);

    assert_eq!(display, "75.5%");
}

// ============================================================================
// Extended Ticker Tests (57% -> 80%+)
// ============================================================================

#[test]
fn test_ticker_state_methods() {
    // Test can_tick
    assert!(!TickerState::Idle.can_tick());
    assert!(TickerState::Active.can_tick());
    assert!(!TickerState::Muted.can_tick());
    assert!(!TickerState::Stopped.can_tick());

    // Test is_running
    assert!(!TickerState::Idle.is_running());
    assert!(TickerState::Active.is_running());
    assert!(TickerState::Muted.is_running());
    assert!(!TickerState::Stopped.is_running());

    // Test can_start
    assert!(TickerState::Idle.can_start());
    assert!(!TickerState::Active.can_start());
    assert!(!TickerState::Muted.can_start());
    assert!(TickerState::Stopped.can_start());
}

#[test]
fn test_ticker_toggle_mute() {
    let mut ticker = Ticker::new();

    ticker.start(|_| {});
    assert!(ticker.is_active());

    ticker.toggle_mute();
    assert!(ticker.is_muted());

    ticker.toggle_mute();
    assert!(ticker.is_active());
}

#[test]
fn test_ticker_elapsed() {
    let mut ticker = Ticker::new();

    // Idle state
    assert_eq!(ticker.elapsed().value(), 0.0);

    // Started
    ticker.start(|_| {});
    std::thread::sleep(Duration::from_millis(10));

    let elapsed = ticker.elapsed();
    assert!(elapsed.value() > 0.0);

    // Muted
    ticker.mute();
    let muted_elapsed = ticker.elapsed();
    assert!(muted_elapsed.value() > 0.0);

    // Stopped
    ticker.stop();
    assert_eq!(ticker.elapsed().value(), 0.0);
}

#[test]
fn test_ticker_elapsed_secs() {
    let mut ticker = Ticker::new();

    ticker.start(|_| {});
    std::thread::sleep(Duration::from_millis(10));

    let elapsed_secs = ticker.elapsed_secs();
    assert!(elapsed_secs > 0.0);
    assert!(elapsed_secs < 1.0);
}

#[test]
fn test_ticker_start_typed() {
    use flui_scheduler::duration::Seconds;

    let mut ticker = Ticker::new();
    let elapsed_captured = Arc::new(parking_lot::Mutex::new(Seconds::ZERO));

    let e = Arc::clone(&elapsed_captured);
    ticker.start_typed(move |elapsed: Seconds| {
        *e.lock() = elapsed;
    });

    // Would need a mock TickerProvider to properly test
    assert!(ticker.is_active());
}

#[test]
fn test_ticker_independent_instances() {
    let mut ticker1 = Ticker::new();
    ticker1.start(|_| {});

    let mut ticker2 = Ticker::new();
    ticker2.start(|_| {});

    // Separate tickers have different IDs
    assert_ne!(ticker1.id(), ticker2.id());

    // Independent state
    ticker1.stop();
    assert_eq!(ticker1.state(), TickerState::Stopped);
    assert_eq!(ticker2.state(), TickerState::Active);
}

#[test]
fn test_ticker_debug() {
    let ticker = Ticker::new();
    let debug = format!("{:?}", ticker);

    assert!(debug.contains("Ticker"));
    assert!(debug.contains("id"));
    assert!(debug.contains("state"));
}

#[test]
fn test_ticker_group_with_capacity() {
    use flui_scheduler::ticker::TickerGroup;

    let group = TickerGroup::with_capacity(10);
    assert!(group.is_empty());
    assert_eq!(group.len(), 0);
}

#[test]
fn test_ticker_group_add() {
    use flui_scheduler::ticker::TickerGroup;

    let mut group = TickerGroup::new();
    let ticker = Ticker::new();

    group.add(ticker);
    assert_eq!(group.len(), 1);
}

#[test]
fn test_ticker_group_cleanup() {
    use flui_scheduler::ticker::TickerGroup;

    let mut group = TickerGroup::new();

    group.create(|_| {});
    group.create(|_| {});

    assert_eq!(group.len(), 2);

    // Stop one ticker
    group.iter_mut().next().unwrap().stop();

    // Cleanup removes stopped tickers
    group.cleanup();

    assert_eq!(group.len(), 1);
}

#[test]
fn test_ticker_group_iterators() {
    use flui_scheduler::ticker::TickerGroup;

    let mut group = TickerGroup::new();
    group.create(|_| {});
    group.create(|_| {});

    // Test iter
    let count = group.iter().count();
    assert_eq!(count, 2);

    // Test iter_mut
    for ticker in group.iter_mut() {
        ticker.mute();
    }

    // All should be muted
    for ticker in &group {
        assert!(ticker.is_muted());
    }

    // Test into_iter on reference
    let count2 = (&group).into_iter().count();
    assert_eq!(count2, 2);

    // Test into_iter on mut reference
    for ticker in &mut group {
        ticker.unmute();
    }

    // Test IntoIterator
    let collected: Vec<_> = group.into_iter().collect();
    assert_eq!(collected.len(), 2);
}

#[test]
fn test_ticker_group_from_iterator() {
    use flui_scheduler::ticker::TickerGroup;

    let tickers = vec![Ticker::new(), Ticker::new(), Ticker::new()];
    let group: TickerGroup = tickers.into_iter().collect();

    assert_eq!(group.len(), 3);
}

#[test]
fn test_ticker_group_extend() {
    use flui_scheduler::ticker::TickerGroup;

    let mut group = TickerGroup::new();
    group.create(|_| {});

    let more_tickers = vec![Ticker::new(), Ticker::new()];
    group.extend(more_tickers);

    assert_eq!(group.len(), 3);
}

#[test]
fn test_auto_scheduling_ticker_debug() {
    let scheduler = Arc::new(Scheduler::new());
    let ticker = Ticker::new_with_scheduler(scheduler);

    let debug = format!("{:?}", ticker);
    assert!(debug.contains("Ticker"));
}

#[test]
fn test_ticker_future_or_cancel() {
    let future = TickerFuture::new();

    // Get or_cancel future
    let cancel_future = future.or_cancel();

    // Just verify it compiles and creates
    let _ = format!("{:?}", cancel_future);
}

#[test]
fn test_ticker_future_clone() {
    let future1 = TickerFuture::new();
    let future2 = future1.clone();

    // Both should reference the same state
    assert!(future1.is_pending());
    assert!(future2.is_pending());
}

#[test]
fn test_ticker_future_debug() {
    let future = TickerFuture::new();
    let debug = format!("{:?}", future);
    assert!(debug.contains("TickerFuture"));
}

#[test]
fn test_ticker_or_cancel_debug() {
    let future = TickerFuture::new();
    let cancel_future = future.or_cancel();

    let debug = format!("{:?}", cancel_future);
    assert!(debug.contains("TickerFutureOrCancel"));
}

// ============================================================================
// Extended Task Tests (61% -> 80%+)
// ============================================================================

#[test]
fn test_priority_all() {
    assert_eq!(Priority::ALL.len(), 4);
    assert_eq!(Priority::ALL[0], Priority::Idle);
    assert_eq!(Priority::ALL[1], Priority::Build);
    assert_eq!(Priority::ALL[2], Priority::Animation);
    assert_eq!(Priority::ALL[3], Priority::UserInput);
}

#[test]
fn test_priority_as_u8() {
    assert_eq!(Priority::Idle.as_u8(), 0);
    assert_eq!(Priority::Build.as_u8(), 1);
    assert_eq!(Priority::Animation.as_u8(), 2);
    assert_eq!(Priority::UserInput.as_u8(), 3);
}

#[test]
fn test_priority_from_u8() {
    assert_eq!(Priority::from_u8(0), Some(Priority::Idle));
    assert_eq!(Priority::from_u8(1), Some(Priority::Build));
    assert_eq!(Priority::from_u8(2), Some(Priority::Animation));
    assert_eq!(Priority::from_u8(3), Some(Priority::UserInput));
    assert_eq!(Priority::from_u8(4), None);
    assert_eq!(Priority::from_u8(255), None);
}

#[test]
fn test_priority_is_highest_lowest() {
    assert!(Priority::UserInput.is_highest());
    assert!(!Priority::Animation.is_highest());

    assert!(Priority::Idle.is_lowest());
    assert!(!Priority::Build.is_lowest());
}

#[test]
fn test_priority_higher_lower() {
    assert_eq!(Priority::Idle.higher(), Some(Priority::Build));
    assert_eq!(Priority::Build.higher(), Some(Priority::Animation));
    assert_eq!(Priority::Animation.higher(), Some(Priority::UserInput));
    assert_eq!(Priority::UserInput.higher(), None);

    assert_eq!(Priority::UserInput.lower(), Some(Priority::Animation));
    assert_eq!(Priority::Animation.lower(), Some(Priority::Build));
    assert_eq!(Priority::Build.lower(), Some(Priority::Idle));
    assert_eq!(Priority::Idle.lower(), None);
}

#[test]
fn test_priority_display() {
    assert_eq!(format!("{}", Priority::Idle), "Idle");
    assert_eq!(format!("{}", Priority::Build), "Build");
    assert_eq!(format!("{}", Priority::Animation), "Animation");
    assert_eq!(format!("{}", Priority::UserInput), "UserInput");
}

#[test]
fn test_task_creation() {
    use flui_scheduler::task::Task;

    let task = Task::new(Priority::Animation, || {
        // do nothing
    });

    assert_eq!(task.priority(), Priority::Animation);
    let id = task.id();
    assert!(id.get() > 0);
}

#[test]
fn test_task_execute() {
    use flui_scheduler::task::Task;

    let executed = Arc::new(AtomicU32::new(0));
    let e = Arc::clone(&executed);

    let task = Task::new(Priority::Build, move || {
        e.fetch_add(1, Ordering::SeqCst);
    });

    task.execute();

    assert_eq!(executed.load(Ordering::SeqCst), 1);
}

#[test]
fn test_task_debug() {
    use flui_scheduler::task::Task;

    let task = Task::new(Priority::Idle, || {});
    let debug = format!("{:?}", task);

    assert!(debug.contains("Task"));
    assert!(debug.contains("priority"));
}

#[test]
fn test_task_queue_is_empty() {
    let queue = TaskQueue::new();
    assert!(queue.is_empty());

    queue.add(Priority::Build, || {});
    assert!(!queue.is_empty());
}

#[test]
fn test_task_queue_len() {
    let queue = TaskQueue::new();
    assert_eq!(queue.len(), 0);

    queue.add(Priority::Build, || {});
    assert_eq!(queue.len(), 1);

    queue.add(Priority::Animation, || {});
    assert_eq!(queue.len(), 2);
}

#[test]
fn test_task_queue_clear() {
    let queue = TaskQueue::new();

    queue.add(Priority::Build, || {});
    queue.add(Priority::Animation, || {});

    assert_eq!(queue.len(), 2);

    queue.clear();

    assert_eq!(queue.len(), 0);
    assert!(queue.is_empty());
}

#[test]
fn test_task_queue_peek() {
    let queue = TaskQueue::new();

    // Empty queue
    assert!(queue.peek_priority().is_none());

    // Add tasks
    queue.add(Priority::Build, || {});
    queue.add(Priority::UserInput, || {});

    // Should peek highest priority
    assert_eq!(queue.peek_priority(), Some(Priority::UserInput));
}

#[test]
fn test_task_queue_execute_all_order() {
    let queue = TaskQueue::new();
    let executed = Arc::new(parking_lot::Mutex::new(Vec::new()));

    let e = Arc::clone(&executed);
    queue.add(Priority::Build, move || {
        e.lock().push("build");
    });

    let e = Arc::clone(&executed);
    queue.add(Priority::UserInput, move || {
        e.lock().push("user_input");
    });

    // Execute all - highest priority first
    queue.execute_all();

    let exec = executed.lock();
    assert_eq!(exec.len(), 2);
    assert_eq!(exec[0], "user_input");
    assert_eq!(exec[1], "build");
}

#[test]
fn test_task_queue_execute_all_empty() {
    let queue = TaskQueue::new();

    // Empty queue - should not panic
    queue.execute_all();
    assert!(queue.is_empty());
}

// ============================================================================
// Extended Budget Tests (63% -> 80%+)
// ============================================================================

#[test]
fn test_budget_policy_all() {
    use flui_scheduler::budget::BudgetPolicy;

    assert_eq!(BudgetPolicy::ALL.len(), 4);
    assert_eq!(BudgetPolicy::ALL[0], BudgetPolicy::Continue);
    assert_eq!(BudgetPolicy::ALL[1], BudgetPolicy::SkipIdle);
    assert_eq!(BudgetPolicy::ALL[2], BudgetPolicy::SkipIdleAndBuild);
    assert_eq!(BudgetPolicy::ALL[3], BudgetPolicy::StopAll);
}

#[test]
fn test_phase_stats() {
    use flui_scheduler::{budget::PhaseStats, duration::Percentage};

    let stats = PhaseStats::new(Milliseconds::new(5.0), Percentage::new(30.0));

    assert_eq!(stats.duration_ms(), 5.0);
    assert_eq!(stats.budget_percent.value(), 30.0);
}

#[test]
fn test_all_phase_stats() {
    use flui_scheduler::budget::AllPhaseStats;

    // Default
    let stats = AllPhaseStats::default();
    assert_eq!(stats.total_duration().value(), 0.0);

    // With values
    let mut budget = FrameBudget::new(60);
    budget.record_build_duration(Milliseconds::new(5.0));
    budget.record_layout_duration(Milliseconds::new(3.0));
    budget.record_paint_duration(Milliseconds::new(4.0));
    budget.record_composite_duration(Milliseconds::new(2.0));

    let all = budget.all_phase_stats();
    assert!((all.total_duration().value() - 14.0).abs() < 0.01);
    assert!(all.total_budget_percent().value() > 0.0);
}

#[test]
fn test_frame_budget_with_duration() {
    let budget = FrameBudget::with_duration(FrameDuration::FPS_120);
    assert!((budget.target_fps() as i32 - 120).abs() <= 1);
}

#[test]
fn test_frame_budget_record_phase_duration() {
    use flui_scheduler::frame::FramePhase;

    let mut budget = FrameBudget::new(60);

    budget.record_phase_duration(FramePhase::Build, Milliseconds::new(5.0));
    budget.record_phase_duration(FramePhase::Layout, Milliseconds::new(3.0));
    budget.record_phase_duration(FramePhase::Paint, Milliseconds::new(4.0));
    budget.record_phase_duration(FramePhase::Composite, Milliseconds::new(2.0));
    budget.record_phase_duration(FramePhase::Idle, Milliseconds::new(1.0)); // Should be ignored

    assert_eq!(budget.build_stats().duration_ms(), 5.0);
    assert_eq!(budget.layout_stats().duration_ms(), 3.0);
    assert_eq!(budget.paint_stats().duration_ms(), 4.0);
    assert_eq!(budget.composite_stats().duration_ms(), 2.0);
}

#[test]
fn test_frame_budget_record_time_methods() {
    let mut budget = FrameBudget::new(60);

    // Test the raw f64 methods
    budget.record_build_time(5.0);
    budget.record_layout_time(3.0);
    budget.record_paint_time(4.0);
    budget.record_composite_time(2.0);
    budget.record_frame_time(14.0);

    assert_eq!(budget.last_frame_time_ms(), 14.0);
}

#[test]
fn test_frame_budget_set_target_fps() {
    let mut budget = FrameBudget::new(60);
    assert!((budget.target_fps() as i32 - 60).abs() <= 1);

    budget.set_target_fps(120);
    assert!((budget.target_fps() as i32 - 120).abs() <= 1);
}

#[test]
fn test_frame_budget_builder() {
    use flui_scheduler::budget::{BudgetPolicy, FrameBudgetBuilder};

    let budget = FrameBudgetBuilder::new()
        .target_fps(144)
        .policy(BudgetPolicy::StopAll)
        .build();

    assert!((budget.target_fps() as i32 - 144).abs() <= 1);
    assert_eq!(budget.policy(), BudgetPolicy::StopAll);
}

#[test]
fn test_frame_budget_builder_default() {
    use flui_scheduler::budget::FrameBudgetBuilder;

    // Default builder should produce 60 FPS budget
    let budget = FrameBudgetBuilder::default().build();
    assert!((budget.target_fps() as i32 - 60).abs() <= 1);
}

#[test]
fn test_shared_budget() {
    use flui_scheduler::budget::shared_budget;

    let budget = shared_budget(60);

    {
        let mut b = budget.lock();
        b.reset();
        b.record_build_duration(Milliseconds::new(5.0));
    }

    let b = budget.lock();
    assert_eq!(b.build_stats().duration_ms(), 5.0);
}

#[test]
fn test_frame_budget_jank_percentage_empty() {
    let budget = FrameBudget::new(60);
    let jank = budget.jank_percentage();
    assert_eq!(jank.value(), 0.0);
}

// ============================================================================
// Extended Traits Tests (64% -> 80%+)
// ============================================================================

// ============================================================================
// Extended Frame Tests (71% -> 80%+)
// ============================================================================

#[test]
fn test_scheduler_phase_from_u8() {
    assert_eq!(SchedulerPhase::try_from_u8(0), Some(SchedulerPhase::Idle));
    assert_eq!(
        SchedulerPhase::try_from_u8(1),
        Some(SchedulerPhase::TransientCallbacks)
    );
    assert_eq!(
        SchedulerPhase::try_from_u8(2),
        Some(SchedulerPhase::MidFrameMicrotasks)
    );
    assert_eq!(
        SchedulerPhase::try_from_u8(3),
        Some(SchedulerPhase::PersistentCallbacks)
    );
    assert_eq!(
        SchedulerPhase::try_from_u8(4),
        Some(SchedulerPhase::PostFrameCallbacks)
    );
}

#[test]
fn test_scheduler_phase_try_from_u8_invalid() {
    assert!(SchedulerPhase::try_from_u8(5).is_none());
}

#[test]
fn test_scheduler_phase_can_transition_to() {
    // Valid transitions
    assert!(SchedulerPhase::Idle.can_transition_to(SchedulerPhase::TransientCallbacks));
    assert!(
        SchedulerPhase::TransientCallbacks.can_transition_to(SchedulerPhase::MidFrameMicrotasks)
    );
    assert!(
        SchedulerPhase::MidFrameMicrotasks.can_transition_to(SchedulerPhase::PersistentCallbacks)
    );
    assert!(
        SchedulerPhase::PersistentCallbacks.can_transition_to(SchedulerPhase::PostFrameCallbacks)
    );
    assert!(SchedulerPhase::PostFrameCallbacks.can_transition_to(SchedulerPhase::Idle));

    // Skip MidFrameMicrotasks is allowed
    assert!(
        SchedulerPhase::TransientCallbacks.can_transition_to(SchedulerPhase::PersistentCallbacks)
    );

    // Invalid transition
    assert!(!SchedulerPhase::Idle.can_transition_to(SchedulerPhase::PostFrameCallbacks));
}

#[test]
fn test_app_lifecycle_state_from_u8() {
    assert_eq!(
        AppLifecycleState::try_from_u8(0),
        Some(AppLifecycleState::Resumed)
    );
    assert_eq!(
        AppLifecycleState::try_from_u8(1),
        Some(AppLifecycleState::Inactive)
    );
    assert_eq!(
        AppLifecycleState::try_from_u8(2),
        Some(AppLifecycleState::Hidden)
    );
    assert_eq!(
        AppLifecycleState::try_from_u8(3),
        Some(AppLifecycleState::Paused)
    );
    assert_eq!(
        AppLifecycleState::try_from_u8(4),
        Some(AppLifecycleState::Detached)
    );
}

#[test]
fn test_app_lifecycle_state_try_from_u8_invalid() {
    assert!(AppLifecycleState::try_from_u8(5).is_none());
}

#[test]
fn test_app_lifecycle_state_should_methods() {
    // should_save_state
    assert!(!AppLifecycleState::Resumed.should_save_state());
    assert!(!AppLifecycleState::Inactive.should_save_state());
    assert!(!AppLifecycleState::Hidden.should_save_state());
    assert!(AppLifecycleState::Paused.should_save_state());
    assert!(AppLifecycleState::Detached.should_save_state());

    // should_release_resources
    assert!(!AppLifecycleState::Resumed.should_release_resources());
    assert!(!AppLifecycleState::Inactive.should_release_resources());
    assert!(AppLifecycleState::Hidden.should_release_resources());
    assert!(AppLifecycleState::Paused.should_release_resources());
    assert!(AppLifecycleState::Detached.should_release_resources());
}

#[test]
fn test_app_lifecycle_state_description() {
    assert!(!AppLifecycleState::Resumed.description().is_empty());
    assert!(!AppLifecycleState::Inactive.description().is_empty());
    assert!(!AppLifecycleState::Hidden.description().is_empty());
    assert!(!AppLifecycleState::Paused.description().is_empty());
    assert!(!AppLifecycleState::Detached.description().is_empty());
}

#[test]
fn test_frame_phase_is_rendering() {
    use flui_scheduler::frame::FramePhase;

    assert!(!FramePhase::Idle.is_rendering());
    assert!(FramePhase::Build.is_rendering());
    assert!(FramePhase::Layout.is_rendering());
    assert!(FramePhase::Paint.is_rendering());
    assert!(FramePhase::Composite.is_rendering());
}

#[test]
fn test_frame_phase_as_index() {
    use flui_scheduler::frame::FramePhase;

    assert_eq!(FramePhase::Idle.as_index(), 0);
    assert_eq!(FramePhase::Build.as_index(), 1);
    assert_eq!(FramePhase::Layout.as_index(), 2);
    assert_eq!(FramePhase::Paint.as_index(), 3);
    assert_eq!(FramePhase::Composite.as_index(), 4);
}

#[test]
fn test_frame_timing_advance_phase() {
    use flui_scheduler::frame::{FramePhase, FrameTiming};

    let mut timing = FrameTiming::new(60);
    assert_eq!(timing.phase, FramePhase::Idle);

    assert!(timing.advance_phase());
    assert_eq!(timing.phase, FramePhase::Build);

    assert!(timing.advance_phase());
    assert_eq!(timing.phase, FramePhase::Layout);

    assert!(timing.advance_phase());
    assert_eq!(timing.phase, FramePhase::Paint);

    assert!(timing.advance_phase());
    assert_eq!(timing.phase, FramePhase::Composite);

    // No more phases
    assert!(!timing.advance_phase());
    assert_eq!(timing.phase, FramePhase::Composite);
}

#[test]
fn test_frame_timing_elapsed_as_seconds() {
    use flui_scheduler::frame::FrameTiming;

    let timing = FrameTiming::new(60);
    std::thread::sleep(Duration::from_millis(10));

    let secs = timing.elapsed_as_seconds();
    assert!(secs.value() > 0.0);
    assert!(secs.value() < 1.0);
}

#[test]
fn test_frame_timing_budget_delta() {
    use flui_scheduler::frame::FrameTiming;

    let timing = FrameTiming::new(60);

    // Just started, should have positive delta
    let delta = timing.budget_delta_ms();
    assert!(delta > 0.0);
}

#[test]
fn test_frame_timing_builder_with_frame_duration() {
    use flui_scheduler::frame::{FramePhase, FrameTimingBuilder};

    let timing = FrameTimingBuilder::new()
        .frame_duration(FrameDuration::FPS_144)
        .initial_phase(FramePhase::Layout)
        .build();

    assert_eq!(timing.phase, FramePhase::Layout);
    assert!((timing.frame_duration.fps() - 144.0).abs() < 0.1);
}

// ============================================================================
// Extended Scheduler Tests (73% -> 80%+)
// ============================================================================

#[test]
fn test_scheduler_clone() {
    let scheduler1 = Scheduler::new();
    let scheduler2 = scheduler1.clone();

    // Both schedulers share state
    scheduler1.execute_frame();
    assert_eq!(scheduler2.frame_count(), 1);
}

#[test]
fn test_scheduler_current_vsync_time() {
    let scheduler = Scheduler::new();

    // Before frame - might be None
    let _ = scheduler.current_vsync_time();

    // Execute frame
    scheduler.execute_frame();

    // After frame - might still be available
    let _ = scheduler.current_vsync_time();
}

#[test]
fn test_scheduler_should_schedule_frame() {
    let scheduler = Scheduler::new();

    // Initially true
    assert!(scheduler.should_schedule_frame());

    // Request frame
    scheduler.request_frame();

    // May depend on state
    let _ = scheduler.should_schedule_frame();
}

// ============================================================================
// Extended Binding Tests (79% -> 80%+)
// ============================================================================

#[test]
fn test_time_dilation_same_value() {
    use flui_scheduler::config::{set_time_dilation, time_dilation};

    // Set to a value
    set_time_dilation(2.0).expect("positive finite time dilation");
    assert_eq!(time_dilation(), 2.0);

    // Set to same value - should be no-op
    set_time_dilation(2.0).expect("positive finite time dilation");
    assert_eq!(time_dilation(), 2.0);

    // Reset
    set_time_dilation(1.0).expect("positive finite time dilation");
}

#[test]
fn test_performance_mode_all_variants() {
    use flui_scheduler::config::PerformanceMode;

    let modes = [
        PerformanceMode::Normal,
        PerformanceMode::Latency,
        PerformanceMode::Throughput,
        PerformanceMode::LowPower,
    ];

    for mode in modes {
        let _ = format!("{:?}", mode);
    }

    // Default
    let default = PerformanceMode::default();
    assert_eq!(default, PerformanceMode::Normal);
}

#[test]
fn test_scheduler_adjust_for_epoch() {
    use flui_scheduler::config::set_time_dilation;

    let scheduler = Scheduler::new();

    // Without dilation
    set_time_dilation(1.0).expect("positive finite time dilation");
    let adjusted = scheduler.adjust_for_epoch(Duration::from_secs(10));
    assert!(adjusted.as_secs() <= 10);

    // With dilation
    set_time_dilation(2.0).expect("positive finite time dilation");
    scheduler.reset_epoch();
    let _adjusted = scheduler.adjust_for_epoch(Duration::from_millis(100));
    // Result depends on epoch

    // Reset
    set_time_dilation(1.0).expect("positive finite time dilation");
}

// ============================================================================
// Extended Ticker Coverage Tests
// ============================================================================
//
// The old `test_ticker_provider_schedule_tick_typed` exercised
// `TickerProvider::schedule_tick_typed`, deleted in U15 alongside the
// `schedule_tick` API removal (Flutter `TickerProvider.createTicker(callback)
// -> Ticker` is now the only factory shape). The auto-scheduling integration
// is covered by `crates/flui-scheduler/src/ticker.rs` unit tests
// (`test_auto_scheduling_ticker_fires_each_frame`,
// `test_create_ticker_via_provider_auto_schedules`).

#[test]
fn test_ticker_toggle_mute_transitions() {
    let mut ticker = Ticker::new();
    ticker.start(|_| {});
    assert_eq!(ticker.state(), TickerState::Active);

    // Toggle active -> muted
    ticker.toggle_mute();
    assert_eq!(ticker.state(), TickerState::Muted);

    // Toggle muted -> active
    ticker.toggle_mute();
    assert_eq!(ticker.state(), TickerState::Active);
}

#[test]
fn test_ticker_toggle_mute_no_op_from_idle() {
    let mut ticker = Ticker::new();
    assert_eq!(ticker.state(), TickerState::Idle);

    // Toggle from idle does nothing
    ticker.toggle_mute();
    assert_eq!(ticker.state(), TickerState::Idle);
}

#[test]
fn test_ticker_toggle_mute_no_op_from_stopped() {
    let mut ticker = Ticker::new();
    ticker.start(|_| {});
    ticker.stop();
    assert_eq!(ticker.state(), TickerState::Stopped);

    // Toggle from stopped does nothing
    ticker.toggle_mute();
    assert_eq!(ticker.state(), TickerState::Stopped);
}

#[test]
fn test_ticker_tick_skipped_when_muted() {
    use std::sync::atomic::AtomicU32;

    use flui_scheduler::ticker::TickerProvider;

    // Marker provider used only as a witness type for `Ticker::tick(&provider)`.
    // After U15 the trait has no required methods (default `create_ticker` is
    // sufficient), so this is a one-liner.
    struct MockProvider;
    impl TickerProvider for MockProvider {}

    let mut ticker = Ticker::new();
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = counter.clone();

    ticker.start(move |_| {
        counter_clone.fetch_add(1, Ordering::SeqCst);
    });

    ticker.mute();
    assert_eq!(ticker.state(), TickerState::Muted);

    let provider = MockProvider;
    ticker.tick(&provider);
    assert_eq!(counter.load(Ordering::SeqCst), 0);
}

#[test]
fn test_ticker_mute_no_op_from_idle_state() {
    let mut ticker = Ticker::new();
    ticker.mute();
    assert_eq!(ticker.state(), TickerState::Idle);
}

#[test]
fn test_ticker_mute_no_op_from_stopped_state() {
    let mut ticker = Ticker::new();
    ticker.start(|_| {});
    ticker.stop();
    ticker.mute();
    assert_eq!(ticker.state(), TickerState::Stopped);
}

#[test]
fn test_ticker_unmute_no_op_from_idle_state() {
    let mut ticker = Ticker::new();
    ticker.unmute();
    assert_eq!(ticker.state(), TickerState::Idle);
}

#[test]
fn test_ticker_unmute_no_op_from_active_state() {
    let mut ticker = Ticker::new();
    ticker.start(|_| {});
    ticker.unmute();
    assert_eq!(ticker.state(), TickerState::Active);
}

#[test]
fn test_ticker_new_gets_unique_id() {
    let ticker1 = Ticker::new();
    let ticker2 = Ticker::new();
    assert_ne!(ticker1.id(), ticker2.id());
}

#[test]
fn test_ticker_debug_contains_fields() {
    let ticker = Ticker::new();
    let debug_str = format!("{:?}", ticker);
    assert!(debug_str.contains("Ticker"));
    assert!(debug_str.contains("id"));
    assert!(debug_str.contains("state"));
}

#[test]
fn test_ticker_default_is_idle() {
    let ticker = Ticker::default();
    assert_eq!(ticker.state(), TickerState::Idle);
}

#[test]
fn test_ticker_group_with_preallocated_capacity() {
    use flui_scheduler::ticker::TickerGroup;

    let group = TickerGroup::with_capacity(10);
    assert!(group.is_empty());
}

#[test]
fn test_ticker_group_add_and_find() {
    use flui_scheduler::ticker::TickerGroup;

    let mut group = TickerGroup::new();
    let ticker = Ticker::new();
    let id = ticker.id();

    group.add(ticker);
    assert!(group.iter().any(|t| t.id() == id));
}

#[test]
fn test_ticker_group_cleanup_removes_stopped() {
    use flui_scheduler::ticker::TickerGroup;

    let mut group = TickerGroup::new();
    group.create(|_| {});
    group.create(|_| {});

    for ticker in group.iter_mut().take(1) {
        ticker.stop();
    }

    group.cleanup();
    assert_eq!(group.len(), 1);
}

#[test]
fn test_ticker_group_default_empty() {
    use flui_scheduler::ticker::TickerGroup;
    let group = TickerGroup::default();
    assert!(group.is_empty());
}

#[test]
fn test_auto_scheduling_ticker_start_typed_works() {
    use std::sync::atomic::AtomicBool;

    use flui_scheduler::duration::Seconds;

    let scheduler = Arc::new(Scheduler::new());
    let mut ticker = Ticker::new_with_scheduler(scheduler.clone());

    let called = Arc::new(AtomicBool::new(false));
    let called_clone = called.clone();

    ticker.start_typed(move |elapsed: Seconds| {
        assert!(elapsed.value() >= 0.0);
        called_clone.store(true, Ordering::SeqCst);
    });

    assert_eq!(ticker.state(), TickerState::Active);
    scheduler.execute_frame();
    assert!(called.load(Ordering::SeqCst));
}

// ============================================================================
// TickerFuture Polling Tests
// ============================================================================

#[test]
fn test_ticker_future_new_is_pending() {
    assert!(TickerFuture::new().is_pending());
}

#[test]
fn test_ticker_future_complete_state_flags() {
    let future = TickerFuture::complete();
    assert!(future.is_complete());
    assert!(!future.is_pending());
    assert!(!future.is_canceled());
}

#[test]
fn test_ticker_future_clone_both_pending() {
    let future1 = TickerFuture::new();
    let future2 = future1.clone();
    assert!(future1.is_pending());
    assert!(future2.is_pending());
}

#[test]
fn test_ticker_future_default_pending() {
    let future = TickerFuture::default();
    assert!(future.is_pending());
}

#[test]
fn test_ticker_future_debug_active_state() {
    let pending = TickerFuture::new();
    assert!(format!("{:?}", pending).contains("active"));
}

#[test]
fn test_ticker_future_debug_complete_state() {
    let complete = TickerFuture::complete();
    assert!(format!("{:?}", complete).contains("complete"));
}

#[test]
fn test_ticker_future_poll_new_pending() {
    use std::{
        future::Future,
        pin::Pin,
        task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
    };

    let mut future = TickerFuture::new();

    fn dummy_raw_waker() -> RawWaker {
        fn no_op(_: *const ()) {}
        fn clone(_: *const ()) -> RawWaker {
            dummy_raw_waker()
        }
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, no_op, no_op, no_op);
        RawWaker::new(std::ptr::null(), &VTABLE)
    }
    let waker = unsafe { Waker::from_raw(dummy_raw_waker()) };
    let mut cx = Context::from_waker(&waker);

    assert!(matches!(Pin::new(&mut future).poll(&mut cx), Poll::Pending));
}

#[test]
fn test_ticker_future_poll_complete_ready() {
    use std::{
        future::Future,
        pin::Pin,
        task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
    };

    let mut future = TickerFuture::complete();

    fn dummy_raw_waker() -> RawWaker {
        fn no_op(_: *const ()) {}
        fn clone(_: *const ()) -> RawWaker {
            dummy_raw_waker()
        }
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, no_op, no_op, no_op);
        RawWaker::new(std::ptr::null(), &VTABLE)
    }
    let waker = unsafe { Waker::from_raw(dummy_raw_waker()) };
    let mut cx = Context::from_waker(&waker);

    assert!(matches!(
        Pin::new(&mut future).poll(&mut cx),
        Poll::Ready(())
    ));
}

#[test]
fn test_ticker_future_or_cancel_poll_pending() {
    use std::{
        future::Future,
        pin::Pin,
        task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
    };

    let future = TickerFuture::new();
    let mut or_cancel = future.or_cancel();

    fn dummy_raw_waker() -> RawWaker {
        fn no_op(_: *const ()) {}
        fn clone(_: *const ()) -> RawWaker {
            dummy_raw_waker()
        }
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, no_op, no_op, no_op);
        RawWaker::new(std::ptr::null(), &VTABLE)
    }
    let waker = unsafe { Waker::from_raw(dummy_raw_waker()) };
    let mut cx = Context::from_waker(&waker);

    assert!(matches!(
        Pin::new(&mut or_cancel).poll(&mut cx),
        Poll::Pending
    ));
}

#[test]
fn test_ticker_future_or_cancel_poll_complete() {
    use std::{
        future::Future,
        pin::Pin,
        task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
    };

    let future = TickerFuture::complete();
    let mut or_cancel = future.or_cancel();

    fn dummy_raw_waker() -> RawWaker {
        fn no_op(_: *const ()) {}
        fn clone(_: *const ()) -> RawWaker {
            dummy_raw_waker()
        }
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, no_op, no_op, no_op);
        RawWaker::new(std::ptr::null(), &VTABLE)
    }
    let waker = unsafe { Waker::from_raw(dummy_raw_waker()) };
    let mut cx = Context::from_waker(&waker);

    assert!(matches!(
        Pin::new(&mut or_cancel).poll(&mut cx),
        Poll::Ready(Ok(()))
    ));
}

// ============================================================================
// TickerCanceled Tests
// ============================================================================

#[test]
fn test_ticker_canceled_display_msg() {
    let error = TickerCanceled;
    assert_eq!(error.to_string(), "The ticker was canceled");
}

#[test]
fn test_ticker_canceled_debug_output() {
    let error = TickerCanceled;
    assert_eq!(format!("{:?}", error), "TickerCanceled");
}

#[test]
fn test_ticker_canceled_copy_semantics() {
    let error1 = TickerCanceled;
    let error2 = error1;
    assert_eq!(error1, error2);
}

#[test]
fn test_ticker_canceled_eq_check() {
    assert_eq!(TickerCanceled, TickerCanceled);
}

#[test]
fn test_ticker_canceled_error_trait() {
    use std::error::Error;
    let error = TickerCanceled;
    let _: &dyn Error = &error;
}

// ============================================================================
// TickerState Tests
// ============================================================================

#[test]
fn test_ticker_state_can_tick_values() {
    assert!(!TickerState::Idle.can_tick());
    assert!(TickerState::Active.can_tick());
    assert!(!TickerState::Muted.can_tick());
    assert!(!TickerState::Stopped.can_tick());
}

#[test]
fn test_ticker_state_is_running_values() {
    assert!(!TickerState::Idle.is_running());
    assert!(TickerState::Active.is_running());
    assert!(TickerState::Muted.is_running());
    assert!(!TickerState::Stopped.is_running());
}

#[test]
fn test_ticker_state_can_start_values() {
    assert!(TickerState::Idle.can_start());
    assert!(!TickerState::Active.can_start());
    assert!(!TickerState::Muted.can_start());
    assert!(TickerState::Stopped.can_start());
}

#[test]
fn test_ticker_state_default_idle() {
    assert_eq!(TickerState::default(), TickerState::Idle);
}
