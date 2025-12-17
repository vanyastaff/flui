//! Basic Scheduler Example
//!
//! This example demonstrates the fundamental usage of the FLUI scheduler,
//! including frame callbacks, task scheduling, and lifecycle management.
//!
//! Run with: `cargo run --example basic_scheduler -p flui-scheduler`

use flui_scheduler::{
    scheduler::Scheduler,
    task::{Priority, TaskQueue},
    SchedulerBinding,
};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

fn main() {
    println!("=== FLUI Scheduler Basic Example ===\n");

    // Create a new scheduler
    let scheduler = Scheduler::new();

    // Track callback invocations
    let transient_count = Arc::new(AtomicU32::new(0));
    let persistent_count = Arc::new(AtomicU32::new(0));
    let post_frame_count = Arc::new(AtomicU32::new(0));

    // 1. Schedule a transient callback (one-shot, runs once)
    let tc = Arc::clone(&transient_count);
    scheduler.schedule_frame_callback(Box::new(move |timestamp| {
        tc.fetch_add(1, Ordering::SeqCst);
        println!("  Transient callback fired! Timestamp: {:?}", timestamp);
    }));

    // 2. Add a persistent callback (runs every frame)
    let pc = Arc::clone(&persistent_count);
    scheduler.add_persistent_frame_callback(Arc::new(move |timing| {
        pc.fetch_add(1, Ordering::SeqCst);
        println!(
            "  Persistent callback fired! Frame phase: {:?}",
            timing.phase
        );
    }));

    // 3. Add a post-frame callback (cleanup, runs after frame)
    let pfc = Arc::clone(&post_frame_count);
    scheduler.add_post_frame_callback(Box::new(move |_timing| {
        pfc.fetch_add(1, Ordering::SeqCst);
        println!("  Post-frame callback fired!");
    }));

    // Execute 3 frames
    println!("Executing 3 frames...\n");

    for i in 1..=3 {
        println!("--- Frame {} ---", i);
        println!("  Phase before: {:?}", scheduler.scheduler_phase());

        scheduler.execute_frame();

        println!("  Phase after: {:?}", scheduler.scheduler_phase());
        println!();
    }

    // Print statistics
    println!("=== Statistics ===");
    println!(
        "Transient callbacks:  {} (expected: 1 - one-shot)",
        transient_count.load(Ordering::SeqCst)
    );
    println!(
        "Persistent callbacks: {} (expected: 3 - every frame)",
        persistent_count.load(Ordering::SeqCst)
    );
    println!(
        "Post-frame callbacks: {} (expected: 1 - one-shot)",
        post_frame_count.load(Ordering::SeqCst)
    );
    println!("Total frames: {}", scheduler.frame_count());

    // Demonstrate task queue
    println!("\n=== Task Queue Demo ===\n");

    let task_order = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let queue = TaskQueue::new();

    // Schedule tasks in reverse priority order using TaskQueue directly
    let to = Arc::clone(&task_order);
    queue.add(Priority::Idle, move || {
        to.lock().push("Idle");
    });

    let to = Arc::clone(&task_order);
    queue.add(Priority::Build, move || {
        to.lock().push("Build");
    });

    let to = Arc::clone(&task_order);
    queue.add(Priority::Animation, move || {
        to.lock().push("Animation");
    });

    let to = Arc::clone(&task_order);
    queue.add(Priority::UserInput, move || {
        to.lock().push("UserInput");
    });

    // Execute all tasks
    queue.execute_all();

    println!("Task execution order (highest priority first):");
    for (i, task) in task_order.lock().iter().enumerate() {
        println!("  {}. {}", i + 1, task);
    }

    println!("\n=== Example Complete ===");
}
