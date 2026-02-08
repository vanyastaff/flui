//! Scheduler configuration types and utilities.
//!
//! This module provides standalone types used by the [`Scheduler`](crate::Scheduler):
//!
//! - **Time dilation**: Slow down animations for debugging
//! - **Performance mode**: Hint to the runtime about expected workload
//! - **Service extensions**: Debug/dev tool integration points
//! - **Scheduling strategy**: Customizable task execution policy
//! - **Timings callbacks**: Frame performance reporting

use crate::frame::FrameTiming;
use crate::scheduler::Scheduler;
use flui_foundation::{BindingBase, HasInstance};

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

// ============================================================================
// Callback Type Aliases
// ============================================================================

/// Timings callback for receiving [`FrameTiming`] reports from the engine.
///
/// Callbacks receive batched `FrameTiming` data approximately once per second
/// in release mode, or every ~100ms in debug/profile builds.
pub type TimingsCallback = Arc<dyn Fn(&[FrameTiming]) + Send + Sync>;

/// Scheduling strategy callback.
///
/// Called to determine whether a task at a given priority should run.
/// Returns `true` if the task should execute, `false` to defer.
pub type SchedulingStrategy = Box<dyn Fn(crate::task::Priority, &Scheduler) -> bool + Send + Sync>;

/// Default scheduling strategy — runs tasks when not over budget.
pub fn default_scheduling_strategy(priority: crate::task::Priority, scheduler: &Scheduler) -> bool {
    // Always run high priority tasks (Animation, UserInput)
    if priority >= crate::task::Priority::Animation {
        return true;
    }

    // Run lower priority tasks only if we have budget remaining
    !scheduler.is_over_budget()
}

// ============================================================================
// Time Dilation
// ============================================================================

/// Global time dilation factor for animations.
///
/// This slows down animations by the given factor to help with development.
/// A value of 1.0 means normal speed, 2.0 means half speed, etc.
///
/// # Thread Safety
///
/// This uses atomic operations and is safe to access from any thread.
static TIME_DILATION: AtomicU64 = AtomicU64::new(0x3FF0_0000_0000_0000); // 1.0 as f64 bits

/// Get the current time dilation factor.
///
/// # Example
///
/// ```rust
/// use flui_scheduler::config::time_dilation;
///
/// let dilation = time_dilation();
/// assert_eq!(dilation, 1.0); // Default is normal speed
/// ```
#[inline]
pub fn time_dilation() -> f64 {
    f64::from_bits(TIME_DILATION.load(Ordering::Relaxed))
}

/// Set the time dilation factor.
///
/// # Panics
///
/// Panics if `value` is not positive (must be > 0.0).
///
/// # Example
///
/// ```rust
/// use flui_scheduler::config::{set_time_dilation, time_dilation};
///
/// // Slow down animations to 50% speed
/// set_time_dilation(2.0);
/// assert_eq!(time_dilation(), 2.0);
///
/// // Reset to normal
/// set_time_dilation(1.0);
/// ```
pub fn set_time_dilation(value: f64) {
    assert!(value > 0.0, "timeDilation must be positive");

    let old_bits = TIME_DILATION.load(Ordering::Relaxed);
    let old_value = f64::from_bits(old_bits);

    if (old_value - value).abs() < f64::EPSILON {
        return;
    }

    // If scheduler is initialized, reset epoch first
    if <Scheduler as BindingBase>::is_initialized() {
        Scheduler::instance().reset_epoch();
    }

    TIME_DILATION.store(value.to_bits(), Ordering::Relaxed);
}

// ============================================================================
// Performance Mode
// ============================================================================

/// Performance mode for the runtime.
///
/// This hints to the runtime about expected workload patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum PerformanceMode {
    /// Normal operation — no special optimizations.
    #[default]
    Normal,

    /// Latency-optimized mode for interactive scenarios.
    ///
    /// Hints that low latency is more important than throughput.
    /// The runtime may disable some background optimizations.
    Latency,

    /// Throughput-optimized mode for batch processing.
    ///
    /// Hints that throughput is more important than latency.
    /// The runtime may batch operations more aggressively.
    Throughput,

    /// Battery-saving mode for background operation.
    ///
    /// Hints that power consumption should be minimized.
    /// The runtime may reduce polling frequency and defer work.
    LowPower,
}

/// Handle for a performance mode request.
///
/// When dropped, the performance mode request is released.
/// Multiple handles can be active; the highest-priority mode wins.
///
/// # Example
///
/// ```rust
/// use flui_scheduler::config::{PerformanceModeRequestHandle, PerformanceMode};
/// use flui_scheduler::Scheduler;
///
/// let scheduler = Scheduler::new();
///
/// // Request latency mode for an interactive operation
/// let handle = scheduler.request_performance_mode(PerformanceMode::Latency);
///
/// // Do latency-sensitive work...
///
/// // Mode is released when handle is dropped
/// drop(handle);
/// ```
pub struct PerformanceModeRequestHandle {
    cleanup: Option<Box<dyn FnOnce() + Send>>,
}

