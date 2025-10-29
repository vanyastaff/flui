//! Performance profiler for FLUI applications
//!
//! Tracks frame timing, build/layout/paint phases, and detects performance issues.
//! Provides detailed metrics and history for performance analysis.
//!
//! # Example
//!
//! ```rust
//! use flui_devtools::profiler::{Profiler, FramePhase};
//!
//! let mut profiler = Profiler::new();
//!
//! // Start a new frame
//! profiler.begin_frame();
//!
//! // Profile build phase
//! {
//!     let _guard = profiler.profile_phase(FramePhase::Build);
//!     // Your build code here
//! }
//!
//! // Profile layout phase
//! {
//!     let _guard = profiler.profile_phase(FramePhase::Layout);
//!     // Your layout code here
//! }
//!
//! // Profile paint phase
//! {
//!     let _guard = profiler.profile_phase(FramePhase::Paint);
//!     // Your paint code here
//! }
//!
//! // End frame
//! profiler.end_frame();
//!
//! // Get stats
//! if let Some(stats) = profiler.frame_stats() {
//!     println!("Frame time: {:.2}ms", stats.total_time_ms());
//!     if stats.is_jank() {
//!         println!("JANK detected!");
//!     }
//! }
//!
//! // Print summary
//! profiler.print_frame_summary();
//! ```

use crate::common::DevToolsConfig;
use instant::{Duration, Instant};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;

