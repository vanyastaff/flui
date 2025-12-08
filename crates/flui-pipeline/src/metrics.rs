//! Pipeline performance metrics
//!
//! Tracks frame times, build/layout/paint durations, cache statistics,
//! and other performance-related data.
//!
//! # Example
//!
//! ```rust
//! use flui_pipeline::PipelineMetrics;
//! use std::time::Duration;
//!
//! let mut metrics = PipelineMetrics::new();
//!
//! // Record frame timing
//! metrics.record_frame(
//!     Duration::from_micros(2000),  // build
//!     Duration::from_micros(1000),  // layout
//!     Duration::from_micros(500),   // paint
//! );
//!
//! println!("FPS: {:.1}", metrics.fps());
//! println!("Avg frame time: {:?}", metrics.avg_frame_time());
//! ```

use std::time::{Duration, Instant};

/// Number of frames to keep in ring buffer for FPS calculation.
const FRAME_HISTORY_SIZE: usize = 60;

/// Pipeline performance metrics
///
/// Collects and computes statistics about pipeline performance:
/// - Frame timing (total, build, layout, paint)
/// - FPS calculation
/// - Cache hit rates
/// - Dropped frame tracking
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PipelineMetrics {
    /// Total frames processed
    total_frames: u64,

    /// Dropped frames (exceeded budget)
    dropped_frames: u64,

    /// Total build time in microseconds
    total_build_time: u64,

    /// Total layout time in microseconds
    total_layout_time: u64,

    /// Total paint time in microseconds
    total_paint_time: u64,

    /// Cache hits
    cache_hits: u64,

    /// Cache misses
    cache_misses: u64,

    /// Last frame start time
    #[cfg_attr(feature = "serde", serde(skip))]
    last_frame_start: Option<Instant>,

    /// Frame budget in microseconds (default: 16667 = 60 FPS)
    frame_budget_us: u64,

    /// Recent frame times for FPS calculation (ring buffer)
    #[cfg_attr(feature = "serde", serde(skip))]
    recent_frame_times: [u64; FRAME_HISTORY_SIZE],

    /// Current index in ring buffer
    #[cfg_attr(feature = "serde", serde(skip))]
    frame_time_index: usize,
}

impl PipelineMetrics {
    /// Create new metrics with default 60 FPS budget
    #[must_use]
    pub fn new() -> Self {
        Self::with_target_fps(60)
    }

    /// Create metrics with custom target FPS
    #[must_use]
    pub fn with_target_fps(fps: u32) -> Self {
        let frame_budget_us = 1_000_000 / u64::from(fps);
        Self {
            total_frames: 0,
            dropped_frames: 0,
            total_build_time: 0,
            total_layout_time: 0,
            total_paint_time: 0,
            cache_hits: 0,
            cache_misses: 0,
            last_frame_start: None,
            frame_budget_us,
            recent_frame_times: [0; FRAME_HISTORY_SIZE],
            frame_time_index: 0,
        }
    }

    /// Start timing a frame
    pub fn start_frame(&mut self) {
        self.last_frame_start = Some(Instant::now());
    }

    /// End timing a frame
    pub fn end_frame(&mut self) {
        if let Some(start) = self.last_frame_start.take() {
            let elapsed = start.elapsed().as_micros() as u64;

            // Store in ring buffer
            self.recent_frame_times[self.frame_time_index] = elapsed;
            self.frame_time_index = (self.frame_time_index + 1) % FRAME_HISTORY_SIZE;

            // Check for dropped frame
            if elapsed > self.frame_budget_us {
                self.dropped_frames += 1;
            }

            self.total_frames += 1;
        }
    }

    /// Record frame with individual phase timings
    pub fn record_frame(
        &mut self,
        build_time: Duration,
        layout_time: Duration,
        paint_time: Duration,
    ) {
        self.total_build_time += build_time.as_micros() as u64;
        self.total_layout_time += layout_time.as_micros() as u64;
        self.total_paint_time += paint_time.as_micros() as u64;

        let total = build_time + layout_time + paint_time;

        // Store in ring buffer
        self.recent_frame_times[self.frame_time_index] = total.as_micros() as u64;
        self.frame_time_index = (self.frame_time_index + 1) % FRAME_HISTORY_SIZE;

        // Check for dropped frame
        if total.as_micros() as u64 > self.frame_budget_us {
            self.dropped_frames += 1;
        }

        self.total_frames += 1;
    }

    /// Record cache hit
    #[inline]
    pub fn record_cache_hit(&mut self) {
        self.cache_hits += 1;
    }

    /// Record cache miss
    #[inline]
    pub fn record_cache_miss(&mut self) {
        self.cache_misses += 1;
    }

    // =========================================================================
    // Getters
    // =========================================================================

    /// Get total frames processed
    #[inline]
    #[must_use]
    pub const fn total_frames(&self) -> u64 {
        self.total_frames
    }

    /// Get dropped frames count
    #[inline]
    #[must_use]
    pub const fn dropped_frames(&self) -> u64 {
        self.dropped_frames
    }

    /// Get cache hits count
    #[inline]
    #[must_use]
    pub const fn cache_hits(&self) -> u64 {
        self.cache_hits
    }

    /// Get cache misses count
    #[inline]
    #[must_use]
    pub const fn cache_misses(&self) -> u64 {
        self.cache_misses
    }

