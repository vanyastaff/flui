//! Pipeline performance metrics
//!
//! Provides real-time performance monitoring for the pipeline system.
//!
//! # Metrics Tracked
//!
//! - **FPS**: Frames per second over last 60 frames
//! - **Frame time**: Min/max/average frame duration
//! - **Phase timing**: Build/layout/paint phase breakdown
//! - **Frame drops**: Count of frames exceeding 16ms budget
//! - **Cache metrics**: Cache hit/miss rates
//!
//! # Example
//!
//! ```rust
//! use flui_core::pipeline::PipelineMetrics;
//! use std::time::Duration;
//!
//! let mut metrics = PipelineMetrics::new();
//!
//! // Start frame
//! metrics.frame_start();
//!
//! // Record phase timings
//! metrics.record_build_time(Duration::from_micros(500));
//! metrics.record_layout_time(Duration::from_micros(3000));
//! metrics.record_paint_time(Duration::from_micros(2000));
//!
//! // End frame
//! metrics.frame_end();
//!
//! // Query metrics
//! println!("FPS: {:.1}", metrics.fps());
//! println!("Dropped frames: {}", metrics.dropped_frames());
//! ```

use std::time::{Duration, Instant};

/// Frame budget for 60 FPS (16.67ms)
const FRAME_BUDGET_MS: u64 = 16;

/// Number of frames to track for FPS calculation
const FPS_WINDOW_SIZE: usize = 60;

/// Pipeline performance metrics
///
/// Tracks real-time performance metrics for the pipeline system.
///
/// # Thread Safety
///
/// PipelineMetrics is NOT thread-safe. It should be owned by PipelineOwner
/// which runs on a single thread.
///
/// # Memory
///
/// Uses a ring buffer to track last 60 frame times (~480 bytes).
#[derive(Debug)]
pub struct PipelineMetrics {
    // ========== Frame Timing ==========

    /// Current frame start time
    frame_start: Option<Instant>,

    /// Ring buffer of recent frame durations (microseconds)
    frame_times: Vec<u64>,

    /// Current position in ring buffer
    frame_index: usize,

    /// Total frames processed
    total_frames: u64,

    /// Total dropped frames (exceeding budget)
    dropped_frames: u64,

    // ========== Phase Timing ==========

    /// Total build phase time (microseconds)
    total_build_time: u64,

    /// Total layout phase time (microseconds)
    total_layout_time: u64,

    /// Total paint phase time (microseconds)
    total_paint_time: u64,

    // ========== Cache Metrics ==========

    /// Cache hits
    cache_hits: u64,

    /// Cache misses
    cache_misses: u64,
}

impl PipelineMetrics {
    /// Create new metrics tracker
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::PipelineMetrics;
    ///
    /// let metrics = PipelineMetrics::new();
    /// ```
    pub fn new() -> Self {
        Self {
            frame_start: None,
            frame_times: vec![0; FPS_WINDOW_SIZE],
            frame_index: 0,
            total_frames: 0,
            dropped_frames: 0,
            total_build_time: 0,
            total_layout_time: 0,
            total_paint_time: 0,
            cache_hits: 0,
            cache_misses: 0,
        }
    }

    // ========== Frame Tracking ==========

    /// Start new frame
    ///
    /// Call this at the beginning of each frame.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::PipelineMetrics;
    ///
    /// let mut metrics = PipelineMetrics::new();
    /// metrics.frame_start();
    /// ```
    #[inline]
    pub fn frame_start(&mut self) {
        self.frame_start = Some(Instant::now());
    }

    /// End current frame
    ///
    /// Call this at the end of each frame.
    /// Records frame time and updates metrics.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::PipelineMetrics;
    ///
    /// let mut metrics = PipelineMetrics::new();
    /// metrics.frame_start();
    /// // ... do work ...
    /// metrics.frame_end();
    /// ```
    pub fn frame_end(&mut self) {
        if let Some(start) = self.frame_start.take() {
            let duration = start.elapsed();
            let micros = duration.as_micros() as u64;

            // Record in ring buffer
            self.frame_times[self.frame_index] = micros;
            self.frame_index = (self.frame_index + 1) % FPS_WINDOW_SIZE;

            // Update counters
            self.total_frames += 1;

            // Check if frame was dropped
            if duration.as_millis() as u64 > FRAME_BUDGET_MS {
                self.dropped_frames += 1;
            }
        }
    }

