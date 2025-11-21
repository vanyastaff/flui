//! Integration tests for thread safety
//!
//! This test suite verifies that the flui-reactivity crate is truly thread-safe
//! by spawning multiple threads and performing concurrent operations on signals.

use flui_reactivity::{batch, Signal};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Test concurrent signal creation from multiple threads
#[test]
fn test_concurrent_signal_creation() {
    const NUM_THREADS: usize = 10;
    const SIGNALS_PER_THREAD: usize = 100;

    let handles: Vec<_> = (0..NUM_THREADS)
        .map(|i| {
            thread::spawn(move || {
                for j in 0..SIGNALS_PER_THREAD {
                    let signal = Signal::new(i * SIGNALS_PER_THREAD + j);
                    assert_eq!(signal.get(), i * SIGNALS_PER_THREAD + j);
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("Thread panicked");
    }
}

/// Test concurrent signal reads/writes from multiple threads
#[test]
fn test_concurrent_signal_updates() {
    const NUM_THREADS: usize = 10;
    const UPDATES_PER_THREAD: usize = 100;

    let signal = Signal::new(0);
    let signal_clone = signal.clone();

    // Spawn multiple writer threads
    let handles: Vec<_> = (0..NUM_THREADS)
        .map(|_| {
            let sig = signal_clone.clone();
            thread::spawn(move || {
                for _ in 0..UPDATES_PER_THREAD {
                    sig.update(|v| v + 1);
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("Thread panicked");
    }

    // Verify final value
    assert_eq!(signal.get(), NUM_THREADS * UPDATES_PER_THREAD);
}

/// Test concurrent subscriptions from multiple threads
#[test]
fn test_concurrent_subscriptions() {
    const NUM_THREADS: usize = 10;
    const SUBSCRIPTIONS_PER_THREAD: usize = 50;

    let signal = Signal::new(0);
    let counter = Arc::new(AtomicU32::new(0));

    // Spawn multiple threads that subscribe
    let handles: Vec<_> = (0..NUM_THREADS)
        .map(|_| {
            let sig = signal.clone();
            let cnt = counter.clone();
            thread::spawn(move || {
                for _ in 0..SUBSCRIPTIONS_PER_THREAD {
                    let c = cnt.clone();
                    sig.subscribe(move || {
                        c.fetch_add(1, Ordering::SeqCst);
                    })
                    .expect("Subscription failed");
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("Thread panicked");
    }

    // Trigger all subscriptions
    signal.set(1);

    // Wait for all notifications
    thread::sleep(Duration::from_millis(100));

    // Verify all subscribers were notified
    assert_eq!(
        counter.load(Ordering::SeqCst),
        (NUM_THREADS * SUBSCRIPTIONS_PER_THREAD) as u32
    );
}

/// Test batch updates from multiple threads simultaneously
#[test]
fn test_concurrent_batch_updates() {
    const NUM_THREADS: usize = 10;

    let signal = Signal::new(0);
    let counter = Arc::new(AtomicU32::new(0));

    // Subscribe once
    let cnt = counter.clone();
    signal
        .subscribe(move || {
            cnt.fetch_add(1, Ordering::SeqCst);
        })
        .expect("Subscription failed");

    // Spawn multiple threads that perform batch updates
    let handles: Vec<_> = (0..NUM_THREADS)
        .map(|_| {
            let sig = signal.clone();
            thread::spawn(move || {
                batch(|| {
                    sig.update(|v| v + 1);
                    sig.update(|v| v + 1);
                    sig.update(|v| v + 1);
                });
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("Thread panicked");
    }

    // Wait for all notifications
    thread::sleep(Duration::from_millis(100));

    // Each thread's batch should trigger 1 notification (deduped)
    // So we expect NUM_THREADS notifications
    assert_eq!(counter.load(Ordering::SeqCst), NUM_THREADS as u32);

    // Signal value should be 3 * NUM_THREADS (3 updates per thread)
    assert_eq!(signal.get(), 3 * NUM_THREADS);
}

/// Test nested batch updates from multiple threads
#[test]
fn test_concurrent_nested_batches() {
    const NUM_THREADS: usize = 5;

    let signal = Signal::new(0);
    let counter = Arc::new(AtomicU32::new(0));

    let cnt = counter.clone();
    signal
        .subscribe(move || {
            cnt.fetch_add(1, Ordering::SeqCst);
        })
        .expect("Subscription failed");

    let handles: Vec<_> = (0..NUM_THREADS)
        .map(|_| {
            let sig = signal.clone();
            thread::spawn(move || {
                batch(|| {
                    sig.update(|v| v + 1);
                    batch(|| {
                        sig.update(|v| v + 1);
                        batch(|| {
                            sig.update(|v| v + 1);
                        });
                    });
                });
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("Thread panicked");
    }

    thread::sleep(Duration::from_millis(100));

    // Nested batches should still dedupe to 1 notification per thread
    assert_eq!(counter.load(Ordering::SeqCst), NUM_THREADS as u32);
    assert_eq!(signal.get(), 3 * NUM_THREADS);
}

/// Test concurrent subscribe/unsubscribe operations
#[test]
fn test_concurrent_subscribe_unsubscribe() {
    const NUM_THREADS: usize = 10;
    const ITERATIONS: usize = 100;

    let signal = Signal::new(0);

    let handles: Vec<_> = (0..NUM_THREADS)
        .map(|_| {
            let sig = signal.clone();
            thread::spawn(move || {
                for _ in 0..ITERATIONS {
                    // Subscribe
                    let sub = sig.subscribe(|| {}).expect("Subscription failed");

                    // Unsubscribe immediately
                    sig.unsubscribe(sub);
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("Thread panicked");
    }

    // Signal should still be usable
    signal.set(42);
    assert_eq!(signal.get(), 42);
}

/// Test signal memory cleanup under concurrent load
#[test]
fn test_concurrent_signal_cleanup() {
    const NUM_THREADS: usize = 10;
    const SIGNALS_PER_THREAD: usize = 1000;

    let handles: Vec<_> = (0..NUM_THREADS)
        .map(|_| {
            thread::spawn(move || {
                for i in 0..SIGNALS_PER_THREAD {
                    let signal = Signal::new(i);
                    signal.set(i + 1);
                    // Signal drops here, should cleanup properly
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("Thread panicked");
    }

    // If cleanup is broken, this test will leak memory
    // Run with valgrind or miri to verify
}

/// Test that parallel batch updates maintain consistency
#[test]
fn test_parallel_batch_consistency() {
    const NUM_THREADS: usize = 20;

    let sig_a = Signal::new(0);
    let sig_b = Signal::new(0);
    let notification_count = Arc::new(AtomicU32::new(0));

    // Subscribe to both signals
    let cnt = notification_count.clone();
    sig_a
        .subscribe(move || {
            cnt.fetch_add(1, Ordering::SeqCst);
        })
        .expect("Subscription failed");

    let cnt = notification_count.clone();
    sig_b
        .subscribe(move || {
            cnt.fetch_add(1, Ordering::SeqCst);
        })
        .expect("Subscription failed");

    // Each thread updates both signals in a batch
    let handles: Vec<_> = (0..NUM_THREADS)
        .map(|_| {
            let sa = sig_a.clone();
            let sb = sig_b.clone();
            thread::spawn(move || {
                batch(|| {
                    sa.update(|v| v + 1);
                    sb.update(|v| v + 1);
                });
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("Thread panicked");
    }

    thread::sleep(Duration::from_millis(100));

    // Each batch should trigger 2 notifications (one per signal)
    assert_eq!(
        notification_count.load(Ordering::SeqCst),
        (NUM_THREADS * 2) as u32
    );

    // Both signals should have same final value
    assert_eq!(sig_a.get(), NUM_THREADS);
    assert_eq!(sig_b.get(), NUM_THREADS);
}

/// Test that thread-local batching state is truly thread-local
#[test]
fn test_thread_local_batching_isolation() {
    const NUM_THREADS: usize = 10;

    let signal = Signal::new(0);
    let barrier = Arc::new(std::sync::Barrier::new(NUM_THREADS));

    let handles: Vec<_> = (0..NUM_THREADS)
        .map(|i| {
            let sig = signal.clone();
            let bar = barrier.clone();
            thread::spawn(move || {
                // Start batch on all threads simultaneously
                batch(|| {
                    bar.wait(); // Synchronize all threads

                    // Each thread updates in its own batch
                    sig.update(|v| v + 1);

                    // Sleep to ensure other threads are still in their batches
                    thread::sleep(Duration::from_millis(10));

                    // Another update in same batch
                    sig.update(|v| v + 1);

                    if i == 0 {
                        // First thread does nested batch
                        batch(|| {
                            sig.update(|v| v + 1);
                        });
                    }
                });
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("Thread panicked");
    }

    // Final value: 2 updates per thread + 1 extra from thread 0
    assert_eq!(signal.get(), 2 * NUM_THREADS + 1);
}

/// Stress test with high contention
#[test]
#[ignore] // Run with `cargo test -- --ignored` for stress testing
fn stress_test_high_contention() {
    const NUM_THREADS: usize = 100;
    const OPERATIONS_PER_THREAD: usize = 10000;

    let signal = Signal::new(0_u64);
    let signal_clone = signal.clone();

    let start = std::time::Instant::now();

    let handles: Vec<_> = (0..NUM_THREADS)
        .map(|_| {
            let sig = signal_clone.clone();
            thread::spawn(move || {
                for _ in 0..OPERATIONS_PER_THREAD {
                    sig.update(|v| v + 1);
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("Thread panicked");
    }

    let elapsed = start.elapsed();

    println!(
        "Stress test completed in {:?}: {} operations/sec",
        elapsed,
        (NUM_THREADS * OPERATIONS_PER_THREAD) as f64 / elapsed.as_secs_f64()
    );

    assert_eq!(signal.get(), (NUM_THREADS * OPERATIONS_PER_THREAD) as u64);
}