/// Frame rendering phase
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FramePhase {
    /// Widget tree build phase
    Build,
    /// Layout computation phase
    Layout,
    /// Paint/rendering phase
    Paint,
    /// Custom user-defined phase
    Custom(&'static str),
}

impl FramePhase {
    /// Get the phase name as a string
    pub fn name(&self) -> &str {
        match self {
            FramePhase::Build => "Build",
            FramePhase::Layout => "Layout",
            FramePhase::Paint => "Paint",
            FramePhase::Custom(name) => name,
        }
    }
}

/// Information about a single phase within a frame
#[derive(Debug, Clone)]
pub struct PhaseInfo {
    /// Phase type
    pub phase: FramePhase,
    /// Duration of this phase
    pub duration: Duration,
    /// Start time relative to frame start
    pub start_offset: Duration,
}

impl PhaseInfo {
    /// Get duration in milliseconds
    pub fn duration_ms(&self) -> f64 {
        self.duration.as_secs_f64() * 1000.0
    }

    /// Get start offset in milliseconds
    pub fn start_offset_ms(&self) -> f64 {
        self.start_offset.as_secs_f64() * 1000.0
    }
}

/// Statistics for a single frame
#[derive(Debug, Clone)]
pub struct FrameStats {
    /// Frame number
    pub frame_number: u64,
    /// Total frame time
    pub total_time: Duration,
    /// Individual phase timings
    pub phases: Vec<PhaseInfo>,
    /// Whether this frame was jank (exceeded target time)
    pub is_jank: bool,
    /// Estimated FPS for this frame
    pub fps: f64,
}

impl FrameStats {
    /// Get total frame time in milliseconds
    pub fn total_time_ms(&self) -> f64 {
        self.total_time.as_secs_f64() * 1000.0
    }

    /// Check if frame was jank
    pub fn is_jank(&self) -> bool {
        self.is_jank
    }

    /// Get FPS for this frame
    pub fn fps(&self) -> f64 {
        self.fps
    }

    /// Get phase by type
    pub fn phase(&self, phase: FramePhase) -> Option<&PhaseInfo> {
        self.phases.iter().find(|p| p.phase == phase)
    }

    /// Get duration of a specific phase in milliseconds
    pub fn phase_duration_ms(&self, phase: FramePhase) -> Option<f64> {
        self.phase(phase).map(|p| p.duration_ms())
    }
}

/// RAII guard for profiling a phase
///
/// Automatically records the phase duration when dropped.
#[must_use = "PhaseGuard does nothing if not held"]
pub struct PhaseGuard {
    profiler: Arc<Mutex<ProfilerInner>>,
    phase: FramePhase,
    start: Instant,
}

impl Drop for PhaseGuard {
    fn drop(&mut self) {
        let duration = self.start.elapsed();
        let mut inner = self.profiler.lock();
        inner.end_phase(self.phase, duration);
    }
}

/// Internal profiler state
struct ProfilerInner {
    /// Configuration
    config: DevToolsConfig,
    /// Current frame number
    frame_number: u64,
    /// Frame start time
    frame_start: Option<Instant>,
    /// Current frame phases
    current_phases: Vec<PhaseInfo>,
    /// Frame history
    frame_history: VecDeque<FrameStats>,
    /// Total frames processed
    total_frames: u64,
    /// Total jank frames
    jank_frames: u64,
}

impl ProfilerInner {
    fn new(config: DevToolsConfig) -> Self {
        let max_history = config.max_frame_history;
        Self {
            config,
            frame_number: 0,
            frame_start: None,
            current_phases: Vec::new(),
            frame_history: VecDeque::with_capacity(max_history),
            total_frames: 0,
            jank_frames: 0,
        }
    }

    fn begin_frame(&mut self) {
        self.frame_start = Some(Instant::now());
        self.current_phases.clear();
    }

    fn end_frame(&mut self) {
        let Some(start) = self.frame_start.take() else {
            return;
        };

        let total_time = start.elapsed();
        let total_time_ms = total_time.as_secs_f64() * 1000.0;

        // Check if jank
        let is_jank = total_time_ms > self.config.jank_threshold_ms;
        if is_jank {
            self.jank_frames += 1;
        }

        // Calculate FPS
        let fps = if total_time_ms > 0.0 {
            1000.0 / total_time_ms
        } else {
            0.0
        };

        let stats = FrameStats {
            frame_number: self.frame_number,
            total_time,
            phases: std::mem::take(&mut self.current_phases),
            is_jank,
            fps,
        };

        // Add to history
        if self.frame_history.len() >= self.config.max_frame_history {
            self.frame_history.pop_front();
        }
        self.frame_history.push_back(stats);

        self.frame_number += 1;
        self.total_frames += 1;
    }

    fn start_phase(&self, _phase: FramePhase) -> Instant {
        Instant::now()
    }

    fn end_phase(&mut self, phase: FramePhase, duration: Duration) {
        let start_offset = if let Some(frame_start) = self.frame_start {
            Instant::now() - frame_start - duration
        } else {
            Duration::ZERO
        };

        self.current_phases.push(PhaseInfo {
            phase,
            duration,
            start_offset,
        });
    }

    fn frame_stats(&self) -> Option<FrameStats> {
        self.frame_history.back().cloned()
    }

    fn frame_history(&self) -> Vec<FrameStats> {
        self.frame_history.iter().cloned().collect()
    }

    fn average_fps(&self) -> f64 {
        if self.frame_history.is_empty() {
            return 0.0;
        }

        let sum: f64 = self.frame_history.iter().map(|s| s.fps).sum();
        sum / self.frame_history.len() as f64
    }

    fn jank_percentage(&self) -> f64 {
        if self.total_frames == 0 {
            return 0.0;
        }

        (self.jank_frames as f64 / self.total_frames as f64) * 100.0
    }
}

/// Performance profiler for FLUI applications
///
/// Thread-safe profiler that tracks frame timing and phase durations.
/// Use this to identify performance bottlenecks and jank in your application.
#[derive(Clone)]
pub struct Profiler {
    inner: Arc<Mutex<ProfilerInner>>,
}

impl Profiler {
    /// Create a new profiler with default configuration
    pub fn new() -> Self {
        Self::with_config(DevToolsConfig::default())
    }

    /// Create a new profiler with custom configuration
    pub fn with_config(config: DevToolsConfig) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ProfilerInner::new(config))),
        }
    }

    /// Begin a new frame
    ///
    /// Call this at the start of each frame.
    pub fn begin_frame(&self) {
        self.inner.lock().begin_frame();
    }

    /// End the current frame
    ///
    /// Call this at the end of each frame to record timing.
    pub fn end_frame(&self) {
        self.inner.lock().end_frame();
    }

    /// Profile a phase with RAII guard
    ///
    /// Returns a guard that automatically records the phase duration when dropped.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use flui_devtools::profiler::{Profiler, FramePhase};
    /// # let profiler = Profiler::new();
    /// # profiler.begin_frame();
    /// {
    ///     let _guard = profiler.profile_phase(FramePhase::Build);
    ///     // Your code here
    /// } // Phase duration recorded here
    /// # profiler.end_frame();
    /// ```
    pub fn profile_phase(&self, phase: FramePhase) -> PhaseGuard {
        let start = self.inner.lock().start_phase(phase);
        PhaseGuard {
            profiler: self.inner.clone(),
            phase,
            start,
        }
    }

    /// Get statistics for the most recent frame
    pub fn frame_stats(&self) -> Option<FrameStats> {
        self.inner.lock().frame_stats()
    }

    /// Get frame history
    ///
    /// Returns statistics for all frames in the history buffer.
    pub fn frame_history(&self) -> Vec<FrameStats> {
        self.inner.lock().frame_history()
    }

    /// Get average FPS across all frames in history
    pub fn average_fps(&self) -> f64 {
        self.inner.lock().average_fps()
    }

    /// Get percentage of frames that were jank
    pub fn jank_percentage(&self) -> f64 {
        self.inner.lock().jank_percentage()
    }

    /// Print a summary of recent frame performance
    pub fn print_frame_summary(&self) {
        let inner = self.inner.lock();

        println!("=== Frame Performance Summary ===");
        println!("Total frames: {}", inner.total_frames);
        println!("Average FPS: {:.2}", inner.average_fps());
        println!(
            "Jank frames: {} ({:.2}%)",
            inner.jank_frames,
            inner.jank_percentage()
        );

        if let Some(stats) = inner.frame_stats() {
            println!("\nLast frame:");
            println!(
                "  Frame #{}: {:.2}ms{}",
                stats.frame_number,
                stats.total_time_ms(),
                if stats.is_jank { " (JANK)" } else { "" }
            );

            for phase in &stats.phases {
                println!("    {}: {:.2}ms", phase.phase.name(), phase.duration_ms());
            }
        }

        // Recent history
        if inner.frame_history.len() > 1 {
            println!("\nRecent frames:");
            for stats in inner.frame_history.iter().rev().take(5) {
                println!(
                    "  Frame #{}: {:.2}ms{}",
                    stats.frame_number,
                    stats.total_time_ms(),
                    if stats.is_jank { " (JANK)" } else { "" }
                );
            }
        }
    }
}

