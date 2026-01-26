//! Executor System Tests
//!
//! Comprehensive tests for background and foreground executor implementations,
//! covering thread safety, task execution order, and performance characteristics.

use flui_platform::executor::{BackgroundExecutor, ForegroundExecutor};
use flui_platform::PlatformExecutor;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();
}

/// Test that background executor runs tasks on worker threads, not the spawning thread
#[test]
fn test_background_executor_runs_on_worker_thread() {
    init_tracing();
    tracing::info!("Testing background executor thread isolation");

    let executor = BackgroundExecutor::new();
    let spawning_thread_id = thread::current().id();
    let task_thread_id = Arc::new(Mutex::new(None));
    let task_thread_id_clone = Arc::clone(&task_thread_id);

    executor.spawn(Box::new(move || {
        let current_id = thread::current().id();
        *task_thread_id_clone.lock() = Some(current_id);
        tracing::debug!("Task executing on thread {:?}", current_id);
    }));

    // Wait for task to complete
    thread::sleep(Duration::from_millis(100));

    let executed_on = task_thread_id.lock().clone().expect("Task should have executed");
    assert_ne!(
        executed_on, spawning_thread_id,
        "Background task should NOT run on spawning thread"
    );

    tracing::info!("✓ PASS: Background task executed on worker thread (not UI thread)");
}

/// Test that foreground executor runs tasks on next event loop iteration (not immediately)
#[test]
fn test_foreground_executor_deferred_execution() {
    init_tracing();
    tracing::info!("Testing foreground executor deferred execution");

    let executor = ForegroundExecutor::new();
    let executed = Arc::new(AtomicBool::new(false));
    let executed_clone = Arc::clone(&executed);

    executor.spawn(Box::new(move || {
        executed_clone.store(true, Ordering::SeqCst);
    }));

    // Task should NOT execute immediately
    assert!(
        !executed.load(Ordering::SeqCst),
        "Task should not execute immediately after spawn"
    );

    // Simulate event loop iteration
    executor.drain_tasks();

    // Now task should have executed
    assert!(
        executed.load(Ordering::SeqCst),
        "Task should execute after drain_tasks()"
    );

    tracing::info!("✓ PASS: Foreground task executed on next event loop iteration");
}

/// Test that background task callbacks can safely update UI state via foreground executor
#[test]
fn test_background_callback_updates_ui_safely() {
    init_tracing();
    tracing::info!("Testing background task with foreground callback");

    let background_executor = BackgroundExecutor::new();
    let foreground_executor = ForegroundExecutor::new();

    let ui_state = Arc::new(Mutex::new(String::from("initial")));
    let ui_state_bg = Arc::clone(&ui_state);
    let foreground_clone = foreground_executor.clone();

    // Simulate background work that updates UI
    background_executor.spawn(Box::new(move || {
        tracing::debug!("Background task: processing data...");
        thread::sleep(Duration::from_millis(50)); // Simulate work

        let result = "processed_data".to_string();

        // Schedule UI update on foreground executor
        foreground_clone.spawn(Box::new(move || {
            *ui_state_bg.lock() = result;
            tracing::debug!("Foreground task: updated UI state");
        }));
    }));

    // Wait for background work
    thread::sleep(Duration::from_millis(150));

    // UI state should still be initial (foreground not drained)
    assert_eq!(*ui_state.lock(), "initial");

    // Drain foreground tasks (simulate event loop)
    foreground_executor.drain_tasks();

    // Now UI should be updated
    assert_eq!(*ui_state.lock(), "processed_data");

    tracing::info!("✓ PASS: Background task callback safely updated UI state");
}

/// Test that multiple background tasks execute in parallel
#[test]
fn test_multiple_background_tasks_parallel_execution() {
    init_tracing();
    tracing::info!("Testing parallel execution of background tasks");

    let executor = BackgroundExecutor::new();
    let task_count = 4;
    let start_time = Instant::now();

    let completion_times = Arc::new(Mutex::new(Vec::new()));

    for i in 0..task_count {
        let completion_times_clone = Arc::clone(&completion_times);
        executor.spawn(Box::new(move || {
            tracing::debug!("Task {} starting", i);
            thread::sleep(Duration::from_millis(100)); // Simulate work
            let elapsed = start_time.elapsed();
            completion_times_clone.lock().push(elapsed);
            tracing::debug!("Task {} completed at {:?}", i, elapsed);
        }));
    }

    // Wait for all tasks
    thread::sleep(Duration::from_millis(300));

    let times = completion_times.lock();
    assert_eq!(times.len(), task_count, "All tasks should complete");

    // If tasks ran sequentially, total time would be ~400ms
    // If tasks ran in parallel, total time should be ~100-150ms
    let max_time = times.iter().max().unwrap();
    assert!(
        max_time.as_millis() < 250,
        "Tasks should complete in parallel (took {:?}ms, expected <250ms)",
        max_time.as_millis()
    );

    tracing::info!(
        "✓ PASS: {} background tasks executed in parallel ({:?}ms)",
        task_count,
        max_time.as_millis()
    );
}

