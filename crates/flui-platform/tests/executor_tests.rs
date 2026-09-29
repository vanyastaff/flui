//! Executor System Tests
//!
//! Tests for background executor thread safety, task execution, and overhead.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
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