    // ========== Phase Timing ==========

    /// Record build phase time
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::PipelineMetrics;
    /// use std::time::Duration;
    ///
    /// let mut metrics = PipelineMetrics::new();
    /// metrics.record_build_time(Duration::from_micros(500));
    /// ```
    #[inline]
    pub fn record_build_time(&mut self, duration: Duration) {
        self.total_build_time += duration.as_micros() as u64;
    }

    /// Record layout phase time
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::PipelineMetrics;
    /// use std::time::Duration;
    ///
    /// let mut metrics = PipelineMetrics::new();
    /// metrics.record_layout_time(Duration::from_micros(3000));
    /// ```
    #[inline]
    pub fn record_layout_time(&mut self, duration: Duration) {
        self.total_layout_time += duration.as_micros() as u64;
    }

    /// Record paint phase time
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::PipelineMetrics;
    /// use std::time::Duration;
    ///
    /// let mut metrics = PipelineMetrics::new();
    /// metrics.record_paint_time(Duration::from_micros(2000));
    /// ```
    #[inline]
    pub fn record_paint_time(&mut self, duration: Duration) {
        self.total_paint_time += duration.as_micros() as u64;
    }

    // ========== Cache Tracking ==========

    /// Record cache hit
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::PipelineMetrics;
    ///
    /// let mut metrics = PipelineMetrics::new();
    /// metrics.record_cache_hit();
    /// ```
    #[inline]
    pub fn record_cache_hit(&mut self) {
        self.cache_hits += 1;
    }

    /// Record cache miss
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::PipelineMetrics;
    ///
    /// let mut metrics = PipelineMetrics::new();
    /// metrics.record_cache_miss();
    /// ```
    #[inline]
    pub fn record_cache_miss(&mut self) {
        self.cache_misses += 1;
    }

    // ========== Queries ==========

    /// Get current FPS
    ///
    /// Calculates FPS over the last 60 frames (or fewer if just started).
    ///
    /// # Returns
    ///
    /// Frames per second as f64.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::PipelineMetrics;
    ///
    /// let metrics = PipelineMetrics::new();
    /// println!("FPS: {:.1}", metrics.fps());
    /// ```
    pub fn fps(&self) -> f64 {
        if self.total_frames == 0 {
            return 0.0;
        }

        // Calculate average frame time
        let count = self.total_frames.min(FPS_WINDOW_SIZE as u64) as usize;
        let total_micros: u64 = self.frame_times[..count].iter().sum();

        if total_micros == 0 {
            return 0.0;
        }

        // Convert to FPS
        let avg_micros = total_micros as f64 / count as f64;
        1_000_000.0 / avg_micros
    }

    /// Get average frame time
    ///
    /// # Returns
    ///
    /// Average frame duration over last 60 frames.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::PipelineMetrics;
    ///
    /// let metrics = PipelineMetrics::new();
    /// println!("Avg frame time: {:?}", metrics.avg_frame_time());
    /// ```
    pub fn avg_frame_time(&self) -> Duration {
        if self.total_frames == 0 {
            return Duration::ZERO;
        }

        let count = self.total_frames.min(FPS_WINDOW_SIZE as u64) as usize;
        let total_micros: u64 = self.frame_times[..count].iter().sum();
        let avg_micros = total_micros / count as u64;

        Duration::from_micros(avg_micros)
    }

    /// Get minimum frame time
    ///
    /// # Returns
    ///
    /// Minimum frame duration over last 60 frames.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::PipelineMetrics;
    ///
    /// let metrics = PipelineMetrics::new();
    /// println!("Min frame time: {:?}", metrics.min_frame_time());
    /// ```
    pub fn min_frame_time(&self) -> Duration {
        if self.total_frames == 0 {
            return Duration::ZERO;
        }

        let count = self.total_frames.min(FPS_WINDOW_SIZE as u64) as usize;
        let min_micros = *self.frame_times[..count].iter().min().unwrap_or(&0);

        Duration::from_micros(min_micros)
    }

