//! Frame timing reports consumed by [`UpdateScheduler`](crate::UpdateScheduler).

use std::rc::Rc;

use crate::frame::FrameTiming;

// ============================================================================
// Callback Type Aliases
// ============================================================================

/// Timings callback for receiving [`FrameTiming`] reports from the engine.
///
/// Callbacks receive batched `FrameTiming` data approximately once per second
/// in release mode, or every ~100ms in debug/profile builds.
pub type TimingsCallback = Rc<dyn Fn(&[FrameTiming])>;