    /// Get current FPS based on recent frames
    #[must_use]
    pub fn fps(&self) -> f64 {
        let frames = self.total_frames.min(FRAME_HISTORY_SIZE as u64);
        if frames == 0 {
            return 0.0;
        }

        let total_time: u64 = self.recent_frame_times.iter().take(frames as usize).sum();
        if total_time == 0 {
            return 0.0;
        }

        // frames per microsecond * 1_000_000 = frames per second
        (frames as f64 / total_time as f64) * 1_000_000.0
    }

    /// Get average frame time
    #[must_use]
    pub fn avg_frame_time(&self) -> Duration {
        if self.total_frames == 0 {
            return Duration::ZERO;
        }

        let total = self.total_build_time + self.total_layout_time + self.total_paint_time;
        Duration::from_micros(total / self.total_frames)
    }

    /// Get frame drop rate as percentage
    #[must_use]
    pub fn drop_rate(&self) -> f64 {
        if self.total_frames == 0 {
            return 0.0;
        }

        (self.dropped_frames as f64 / self.total_frames as f64) * 100.0
    }

    /// Get average build time
    #[must_use]
    pub fn avg_build_time(&self) -> Duration {
        if self.total_frames == 0 {
            return Duration::ZERO;
        }

        Duration::from_micros(self.total_build_time / self.total_frames)
    }

    /// Get average layout time
    #[must_use]
    pub fn avg_layout_time(&self) -> Duration {
        if self.total_frames == 0 {
            return Duration::ZERO;
        }

        Duration::from_micros(self.total_layout_time / self.total_frames)
    }

    /// Get average paint time
    #[must_use]
    pub fn avg_paint_time(&self) -> Duration {
        if self.total_frames == 0 {
            return Duration::ZERO;
        }

        Duration::from_micros(self.total_paint_time / self.total_frames)
    }

    /// Get cache hit rate as percentage
    #[must_use]
    pub fn cache_hit_rate(&self) -> f64 {
        let total = self.cache_hits + self.cache_misses;
        if total == 0 {
            return 0.0;
        }

        (self.cache_hits as f64 / total as f64) * 100.0
    }

    /// Get total cache accesses
    #[inline]
    #[must_use]
    pub const fn total_cache_accesses(&self) -> u64 {
        self.cache_hits + self.cache_misses
    }

    /// Get frame budget
    #[inline]
    #[must_use]
    pub const fn frame_budget(&self) -> Duration {
        Duration::from_micros(self.frame_budget_us)
    }

    /// Returns `true` if no frames have been processed.
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.total_frames == 0
    }

    // =========================================================================
    // Mutation
    // =========================================================================

    /// Reset all metrics
    pub fn reset(&mut self) {
        self.total_frames = 0;
        self.dropped_frames = 0;
        self.total_build_time = 0;
        self.total_layout_time = 0;
        self.total_paint_time = 0;
        self.cache_hits = 0;
        self.cache_misses = 0;
        self.last_frame_start = None;
        self.recent_frame_times.fill(0);
        self.frame_time_index = 0;
    }

    /// Set frame budget (target FPS)
    pub fn set_target_fps(&mut self, fps: u32) {
        self.frame_budget_us = 1_000_000 / u64::from(fps);
    }
}

impl Default for PipelineMetrics {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_creation() {
        let metrics = PipelineMetrics::new();
        assert_eq!(metrics.total_frames(), 0);
        assert_eq!(metrics.dropped_frames(), 0);
        assert!(metrics.is_empty());
    }

    #[test]
    fn test_record_frame() {
        let mut metrics = PipelineMetrics::new();

        metrics.record_frame(
            Duration::from_micros(2000),
            Duration::from_micros(1000),
            Duration::from_micros(500),
        );

        assert_eq!(metrics.total_frames(), 1);
        assert!(!metrics.is_empty());
    }

    #[test]
    fn test_cache_tracking() {
        let mut metrics = PipelineMetrics::new();

        metrics.record_cache_hit();
        metrics.record_cache_hit();
        metrics.record_cache_miss();

        assert_eq!(metrics.cache_hits(), 2);
        assert_eq!(metrics.cache_misses(), 1);
        assert!((metrics.cache_hit_rate() - 66.67).abs() < 1.0);
    }

    #[test]
    fn test_drop_rate() {
        let mut metrics = PipelineMetrics::with_target_fps(60);

        // Fast frame (under budget)
        metrics.record_frame(
            Duration::from_micros(5000),
            Duration::from_micros(3000),
            Duration::from_micros(2000),
        );

        // Slow frame (over budget - 20ms > 16.67ms)
        metrics.record_frame(
            Duration::from_micros(10000),
            Duration::from_micros(8000),
            Duration::from_micros(5000),
        );

        assert_eq!(metrics.total_frames(), 2);
        assert_eq!(metrics.dropped_frames(), 1);
        assert!((metrics.drop_rate() - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_reset() {
        let mut metrics = PipelineMetrics::new();
        metrics.record_frame(
            Duration::from_micros(1000),
            Duration::from_micros(1000),
            Duration::from_micros(1000),
        );
        metrics.record_cache_hit();

        metrics.reset();

        assert_eq!(metrics.total_frames(), 0);
        assert_eq!(metrics.cache_hits(), 0);
        assert!(metrics.is_empty());
    }
}
