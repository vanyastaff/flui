//! Backend-side velocity and timestamp helpers.
//!
//! These stay in `flui-platform` rather than moving to `flui-platform-api`
//! with the rest of the input vocabulary: they are backend helpers, not a
//! contract the framework programs against, and ADR-0082 §5 deletes
//! [`BasicVelocityTracker`] once nothing needs it.

use std::collections::VecDeque;

use flui_foundation::geometry::Offset;
// web-time: std::time re-export on native; performance.now()-backed on
// wasm32, where std::time::Instant::now() panics — this module is in the
// wasm-check set and timestamps must be mintable there.
use web_time::Instant;

/// Velocity tracker for gesture recognition
///
/// A minimal tracker for the platform layer only. For full velocity
/// tracking, use `flui_interaction::processing::VelocityTracker`.
#[derive(Debug, Clone)]
pub struct BasicVelocityTracker {
    samples: VecDeque<VelocitySample>,
    max_samples: usize,
}

#[derive(Debug, Clone, Copy)]
struct VelocitySample {
    timestamp: Instant,
    position: Offset<f64>,
}

impl BasicVelocityTracker {
    /// Create a new velocity tracker
    pub fn new() -> Self {
        Self {
            samples: VecDeque::with_capacity(20),
            max_samples: 20,
        }
    }

    /// Add a sample
    pub fn add_sample(&mut self, timestamp: Instant, position: Offset<f64>) {
        if self.samples.len() >= self.max_samples {
            self.samples.pop_front(); // O(1) instead of Vec::remove(0) O(n)
        }
        self.samples.push_back(VelocitySample {
            timestamp,
            position,
        });
    }

    /// Calculate velocity (pixels per second)
    pub fn velocity(&self) -> Option<Offset<f64>> {
        if self.samples.len() < 2 {
            return None;
        }

        let first = self.samples.front()?;
        let last = self.samples.back()?;

        let dt = last.timestamp.duration_since(first.timestamp);
        if dt.as_secs_f64() < 0.001 {
            return None;
        }

        let dx = last.position.dx - first.position.dx;
        let dy = last.position.dy - first.position.dy;
        let dt_secs = dt.as_secs_f64();

        Some(Offset::new(dx / dt_secs, dy / dt_secs))
    }

    /// Clear samples
    pub fn clear(&mut self) {
        self.samples.clear();
    }
}

impl Default for BasicVelocityTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Timestamp provider for platform events
pub trait TimestampProvider {
    /// Current instant used to stamp platform input events
    fn now() -> Instant {
        Instant::now()
    }
}

/// Default timestamp provider using the wasm-safe monotonic clock
#[derive(Debug)]
pub struct SystemTimestamp;

impl TimestampProvider for SystemTimestamp {}
