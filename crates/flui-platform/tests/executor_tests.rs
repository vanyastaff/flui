//! Executor System Tests
//!
//! Tests for background executor thread safety, task execution, and overhead.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use flui_platform::executor::BackgroundExecutor;
use parking_lot::Mutex;

fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_test_writer()
        .with_max_level(tracing::Level::DEBUG)
        .try_init();
}

/// Test that background executor runs tasks on worker threads, not the spawning
/// thread
#[test]
fn test_background_executor_runs_on_worker_thread() {
    init_tracing();
    tracing::info!("Testing background executor thread isolation");

    let executor = BackgroundExecutor::new();
    let spawning_thread_id = thread::current().id();
    let task_thread_id = Arc::new(Mutex::new(None));
    let task_thread_id_clone = Arc::clone(&task_thread_id);

    executor
        .spawn(async move {
            let current_id = thread::current().id();
            *task_thread_id_clone.lock() = Some(current_id);
            tracing::debug!("Task executing on thread {:?}", current_id);
        })
        .detach();

    // Wait for task to complete
    thread::sleep(Duration::from_millis(100));

    let executed_on = task_thread_id.lock().expect("Task should have executed");
    assert_ne!(
        executed_on, spawning_thread_id,
        "Background task should NOT run on spawning thread"
    );

    tracing::info!("PASS: Background task executed on worker thread (not UI thread)");
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
        executor
            .spawn(async move {
                tracing::debug!("Task {} starting", i);
                tokio::time::sleep(Duration::from_millis(100)).await;
                let elapsed = start_time.elapsed();
                completion_times_clone.lock().push(elapsed);
                tracing::debug!("Task {} completed at {:?}", i, elapsed);
            })
            .detach();
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
        "PASS: {} background tasks executed in parallel ({:?}ms)",
        task_count,
        max_time.as_millis()
    );
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
                executor_clone
                    .spawn(async move {
                        counter_clone.fetch_add(1, Ordering::SeqCst);
                    })
                    .detach();
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    thread::sleep(Duration::from_millis(100));
    assert_eq!(counter.load(Ordering::SeqCst), 4);

    tracing::info!("PASS: BackgroundExecutor is Send+Sync and works across threads");
}

/// Benchmark executor spawn overhead to ensure it's under 100us
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
            executor.spawn(async {}).detach();
        }

        let duration = start.elapsed();
        let avg_micros = duration.as_micros() as f64 / iterations as f64;

        tracing::info!(
            "Background executor: {:.2}us average spawn overhead ({} iterations)",
            avg_micros,
            iterations
        );

        assert!(
            avg_micros < 100.0,
            "Background spawn overhead ({avg_micros:.2}us) exceeds 100us target"
        );
    }

    tracing::info!("PASS: Background executor spawn overhead <100us");
}

/// Test background executor with actual async/await tasks
#[test]
fn test_background_executor_async_integration() {
    init_tracing();
    tracing::info!("Testing background executor with async tasks");

    let executor = BackgroundExecutor::new();
    let completed = Arc::new(AtomicBool::new(false));
    let completed_clone = Arc::clone(&completed);

    // Spawn async task
    executor
        .spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            completed_clone.store(true, Ordering::SeqCst);
        })
        .detach();

    thread::sleep(Duration::from_millis(150));
    assert!(completed.load(Ordering::SeqCst));

    tracing::info!("PASS: Background executor integrates with async/await");
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
    executor
        .spawn(async {
            panic!("Intentional panic for testing");
        })
        .detach();

    thread::sleep(Duration::from_millis(50));

    // Spawn another task after panic
    executor
        .spawn(async move {
            post_panic_clone.store(true, Ordering::SeqCst);
        })
        .detach();

    thread::sleep(Duration::from_millis(50));

    // Verify executor still works after panic
    assert!(
        post_panic_executed.load(Ordering::SeqCst),
        "Executor should continue working after task panic"
    );

    tracing::info!("PASS: Background executor handles task panics gracefully");
}
