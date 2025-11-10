//! Frame scheduling and budget management
//!
//! Provides frame scheduling logic with budget enforcement and deadline tracking.
//!
//! # Design
//!
//! `FrameScheduler` manages frame timing, budgets, and deadlines to ensure smooth
//! rendering at target frame rates (e.g., 60 FPS).
//!
//! # Key Features
//!
//! - **Frame budget tracking**: Ensures frames complete within target duration
//! - **Deadline checking**: Detects when frame deadline is approaching
//! - **Frame skip policy**: Handles dropped frames gracefully
//! - **Budget awareness**: Provides remaining time for adaptive work scheduling
//!
//! # Example
//!
//! ```rust
//! use flui_core::pipeline::FrameScheduler;
//! use std::time::Duration;
//!
//! let mut scheduler = FrameScheduler::new();
//!
//! // Start a new frame
//! let budget = scheduler.start_frame();
//! println!("Frame budget: {:?}", budget);
//!
//! // Check if deadline is near
//! if scheduler.is_deadline_near() {
//!     println!("Warning: Frame deadline approaching!");
//! }
//!
//! // Finish frame
//! scheduler.finish_frame();
//! ```

use std::time::{Duration, Instant};

/// Default target FPS (60 FPS = 16.67ms per frame)
const DEFAULT_TARGET_FPS: u32 = 60;

/// Deadline warning threshold (80% of frame budget)
const DEADLINE_WARNING_THRESHOLD: f64 = 0.8;

/// Frame skip policy
///
/// Determines how the scheduler handles frames that exceed the budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameSkipPolicy {
    /// Never skip frames (may cause stuttering)
    Never,

    /// Skip frames when deadline is missed (smoother but may drop frames)
    OnDeadlineMiss,

    /// Skip frames when multiple deadlines are missed in a row
    OnConsecutiveMisses(u32),
}

impl Default for FrameSkipPolicy {
    fn default() -> Self {
        Self::OnConsecutiveMisses(2)
    }
}

/// Frame scheduling and budget management
///
/// Manages frame timing, budgets, and deadlines to ensure smooth rendering.
///
/// # Thread Safety
///
/// `FrameScheduler` is NOT thread-safe. It should be owned by `FrameCoordinator`
/// which runs on a single thread.
///
/// # Example
///
/// ```rust
/// use flui_core::pipeline::FrameScheduler;
///
/// let mut scheduler = FrameScheduler::new();
///
/// // Start frame
/// let budget = scheduler.start_frame();
///
/// // Do work...
///
/// // Check if we're running out of time
/// if scheduler.is_deadline_near() {
///     // Skip non-critical work
/// }
///
/// // Finish frame
/// scheduler.finish_frame();
/// ```
#[derive(Debug)]
pub struct FrameScheduler {
    /// Target frame duration (e.g., 16.67ms for 60 FPS)
    frame_budget: Duration,

    /// Target frames per second
    target_fps: u32,

    /// Current frame start time
    frame_start: Option<Instant>,

    /// Last frame start time
    last_frame_start: Option<Instant>,

    /// Frame deadline (frame_start + frame_budget)
    deadline: Option<Instant>,

    /// Frame skip policy
    skip_policy: FrameSkipPolicy,

    /// Number of consecutive missed deadlines
    consecutive_misses: u32,

    /// Total frames scheduled
    total_frames: u64,

    /// Total frames skipped
    skipped_frames: u64,
}

impl FrameScheduler {
    /// Create a new frame scheduler with default settings (60 FPS)
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::FrameScheduler;
    ///
    /// let scheduler = FrameScheduler::new();
    /// ```
    pub fn new() -> Self {
        Self::with_target_fps(DEFAULT_TARGET_FPS)
    }

    /// Create a new frame scheduler with custom target FPS
    ///
    /// # Parameters
    ///
    /// - `target_fps`: Target frames per second (e.g., 60, 120)
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::FrameScheduler;
    ///
    /// // 120 FPS target (8.33ms per frame)
    /// let scheduler = FrameScheduler::with_target_fps(120);
    /// ```
    pub fn with_target_fps(target_fps: u32) -> Self {
        let frame_budget = Duration::from_secs_f64(1.0 / target_fps as f64);

        Self {
            frame_budget,
            target_fps,
            frame_start: None,
            last_frame_start: None,
            deadline: None,
            skip_policy: FrameSkipPolicy::default(),
            consecutive_misses: 0,
            total_frames: 0,
            skipped_frames: 0,
        }
    }

    /// Set the frame skip policy
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::{FrameScheduler, FrameSkipPolicy};
    ///
    /// let mut scheduler = FrameScheduler::new();
    /// scheduler.set_skip_policy(FrameSkipPolicy::Never);
    /// ```
    pub fn set_skip_policy(&mut self, policy: FrameSkipPolicy) {
        self.skip_policy = policy;
    }

