//! Performance Tests (Phase 6: T072)
//!
//! Tests to ensure test suite execution meets performance targets.

use std::process::Command;
use std::time::Instant;

#[test]
#[ignore] // Run explicitly: cargo test -p flui-platform --test performance -- --ignored
fn test_t072_full_test_suite_under_30s() {
    // T072: Full test suite completes in <30 seconds with FLUI_HEADLESS=1

    let start = Instant::now();

    let output = Command::new("cargo")
        .args(["test", "-p", "flui-platform", "--", "--test-threads=1"])
        .env("FLUI_HEADLESS", "1")
        .output()
        .expect("Failed to run test suite");

    let duration = start.elapsed();

    println!("Test suite completed in {:.2}s", duration.as_secs_f64());
    println!("Exit status: {}", output.status);

    // Verify tests passed
    assert!(
        output.status.success(),
        "Test suite failed. Stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Performance requirement: <30 seconds
    assert!(
        duration.as_secs() < 30,
        "Test suite took {:.2}s, expected <30s. This may indicate performance regression.",
        duration.as_secs_f64()
    );
}

#[test]
#[ignore] // Run explicitly: cargo test -p flui-platform --test performance -- --ignored
fn test_headless_platform_startup_under_10ms() {
    // Verify headless platform has minimal startup overhead

    use flui_platform::headless_platform;

    let start = Instant::now();
    let _platform = headless_platform();
    let duration = start.elapsed();

    println!(
        "Headless platform startup: {:.3}ms",
        duration.as_secs_f64() * 1000.0
    );

    assert!(
        duration.as_millis() < 10,
        "Headless platform startup took {}ms, expected <10ms",
        duration.as_millis()
    );
}

#[test]
#[ignore] // Run explicitly: cargo test -p flui-platform --test performance -- --ignored
fn test_headless_window_creation_under_1ms() {
    // Verify headless window creation has minimal overhead

    use flui_platform::{headless_platform, WindowOptions};
    use flui_types::geometry::{px, Size};

    let platform = headless_platform();

    let options = WindowOptions {
        title: "Benchmark".to_string(),
        size: Size::new(px(800.0), px(600.0)),
        visible: true,
        ..Default::default()
    };

    let start = Instant::now();
    let _window = platform
        .open_window(options)
        .expect("Failed to create window");
    let duration = start.elapsed();

    println!(
        "Headless window creation: {:.3}ms",
        duration.as_secs_f64() * 1000.0
    );

    assert!(
        duration.as_millis() < 1,
        "Headless window creation took {}μs, expected <1ms",
        duration.as_micros()
    );
}

#[test]
fn test_parallel_test_execution_scales() {
    // Verify parallel test execution provides speedup

    use flui_platform::headless_platform;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let counter = Arc::new(AtomicUsize::new(0));
    let num_threads = 8;
    let tasks_per_thread = 100;

    let start = Instant::now();

    let handles: Vec<_> = (0..num_threads)
        .map(|_| {
            let counter = Arc::clone(&counter);
            std::thread::spawn(move || {
                for _ in 0..tasks_per_thread {
                    let _platform = headless_platform();
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("Thread panicked");
    }

    let duration = start.elapsed();
    let total_tasks = num_threads * tasks_per_thread;
    let tasks_per_sec = total_tasks as f64 / duration.as_secs_f64();

    println!(
        "Parallel execution: {} tasks in {:.2}s ({:.0} tasks/sec)",
        total_tasks,
        duration.as_secs_f64(),
        tasks_per_sec
    );

    assert_eq!(
        counter.load(Ordering::SeqCst),
        total_tasks,
        "Not all tasks completed"
    );

    // Should complete 800 tasks in reasonable time (no contention issues)
    assert!(
        duration.as_secs() < 5,
        "Parallel execution took too long: {:.2}s",
        duration.as_secs_f64()
    );
}

#[test]
fn test_clipboard_operations_performance() {
    // Verify clipboard operations have minimal overhead in headless mode

    use flui_platform::headless_platform;

    let platform = headless_platform();
    let clipboard = platform.clipboard();

    let iterations = 1000;
    let test_text = "Performance test string";

    let start = Instant::now();

    for _ in 0..iterations {
        clipboard.write_text(test_text.to_string());
        let _ = clipboard.read_text();
    }

    let duration = start.elapsed();
    let ops_per_sec = (iterations * 2) as f64 / duration.as_secs_f64();

    println!(
        "Clipboard operations: {} read/write pairs in {:.2}ms ({:.0} ops/sec)",
        iterations,
        duration.as_secs_f64() * 1000.0,
        ops_per_sec
    );

    // Should be able to do 1000 read/write pairs quickly
    assert!(
        duration.as_millis() < 100,
        "Clipboard operations took {}ms, expected <100ms",
        duration.as_millis()
    );
}

#[test]
fn test_executor_spawn_performance() {
    // Verify executor spawn has minimal overhead

    use flui_platform::headless_platform;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    let platform = headless_platform();
    let executor = platform.background_executor();

    let counter = Arc::new(AtomicUsize::new(0));
    let tasks = 100;

    let start = Instant::now();

    for _ in 0..tasks {
        let counter = Arc::clone(&counter);
        executor.spawn(Box::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }));
    }

    // Give tasks time to complete
    std::thread::sleep(std::time::Duration::from_millis(200));

    let duration = start.elapsed();

    println!(
        "Spawned {} tasks in {:.3}ms",
        tasks,
        duration.as_secs_f64() * 1000.0
    );

    assert_eq!(
        counter.load(Ordering::SeqCst),
        tasks,
        "Not all tasks executed"
    );

    // Spawning tasks should be fast (<100ms for 100 tasks)
    assert!(
        duration.as_millis() < 300,
        "Executor spawn took {}ms, expected <300ms",
        duration.as_millis()
    );
}
