//! Backend-side velocity and timestamp helpers.
//!
//! These stay in `flui-platform` rather than moving to `flui-platform-api`
//! with the rest of the input vocabulary: they are backend helpers, not a
//! contract the framework programs against, and ADR-0082 §5 deletes
//! [`BasicVelocityTracker`] once nothing needs it.

use std::collections::VecDeque;

use flui_types::geometry::{Offset, Pixels};
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
    position: Offset<Pixels>,
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
    pub fn add_sample(&mut self, timestamp: Instant, position: Offset<Pixels>) {
        if self.samples.len() >= self.max_samples {
            self.samples.pop_front(); // O(1) instead of Vec::remove(0) O(n)
        }
        self.samples.push_back(VelocitySample {
            timestamp,
            position,
        });
    }

    /// Calculate velocity (pixels per second)
    pub fn velocity(&self) -> Option<Offset<Pixels>> {
        use flui_types::geometry::px;

        if self.samples.len() < 2 {
            return None;
        }

        let first = self.samples.front()?;
        let last = self.samples.back()?;

        let dt = last.timestamp.duration_since(first.timestamp);
        if dt.as_secs_f32() < 0.001 {
            return None;
        }

        let dx = last.position.dx.0 - first.position.dx.0;
        let dy = last.position.dy.0 - first.position.dy.0;
        let dt_secs = dt.as_secs_f32();

        Some(Offset::new(px(dx / dt_secs), px(dy / dt_secs)))
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

#[cfg(test)]
mod tests {
    use flui_platform_api::offset_from_coords;

    use super::*;

    #[test]
    fn test_velocity_tracker() {
        let mut tracker = BasicVelocityTracker::new();
        let t0 = Instant::now();

        tracker.add_sample(t0, offset_from_coords(0.0, 0.0));

        // Simulate 100ms later, moved 50 pixels
        std::thread::sleep(std::time::Duration::from_millis(100));
        let t1 = Instant::now();
        tracker.add_sample(t1, offset_from_coords(50.0, 0.0));

        if let Some(vel) = tracker.velocity() {
            // Should be ~500 pixels/sec (50px in 0.1s)
            use flui_types::geometry::px;
            assert!(vel.dx > px(400.0) && vel.dx < px(600.0));
        }
    }
}
