//! Executor System Example
//!
//! Demonstrates background and foreground executor usage for async task execution.
//! Shows how to run CPU-intensive work on background threads and update UI safely
//! on the foreground thread.

use flui_platform::executor::{BackgroundExecutor, ForegroundExecutor};
use flui_platform::PlatformExecutor;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

fn main() {
    // Initialize tracing for observability
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== Executor System Example ===\n");

    // Example 1: Background executor for CPU-intensive work
    example_background_cpu_work();

    // Example 2: Background work with foreground callback
    example_background_with_ui_update();

    // Example 3: Parallel background tasks
    example_parallel_background_tasks();

    // Example 4: Foreground task queue and batching
    example_foreground_task_batching();

    // Example 5: Async/await integration
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
            let result = (0..10_000_000).fold(0u64, |acc, x| acc.wrapping_add(x));

            println!("Background work complete: result = {}", result);
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

/// Example 2: Background work with foreground UI update callback
fn example_background_with_ui_update() {
    println!("\n--- Example 2: Background + Foreground Callback ---");

    let background_executor = BackgroundExecutor::new();
    let foreground_executor = ForegroundExecutor::new();

    let ui_state = Arc::new(AtomicU32::new(0));
    let ui_state_bg = Arc::clone(&ui_state);
    let foreground_clone = foreground_executor.clone();

    // Simulate loading data in background
    background_executor
        .spawn(async move {
            println!("Background: Loading data...");
            tokio::time::sleep(Duration::from_millis(100)).await;

            let data = 42u32; // Simulated loaded data

            // Schedule UI update on foreground thread via PlatformExecutor trait
            PlatformExecutor::spawn(
                &foreground_clone,
                Box::new(move || {
                    println!("Foreground: Updating UI with loaded data: {}", data);
                    ui_state_bg.store(data, Ordering::SeqCst);
                }),
            );
        })
        .detach();

    // Simulate event loop draining foreground tasks
    thread::sleep(Duration::from_millis(150));
    foreground_executor.drain_tasks();

    println!("UI state updated to: {}", ui_state.load(Ordering::SeqCst));
}

/// Example 3: Parallel background tasks
fn example_parallel_background_tasks() {
    println!("\n--- Example 3: Parallel Background Tasks ---");

    let executor = BackgroundExecutor::new();
    let completed_count = Arc::new(AtomicU32::new(0));

    let start = Instant::now();

    // Spawn multiple independent tasks
    for i in 0..4 {
        let count_clone = Arc::clone(&completed_count);
        executor
            .spawn(async move {
                println!("Task {} started", i);
                tokio::time::sleep(Duration::from_millis(100)).await;
                println!("Task {} completed", i);
                count_clone.fetch_add(1, Ordering::SeqCst);
            })
            .detach();
    }

    // Wait for all tasks
    while completed_count.load(Ordering::SeqCst) < 4 {
        thread::sleep(Duration::from_millis(10));
    }

    let elapsed = start.elapsed();
    println!(
        "All 4 tasks completed in {:?} (parallel execution)",
        elapsed
    );
    println!(
        "Sequential would take ~400ms, parallel took ~{}ms",
        elapsed.as_millis()
    );
}

/// Example 4: Foreground task batching and ordering
fn example_foreground_task_batching() {
    println!("\n--- Example 4: Foreground Task Batching ---");

    let executor = ForegroundExecutor::new();
    let execution_order = Arc::new(std::sync::Mutex::new(Vec::new()));

    // Queue multiple UI updates via PlatformExecutor trait
    for i in 0..5 {
        let order_clone = Arc::clone(&execution_order);
        PlatformExecutor::spawn(
            &executor,
            Box::new(move || {
                order_clone.lock().unwrap().push(i);
            }),
        );
    }

    println!(
        "Queued {} tasks, pending count: {}",
        5,
        executor.pending_count()
    );

    // Process all tasks in single batch (typical event loop behavior)
    executor.drain_tasks();

    let order = execution_order.lock().unwrap();
    println!("Tasks executed in FIFO order: {:?}", *order);
    assert_eq!(*order, vec![0, 1, 2, 3, 4]);
}

/// Example 5: Async/await integration with Tokio
fn example_async_await_integration() {
    println!("\n--- Example 5: Async/Await Integration ---");

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
            println!("Async task: computed result = {}", result);

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