/// Test that foreground tasks execute in FIFO order
#[test]
fn test_foreground_tasks_fifo_order() {
    init_tracing();
    tracing::info!("Testing foreground task FIFO execution order");

    let executor = ForegroundExecutor::new();
    let execution_order = Arc::new(Mutex::new(Vec::new()));

    // Spawn tasks in specific order
    for i in 0..10 {
        let order_clone = Arc::clone(&execution_order);
        executor.spawn(Box::new(move || {
            order_clone.lock().push(i);
        }));
    }

    // Drain all tasks
    executor.drain_tasks();

    let order = execution_order.lock();
    assert_eq!(*order, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);

    tracing::info!("✓ PASS: Foreground tasks executed in FIFO order");
}

/// Test that BackgroundExecutor is Send+Sync
#[test]
fn test_background_executor_send_sync() {
    init_tracing();
    tracing::info!("Testing BackgroundExecutor Send+Sync bounds");

    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}

    assert_send::<BackgroundExecutor>();
    assert_sync::<BackgroundExecutor>();

    // Verify we can share across threads
    let executor = Arc::new(BackgroundExecutor::new());
    let counter = Arc::new(AtomicU32::new(0));

    let handles: Vec<_> = (0..4)
        .map(|_| {
            let executor_clone = Arc::clone(&executor);
            let counter_clone = Arc::clone(&counter);

            thread::spawn(move || {
                executor_clone.spawn(Box::new(move || {
                    counter_clone.fetch_add(1, Ordering::SeqCst);
                }));
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    thread::sleep(Duration::from_millis(100));
    assert_eq!(counter.load(Ordering::SeqCst), 4);

    tracing::info!("✓ PASS: BackgroundExecutor is Send+Sync and works across threads");
}

/// Test that ForegroundExecutor sender is Send+Sync but executor itself is !Send
///
/// Note: Rust's type system ensures !Send at compile time, so this test
/// verifies the Send+Sync properties of the sender component.
#[test]
fn test_foreground_executor_thread_safety() {
    init_tracing();
    tracing::info!("Testing ForegroundExecutor sender thread safety");

    // Verify sender can be cloned and used from multiple threads
    let executor = ForegroundExecutor::new();
    let counter = Arc::new(AtomicU32::new(0));

    let handles: Vec<_> = (0..4)
        .map(|_| {
            let executor_clone = executor.clone();
            let counter_clone = Arc::clone(&counter);

            thread::spawn(move || {
                executor_clone.spawn(Box::new(move || {
                    counter_clone.fetch_add(1, Ordering::SeqCst);
                }));
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    // Drain from main thread (simulating UI thread)
    executor.drain_tasks();
    assert_eq!(counter.load(Ordering::SeqCst), 4);

    tracing::info!("✓ PASS: ForegroundExecutor sender works from multiple threads");
}

/// Test foreground executor pending count tracking
#[test]
fn test_foreground_executor_pending_count() {
    init_tracing();
    tracing::info!("Testing foreground executor pending count");

    let executor = ForegroundExecutor::new();

    assert_eq!(executor.pending_count(), 0);

    // Add tasks
    for _ in 0..5 {
        executor.spawn(Box::new(|| {}));
    }

    assert_eq!(executor.pending_count(), 5);

    // Drain all
    executor.drain_tasks();
    assert_eq!(executor.pending_count(), 0);

    tracing::info!("✓ PASS: Pending count tracking works correctly");
}

/// Test that foreground executor handles nested spawns correctly
#[test]
fn test_foreground_executor_nested_spawns() {
    init_tracing();
    tracing::info!("Testing foreground executor with nested spawns");

    let executor = ForegroundExecutor::new();
    let execution_log = Arc::new(Mutex::new(Vec::new()));

    let log_clone1 = Arc::clone(&execution_log);
    let executor_clone = executor.clone();
    executor.spawn(Box::new(move || {
        log_clone1.lock().push("outer");

        // Spawn inner task
        let log_clone2 = Arc::clone(&log_clone1);
        executor_clone.spawn(Box::new(move || {
            log_clone2.lock().push("inner");
        }));
    }));

    // Drain all tasks - drain_tasks() continues until queue is empty,
    // so it executes outer (which spawns inner), then immediately executes inner
    executor.drain_tasks();

    let log = execution_log.lock().clone();
    assert_eq!(log, vec!["outer", "inner"], "drain_tasks() should execute all tasks including nested spawns");

    tracing::info!("✓ PASS: Nested spawns handled correctly in single drain cycle");
}

/// Benchmark executor spawn overhead to ensure it's under 100µs
#[test]
fn test_executor_spawn_overhead_benchmark() {
    init_tracing();
    tracing::info!("Benchmarking executor spawn overhead");

    let iterations = 1000;

    // Benchmark background executor
    {
        let executor = BackgroundExecutor::new();
        let start = Instant::now();

        for _ in 0..iterations {
            executor.spawn(Box::new(|| {}));
        }

        let duration = start.elapsed();
        let avg_micros = duration.as_micros() as f64 / iterations as f64;

        tracing::info!(
            "Background executor: {:.2}µs average spawn overhead ({} iterations)",
            avg_micros,
            iterations
        );

        assert!(
            avg_micros < 100.0,
            "Background spawn overhead ({:.2}µs) exceeds 100µs target",
            avg_micros
        );
    }

    // Benchmark foreground executor
    {
        let executor = ForegroundExecutor::new();
        let start = Instant::now();

        for _ in 0..iterations {
            executor.spawn(Box::new(|| {}));
        }

        let duration = start.elapsed();
        let avg_micros = duration.as_micros() as f64 / iterations as f64;

        tracing::info!(
            "Foreground executor: {:.2}µs average spawn overhead ({} iterations)",
            avg_micros,
            iterations
        );

        assert!(
            avg_micros < 100.0,
            "Foreground spawn overhead ({:.2}µs) exceeds 100µs target",
            avg_micros
        );
    }

    tracing::info!("✓ PASS: Executor spawn overhead <100µs for both executors");
}

/// Test background executor with actual async/await tasks
#[test]
fn test_background_executor_async_integration() {
    init_tracing();
    tracing::info!("Testing background executor with async tasks");

    let executor = BackgroundExecutor::new();
    let handle = executor.handle();
    let completed = Arc::new(AtomicBool::new(false));
    let completed_clone = Arc::clone(&completed);

    // Spawn native async task
    handle.spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        completed_clone.store(true, Ordering::SeqCst);
    });

    thread::sleep(Duration::from_millis(150));
    assert!(completed.load(Ordering::SeqCst));

    tracing::info!("✓ PASS: Background executor integrates with async/await");
}

/// Test that background executor handles panic in tasks gracefully
#[test]
fn test_background_executor_panic_handling() {
    init_tracing();
    tracing::info!("Testing background executor panic handling");

    let executor = BackgroundExecutor::new();
    let post_panic_executed = Arc::new(AtomicBool::new(false));
    let post_panic_clone = Arc::clone(&post_panic_executed);

    // Spawn task that panics
    executor.spawn(Box::new(|| {
        panic!("Intentional panic for testing");
    }));

    thread::sleep(Duration::from_millis(50));

    // Spawn another task after panic
    executor.spawn(Box::new(move || {
        post_panic_clone.store(true, Ordering::SeqCst);
    }));

    thread::sleep(Duration::from_millis(50));

    // Verify executor still works after panic
    assert!(
        post_panic_executed.load(Ordering::SeqCst),
        "Executor should continue working after task panic"
    );

    tracing::info!("✓ PASS: Background executor handles task panics gracefully");
}

/// Test foreground executor with high task volume
#[test]
fn test_foreground_executor_high_volume() {
    init_tracing();
    tracing::info!("Testing foreground executor with high task volume");

    let executor = ForegroundExecutor::new();
    let task_count = 10_000;
    let counter = Arc::new(AtomicUsize::new(0));

    let start = Instant::now();

    for _ in 0..task_count {
        let counter_clone = Arc::clone(&counter);
        executor.spawn(Box::new(move || {
            counter_clone.fetch_add(1, Ordering::SeqCst);
        }));
    }

    let spawn_duration = start.elapsed();
    tracing::info!("Spawned {} tasks in {:?}", task_count, spawn_duration);

    let drain_start = Instant::now();
    executor.drain_tasks();
    let drain_duration = drain_start.elapsed();

    tracing::info!("Drained {} tasks in {:?}", task_count, drain_duration);

    assert_eq!(counter.load(Ordering::SeqCst), task_count);
    assert!(
        drain_duration.as_millis() < 1000,
        "Draining {} tasks should take <1s, took {:?}ms",
        task_count,
        drain_duration.as_millis()
    );

    tracing::info!("✓ PASS: Foreground executor handles {} tasks efficiently", task_count);
}