    /// Get maximum frame time
    ///
    /// # Returns
    ///
    /// Maximum frame duration over last 60 frames.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::PipelineMetrics;
    ///
    /// let metrics = PipelineMetrics::new();
    /// println!("Max frame time: {:?}", metrics.max_frame_time());
    /// ```
    pub fn max_frame_time(&self) -> Duration {
        if self.total_frames == 0 {
            return Duration::ZERO;
        }

        let count = self.total_frames.min(FPS_WINDOW_SIZE as u64) as usize;
        let max_micros = *self.frame_times[..count].iter().max().unwrap_or(&0);

        Duration::from_micros(max_micros)
    }

    /// Get total frames processed
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::PipelineMetrics;
    ///
    /// let metrics = PipelineMetrics::new();
    /// println!("Total frames: {}", metrics.total_frames());
    /// ```
    #[inline]
    pub fn total_frames(&self) -> u64 {
        self.total_frames
    }

    /// Get total dropped frames
    ///
    /// A frame is considered "dropped" if it exceeds the 16ms budget.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::PipelineMetrics;
    ///
    /// let metrics = PipelineMetrics::new();
    /// println!("Dropped frames: {}", metrics.dropped_frames());
    /// ```
    #[inline]
    pub fn dropped_frames(&self) -> u64 {
        self.dropped_frames
    }

    /// Get drop rate
    ///
    /// # Returns
    ///
    /// Percentage of dropped frames (0.0 - 100.0).
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::PipelineMetrics;
    ///
    /// let metrics = PipelineMetrics::new();
    /// println!("Drop rate: {:.1}%", metrics.drop_rate());
    /// ```
    pub fn drop_rate(&self) -> f64 {
        if self.total_frames == 0 {
            return 0.0;
        }

        (self.dropped_frames as f64 / self.total_frames as f64) * 100.0
    }

    /// Get average build time
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::PipelineMetrics;
    ///
    /// let metrics = PipelineMetrics::new();
    /// println!("Avg build time: {:?}", metrics.avg_build_time());
    /// ```
    pub fn avg_build_time(&self) -> Duration {
        if self.total_frames == 0 {
            return Duration::ZERO;
        }

        Duration::from_micros(self.total_build_time / self.total_frames)
    }

    /// Get average layout time
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::PipelineMetrics;
    ///
    /// let metrics = PipelineMetrics::new();
    /// println!("Avg layout time: {:?}", metrics.avg_layout_time());
    /// ```
    pub fn avg_layout_time(&self) -> Duration {
        if self.total_frames == 0 {
            return Duration::ZERO;
        }

        Duration::from_micros(self.total_layout_time / self.total_frames)
    }

    /// Get average paint time
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::PipelineMetrics;
    ///
    /// let metrics = PipelineMetrics::new();
    /// println!("Avg paint time: {:?}", metrics.avg_paint_time());
    /// ```
    pub fn avg_paint_time(&self) -> Duration {
        if self.total_frames == 0 {
            return Duration::ZERO;
        }

        Duration::from_micros(self.total_paint_time / self.total_frames)
    }

    /// Get cache hit rate
    ///
    /// # Returns
    ///
    /// Percentage of cache hits (0.0 - 100.0).
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::PipelineMetrics;
    ///
    /// let metrics = PipelineMetrics::new();
    /// println!("Cache hit rate: {:.1}%", metrics.cache_hit_rate());
    /// ```
    pub fn cache_hit_rate(&self) -> f64 {
        let total = self.cache_hits + self.cache_misses;
        if total == 0 {
            return 0.0;
        }

        (self.cache_hits as f64 / total as f64) * 100.0
    }

    /// Get total cache accesses
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::PipelineMetrics;
    ///
    /// let metrics = PipelineMetrics::new();
    /// println!("Total cache accesses: {}", metrics.total_cache_accesses());
    /// ```
    #[inline]
    pub fn total_cache_accesses(&self) -> u64 {
        self.cache_hits + self.cache_misses
    }

    /// Reset all metrics
    ///
    /// Clears all counters and timing data.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::PipelineMetrics;
    ///
    /// let mut metrics = PipelineMetrics::new();
    /// // ... collect metrics ...
    /// metrics.reset();
    /// ```
    pub fn reset(&mut self) {
        self.frame_start = None;
        self.frame_times.fill(0);
        self.frame_index = 0;
        self.total_frames = 0;
        self.dropped_frames = 0;
        self.total_build_time = 0;
        self.total_layout_time = 0;
        self.total_paint_time = 0;
        self.cache_hits = 0;
        self.cache_misses = 0;
    }
}