impl PerformanceModeRequestHandle {
    /// Create a new handle with a cleanup callback.
    pub(crate) fn new(cleanup: impl FnOnce() + Send + 'static) -> Self {
        Self {
            cleanup: Some(Box::new(cleanup)),
        }
    }

    /// Dispose of this handle, releasing the performance mode request.
    ///
    /// This is called automatically when the handle is dropped.
    pub fn dispose(mut self) {
        if let Some(cleanup) = self.cleanup.take() {
            cleanup();
        }
    }
}

impl Drop for PerformanceModeRequestHandle {
    fn drop(&mut self) {
        if let Some(cleanup) = self.cleanup.take() {
            cleanup();
        }
    }
}

// ============================================================================
// Service Extensions
// ============================================================================

/// Service extension name for time dilation dev tools.
///
/// Used when registering service extensions for debugging and development.
///
/// # Example
///
/// ```rust
/// use flui_scheduler::config::SERVICE_EXT_TIME_DILATION;
///
/// assert_eq!(SERVICE_EXT_TIME_DILATION, "timeDilation");
/// ```
pub const SERVICE_EXT_TIME_DILATION: &str = "timeDilation";

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_time_dilation() {
        // Reset to default
        set_time_dilation(1.0);
        assert!((time_dilation() - 1.0).abs() < f64::EPSILON);

        // Set to 2x (half speed)
        set_time_dilation(2.0);
        assert!((time_dilation() - 2.0).abs() < f64::EPSILON);

        // Reset
        set_time_dilation(1.0);
    }

    #[test]
    #[should_panic(expected = "timeDilation must be positive")]
    fn test_time_dilation_zero() {
        set_time_dilation(0.0);
    }

    #[test]
    #[should_panic(expected = "timeDilation must be positive")]
    fn test_time_dilation_negative() {
        set_time_dilation(-1.0);
    }

    #[test]
    fn test_performance_mode_handle() {
        let scheduler = Scheduler::new();

        // Request latency mode
        let handle = scheduler.request_performance_mode(PerformanceMode::Latency);

        // Check request count
        assert_eq!(scheduler.performance_mode_request_count(), 1);

        // Drop handle
        drop(handle);

        // Check request is released
        assert_eq!(scheduler.performance_mode_request_count(), 0);
    }

    #[test]
    fn test_binding_state_isolation() {
        // Test that each scheduler has its own state
        let scheduler1 = Scheduler::new();
        let scheduler2 = Scheduler::new();

        // Request performance mode on scheduler1
        let _handle = scheduler1.request_performance_mode(PerformanceMode::Latency);

        // Verify scheduler1 has the request
        assert_eq!(scheduler1.performance_mode_request_count(), 1);

        // Verify scheduler2 is unaffected (proper isolation)
        assert_eq!(scheduler2.performance_mode_request_count(), 0);
    }

    #[test]
    fn test_default_scheduling_strategy() {
        use crate::task::Priority;

        let scheduler = Scheduler::new();

        // High priority should always run
        assert!(default_scheduling_strategy(Priority::Animation, &scheduler));
        assert!(default_scheduling_strategy(Priority::UserInput, &scheduler));

        // Lower priority depends on budget
        assert!(default_scheduling_strategy(Priority::Idle, &scheduler));
    }

    #[test]
    fn test_adjust_for_epoch() {
        use web_time::Duration;

        let raw = Duration::from_secs(10);
        let epoch = Duration::from_secs(5);

        // Without dilation
        set_time_dilation(1.0);
        let adjusted = super::adjust_duration_for_epoch(raw, epoch);
        assert_eq!(adjusted, Duration::from_secs(5));

        // With 2x dilation (half speed)
        set_time_dilation(2.0);
        let adjusted = super::adjust_duration_for_epoch(raw, epoch);
        assert!((adjusted.as_secs_f64() - 2.5).abs() < 0.001);

        // Reset
        set_time_dilation(1.0);
    }
}

// ============================================================================
// Internal Helper
// ============================================================================

/// Adjust a duration for the epoch and time dilation.
pub(crate) fn adjust_duration_for_epoch(
    raw: web_time::Duration,
    epoch_start: web_time::Duration,
) -> web_time::Duration {
    let since_epoch = raw.saturating_sub(epoch_start);
    let dilation = time_dilation();

    if (dilation - 1.0).abs() < f64::EPSILON {
        since_epoch
    } else {
        web_time::Duration::from_secs_f64(since_epoch.as_secs_f64() / dilation)
    }
}
