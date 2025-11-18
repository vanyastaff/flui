//! Batch updates for multiple signals.
//!
//! This module provides utilities for updating multiple signals atomically,
//! deferring notifications until all updates are complete.

use parking_lot::Mutex;
use std::collections::HashMap;
use tracing::{debug, trace, warn};

use crate::signal::SignalId;

/// Maximum number of pending notifications before forcing a flush.
///
/// This prevents unbounded memory growth in long-lived threads.
/// If this limit is reached, notifications are flushed early with a warning.
const MAX_PENDING_NOTIFICATIONS: usize = 10_000;

/// Maximum nesting depth for batch() calls.
///
/// This prevents stack overflow from deeply nested batches.
/// Warns at depth 10, panics at depth 20.
const MAX_BATCH_DEPTH: usize = 20;
const WARN_BATCH_DEPTH: usize = 10;

thread_local! {
    /// Thread-local batch update state.
    ///
    /// Using thread-local instead of global atomic to prevent race conditions
    /// where Thread A's batching state would affect Thread B.
    static BATCHING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };

    /// Current batch nesting depth.
    static BATCH_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };

    /// Pending notifications deduped by SignalId.
    ///
    /// Each signal only notifies once per batch, even if set() is called multiple times.
    static PENDING_NOTIFICATIONS: Mutex<HashMap<SignalId, Box<dyn FnOnce() + Send>>> =
        Mutex::new(HashMap::new());
}

/// Check if currently in a batch update context.
///
/// This checks the thread-local batching state, so each thread has its own
/// independent batching context.
pub fn is_batching() -> bool {
    BATCHING.with(|b| b.get())
}

/// Queue a notification to be executed after the batch completes.
///
/// Notifications are deduped by SignalId, so each signal only notifies once per batch
/// even if set() is called multiple times.
///
/// If the number of pending notifications exceeds `MAX_PENDING_NOTIFICATIONS`,
/// this function will flush all pending notifications immediately to prevent
/// unbounded memory growth.
pub(crate) fn queue_notification<F>(signal_id: SignalId, f: F)
where
    F: FnOnce() + Send + 'static,
{
    if is_batching() {
        PENDING_NOTIFICATIONS.with(|pending| {
            let mut pending = pending.lock();

            // Check if we've exceeded the limit
            if pending.len() >= MAX_PENDING_NOTIFICATIONS {
                warn!(
                    "Pending notifications exceeded limit ({}), flushing early to prevent memory leak",
                    MAX_PENDING_NOTIFICATIONS
                );

                // CRITICAL FIX: Insert current notification BEFORE flushing
                // This prevents race condition where another thread could flush
                // between our lock release and re-acquisition
                pending.insert(signal_id, Box::new(f));
                trace!(
                    signal_id = ?signal_id,
                    "Current notification added before emergency flush"
                );

                // Take all pending notifications (including current one)
                let notifications = std::mem::take(&mut *pending);

                // Release lock before executing
                drop(pending);

                // Execute all pending notifications with panic safety
                debug!(count = notifications.len(), "Flushing pending notifications (emergency flush)");
                for (signal_id, notification) in notifications {
                    if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        notification();
                    })) {
                        warn!(
                            signal_id = ?signal_id,
                            error = ?e,
                            "Notification panicked during emergency flush - continuing with remaining notifications"
                        );
                    }
                }

                // No need to re-acquire lock - current notification already executed
            } else {
                // Deduplicate: replace any existing notification for this signal
                pending.insert(signal_id, Box::new(f));
                trace!(signal_id = ?signal_id, count = pending.len(), "Notification queued for batch");
            }
        });
    } else {
        // Execute immediately if not batching
        f();
    }
}

