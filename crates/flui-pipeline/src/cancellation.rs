//! Cancellation support for pipeline operations
//!
//! Provides a thread-safe cancellation token that can be used to
//! cancel long-running pipeline operations (build, layout, paint).
//!
//! # Example
//!
//! ```rust
//! use flui_pipeline::CancellationToken;
//! use std::time::Duration;
//!
//! let token = CancellationToken::new();
//! token.set_timeout(Duration::from_millis(16));
//!
//! // In pipeline code:
//! // if token.is_cancelled() { return Err(PipelineError::cancelled("timeout")); }
//!
//! // Manual cancellation:
//! token.cancel();
//! assert!(token.is_cancelled());
//! ```

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Thread-safe cancellation token for pipeline operations
///
/// Can be cancelled manually or automatically via timeout.
/// All methods are lock-free for minimal overhead.
#[derive(Debug)]
pub struct CancellationToken {
    /// Whether cancellation has been requested
    cancelled: AtomicBool,

    /// Deadline timestamp (nanoseconds since epoch, 0 = no deadline)
    deadline_ns: AtomicU64,

    /// Start time for timeout calculation
    start: Instant,
}

impl CancellationToken {
    /// Create a new cancellation token
    pub fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            deadline_ns: AtomicU64::new(0),
            start: Instant::now(),
        }
    }

    /// Create a token with a timeout
    pub fn with_timeout(timeout: Duration) -> Self {
        let token = Self::new();
        token.set_timeout(timeout);
        token
    }

    /// Set a timeout from now
    ///
    /// The token will report as cancelled after the timeout expires.
    pub fn set_timeout(&self, timeout: Duration) {
        let deadline = self.start.elapsed() + timeout;
        self.deadline_ns
            .store(deadline.as_nanos() as u64, Ordering::Release);
    }

    /// Clear the timeout
    pub fn clear_timeout(&self) {
        self.deadline_ns.store(0, Ordering::Release);
    }

    /// Cancel the token manually
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    /// Check if cancelled (manual or timeout)
    #[inline]
    pub fn is_cancelled(&self) -> bool {
        // Check manual cancellation first
        if self.cancelled.load(Ordering::Acquire) {
            return true;
        }

        // Check timeout
        let deadline_ns = self.deadline_ns.load(Ordering::Acquire);
        if deadline_ns > 0 {
            let elapsed_ns = self.start.elapsed().as_nanos() as u64;
            if elapsed_ns >= deadline_ns {
                return true;
            }
        }

        false
    }

    /// Reset the token for reuse
    ///
    /// Clears cancellation flag and resets the start time.
    pub fn reset(&mut self) {
        self.cancelled.store(false, Ordering::Release);
        self.deadline_ns.store(0, Ordering::Release);
        self.start = Instant::now();
    }

    /// Get remaining time before timeout
    ///
    /// Returns `None` if no timeout is set or already expired.
    pub fn remaining(&self) -> Option<Duration> {
        let deadline_ns = self.deadline_ns.load(Ordering::Acquire);
        if deadline_ns == 0 {
            return None;
        }

        let elapsed_ns = self.start.elapsed().as_nanos() as u64;
        if elapsed_ns >= deadline_ns {
            return None;
        }

        Some(Duration::from_nanos(deadline_ns - elapsed_ns))
    }

    /// Check if there's a timeout set
    pub fn has_timeout(&self) -> bool {
        self.deadline_ns.load(Ordering::Acquire) > 0
    }

    /// Get the timeout duration (if set)
    pub fn timeout(&self) -> Option<Duration> {
        let deadline_ns = self.deadline_ns.load(Ordering::Acquire);
        if deadline_ns == 0 {
            None
        } else {
            Some(Duration::from_nanos(deadline_ns))
        }
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for CancellationToken {
    fn clone(&self) -> Self {
        Self {
            cancelled: AtomicBool::new(self.cancelled.load(Ordering::Acquire)),
            deadline_ns: AtomicU64::new(self.deadline_ns.load(Ordering::Acquire)),
            start: self.start,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_manual_cancellation() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());

        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn test_timeout_cancellation() {
        let token = CancellationToken::with_timeout(Duration::from_millis(10));
        assert!(!token.is_cancelled());

        // Wait for timeout
        thread::sleep(Duration::from_millis(20));
        assert!(token.is_cancelled());
    }

    #[test]
    fn test_remaining_time() {
        let token = CancellationToken::new();
        assert!(token.remaining().is_none());

        token.set_timeout(Duration::from_secs(10));
        let remaining = token.remaining().unwrap();
        assert!(remaining > Duration::from_secs(9));
    }

    #[test]
    fn test_reset() {
        let mut token = CancellationToken::new();
        token.cancel();
        assert!(token.is_cancelled());

        token.reset();
        assert!(!token.is_cancelled());
    }

    #[test]
    fn test_clear_timeout() {
        let token = CancellationToken::with_timeout(Duration::from_millis(10));
        assert!(token.has_timeout());

        token.clear_timeout();
        assert!(!token.has_timeout());

        // Should not be cancelled after timeout would have expired
        thread::sleep(Duration::from_millis(20));
        assert!(!token.is_cancelled());
    }

    #[test]
    fn test_clone() {
        let token1 = CancellationToken::new();
        token1.set_timeout(Duration::from_secs(10));

        let token2 = token1.clone();

        token1.cancel();
        // Clone should have same state at time of clone
        assert!(token1.is_cancelled());
        // But not affected by subsequent changes
        assert!(!token2.cancelled.load(Ordering::Acquire));
    }
}