impl Default for Profiler {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Profiler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Profiler")
            .field("frame_count", &self.inner.lock().total_frames)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn test_basic_profiling() {
        let profiler = Profiler::new();

        profiler.begin_frame();

        // Simulate some work
        {
            let _guard = profiler.profile_phase(FramePhase::Build);
            thread::sleep(Duration::from_millis(5));
        }

        {
            let _guard = profiler.profile_phase(FramePhase::Layout);
            thread::sleep(Duration::from_millis(3));
        }

        {
            let _guard = profiler.profile_phase(FramePhase::Paint);
            thread::sleep(Duration::from_millis(2));
        }

        profiler.end_frame();

        // Check stats
        let stats = profiler.frame_stats().unwrap();
        assert_eq!(stats.phases.len(), 3);
        assert!(stats.total_time_ms() >= 10.0);

        // Check individual phases
        let build = stats.phase(FramePhase::Build).unwrap();
        assert!(build.duration_ms() >= 5.0);

        let layout = stats.phase(FramePhase::Layout).unwrap();
        assert!(layout.duration_ms() >= 3.0);

        let paint = stats.phase(FramePhase::Paint).unwrap();
        assert!(paint.duration_ms() >= 2.0);
    }

    #[test]
    fn test_multiple_frames() {
        let profiler = Profiler::new();

        // Simulate 5 frames
        for _ in 0..5 {
            profiler.begin_frame();

            {
                let _guard = profiler.profile_phase(FramePhase::Build);
                thread::sleep(Duration::from_millis(2));
            }

            profiler.end_frame();
        }

        // Check history
        let history = profiler.frame_history();
        assert_eq!(history.len(), 5);

        // Check frame numbers
        for (i, stats) in history.iter().enumerate() {
            assert_eq!(stats.frame_number, i as u64);
        }
    }

    #[test]
    fn test_jank_detection() {
        let mut config = DevToolsConfig::default();
        config.jank_threshold_ms = 10.0; // 10ms threshold

        let profiler = Profiler::with_config(config);

        // Normal frame
        profiler.begin_frame();
        thread::sleep(Duration::from_millis(5));
        profiler.end_frame();

        let stats = profiler.frame_stats().unwrap();
        assert!(!stats.is_jank());

        // Jank frame
        profiler.begin_frame();
        thread::sleep(Duration::from_millis(15));
        profiler.end_frame();

        let stats = profiler.frame_stats().unwrap();
        assert!(stats.is_jank());

        // Check jank percentage
        assert_eq!(profiler.jank_percentage(), 50.0);
    }

    #[test]
    fn test_average_fps() {
        let profiler = Profiler::new();

        // Simulate frames with known duration
        for _ in 0..10 {
            profiler.begin_frame();
            thread::sleep(Duration::from_millis(16)); // ~60 FPS
            profiler.end_frame();
        }

        let avg_fps = profiler.average_fps();
        // Should be close to 60 FPS (allowing for some variance)
        assert!(avg_fps > 50.0 && avg_fps < 70.0, "FPS was {}", avg_fps);
    }

    #[test]
    fn test_frame_history_limit() {
        let mut config = DevToolsConfig::default();
        config.max_frame_history = 5;

        let profiler = Profiler::with_config(config);

        // Add more frames than the limit
        for _ in 0..10 {
            profiler.begin_frame();
            profiler.end_frame();
        }

        // Should only keep the last 5
        let history = profiler.frame_history();
        assert_eq!(history.len(), 5);

        // Should be frames 5-9
        assert_eq!(history[0].frame_number, 5);
        assert_eq!(history[4].frame_number, 9);
    }

    #[test]
    fn test_custom_phase() {
        let profiler = Profiler::new();

        profiler.begin_frame();

        {
            let _guard = profiler.profile_phase(FramePhase::Custom("MyPhase"));
            thread::sleep(Duration::from_millis(5));
        }

        profiler.end_frame();

        let stats = profiler.frame_stats().unwrap();
        let phase = stats.phase(FramePhase::Custom("MyPhase")).unwrap();
        assert!(phase.duration_ms() >= 5.0);
        assert_eq!(phase.phase.name(), "MyPhase");
    }

    #[test]
    fn test_thread_safety() {
        let profiler = Profiler::new();
        let profiler_clone = profiler.clone();

        let handle = thread::spawn(move || {
            profiler_clone.begin_frame();
            {
                let _guard = profiler_clone.profile_phase(FramePhase::Build);
                thread::sleep(Duration::from_millis(5));
            }
            profiler_clone.end_frame();
        });

        handle.join().unwrap();

        let stats = profiler.frame_stats().unwrap();
        assert_eq!(stats.phases.len(), 1);
    }
}
