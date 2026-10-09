//! UpdateScheduler configuration types and utilities.
//!
//! This module provides standalone types used by the
//! [`UpdateScheduler`](crate::scheduler::UpdateScheduler):
//!
//! - **Performance mode**: Hint to the runtime about expected workload
//! - **Service extensions**: Debug/dev tool integration points
//! - **Timings callbacks**: Frame performance reporting

use std::rc::Rc;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::frame::FrameTiming;

// ============================================================================
// Callback Type Aliases
// ============================================================================

/// Timings callback for receiving [`FrameTiming`] reports from the engine.
///
/// Callbacks receive batched `FrameTiming` data approximately once per second
/// in release mode, or every ~100ms in debug/profile builds.
pub type TimingsCallback = Rc<dyn Fn(&[FrameTiming])>;

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
/// use flui_scheduler::{
///     UpdateScheduler,
///     config::{PerformanceMode, PerformanceModeRequestHandle},
/// };
///
/// let scheduler = UpdateScheduler::new();
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
    cleanup: Option<Box<dyn FnOnce()>>,
}

impl std::fmt::Debug for PerformanceModeRequestHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `cleanup` is an opaque `dyn FnOnce`; report only whether the
        // handle has already been disposed.
        f.debug_struct("PerformanceModeRequestHandle")
            .field("disposed", &self.cleanup.is_none())
            .finish_non_exhaustive()
    }
}

impl PerformanceModeRequestHandle {
    /// Create a new handle with a cleanup callback.
    pub(crate) fn new(cleanup: impl FnOnce() + 'static) -> Self {
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
