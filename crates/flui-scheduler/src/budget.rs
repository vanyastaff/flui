//! Frame budget management - enforce frame time limits
//!
//! Tracks time spent in each phase and enforces budget limits to maintain
//! target framerate (e.g., 16.67ms for 60fps).

use instant::Instant;
use parking_lot::Mutex;
use std::sync::Arc;

/// Budget policy - what to do when over budget
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetPolicy {
    /// Continue work (may drop frames)
    Continue,

    /// Skip low-priority work (Idle tasks)
    SkipIdle,

    /// Skip low and normal priority work (Idle + Build)
    SkipIdleAndBuild,

    /// Stop all work immediately
    StopAll,
}

/// Statistics for a single phase
#[derive(Debug, Clone, Copy)]
pub struct PhaseStats {
    /// Time spent in this phase (ms)
    pub duration_ms: f64,

    /// Percentage of frame budget used
    pub budget_percent: f64,
}

/// Frame budget manager
///
/// Tracks time spent in each phase and enforces budget limits.
#[derive(Debug)]
pub struct FrameBudget {
    /// Target frame duration (ms)
    target_duration_ms: f64,

    /// Frame start time
    frame_start: Option<Instant>,

    /// Policy for handling over-budget situations
    policy: BudgetPolicy,

    /// Phase statistics
    build_time_ms: f64,
    layout_time_ms: f64,
    paint_time_ms: f64,
    composite_time_ms: f64,

    /// Total frame time of last frame
    last_frame_time_ms: f64,

    /// Average frame time (rolling window)
    avg_frame_time_ms: f64,

    /// Frame time history (last 60 frames)
    frame_times: Vec<f64>,

    /// Frame counter
    frame_count: u64,
}

impl FrameBudget {
    /// Create a new frame budget for target FPS
    pub fn new(target_fps: u32) -> Self {
        Self {
            target_duration_ms: 1000.0 / target_fps as f64,
            frame_start: None,
            policy: BudgetPolicy::SkipIdle,
            build_time_ms: 0.0,
            layout_time_ms: 0.0,
            paint_time_ms: 0.0,
            composite_time_ms: 0.0,
            last_frame_time_ms: 0.0,
            avg_frame_time_ms: 0.0,
            frame_times: Vec::with_capacity(60),
            frame_count: 0,
        }
    }

    /// Reset for new frame
    pub fn reset(&mut self) {
        self.frame_start = Some(Instant::now());
        self.build_time_ms = 0.0;
        self.layout_time_ms = 0.0;
        self.paint_time_ms = 0.0;
        self.composite_time_ms = 0.0;
    }

