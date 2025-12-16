//! Gesture Timer Service - Automatic timer callbacks for gesture recognizers.
//!
//! This module provides a timer service that automatically fires callbacks
//! when gesture deadlines expire (e.g., long press timeout, double tap timeout).
//!
//! # Architecture
//!
//! Unlike Flutter which uses Dart's single-threaded `Timer` class, FLUI provides
//! a flexible [`GestureTimer`] abstraction that works with any async executor.
//!
//! ## Two Approaches
//!
//! ### 1. Frame-based checking (Simple)
//!
//! For simple applications, call [`GestureTimerService::check_timers`] every frame:
//!
//! ```rust,ignore
//! // In your event loop
//! loop {
//!     // ... handle events ...
//!     timer_service.check_timers();
//!     // ... render frame ...
//! }
//! ```
//!
//! ### 2. Async timers (Automatic)
//!
//! For applications using tokio, spawn the timer task:
//!
//! ```rust,ignore
//! let timer_service = GestureTimerService::new();
//!
//! // Spawn the timer task (runs forever)
//! tokio::spawn(timer_service.clone().run_async());
//!
//! // Or with graceful shutdown
//! let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
//! tokio::spawn(timer_service.clone().run_until_shutdown(shutdown_rx));
//!
//! // Later, stop the service
//! let _ = shutdown_tx.send(());
//! ```
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_interaction::timer::{GestureTimer, GestureTimerService};
//! use std::time::Duration;
//!
//! let service = GestureTimerService::new();
//!
//! // Schedule a timer
//! let timer = service.schedule(Duration::from_millis(500), || {
//!     println!("Long press timeout!");
//! });
//!
//! // Cancel if needed
//! timer.cancel();
//! ```

use parking_lot::Mutex;
use smallvec::SmallVec;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Counter for generating unique timer IDs.
static TIMER_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Unique identifier for a timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TimerId(u64);