/// Execute a function within a **thread-local** batch update context.
///
/// All signal updates within the closure will defer their notifications
/// until the batch completes. This is useful for updating multiple signals
/// atomically without triggering intermediate updates.
///
/// # Thread-Local Batching
///
/// **IMPORTANT:** Batching is **thread-local**. Each thread maintains its own
/// independent batch queue. Cross-thread signals will **NOT** be deduplicated
/// across thread boundaries.
///
/// ```rust,ignore
/// use flui_reactivity::{Signal, batch};
/// use std::thread;
///
/// let signal = Signal::new(0);
///
/// // Thread A batch (independent)
/// thread::spawn(move || {
///     batch(|| {
///         signal.set(1);  // Queued in Thread A's batch
///         signal.set(2);  // Deduplicated in Thread A
///     }); // Notification sent from Thread A
/// });
///
/// // Thread B batch (independent)
/// thread::spawn(move || {
///     batch(|| {
///         signal.set(3);  // Queued in Thread B's batch (separate queue!)
///     }); // Notification sent from Thread B
/// });
/// // Total: 2 notifications sent (one per thread, no cross-thread deduplication)
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use flui_reactivity::{Signal, batch};
///
/// let x = Signal::new(0);
/// let y = Signal::new(0);
/// let sum = Computed::new(move || x.get() + y.get());
///
/// let _sub = sum.subscribe(|| println!("Sum changed!"));
///
/// batch(|| {
///     x.set(10);  // No notification yet
///     y.set(20);  // No notification yet
/// }); // "Sum changed!" printed once here
/// ```
pub fn batch<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    // Check batch depth limit
    let depth = BATCH_DEPTH.with(|d| {
        let current = d.get();
        d.set(current + 1);
        current + 1
    });

    if depth >= MAX_BATCH_DEPTH {
        panic!(
            "Maximum batch nesting depth ({}) exceeded! Current depth: {}. \
             This likely indicates a recursive batching bug.",
            MAX_BATCH_DEPTH, depth
        );
    }

    if depth >= WARN_BATCH_DEPTH {
        warn!(
            "Batch nesting depth ({}) is approaching limit ({}). \
             Consider refactoring to reduce nesting.",
            depth, MAX_BATCH_DEPTH
        );
    }

    // Check if already batching (nested batch) - thread-local state
    let was_batching = BATCHING.with(|b| {
        let prev = b.get();
        b.set(true);
        prev
    });

    if was_batching {
        // Already in batch, just run the function
        trace!(depth, "Nested batch detected");
        let result = f();
        BATCH_DEPTH.with(|d| d.set(d.get() - 1));
        return result;
    }

    debug!(depth, "Starting batch update");

    // Run the function
    let result = f();

    // End batching - thread-local state
    BATCHING.with(|b| b.set(false));

    // Execute all pending notifications
    let notifications = PENDING_NOTIFICATIONS.with(|pending| std::mem::take(&mut *pending.lock()));

    let count = notifications.len();
    debug!(count, "Executing pending notifications");

    // Execute notifications with panic safety
    for (signal_id, notification) in notifications {
        if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            notification();
        })) {
            warn!(
                signal_id = ?signal_id,
                error = ?e,
                "Notification panicked during batch flush - continuing with remaining notifications"
            );
        }
    }

    trace!("Batch update complete");

    // Decrement batch depth
    BATCH_DEPTH.with(|d| d.set(d.get() - 1));

    result
}

/// RAII guard for batch updates.
///
/// Automatically starts batching on creation and flushes on drop.
///
/// # Example
///
/// ```rust,ignore
/// use flui_reactivity::{Signal, BatchGuard};
///
/// let x = Signal::new(0);
/// let y = Signal::new(0);
///
/// {
///     let _batch = BatchGuard::new();
///     x.set(10);
///     y.set(20);
/// } // Notifications executed on drop
/// ```
pub struct BatchGuard {
    was_batching: bool,
}

impl BatchGuard {
    /// Create a new batch guard.
    pub fn new() -> Self {
        let was_batching = BATCHING.with(|b| {
            let prev = b.get();
            b.set(true);
            prev
        });

        if !was_batching {
            debug!("BatchGuard created");
        }

        Self { was_batching }
    }
}

impl Default for BatchGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for BatchGuard {
    fn drop(&mut self) {
        if !self.was_batching {
            // End batching - thread-local state
            BATCHING.with(|b| b.set(false));

            // Execute all pending notifications
            let notifications =
                PENDING_NOTIFICATIONS.with(|pending| std::mem::take(&mut *pending.lock()));

            let count = notifications.len();
            if count > 0 {
                debug!(count, "BatchGuard executing pending notifications");

                for (_, notification) in notifications {
                    notification();
                }
            }

            trace!("BatchGuard dropped");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Signal;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_batch_basic() {
        let count = Signal::new(0);
        let notification_count = Arc::new(AtomicU32::new(0));

        let nc = notification_count.clone();
        let _sub = count
            .subscribe(move || {
                nc.fetch_add(1, Ordering::SeqCst);
            })
            .expect("Failed to subscribe");

        batch(|| {
            count.set(1);
            count.set(2);
            count.set(3);
        });

        // Should notify only once after batch
        assert_eq!(notification_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_batch_guard() {
        let count = Signal::new(0);
        let notification_count = Arc::new(AtomicU32::new(0));

        let nc = notification_count.clone();
        let _sub = count
            .subscribe(move || {
                nc.fetch_add(1, Ordering::SeqCst);
            })
            .expect("Failed to subscribe");

        {
            let _batch = BatchGuard::new();
            count.set(1);
            count.set(2);
            assert_eq!(notification_count.load(Ordering::SeqCst), 0); // Not yet notified
        } // Drop triggers flush

        // Should notify once after guard drops
        assert_eq!(notification_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_nested_batch() {
        let count = Signal::new(0);
        let notification_count = Arc::new(AtomicU32::new(0));

        let nc = notification_count.clone();
        let _sub = count
            .subscribe(move || {
                nc.fetch_add(1, Ordering::SeqCst);
            })
            .expect("Failed to subscribe");

        batch(|| {
            count.set(1);

            batch(|| {
                count.set(2);
            });

            count.set(3);
        });

        // Should notify only once for the outer batch
        assert_eq!(notification_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_no_batch() {
        let count = Signal::new(0);
        let notification_count = Arc::new(AtomicU32::new(0));

        let nc = notification_count.clone();
        let _sub = count
            .subscribe(move || {
                nc.fetch_add(1, Ordering::SeqCst);
            })
            .expect("Failed to subscribe");

        // Without batching, each set() notifies immediately
        count.set(1);
        count.set(2);
        count.set(3);

        assert_eq!(notification_count.load(Ordering::SeqCst), 3);
    }
}