    /// Start a new frame
    ///
    /// Records the frame start time and calculates the deadline.
    ///
    /// # Returns
    ///
    /// The frame budget (time available for this frame).
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::FrameScheduler;
    ///
    /// let mut scheduler = FrameScheduler::new();
    /// let budget = scheduler.start_frame();
    /// println!("Budget: {:?}", budget);
    /// ```
    pub fn start_frame(&mut self) -> Duration {
        let now = Instant::now();

        self.last_frame_start = self.frame_start;
        self.frame_start = Some(now);
        self.deadline = Some(now + self.frame_budget);
        self.total_frames += 1;

        self.frame_budget
    }

    /// Get elapsed time since frame start
    ///
    /// # Returns
    ///
    /// Duration since `start_frame()` was called, or None if no frame is active.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::FrameScheduler;
    ///
    /// let mut scheduler = FrameScheduler::new();
    /// scheduler.start_frame();
    ///
    /// // ... do some work ...
    ///
    /// if let Some(elapsed) = scheduler.elapsed() {
    ///     println!("Elapsed: {:?}", elapsed);
    /// }
    /// ```
    pub fn elapsed(&self) -> Option<Duration> {
        self.frame_start.map(|start| start.elapsed())
    }

    /// Get remaining time in current frame
    ///
    /// # Returns
    ///
    /// Duration until deadline, or None if no frame is active.
    /// Returns `Duration::ZERO` if deadline has passed.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::FrameScheduler;
    ///
    /// let mut scheduler = FrameScheduler::new();
    /// scheduler.start_frame();
    ///
    /// if let Some(remaining) = scheduler.remaining() {
    ///     println!("Remaining: {:?}", remaining);
    /// }
    /// ```
    pub fn remaining(&self) -> Option<Duration> {
        match (self.frame_start, self.deadline) {
            (Some(_start), Some(deadline)) => {
                let now = Instant::now();
                if now >= deadline {
                    Some(Duration::ZERO)
                } else {
                    Some(deadline - now)
                }
            }
            _ => None,
        }
    }

    /// Check if frame deadline is approaching
    ///
    /// Returns `true` if 80% or more of the frame budget has been used.
    ///
    /// # Returns
    ///
    /// `true` if deadline is near (>80% of budget used), `false` otherwise.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::FrameScheduler;
    ///
    /// let mut scheduler = FrameScheduler::new();
    /// scheduler.start_frame();
    ///
    /// if scheduler.is_deadline_near() {
    ///     println!("Warning: Deadline approaching!");
    /// }
    /// ```
    pub fn is_deadline_near(&self) -> bool {
        match self.remaining() {
            Some(remaining) => {
                let used = self.frame_budget.saturating_sub(remaining);
                let used_fraction = used.as_secs_f64() / self.frame_budget.as_secs_f64();
                used_fraction >= DEADLINE_WARNING_THRESHOLD
            }
            None => false,
        }
    }

    /// Check if frame deadline has been missed
    ///
    /// # Returns
    ///
    /// `true` if the current time exceeds the deadline, `false` otherwise.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::FrameScheduler;
    ///
    /// let mut scheduler = FrameScheduler::new();
    /// scheduler.start_frame();
    ///
    /// if scheduler.is_deadline_missed() {
    ///     println!("Frame dropped!");
    /// }
    /// ```
    pub fn is_deadline_missed(&self) -> bool {
        match self.deadline {
            Some(deadline) => Instant::now() >= deadline,
            None => false,
        }
    }

    /// Check if the next frame should be skipped
    ///
    /// Based on the frame skip policy and current deadline misses.
    ///
    /// # Returns
    ///
    /// `true` if the frame should be skipped, `false` otherwise.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::FrameScheduler;
    ///
    /// let mut scheduler = FrameScheduler::new();
    /// scheduler.start_frame();
    ///
    /// if scheduler.should_skip_frame() {
    ///     // Skip non-critical rendering
    /// }
    /// ```
    pub fn should_skip_frame(&self) -> bool {
        match self.skip_policy {
            FrameSkipPolicy::Never => false,
            FrameSkipPolicy::OnDeadlineMiss => self.is_deadline_missed(),
            FrameSkipPolicy::OnConsecutiveMisses(threshold) => self.consecutive_misses >= threshold,
        }
    }

