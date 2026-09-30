//! Integration tests for flui-scheduler
//!
//! These tests verify the complete scheduler system works correctly
//! across multiple components working together.

// Target-level lint relaxations — crate-level allows don't reach this
// target. `unwrap` in test/example code: a panic IS the failure report
// (docs/PANIC-POLICY.md).

use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use flui_scheduler::scheduler::UpdateScheduler;

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

// ============================================================================
// Ticker Integration Tests
// ============================================================================

// ============================================================================
// Task Queue Integration Tests
// ============================================================================

// ============================================================================
// Frame Budget Integration Tests
// ============================================================================

// ============================================================================
// UpdateScheduler Binding Integration Tests
// ============================================================================

// ============================================================================
// Concurrent Access Tests
// ============================================================================

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

// ============================================================================
// Extended Binding Tests (for coverage)
// ============================================================================

// ============================================================================
// Extended Binding Tests (79% -> 80%+)
// ============================================================================

// ============================================================================
// Extended Ticker Coverage Tests
// ============================================================================
//
// The old `test_ticker_provider_schedule_tick_typed` exercised
// `TickerProvider::schedule_tick_typed`, since removed alongside the
// `schedule_tick` API (`create_ticker(callback) -> Ticker` is now the only
// factory shape). The auto-scheduling integration
// is covered by `crates/flui-scheduler/src/ticker.rs` unit tests
// (`test_auto_scheduling_ticker_fires_each_frame`,
// `test_create_ticker_via_provider_auto_schedules`).

// ============================================================================
// TickerFuture Polling Tests
// ============================================================================

// ============================================================================
// TickerState Tests
// ============================================================================
