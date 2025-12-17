//! VSync Scheduling Example
//!
//! This example demonstrates VSync-driven frame scheduling,
//! which synchronizes rendering with the display refresh rate.
//!
//! Run with: `cargo run --example vsync_scheduling -p flui-scheduler`

use flui_scheduler::{
    scheduler::Scheduler,
    vsync::{VsyncDrivenScheduler, VsyncMode, VsyncScheduler},
};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn main() {
    println!("=== FLUI VSync Scheduling Example ===\n");

    // 1. Basic VSync Scheduler
    println!("--- VSync Scheduler Basics ---\n");
    demo_vsync_basics();

    // 2. VSync Modes
    println!("\n--- VSync Modes ---\n");
    demo_vsync_modes();

    // 3. VSync-Driven Scheduler
    println!("\n--- VSync-Driven Scheduler ---\n");
    demo_vsync_driven_scheduler();

    println!("\n=== Example Complete ===");
}

fn demo_vsync_basics() {
    // Create a VSync scheduler for 60Hz display
    let vsync = VsyncScheduler::new(60);

    println!("VSync Scheduler created for 60Hz display");
    println!("Refresh rate: {} Hz", vsync.refresh_rate());
    println!(
        "Frame interval: {:.2} ms",
        vsync.frame_interval_ms().value()
    );

    // Check initial state
    println!("\nInitial state:");
    println!("  Is active: {}", vsync.is_active());
    println!("  Mode: {:?}", vsync.mode());

    // Set a callback
    let frame_count = Arc::new(AtomicU32::new(0));
    let fc = Arc::clone(&frame_count);
    vsync.set_callback(move |_instant| {
        fc.fetch_add(1, Ordering::SeqCst);
    });

    // Start the scheduler
    vsync.start();
    println!("\nVSync started");
    println!("  Is active: {}", vsync.is_active());

    // Simulate a few vsync signals
    println!("\nSimulating 3 vsync signals:");
    for i in 1..=3 {
        vsync.signal_vsync();
        println!(
            "  VSync {} - Total frames: {}",
            i,
            frame_count.load(Ordering::SeqCst)
        );
        std::thread::sleep(Duration::from_millis(16));
    }

    // Check stats
    let stats = vsync.stats();
    println!("\nVSync Stats:");
    println!("  Total signals: {}", stats.signal_count);
    println!("  Missed vsyncs: {}", stats.missed_count);
    println!("  Miss rate: {:.1}%", stats.miss_rate() * 100.0);

    vsync.stop();
    println!("\nVSync stopped");
}

fn demo_vsync_modes() {
    let vsync = VsyncScheduler::new(60);

    println!("Available VSync modes:\n");

    // On - Wait for vsync
    vsync.set_mode(VsyncMode::On);
    println!("VsyncMode::On");
    println!("  Waits for vsync: {}", vsync.mode().waits_for_vsync());
    println!("  Description: Standard vsync, no tearing, may reduce FPS");

    // Off - Don't wait
    vsync.set_mode(VsyncMode::Off);
    println!("\nVsyncMode::Off");
    println!("  Waits for vsync: {}", vsync.mode().waits_for_vsync());
    println!("  Description: No vsync, tearing possible, max FPS");

    // Adaptive - Wait only when under budget
    vsync.set_mode(VsyncMode::Adaptive);
    println!("\nVsyncMode::Adaptive");
    println!("  Waits for vsync: {}", vsync.mode().waits_for_vsync());
    println!("  Description: Adaptive vsync, wait only when under budget");

    // Triple buffering
    vsync.set_mode(VsyncMode::TripleBuffer);
    println!("\nVsyncMode::TripleBuffer");
    println!("  Waits for vsync: {}", vsync.mode().waits_for_vsync());
    println!("  Description: Triple buffering, reduced latency, no tearing");
}

fn demo_vsync_driven_scheduler() {
    let scheduler = Arc::new(Scheduler::new());
    let vsync_scheduler = VsyncDrivenScheduler::new(scheduler.clone(), 60);

    println!("VSync-Driven Scheduler created");
    println!("  Refresh rate: {} Hz", vsync_scheduler.refresh_rate());
    println!("  Is active: {}", vsync_scheduler.is_active());
    println!("  Auto-execute: {}", vsync_scheduler.auto_execute());

    // Add a persistent frame callback
    let render_count = Arc::new(AtomicU32::new(0));
    let rc = Arc::clone(&render_count);
    scheduler.add_persistent_frame_callback(Arc::new(move |timing| {
        rc.fetch_add(1, Ordering::SeqCst);
        println!("  Rendering frame... Phase: {:?}", timing.phase);
    }));

    // Enable auto-execute
    vsync_scheduler.set_auto_execute(true);
    println!("\nAuto-execute enabled");

    // Simulate vsync signals
    println!("\nSimulating 3 vsync signals:");
    for i in 1..=3 {
        println!("\n--- VSync {} ---", i);
        vsync_scheduler.on_vsync();
        std::thread::sleep(Duration::from_millis(16));
    }

    println!("\nTotal renders: {}", render_count.load(Ordering::SeqCst));

    // Check next vsync prediction
    if let Some(next) = vsync_scheduler.predict_next_vsync() {
        println!("Next vsync predicted at: {:?}", next);
    }

    // Get stats
    let stats = vsync_scheduler.stats();
    println!("\nVSync Stats:");
    println!("  Total signals: {}", stats.signal_count);
    println!("  Effective FPS: {:.1}", stats.effective_fps());
}