    /// Finish the current frame
    ///
    /// Records frame completion and updates deadline miss tracking.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::FrameScheduler;
    ///
    /// let mut scheduler = FrameScheduler::new();
    /// scheduler.start_frame();
    /// // ... do work ...
    /// scheduler.finish_frame();
    /// ```
    pub fn finish_frame(&mut self) {
        if self.is_deadline_missed() {
            self.consecutive_misses += 1;

            #[cfg(debug_assertions)]
            if let Some(elapsed) = self.elapsed() {
                tracing::warn!(
                    elapsed_ms = elapsed.as_millis(),
                    budget_ms = self.frame_budget.as_millis(),
                    consecutive_misses = self.consecutive_misses,
                    "Frame deadline missed"
                );
            }
        } else {
            self.consecutive_misses = 0;
        }

        // Clear frame state
        self.frame_start = None;
        self.deadline = None;
    }

    /// Skip the current frame
    ///
    /// Records a skipped frame without completing work.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::FrameScheduler;
    ///
    /// let mut scheduler = FrameScheduler::new();
    /// scheduler.start_frame();
    ///
    /// if scheduler.should_skip_frame() {
    ///     scheduler.skip_frame();
    /// }
    /// ```
    pub fn skip_frame(&mut self) {
        self.skipped_frames += 1;

        #[cfg(debug_assertions)]
        tracing::debug!(
            total_frames = self.total_frames,
            skipped_frames = self.skipped_frames,
            "Frame skipped"
        );

        self.finish_frame();
    }

    // ========== Queries ==========

    /// Get the target FPS
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::FrameScheduler;
    ///
    /// let scheduler = FrameScheduler::new();
    /// assert_eq!(scheduler.target_fps(), 60);
    /// ```
    #[inline]
    pub fn target_fps(&self) -> u32 {
        self.target_fps
    }

    /// Get the frame budget
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::FrameScheduler;
    ///
    /// let scheduler = FrameScheduler::new();
    /// println!("Budget: {:?}", scheduler.frame_budget());
    /// ```
    #[inline]
    pub fn frame_budget(&self) -> Duration {
        self.frame_budget
    }

    /// Get total frames scheduled
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::FrameScheduler;
    ///
    /// let scheduler = FrameScheduler::new();
    /// println!("Total frames: {}", scheduler.total_frames());
    /// ```
    #[inline]
    pub fn total_frames(&self) -> u64 {
        self.total_frames
    }

    /// Get total frames skipped
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::FrameScheduler;
    ///
    /// let scheduler = FrameScheduler::new();
    /// println!("Skipped frames: {}", scheduler.skipped_frames());
    /// ```
    #[inline]
    pub fn skipped_frames(&self) -> u64 {
        self.skipped_frames
    }

    /// Get frame skip rate
    ///
    /// # Returns
    ///
    /// Percentage of skipped frames (0.0 - 100.0).
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::FrameScheduler;
    ///
    /// let scheduler = FrameScheduler::new();
    /// println!("Skip rate: {:.1}%", scheduler.skip_rate());
    /// ```
    pub fn skip_rate(&self) -> f64 {
        if self.total_frames == 0 {
            return 0.0;
        }

        (self.skipped_frames as f64 / self.total_frames as f64) * 100.0
    }

    /// Get number of consecutive deadline misses
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::FrameScheduler;
    ///
    /// let scheduler = FrameScheduler::new();
    /// println!("Consecutive misses: {}", scheduler.consecutive_misses());
    /// ```
    #[inline]
    pub fn consecutive_misses(&self) -> u32 {
        self.consecutive_misses
    }

    /// Reset scheduler state
    ///
    /// Clears all counters and timing data.
    ///
    /// # Example
    ///
    /// ```rust
    /// use flui_core::pipeline::FrameScheduler;
    ///
    /// let mut scheduler = FrameScheduler::new();
    /// scheduler.start_frame();
    /// scheduler.finish_frame();
    ///
    /// scheduler.reset();
    /// assert_eq!(scheduler.total_frames(), 0);
    /// ```
    pub fn reset(&mut self) {
        self.frame_start = None;
        self.last_frame_start = None;
        self.deadline = None;
        self.consecutive_misses = 0;
        self.total_frames = 0;
        self.skipped_frames = 0;
    }
}

impl Default for FrameScheduler {
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
    fn test_scheduler_creation() {
        let scheduler = FrameScheduler::new();
        assert_eq!(scheduler.target_fps(), 60);
        assert_eq!(scheduler.total_frames(), 0);
        assert_eq!(scheduler.skipped_frames(), 0);
    }

    #[test]
    fn test_custom_fps() {
        let scheduler = FrameScheduler::with_target_fps(120);
        assert_eq!(scheduler.target_fps(), 120);

        // 120 FPS = 8.33ms per frame
        let budget = scheduler.frame_budget();
        assert!(budget.as_millis() >= 8 && budget.as_millis() <= 9);
    }

    #[test]
    fn test_frame_lifecycle() {
        let mut scheduler = FrameScheduler::new();

        let budget = scheduler.start_frame();
        assert_eq!(budget, scheduler.frame_budget());
        assert_eq!(scheduler.total_frames(), 1);

        // Should have elapsed time
        assert!(scheduler.elapsed().is_some());

        // Should have remaining time
        assert!(scheduler.remaining().is_some());

        scheduler.finish_frame();
    }