    /// Get elapsed time since frame start (ms)
    pub fn elapsed_ms(&self) -> f64 {
        self.frame_start
            .map(|start| start.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or(0.0)
    }

    /// Check if over budget
    pub fn is_over_budget(&self) -> bool {
        self.elapsed_ms() > self.target_duration_ms
    }

    /// Get remaining budget (ms)
    pub fn remaining_ms(&self) -> f64 {
        (self.target_duration_ms - self.elapsed_ms()).max(0.0)
    }

    /// Get budget utilization (0.0 to 1.0+)
    pub fn utilization(&self) -> f64 {
        self.elapsed_ms() / self.target_duration_ms
    }

    /// Set budget policy
    pub fn set_policy(&mut self, policy: BudgetPolicy) {
        self.policy = policy;
    }

    /// Get current policy
    pub fn policy(&self) -> BudgetPolicy {
        self.policy
    }

    /// Record build phase time
    pub fn record_build_time(&mut self, duration_ms: f64) {
        self.build_time_ms = duration_ms;
    }

    /// Record layout phase time
    pub fn record_layout_time(&mut self, duration_ms: f64) {
        self.layout_time_ms = duration_ms;
    }

    /// Record paint phase time
    pub fn record_paint_time(&mut self, duration_ms: f64) {
        self.paint_time_ms = duration_ms;
    }

    /// Record composite phase time
    pub fn record_composite_time(&mut self, duration_ms: f64) {
        self.composite_time_ms = duration_ms;
    }

    /// Record total frame time
    pub fn record_frame_time(&mut self, total_ms: f64) {
        self.last_frame_time_ms = total_ms;

        // Update rolling average
        self.frame_times.push(total_ms);
        if self.frame_times.len() > 60 {
            self.frame_times.remove(0);
        }

        self.avg_frame_time_ms =
            self.frame_times.iter().sum::<f64>() / self.frame_times.len() as f64;
    }

    /// Get build phase stats
    pub fn build_stats(&self) -> PhaseStats {
        PhaseStats {
            duration_ms: self.build_time_ms,
            budget_percent: (self.build_time_ms / self.target_duration_ms) * 100.0,
        }
    }

    /// Get layout phase stats
    pub fn layout_stats(&self) -> PhaseStats {
        PhaseStats {
            duration_ms: self.layout_time_ms,
            budget_percent: (self.layout_time_ms / self.target_duration_ms) * 100.0,
        }
    }

    /// Get paint phase stats
    pub fn paint_stats(&self) -> PhaseStats {
        PhaseStats {
            duration_ms: self.paint_time_ms,
            budget_percent: (self.paint_time_ms / self.target_duration_ms) * 100.0,
        }
    }

    /// Get composite phase stats
    pub fn composite_stats(&self) -> PhaseStats {
        PhaseStats {
            duration_ms: self.composite_time_ms,
            budget_percent: (self.composite_time_ms / self.target_duration_ms) * 100.0,
        }
    }

    /// Get last frame time
    pub fn last_frame_time_ms(&self) -> f64 {
        self.last_frame_time_ms
    }

    /// Get average frame time (rolling 60-frame window)
    pub fn avg_frame_time_ms(&self) -> f64 {
        self.avg_frame_time_ms
    }

    /// Get target frame duration
    pub fn target_duration_ms(&self) -> f64 {
        self.target_duration_ms
    }

    /// Calculate FPS from average frame time
    pub fn avg_fps(&self) -> f64 {
        if self.avg_frame_time_ms > 0.0 {
            1000.0 / self.avg_frame_time_ms
        } else {
            0.0
        }
    }

    /// Check if frame is "janky" (missed target by >50%)
    pub fn is_janky(&self) -> bool {
        self.last_frame_time_ms > self.target_duration_ms * 1.5
    }

    /// Get the current frame count
    ///
    /// Returns the total number of frames that have been completed since creation.
    ///
    /// # Example
    ///
    /// ```
    /// use flui_scheduler::FrameBudget;
    ///
    /// let mut budget = FrameBudget::new(60);
    /// assert_eq!(budget.frame_count(), 0);
    ///
    /// budget.reset();
    /// budget.finish_frame();
    /// assert_eq!(budget.frame_count(), 1);
    /// ```
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Set the target FPS
    ///
    /// Updates the target frame rate and recalculates the target duration.
    ///
    /// # Example
    ///
    /// ```
    /// use flui_scheduler::FrameBudget;
    ///
    /// let mut budget = FrameBudget::new(60);
    /// budget.set_target_fps(120);
    /// assert_eq!(budget.target_fps(), 120);
    /// ```
    pub fn set_target_fps(&mut self, target_fps: u32) {
        self.target_duration_ms = 1000.0 / target_fps as f64;
    }

    /// Check if deadline is approaching (>80% of budget used)
    ///
    /// Returns `true` if 80% or more of the frame budget has been consumed.
    /// Useful for adaptive work scheduling - skip non-critical work when deadline is near.
    ///
    /// # Example
    ///
    /// ```
    /// use flui_scheduler::FrameBudget;
    ///
    /// let mut budget = FrameBudget::new(60);
    /// budget.reset();
    ///
    /// if budget.is_deadline_near() {
    ///     // Skip non-critical rendering
    /// }
    /// ```
    pub fn is_deadline_near(&self) -> bool {
        self.utilization() >= 0.8
    }

    /// Get target FPS
    ///
    /// Returns the target frames per second configured for this budget.
    ///
    /// # Example
    ///
    /// ```
    /// use flui_scheduler::FrameBudget;
    ///
    /// let budget = FrameBudget::new(60);
    /// assert_eq!(budget.target_fps(), 60);
    /// ```
    pub fn target_fps(&self) -> u32 {
        (1000.0 / self.target_duration_ms).round() as u32
    }

    /// Finish current frame and record total time
    ///
    /// Records the total frame time from `reset()` to now and updates statistics.
    /// This is the companion to `reset()` for frame lifecycle management.
    ///
    /// # Example
    ///
    /// ```
    /// use flui_scheduler::FrameBudget;
    ///
    /// let mut budget = FrameBudget::new(60);
    /// budget.reset();
    /// // ... do frame work ...
    /// budget.finish_frame();
    /// ```
    pub fn finish_frame(&mut self) {
        if let Some(start) = self.frame_start {
            let total_ms = start.elapsed().as_secs_f64() * 1000.0;
            self.record_frame_time(total_ms);
            self.frame_count += 1;
        }
        self.frame_start = None;
    }
}

/// Shared frame budget (thread-safe)
pub type SharedBudget = Arc<Mutex<FrameBudget>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_budget_creation() {
        let budget = FrameBudget::new(60);
        assert_eq!(budget.target_duration_ms(), 1000.0 / 60.0);
        assert_eq!(budget.policy(), BudgetPolicy::SkipIdle);
    }

