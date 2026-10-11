//! Frame phase ordering through the public owning scheduler.

use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use flui_scheduler::scheduler::UpdateScheduler;

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
    scheduler.add_persistent_frame_callback(std::rc::Rc::new(move |_timing| {
        p.fetch_add(1, Ordering::SeqCst);
    }));

    let pf = Arc::clone(&post_frame_called);
    scheduler.add_post_frame_callback(Box::new(move |_timing| {
        pf.fetch_add(1, Ordering::SeqCst);
    }));

    // Execute frame
    flui_scheduler::OwnerFrame::new(&scheduler)
        .expect("the scheduler has no live owner frame")
        .drive_frame(
            flui_scheduler::Instant::now(),
            flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
            || {},
            || {},
        )
        .expect("live owner frame");

    // Verify all callbacks were called
    assert_eq!(transient_called.load(Ordering::SeqCst), 1);
    assert_eq!(persistent_called.load(Ordering::SeqCst), 1);
    assert_eq!(post_frame_called.load(Ordering::SeqCst), 1);

    // Execute another frame - only persistent should be called again
    flui_scheduler::OwnerFrame::new(&scheduler)
        .expect("the scheduler has no live owner frame")
        .drive_frame(
            flui_scheduler::Instant::now(),
            flui_scheduler::IdleDeadline::far_future(flui_scheduler::Instant::now()),
            || {},
            || {},
        )
        .expect("live owner frame");

    assert_eq!(transient_called.load(Ordering::SeqCst), 1); // Still 1
    assert_eq!(persistent_called.load(Ordering::SeqCst), 2); // Now 2
    assert_eq!(post_frame_called.load(Ordering::SeqCst), 1); // Still 1
}