    #[test]
    fn test_deadline_near() {
        let mut scheduler = FrameScheduler::with_target_fps(1000); // 1ms budget
        scheduler.start_frame();

        // Initially not near deadline
        assert!(!scheduler.is_deadline_near());

        // Wait for most of the budget
        thread::sleep(Duration::from_micros(900)); // 90% of 1ms

        // Should be near deadline now
        assert!(scheduler.is_deadline_near());

        scheduler.finish_frame();
    }

    #[test]
    fn test_deadline_missed() {
        let mut scheduler = FrameScheduler::with_target_fps(1000); // 1ms budget
        scheduler.start_frame();

        // Not missed initially
        assert!(!scheduler.is_deadline_missed());

        // Wait past deadline
        thread::sleep(Duration::from_millis(2));

        // Should be missed now
        assert!(scheduler.is_deadline_missed());

        scheduler.finish_frame();

        // Should track consecutive misses
        assert_eq!(scheduler.consecutive_misses(), 1);
    }

    #[test]
    fn test_skip_policy_never() {
        let mut scheduler = FrameScheduler::new();
        scheduler.set_skip_policy(FrameSkipPolicy::Never);

        scheduler.start_frame();
        thread::sleep(Duration::from_millis(20)); // Exceed budget

        assert!(!scheduler.should_skip_frame());
        scheduler.finish_frame();
    }

    #[test]
    fn test_skip_policy_on_deadline_miss() {
        let mut scheduler = FrameScheduler::with_target_fps(1000); // 1ms budget
        scheduler.set_skip_policy(FrameSkipPolicy::OnDeadlineMiss);

        scheduler.start_frame();
        thread::sleep(Duration::from_millis(2)); // Miss deadline

        assert!(scheduler.should_skip_frame());
        scheduler.skip_frame();

        assert_eq!(scheduler.skipped_frames(), 1);
    }

    #[test]
    fn test_skip_policy_consecutive() {
        let mut scheduler = FrameScheduler::with_target_fps(1000); // 1ms budget
        scheduler.set_skip_policy(FrameSkipPolicy::OnConsecutiveMisses(2));

        // First miss - consecutive_misses will be 0 when checked, 1 after finish
        scheduler.start_frame();
        thread::sleep(Duration::from_millis(5));
        assert!(!scheduler.should_skip_frame()); // consecutive_misses = 0 < 2
        scheduler.finish_frame(); // Sets consecutive_misses = 1

        assert_eq!(scheduler.consecutive_misses(), 1);

        // Second miss - consecutive_misses will be 1 when checked, 2 after finish
        scheduler.start_frame();
        thread::sleep(Duration::from_millis(5));
        assert!(!scheduler.should_skip_frame()); // consecutive_misses = 1 < 2
        scheduler.finish_frame(); // Sets consecutive_misses = 2

        assert_eq!(scheduler.consecutive_misses(), 2);

        // Third frame - NOW should_skip_frame() returns true
        scheduler.start_frame();
        assert!(scheduler.should_skip_frame()); // consecutive_misses = 2 >= 2
        scheduler.skip_frame();

        assert_eq!(scheduler.skipped_frames(), 1);
    }

    #[test]
    fn test_remaining_time() {
        let mut scheduler = FrameScheduler::with_target_fps(1000); // 1ms budget
        scheduler.start_frame();

        let remaining1 = scheduler.remaining().unwrap();
        thread::sleep(Duration::from_micros(500));
        let remaining2 = scheduler.remaining().unwrap();

        // Remaining time should decrease
        assert!(remaining2 < remaining1);

        scheduler.finish_frame();
    }

    #[test]
    fn test_reset() {
        let mut scheduler = FrameScheduler::new();

        scheduler.start_frame();
        scheduler.finish_frame();

        assert_eq!(scheduler.total_frames(), 1);

        scheduler.reset();

        assert_eq!(scheduler.total_frames(), 0);
        assert_eq!(scheduler.consecutive_misses(), 0);
    }

    #[test]
    fn test_skip_rate() {
        let mut scheduler = FrameScheduler::new();
        scheduler.set_skip_policy(FrameSkipPolicy::OnDeadlineMiss);

        // 2 normal frames
        scheduler.start_frame();
        scheduler.finish_frame();

        scheduler.start_frame();
        scheduler.finish_frame();

        // 1 skipped frame
        scheduler.start_frame();
        scheduler.skip_frame();

        assert_eq!(scheduler.total_frames(), 3);
        assert_eq!(scheduler.skipped_frames(), 1);
        assert!((scheduler.skip_rate() - 33.33).abs() < 0.1);
    }
}
