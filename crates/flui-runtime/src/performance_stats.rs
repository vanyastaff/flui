//! The rolling frame-time window behind a presentation's performance
//! overlay.
//!
//! Owned by the presentation because it is the thing that knows when a
//! frame was composited; the overlay layer (`flui_layer::PerformanceOverlayLayer`)
//! only draws the numbers it is handed.

use std::collections::VecDeque;

// `std::time::Instant::now()` panics on wasm32 ('time not implemented on this
// platform'); `web_time` is the workspace's cross-platform clock.
use web_time::{Duration, Instant};

/// Frame timing over the last `max_samples` frames.
#[derive(Debug, Clone)]
pub struct PerformanceStats {
    frame_times: VecDeque<Duration>,
    max_samples: usize,
    last_frame: Option<Instant>,
    total_frames: u64,
}

impl Default for PerformanceStats {
    /// Two seconds at 60 fps.
    fn default() -> Self {
        Self::new(120)
    }
}

impl PerformanceStats {
    /// A window that keeps the last `max_samples` frame durations.
    #[must_use]
    pub fn new(max_samples: usize) -> Self {
        Self {
            frame_times: VecDeque::with_capacity(max_samples),
            max_samples,
            last_frame: None,
            total_frames: 0,
        }
    }

    /// Records that a frame was composited now.
    pub fn record_frame(&mut self) {
        let now = Instant::now();
        if let Some(last) = self.last_frame {
            if self.frame_times.len() >= self.max_samples {
                self.frame_times.pop_front();
            }
            self.frame_times.push_back(now.duration_since(last));
        }
        self.last_frame = Some(now);
        self.total_frames += 1;
    }

    /// Average frame time over the window, in milliseconds; `0.0` before the
    /// second frame.
    #[must_use]
    pub fn avg_frame_time_ms(&self) -> f32 {
        if self.frame_times.is_empty() {
            return 0.0;
        }
        let total: Duration = self.frame_times.iter().sum();
        // The window is at most `max_samples` (120 by default) entries, far
        // inside an `f32`'s exact-integer range.
        #[expect(clippy::cast_precision_loss)]
        let samples = self.frame_times.len() as f32;
        total.as_secs_f32() * 1000.0 / samples
    }

    /// Frames per second over the window; `0.0` before the second frame.
    #[must_use]
    pub fn fps(&self) -> f32 {
        let avg_ms = self.avg_frame_time_ms();
        if avg_ms > 0.0 { 1000.0 / avg_ms } else { 0.0 }
    }

    /// Frames recorded since this window was created.
    #[must_use]
    pub fn total_frames(&self) -> u64 {
        self.total_frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_frame_has_no_duration_but_counts() {
        let mut stats = PerformanceStats::new(3);
        stats.record_frame();
        assert_eq!(stats.total_frames(), 1);
        assert_eq!(stats.fps(), 0.0);
        assert_eq!(stats.avg_frame_time_ms(), 0.0);
    }

    #[test]
    fn the_window_is_bounded_and_the_average_is_positive() {
        let mut stats = PerformanceStats::new(3);
        for _ in 0..6 {
            stats.record_frame();
        }
        assert_eq!(stats.total_frames(), 6);
        assert_eq!(stats.frame_times.len(), 3);
        assert!(stats.avg_frame_time_ms() >= 0.0);
        assert!(stats.fps() >= 0.0);
    }
}
