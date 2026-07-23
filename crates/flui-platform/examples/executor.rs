//! Executor System Example
//!
//! Demonstrates background executor usage for asynchronous and CPU-intensive
//! work away from the UI owner thread.

// Target-level lint relaxations — crate-level allows don't reach this
// target. `unwrap` in test/example code: a panic IS the failure report
// (docs/PANIC-POLICY.md); style items here are ship-wave debt.
#![allow(clippy::unwrap_used)]

use std::{
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use flui_platform::executor::BackgroundExecutor;

fn main() {
    // Initialize tracing for observability
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== Executor System Example ===\n");

    // Example 1: Background executor for CPU-intensive work
    example_background_cpu_work();

    // Example 2: Parallel background tasks
    example_parallel_background_tasks();

    // Example 3: Async/await integration
    example_async_await_integration();

    println!("\n=== All Examples Complete ===");
}

/// Example 1: Background executor for CPU-intensive work
fn example_background_cpu_work() {
    println!("\n--- Example 1: Background CPU Work ---");

    let executor = BackgroundExecutor::new();
    let completed = Arc::new(AtomicU32::new(0));

    let start = Instant::now();

    // Spawn CPU-intensive task using async
    let completed_clone = Arc::clone(&completed);
    executor
        .spawn(async move {
            println!(
                "Background task started on thread {:?}",
                thread::current().id()
            );

            // Simulate CPU-intensive work
            let result = (0..10_000_000).fold(0u64, u64::wrapping_add);

            println!("Background work complete: result = {result}");
            completed_clone.store(1, Ordering::SeqCst);
        })
        .detach();

    // Main thread continues immediately
    println!("Main thread continues (non-blocking)");

    // Wait for completion (in real app, this would be event-driven)
    while completed.load(Ordering::SeqCst) == 0 {
        thread::sleep(Duration::from_millis(10));
    }

    println!("Total time: {:?}", start.elapsed());
}

/// Example 2: Parallel background tasks
fn example_parallel_background_tasks() {
    println!("\n--- Example 2: Parallel Background Tasks ---");

    let executor = BackgroundExecutor::new();
    let completed_count = Arc::new(AtomicU32::new(0));

    let start = Instant::now();

    // Spawn multiple independent tasks
    for i in 0..4 {
        let count_clone = Arc::clone(&completed_count);
        executor
            .spawn(async move {
                println!("Task {i} started");
                tokio::time::sleep(Duration::from_millis(100)).await;
                println!("Task {i} completed");
                count_clone.fetch_add(1, Ordering::SeqCst);
            })
            .detach();
    }

    // Wait for all tasks
    while completed_count.load(Ordering::SeqCst) < 4 {
        thread::sleep(Duration::from_millis(10));
    }

    let elapsed = start.elapsed();
    println!("All 4 tasks completed in {elapsed:?} (parallel execution)");
    println!(
        "Sequential would take ~400ms, parallel took ~{}ms",
        elapsed.as_millis()
    );
}

/// Example 3: Async/await integration with Tokio
fn example_async_await_integration() {
    println!("\n--- Example 3: Async/Await Integration ---");

    let executor = BackgroundExecutor::new();

    let completed = Arc::new(AtomicU32::new(0));
    let completed_clone = Arc::clone(&completed);

    // Spawn async task that uses Task<T> for awaiting results
    executor
        .spawn(async move {
            println!("Async task started");

            // Use Tokio's async primitives
            tokio::time::sleep(Duration::from_millis(50)).await;
            println!("Async task: after sleep");

            // Simulate async I/O
            let result = async_compute().await;
            println!("Async task: computed result = {result}");

            completed_clone.store(1, Ordering::SeqCst);
        })
        .detach();

    // Wait for async task
    while completed.load(Ordering::SeqCst) == 0 {
        thread::sleep(Duration::from_millis(10));
    }

    println!("Async task completed successfully");
}

/// Simulated async computation
async fn async_compute() -> u32 {
    tokio::task::yield_now().await;
    42
}