    #[test]
    fn test_budget_tracking() {
        let mut budget = FrameBudget::new(60);
        budget.reset();

        budget.record_build_time(5.0);
        budget.record_layout_time(3.0);
        budget.record_paint_time(4.0);

        let build_stats = budget.build_stats();
        assert_eq!(build_stats.duration_ms, 5.0);
        assert!((build_stats.budget_percent - 30.0).abs() < 0.1); // ~30% of 16.67ms
    }

    #[test]
    fn test_over_budget_detection() {
        let mut budget = FrameBudget::new(60); // 16.67ms target
        budget.reset();

        // Simulate work
        std::thread::sleep(std::time::Duration::from_millis(20));

        assert!(budget.is_over_budget());
        assert!(budget.remaining_ms() == 0.0);
    }

    #[test]
    fn test_avg_frame_time() {
        let mut budget = FrameBudget::new(60);

        budget.record_frame_time(16.0);
        budget.record_frame_time(17.0);
        budget.record_frame_time(15.0);

        let avg = budget.avg_frame_time_ms();
        assert!((avg - 16.0).abs() < 0.1);
        assert!((budget.avg_fps() - 62.5).abs() < 0.1);
    }

    #[test]
    fn test_janky_frame_detection() {
        let mut budget = FrameBudget::new(60); // 16.67ms target

        budget.record_frame_time(15.0);
        assert!(!budget.is_janky());

        budget.record_frame_time(30.0); // >50% over budget
        assert!(budget.is_janky());
    }

    #[test]
    fn test_deadline_near() {
        let mut budget = FrameBudget::new(1000); // 1ms budget for faster test
        budget.reset();

        // Initially not near deadline
        assert!(!budget.is_deadline_near());

        // Wait for 80% of budget
        std::thread::sleep(std::time::Duration::from_micros(800));

        // Should be near deadline now
        assert!(budget.is_deadline_near());
    }

    #[test]
    fn test_target_fps() {
        let budget60 = FrameBudget::new(60);
        assert_eq!(budget60.target_fps(), 60);

        let budget120 = FrameBudget::new(120);
        assert_eq!(budget120.target_fps(), 120);
    }

    #[test]
    fn test_finish_frame() {
        let mut budget = FrameBudget::new(60);
        budget.reset();

        // Simulate some work
        std::thread::sleep(std::time::Duration::from_millis(5));

        budget.finish_frame();

        // Should have recorded frame time
        assert!(budget.last_frame_time_ms() > 0.0);
        assert!(budget.avg_frame_time_ms() > 0.0);
    }
}