impl Default for PipelineMetrics {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_metrics_creation() {
        let metrics = PipelineMetrics::new();
        assert_eq!(metrics.total_frames(), 0);
        assert_eq!(metrics.dropped_frames(), 0);
        assert_eq!(metrics.fps(), 0.0);
    }

    #[test]
    fn test_frame_tracking() {
        let mut metrics = PipelineMetrics::new();

        metrics.frame_start();
        thread::sleep(Duration::from_millis(1));
        metrics.frame_end();

        assert_eq!(metrics.total_frames(), 1);
        assert!(metrics.avg_frame_time().as_millis() >= 1);
    }

    #[test]
    fn test_fps_calculation() {
        let mut metrics = PipelineMetrics::new();

        // Simulate 10 frames at ~10ms each
        for _ in 0..10 {
            metrics.frame_start();
            thread::sleep(Duration::from_millis(10));
            metrics.frame_end();
        }

        // Should be approximately 100 FPS (1000ms / 10ms)
        let fps = metrics.fps();
        assert!(fps > 90.0 && fps < 110.0, "FPS was {}", fps);
    }

    #[test]
    fn test_dropped_frames() {
        let mut metrics = PipelineMetrics::new();

        // Normal frame (< 16ms)
        metrics.frame_start();
        thread::sleep(Duration::from_millis(5));
        metrics.frame_end();

        // Dropped frame (> 16ms)
        metrics.frame_start();
        thread::sleep(Duration::from_millis(20));
        metrics.frame_end();

        assert_eq!(metrics.total_frames(), 2);
        assert_eq!(metrics.dropped_frames(), 1);
        assert_eq!(metrics.drop_rate(), 50.0);
    }

    #[test]
    fn test_phase_timing() {
        let mut metrics = PipelineMetrics::new();

        metrics.frame_start();
        metrics.record_build_time(Duration::from_micros(500));
        metrics.record_layout_time(Duration::from_micros(3000));
        metrics.record_paint_time(Duration::from_micros(2000));
        metrics.frame_end();

        assert_eq!(metrics.avg_build_time(), Duration::from_micros(500));
        assert_eq!(metrics.avg_layout_time(), Duration::from_micros(3000));
        assert_eq!(metrics.avg_paint_time(), Duration::from_micros(2000));
    }

    #[test]
    fn test_cache_metrics() {
        let mut metrics = PipelineMetrics::new();

        metrics.record_cache_hit();
        metrics.record_cache_hit();
        metrics.record_cache_hit();
        metrics.record_cache_miss();

        assert_eq!(metrics.total_cache_accesses(), 4);
        assert_eq!(metrics.cache_hit_rate(), 75.0);
    }

    #[test]
    fn test_min_max_frame_time() {
        let mut metrics = PipelineMetrics::new();

        // Fast frame
        metrics.frame_start();
        thread::sleep(Duration::from_millis(5));
        metrics.frame_end();

        // Slow frame
        metrics.frame_start();
        thread::sleep(Duration::from_millis(15));
        metrics.frame_end();

        assert!(metrics.min_frame_time().as_millis() >= 4);
        assert!(metrics.max_frame_time().as_millis() >= 14);
    }

    #[test]
    fn test_reset() {
        let mut metrics = PipelineMetrics::new();

        metrics.frame_start();
        metrics.frame_end();
        metrics.record_cache_hit();

        assert_eq!(metrics.total_frames(), 1);
        assert_eq!(metrics.total_cache_accesses(), 1);

        metrics.reset();

        assert_eq!(metrics.total_frames(), 0);
        assert_eq!(metrics.total_cache_accesses(), 0);
    }

    #[test]
    fn test_ring_buffer_wrap() {
        let mut metrics = PipelineMetrics::new();

        // Fill more than ring buffer size
        for _ in 0..(FPS_WINDOW_SIZE + 10) {
            metrics.frame_start();
            thread::sleep(Duration::from_millis(1));
            metrics.frame_end();
        }

        assert_eq!(metrics.total_frames(), (FPS_WINDOW_SIZE + 10) as u64);
        // FPS should still be calculated over last 60 frames
        assert!(metrics.fps() > 0.0);
    }
}