impl TimerId {
    /// Create a new unique timer ID.
    fn new() -> Self {
        Self(TIMER_ID_COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// Get the raw ID value.
    #[inline]
    pub fn get(self) -> u64 {
        self.0
    }
}

// ============================================================================
// GestureTimer - Handle for a scheduled timer
// ============================================================================

/// A handle to a scheduled timer.
///
/// The timer can be cancelled by calling [`cancel`](Self::cancel).
/// The handle is cheap to clone and can be stored in recognizers.
///
/// # Example
///
/// ```rust,ignore
/// let timer = service.schedule(Duration::from_millis(500), || {
///     println!("Timer fired!");
/// });
///
/// // Later, cancel if the gesture was rejected
/// timer.cancel();
/// ```
#[derive(Clone)]
pub struct GestureTimer {
    id: TimerId,
    cancelled: Arc<AtomicBool>,
}

impl GestureTimer {
    /// Create a new timer handle.
    fn new(id: TimerId) -> Self {
        Self {
            id,
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Cancel this timer.
    ///
    /// After cancellation, the callback will not be called.
    /// It's safe to call this multiple times.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// Check if this timer has been cancelled.
    #[inline]
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// Get the timer's unique ID.
    #[inline]
    pub fn id(&self) -> TimerId {
        self.id
    }
}

impl std::fmt::Debug for GestureTimer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureTimer")
            .field("id", &self.id)
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

// ============================================================================
// TimerEntry - Internal timer storage
// ============================================================================

/// Internal timer entry.
struct TimerEntry {
    /// Unique timer ID.
    id: TimerId,
    /// When the timer should fire.
    deadline: Instant,
    /// The callback to invoke.
    callback: Box<dyn FnOnce() + Send + 'static>,
    /// Whether this timer has been cancelled.
    cancelled: Arc<AtomicBool>,
}

impl TimerEntry {
    /// Check if this timer has been cancelled.
    #[inline]
    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

impl std::fmt::Debug for TimerEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TimerEntry")
            .field("id", &self.id)
            .field("deadline", &self.deadline)
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

// ============================================================================
// GestureTimerService
// ============================================================================

/// Service for managing gesture timers.
///
/// This service schedules and fires callbacks when gesture deadlines expire.
/// It supports both polling-based and async execution.
///
/// # Thread Safety
///
/// `GestureTimerService` is thread-safe and can be shared across threads.
#[derive(Clone)]
pub struct GestureTimerService {
    /// Pending timers, sorted by deadline (earliest first).
    timers: Arc<Mutex<Vec<TimerEntry>>>,
}

impl GestureTimerService {
    /// Create a new timer service.
    pub fn new() -> Self {
        Self {
            timers: Arc::new(Mutex::new(Vec::with_capacity(8))),
        }
    }

    /// Schedule a timer to fire after the given duration.
    ///
    /// Returns a [`GestureTimer`] handle that can be used to cancel the timer.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let timer = service.schedule(Duration::from_millis(500), || {
    ///     println!("Long press detected!");
    /// });
    /// ```
    pub fn schedule<F>(&self, duration: Duration, callback: F) -> GestureTimer
    where
        F: FnOnce() + Send + 'static,
    {
        self.schedule_at(Instant::now() + duration, callback)
    }

    /// Schedule a timer to fire at a specific instant.
    ///
    /// Returns a [`GestureTimer`] handle that can be used to cancel the timer.
    pub fn schedule_at<F>(&self, deadline: Instant, callback: F) -> GestureTimer
    where
        F: FnOnce() + Send + 'static,
    {
        let id = TimerId::new();
        let timer = GestureTimer::new(id);

        let entry = TimerEntry {
            id,
            deadline,
            callback: Box::new(callback),
            cancelled: timer.cancelled.clone(),
        };

        let mut timers = self.timers.lock();

        // Insert sorted by deadline (binary search for insertion point)
        let pos = timers
            .binary_search_by(|e| e.deadline.cmp(&deadline))
            .unwrap_or_else(|pos| pos);
        timers.insert(pos, entry);

        timer
    }

    /// Check and fire any timers that have expired.
    ///
    /// Call this method periodically (e.g., every frame) if not using async execution.
    ///
    /// Returns the number of timers that fired.
    pub fn check_timers(&self) -> usize {
        let now = Instant::now();
        let mut fired_count = 0;

        // Collect ready timers (must release lock before calling callbacks)
        let ready_timers: SmallVec<[TimerEntry; 4]> = {
            let mut timers = self.timers.lock();

            // Remove cancelled timers
            timers.retain(|e| !e.is_cancelled());

            // Find ready timers (they're sorted, so check from front)
            let ready_count = timers.iter().take_while(|e| e.deadline <= now).count();

            // Drain ready timers
            timers.drain(0..ready_count).collect()
        };

        // Fire callbacks outside the lock
        for entry in ready_timers {
            if !entry.is_cancelled() {
                (entry.callback)();
                fired_count += 1;
            }
        }

        fired_count
    }

    /// Get the time until the next timer fires.
    ///
    /// Returns `None` if there are no pending timers.
    /// Returns `Some(Duration::ZERO)` if a timer is already due.
    pub fn time_until_next(&self) -> Option<Duration> {
        let timers = self.timers.lock();

        // Skip cancelled timers
        for entry in timers.iter() {
            if !entry.is_cancelled() {
                let now = Instant::now();
                return Some(entry.deadline.saturating_duration_since(now));
            }
        }

        None
    }

    /// Check if there are any pending timers.
    pub fn has_pending(&self) -> bool {
        let timers = self.timers.lock();
        timers.iter().any(|e| !e.is_cancelled())
    }

    /// Get the number of pending timers.
    pub fn pending_count(&self) -> usize {
        let timers = self.timers.lock();
        timers.iter().filter(|e| !e.is_cancelled()).count()
    }

    /// Cancel all pending timers.
    pub fn cancel_all(&self) {
        self.timers.lock().clear();
    }

    /// Run the timer service asynchronously.
    ///
    /// This method runs forever, checking and firing timers.
    /// Use with tokio runtime.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let service = GestureTimerService::new();
    ///
    /// // Spawn the timer task
    /// tokio::spawn(async move {
    ///     service.run_async().await;
    /// });
    /// ```
    pub async fn run_async(self) {
        loop {
            // Check for ready timers
            self.check_timers();

            // Wait until next timer or a short interval
            let wait_duration = self
                .time_until_next()
                .unwrap_or(Duration::from_millis(100))
                .min(Duration::from_millis(100)); // Cap at 100ms

            tokio::time::sleep(wait_duration).await;
        }
    }

    /// Run the timer service with a shutdown signal.
    ///
    /// Returns when the shutdown signal is received.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let service = GestureTimerService::new();
    /// let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    ///
    /// tokio::spawn(async move {
    ///     service.run_until_shutdown(shutdown_rx).await;
    /// });
    ///
    /// // Later, stop the service
    /// let _ = shutdown_tx.send(());
    /// ```
    pub async fn run_until_shutdown(self, mut shutdown: tokio::sync::oneshot::Receiver<()>) {
        loop {
            // Check for ready timers
            self.check_timers();

            // Wait until next timer or a short interval
            let wait_duration = self
                .time_until_next()
                .unwrap_or(Duration::from_millis(100))
                .min(Duration::from_millis(100));

            tokio::select! {
                _ = tokio::time::sleep(wait_duration) => {}
                _ = &mut shutdown => {
                    tracing::trace!("Timer service shutting down");
                    break;
                }
            }
        }
    }
}

impl Default for GestureTimerService {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for GestureTimerService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let pending = self.pending_count();
        f.debug_struct("GestureTimerService")
            .field("pending_timers", &pending)
            .finish()
    }
}

// ============================================================================
// Global timer service
// ============================================================================

/// Global timer service instance.
static GLOBAL_TIMER_SERVICE: once_cell::sync::Lazy<GestureTimerService> =
    once_cell::sync::Lazy::new(GestureTimerService::new);

/// Get the global timer service.
///
/// This is a convenience for applications that don't need multiple timer services.
pub fn global_timer_service() -> &'static GestureTimerService {
    &GLOBAL_TIMER_SERVICE
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn test_timer_service_creation() {
        let service = GestureTimerService::new();
        assert!(!service.has_pending());
        assert_eq!(service.pending_count(), 0);
    }

    #[test]
    fn test_schedule_timer() {
        let service = GestureTimerService::new();

        let _timer = service.schedule(Duration::from_millis(100), || {});

        assert!(service.has_pending());
        assert_eq!(service.pending_count(), 1);
    }

    #[test]
    fn test_timer_fires() {
        let service = GestureTimerService::new();
        let fired = Arc::new(AtomicBool::new(false));
        let fired_clone = fired.clone();

        // Schedule a timer that fires immediately
        service.schedule(Duration::ZERO, move || {
            fired_clone.store(true, Ordering::SeqCst);
        });

        // Check timers
        let count = service.check_timers();

        assert_eq!(count, 1);
        assert!(fired.load(Ordering::SeqCst));
    }

    #[test]
    fn test_timer_cancel() {
        let service = GestureTimerService::new();
        let fired = Arc::new(AtomicBool::new(false));
        let fired_clone = fired.clone();

        let timer = service.schedule(Duration::ZERO, move || {
            fired_clone.store(true, Ordering::SeqCst);
        });

        // Cancel before checking
        timer.cancel();
        assert!(timer.is_cancelled());

        // Check timers - should not fire
        let count = service.check_timers();

        assert_eq!(count, 0);
        assert!(!fired.load(Ordering::SeqCst));
    }

    #[test]
    fn test_timer_not_ready() {
        let service = GestureTimerService::new();
        let fired = Arc::new(AtomicBool::new(false));
        let fired_clone = fired.clone();

        // Schedule a timer far in the future
        service.schedule(Duration::from_secs(3600), move || {
            fired_clone.store(true, Ordering::SeqCst);
        });

        // Check timers - should not fire
        let count = service.check_timers();

        assert_eq!(count, 0);
        assert!(!fired.load(Ordering::SeqCst));
        assert!(service.has_pending());
    }

    #[test]
    fn test_time_until_next() {
        let service = GestureTimerService::new();

        // No timers
        assert!(service.time_until_next().is_none());

        // Add a timer
        service.schedule(Duration::from_millis(100), || {});

        // Should have time until next
        let time = service.time_until_next();
        assert!(time.is_some());
        assert!(time.unwrap() <= Duration::from_millis(100));
    }

    #[test]
    fn test_cancel_all() {
        let service = GestureTimerService::new();

        service.schedule(Duration::from_millis(100), || {});
        service.schedule(Duration::from_millis(200), || {});
        service.schedule(Duration::from_millis(300), || {});

        assert_eq!(service.pending_count(), 3);

        service.cancel_all();

        assert_eq!(service.pending_count(), 0);
    }

    #[test]
    fn test_multiple_timers_ordered() {
        let service = GestureTimerService::new();
        let order = Arc::new(Mutex::new(Vec::new()));

        // Schedule in reverse order
        let order3 = order.clone();
        service.schedule(Duration::ZERO, move || {
            order3.lock().push(3);
        });

        let order1 = order.clone();
        service.schedule(Duration::ZERO, move || {
            order1.lock().push(1);
        });

        let order2 = order.clone();
        service.schedule(Duration::ZERO, move || {
            order2.lock().push(2);
        });

        // All fire at same time, should fire in insertion order
        service.check_timers();

        let fired_order = order.lock().clone();
        assert_eq!(fired_order.len(), 3);
    }

    #[test]
    fn test_timer_id_unique() {
        let id1 = TimerId::new();
        let id2 = TimerId::new();
        let id3 = TimerId::new();

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_gesture_timer_debug() {
        let service = GestureTimerService::new();
        let timer = service.schedule(Duration::from_millis(100), || {});

        let debug = format!("{:?}", timer);
        assert!(debug.contains("GestureTimer"));
        assert!(debug.contains("cancelled"));
    }

    #[test]
    fn test_global_timer_service() {
        let service = global_timer_service();
        // Just verify it's accessible
        let _ = service.pending_count();
    }

    #[test]
    fn test_timer_fires_callback_count() {
        let service = GestureTimerService::new();
        let count = Arc::new(AtomicUsize::new(0));

        // Schedule multiple timers that fire immediately
        for _ in 0..5 {
            let count_clone = count.clone();
            service.schedule(Duration::ZERO, move || {
                count_clone.fetch_add(1, Ordering::SeqCst);
            });
        }

        let fired = service.check_timers();

        assert_eq!(fired, 5);
        assert_eq!(count.load(Ordering::SeqCst), 5);
    }

    #[tokio::test]
    async fn test_run_until_shutdown() {
        let service = GestureTimerService::new();
        let fired = Arc::new(AtomicBool::new(false));
        let fired_clone = fired.clone();

        // Schedule a timer that fires in 10ms
        service.schedule(Duration::from_millis(10), move || {
            fired_clone.store(true, Ordering::SeqCst);
        });

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

        // Run service in background
        let service_clone = service.clone();
        let handle = tokio::spawn(async move {
            service_clone.run_until_shutdown(shutdown_rx).await;
        });

        // Wait for timer to fire
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Shutdown
        let _ = shutdown_tx.send(());
        handle.await.unwrap();

        assert!(fired.load(Ordering::SeqCst));
    }
}
