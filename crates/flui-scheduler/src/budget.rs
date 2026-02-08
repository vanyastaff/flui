//! Frame budget management - enforce frame time limits
//!
//! Tracks time spent in each phase and enforces budget limits to maintain
//! target framerate (e.g., 16.67ms for 60fps).
//!
//! ## Type-Safe Budget Management
//!
//! ```rust
//! use flui_scheduler::budget::{FrameBudget, BudgetPolicy};
//! use flui_scheduler::duration::{Milliseconds, FrameDuration};
//!
//! let mut budget = FrameBudget::new(60);
//!
//! // Type-safe time recording
//! budget.record_build_duration(Milliseconds::new(5.0));
//!
//! // Check budget status
//! if budget.is_deadline_near() {
//!     budget.set_policy(BudgetPolicy::SkipIdle);
//! }
//! ```

use crate::duration::{FrameDuration, Milliseconds, Percentage};
use crate::frame::FramePhase;
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;
use web_time::Instant;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Budget policy - what to do when over budget
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[repr(u8)]
pub enum BudgetPolicy {
    /// Continue work (may drop frames)
    Continue = 0,

    /// Skip low-priority work (Idle tasks)
    #[default]
    SkipIdle = 1,

    /// Skip low and normal priority work (Idle + Build)
    SkipIdleAndBuild = 2,

    /// Stop all work immediately
    StopAll = 3,
}

impl BudgetPolicy {
    /// All policies from most permissive to most restrictive
    pub const ALL: [BudgetPolicy; 4] = [
        BudgetPolicy::Continue,
        BudgetPolicy::SkipIdle,
        BudgetPolicy::SkipIdleAndBuild,
        BudgetPolicy::StopAll,
    ];

    /// Get a more restrictive policy
    #[inline]
    pub const fn more_restrictive(self) -> Option<Self> {
        match self {
            Self::Continue => Some(Self::SkipIdle),
            Self::SkipIdle => Some(Self::SkipIdleAndBuild),
            Self::SkipIdleAndBuild => Some(Self::StopAll),
            Self::StopAll => None,
        }
    }

    /// Get a less restrictive policy
    #[inline]
    pub const fn less_restrictive(self) -> Option<Self> {
        match self {
            Self::Continue => None,
            Self::SkipIdle => Some(Self::Continue),
            Self::SkipIdleAndBuild => Some(Self::SkipIdle),
            Self::StopAll => Some(Self::SkipIdleAndBuild),
        }
    }
}

/// Statistics for a single phase with type-safe durations
#[derive(Debug, Clone, Copy, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PhaseStats {
    /// Time spent in this phase
    pub duration: Milliseconds,

    /// Percentage of frame budget used
    pub budget_percent: Percentage,
}

impl PhaseStats {
    /// Create new phase stats
    pub fn new(duration: Milliseconds, budget_percent: Percentage) -> Self {
        Self {
            duration,
            budget_percent,
        }
    }

    /// Get duration in milliseconds (raw f64 for backwards compat)
    #[inline]
    pub fn duration_ms(&self) -> f64 {
        self.duration.value()
    }
}

/// Per-phase timing data
#[derive(Debug, Clone, Copy, Default)]
struct PhaseTiming {
    build: Milliseconds,
    layout: Milliseconds,
    paint: Milliseconds,
    composite: Milliseconds,
}

impl PhaseTiming {
    /// Get timing for a specific phase
    fn get(&self, phase: FramePhase) -> Milliseconds {
        match phase {
            FramePhase::Idle => Milliseconds::ZERO,
            FramePhase::Build => self.build,
            FramePhase::Layout => self.layout,
            FramePhase::Paint => self.paint,
            FramePhase::Composite => self.composite,
        }
    }

    /// Set timing for a specific phase
    fn set(&mut self, phase: FramePhase, duration: Milliseconds) {
        match phase {
            FramePhase::Idle => {}
            FramePhase::Build => self.build = duration,
            FramePhase::Layout => self.layout = duration,
            FramePhase::Paint => self.paint = duration,
            FramePhase::Composite => self.composite = duration,
        }
    }

    /// Reset all timings
    fn reset(&mut self) {
        *self = Self::default();
    }
}

/// Frame budget manager with type-safe durations
///
/// Tracks time spent in each phase and enforces budget limits.
///
/// # Examples
///
/// ```
/// use flui_scheduler::budget::FrameBudget;
/// use flui_scheduler::duration::Milliseconds;
///
/// let mut budget = FrameBudget::new(60);
/// budget.reset();
///
/// // Record phase timings
/// budget.record_build_duration(Milliseconds::new(5.0));
/// budget.record_layout_duration(Milliseconds::new(3.0));
///
/// // Check budget status
/// let stats = budget.build_stats();
/// assert_eq!(stats.duration.value(), 5.0);
/// ```
#[derive(Debug)]
pub struct FrameBudget {
    /// Frame duration configuration
    frame_duration: FrameDuration,

    /// Frame start time
    frame_start: Option<Instant>,

    /// Policy for handling over-budget situations
    policy: BudgetPolicy,

    /// Phase timings for current frame
    phase_timing: PhaseTiming,

    /// Total frame time of last frame
    last_frame_time: Milliseconds,

    /// Average frame time (rolling window)
    avg_frame_time: Milliseconds,

    /// Running sum of frame times in the rolling window (avoids re-summing each frame)
    running_sum: f64,

    /// Frame time history (last 60 frames)
    frame_times: VecDeque<Milliseconds>,

    /// Frame counter
    frame_count: u64,
}

impl FrameBudget {
    /// Create a new frame budget for target FPS
    pub fn new(target_fps: u32) -> Self {
        Self {
            frame_duration: FrameDuration::from_fps(target_fps),
            frame_start: None,
            policy: BudgetPolicy::SkipIdle,
            phase_timing: PhaseTiming::default(),
            last_frame_time: Milliseconds::ZERO,
            avg_frame_time: Milliseconds::ZERO,
            running_sum: 0.0,
            frame_times: VecDeque::with_capacity(60),
            frame_count: 0,
        }
    }

    /// Create with a specific frame duration
    pub fn with_duration(frame_duration: FrameDuration) -> Self {
        Self {
            frame_duration,
            frame_start: None,
            policy: BudgetPolicy::SkipIdle,
            phase_timing: PhaseTiming::default(),
            last_frame_time: Milliseconds::ZERO,
            avg_frame_time: Milliseconds::ZERO,
            running_sum: 0.0,
            frame_times: VecDeque::with_capacity(60),
            frame_count: 0,
        }
    }

    /// Reset for new frame
    pub fn reset(&mut self) {
        self.frame_start = Some(Instant::now());
        self.phase_timing.reset();
    }

    /// Get elapsed time since frame start
    pub fn elapsed(&self) -> Milliseconds {
        self.frame_start
            .map(|start| Milliseconds::new(start.elapsed().as_secs_f64() * 1000.0))
            .unwrap_or(Milliseconds::ZERO)
    }

    /// Get elapsed time since frame start (raw f64 for backwards compat)
    pub fn elapsed_ms(&self) -> f64 {
        self.elapsed().value()
    }

    /// Check if over budget
    pub fn is_over_budget(&self) -> bool {
        self.frame_duration.is_over_budget(self.elapsed())
    }

    /// Get remaining budget
    pub fn remaining(&self) -> Milliseconds {
        self.frame_duration.remaining(self.elapsed())
    }

    /// Get remaining budget (raw f64 for backwards compat)
    pub fn remaining_ms(&self) -> f64 {
        self.remaining().value()
    }

    /// Get budget utilization (0.0 to 1.0+)
    pub fn utilization(&self) -> f64 {
        self.frame_duration.utilization(self.elapsed())
    }

    /// Get budget utilization as percentage
    pub fn utilization_percent(&self) -> Percentage {
        Percentage::from_ratio(self.utilization())
    }

    /// Set budget policy
    pub fn set_policy(&mut self, policy: BudgetPolicy) {
        self.policy = policy;
    }

    /// Get current policy
    pub fn policy(&self) -> BudgetPolicy {
        self.policy
    }

    /// Record phase duration with type-safe Milliseconds
    pub fn record_phase_duration(&mut self, phase: FramePhase, duration: Milliseconds) {
        self.phase_timing.set(phase, duration);
    }

    /// Record build phase time
    pub fn record_build_time(&mut self, duration_ms: f64) {
        self.phase_timing.build = Milliseconds::new(duration_ms);
    }

    /// Record build phase duration (type-safe)
    pub fn record_build_duration(&mut self, duration: Milliseconds) {
        self.phase_timing.build = duration;
    }

    /// Record layout phase time
    pub fn record_layout_time(&mut self, duration_ms: f64) {
        self.phase_timing.layout = Milliseconds::new(duration_ms);
    }

    /// Record layout phase duration (type-safe)
    pub fn record_layout_duration(&mut self, duration: Milliseconds) {
        self.phase_timing.layout = duration;
    }

    /// Record paint phase time
    pub fn record_paint_time(&mut self, duration_ms: f64) {
        self.phase_timing.paint = Milliseconds::new(duration_ms);
    }

    /// Record paint phase duration (type-safe)
    pub fn record_paint_duration(&mut self, duration: Milliseconds) {
        self.phase_timing.paint = duration;
    }

    /// Record composite phase time
    pub fn record_composite_time(&mut self, duration_ms: f64) {
        self.phase_timing.composite = Milliseconds::new(duration_ms);
    }

    /// Record composite phase duration (type-safe)
    pub fn record_composite_duration(&mut self, duration: Milliseconds) {
        self.phase_timing.composite = duration;
    }

    /// Record total frame time
    pub fn record_frame_time(&mut self, total_ms: f64) {
        self.record_frame_duration(Milliseconds::new(total_ms));
    }

    /// Record total frame duration (type-safe)
    pub fn record_frame_duration(&mut self, total: Milliseconds) {
        self.last_frame_time = total;
        self.frame_count += 1;

        // Update running sum: add new, subtract evicted
        self.running_sum += total.value();
        self.frame_times.push_back(total);
        if self.frame_times.len() > 60 {
            if let Some(evicted) = self.frame_times.pop_front() {
                self.running_sum -= evicted.value();
            }
        }

        self.avg_frame_time = Milliseconds::new(self.running_sum / self.frame_times.len() as f64);
    }

    /// Get phase stats
    pub fn phase_stats(&self, phase: FramePhase) -> PhaseStats {
        let duration = self.phase_timing.get(phase);
        let budget_percent =
            Percentage::from_ratio(duration.value() / self.frame_duration.as_ms().value());
        PhaseStats::new(duration, budget_percent)
    }

    /// Get build phase stats
    pub fn build_stats(&self) -> PhaseStats {
        self.phase_stats(FramePhase::Build)
    }

    /// Get layout phase stats
    pub fn layout_stats(&self) -> PhaseStats {
        self.phase_stats(FramePhase::Layout)
    }

    /// Get paint phase stats
    pub fn paint_stats(&self) -> PhaseStats {
        self.phase_stats(FramePhase::Paint)
    }

    /// Get composite phase stats
    pub fn composite_stats(&self) -> PhaseStats {
        self.phase_stats(FramePhase::Composite)
    }

    /// Get all phase stats
    pub fn all_phase_stats(&self) -> AllPhaseStats {
        AllPhaseStats {
            build: self.build_stats(),
            layout: self.layout_stats(),
            paint: self.paint_stats(),
            composite: self.composite_stats(),
        }
    }

    /// Get last frame time
    pub fn last_frame_time(&self) -> Milliseconds {
        self.last_frame_time
    }

    /// Get last frame time (raw f64 for backwards compat)
    pub fn last_frame_time_ms(&self) -> f64 {
        self.last_frame_time.value()
    }

    /// Get average frame time (rolling 60-frame window)
    pub fn avg_frame_time(&self) -> Milliseconds {
        self.avg_frame_time
    }

    /// Get average frame time (raw f64 for backwards compat)
    pub fn avg_frame_time_ms(&self) -> f64 {
        self.avg_frame_time.value()
    }

    /// Get target frame duration
    pub fn target_duration(&self) -> Milliseconds {
        self.frame_duration.as_ms()
    }

    /// Get target frame duration (raw f64 for backwards compat)
    pub fn target_duration_ms(&self) -> f64 {
        self.frame_duration.as_ms().value()
    }

    /// Get frame duration configuration
    pub fn frame_duration(&self) -> FrameDuration {
        self.frame_duration
    }

    /// Calculate FPS from average frame time
    pub fn avg_fps(&self) -> f64 {
        if self.avg_frame_time.value() > 0.0 {
            1000.0 / self.avg_frame_time.value()
        } else {
            0.0
        }
    }

    /// Check if frame is "janky" (missed target by >50%)
    pub fn is_janky(&self) -> bool {
        self.frame_duration.is_janky(self.last_frame_time)
    }

    /// Get the current frame count
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Set the target FPS
    pub fn set_target_fps(&mut self, target_fps: u32) {
        self.frame_duration = FrameDuration::from_fps(target_fps);
    }

    /// Get target FPS
    pub fn target_fps(&self) -> u32 {
        self.frame_duration.fps() as u32
    }

    /// Check if deadline is approaching (>80% of budget used)
    pub fn is_deadline_near(&self) -> bool {
        self.frame_duration.is_deadline_near(self.elapsed())
    }

    /// Finish current frame and record total time
    ///
    /// This is a convenience that calls [`record_frame_duration`](Self::record_frame_duration)
    /// with the elapsed time since [`reset`](Self::reset) was called.
    pub fn finish_frame(&mut self) {
        if let Some(start) = self.frame_start {
            let total = Milliseconds::new(start.elapsed().as_secs_f64() * 1000.0);
            self.record_frame_duration(total);
        }
        self.frame_start = None;
    }

    /// Get frame time variance (standard deviation)
    pub fn frame_time_variance(&self) -> f64 {
        if self.frame_times.len() < 2 {
            return 0.0;
        }

        let mean = self.avg_frame_time.value();
        let variance: f64 = self
            .frame_times
            .iter()
            .map(|t| {
                let diff = t.value() - mean;
                diff * diff
            })
            .sum::<f64>()
            / (self.frame_times.len() - 1) as f64;

        variance.sqrt()
    }

    /// Get jank count (frames that exceeded the target frame duration)
    ///
    /// A frame is considered "janky" if it took longer than the target
    /// frame duration (e.g., >16.67ms for 60 FPS).
    pub fn jank_count(&self) -> usize {
        let threshold = self.frame_duration.as_ms().value();
        self.frame_times
            .iter()
            .filter(|t| t.value() > threshold)
            .count()
    }

    /// Get jank percentage
    pub fn jank_percentage(&self) -> Percentage {
        if self.frame_times.is_empty() {
            return Percentage::ZERO;
        }
        Percentage::from_ratio(self.jank_count() as f64 / self.frame_times.len() as f64)
    }
}

/// All phase statistics
#[derive(Debug, Clone, Copy, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct AllPhaseStats {
    /// Build phase stats
    pub build: PhaseStats,
    /// Layout phase stats
    pub layout: PhaseStats,
    /// Paint phase stats
    pub paint: PhaseStats,
    /// Composite phase stats
    pub composite: PhaseStats,
}

impl AllPhaseStats {
    /// Total duration across all phases
    pub fn total_duration(&self) -> Milliseconds {
        self.build.duration + self.layout.duration + self.paint.duration + self.composite.duration
    }

    /// Total budget percentage used
    pub fn total_budget_percent(&self) -> Percentage {
        Percentage::new(
            self.build.budget_percent.value()
                + self.layout.budget_percent.value()
                + self.paint.budget_percent.value()
                + self.composite.budget_percent.value(),
        )
    }
}

/// Builder for creating frame budgets with custom configuration
///
/// # Examples
///
/// ```
/// use flui_scheduler::budget::{FrameBudgetBuilder, BudgetPolicy};
/// use flui_scheduler::duration::FrameDuration;
///
/// let budget = FrameBudgetBuilder::new()
///     .target_fps(120)
///     .policy(BudgetPolicy::SkipIdleAndBuild)
///     .build();
///
/// assert_eq!(budget.policy(), BudgetPolicy::SkipIdleAndBuild);
/// ```
#[derive(Debug, Clone, Default)]
pub struct FrameBudgetBuilder {
    target_fps: Option<u32>,
    frame_duration: Option<FrameDuration>,
    policy: Option<BudgetPolicy>,
}

impl FrameBudgetBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Set target FPS
    pub fn target_fps(mut self, fps: u32) -> Self {
        self.target_fps = Some(fps);
        self
    }

    /// Set frame duration directly
    pub fn frame_duration(mut self, duration: FrameDuration) -> Self {
        self.frame_duration = Some(duration);
        self
    }

    /// Set budget policy
    pub fn policy(mut self, policy: BudgetPolicy) -> Self {
        self.policy = Some(policy);
        self
    }

    /// Build the frame budget
    pub fn build(self) -> FrameBudget {
        let frame_duration = self
            .frame_duration
            .or_else(|| self.target_fps.map(FrameDuration::from_fps))
            .unwrap_or(FrameDuration::FPS_60);

        let mut budget = FrameBudget::with_duration(frame_duration);

        if let Some(policy) = self.policy {
            budget.set_policy(policy);
        }

        budget
    }
}

/// Shared frame budget (thread-safe)
pub type SharedBudget = Arc<Mutex<FrameBudget>>;

/// Create a new shared budget
pub fn shared_budget(target_fps: u32) -> SharedBudget {
    Arc::new(Mutex::new(FrameBudget::new(target_fps)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_budget_creation() {
        let budget = FrameBudget::new(60);
        assert!((budget.target_duration_ms() - 1000.0 / 60.0).abs() < 0.01);
        assert_eq!(budget.policy(), BudgetPolicy::SkipIdle);
    }

    #[test]
    fn test_budget_tracking() {
        let mut budget = FrameBudget::new(60);
        budget.reset();

        budget.record_build_duration(Milliseconds::new(5.0));
        budget.record_layout_duration(Milliseconds::new(3.0));
        budget.record_paint_duration(Milliseconds::new(4.0));

        let build_stats = budget.build_stats();
        assert_eq!(build_stats.duration.value(), 5.0);
        assert!((build_stats.budget_percent.value() - 30.0).abs() < 1.0); // ~30% of 16.67ms
    }

    #[test]
    fn test_over_budget_detection() {
        let mut budget = FrameBudget::new(60); // 16.67ms target
        budget.reset();

        // Simulate work
        std::thread::sleep(std::time::Duration::from_millis(20));

        assert!(budget.is_over_budget());
        assert_eq!(budget.remaining(), Milliseconds::ZERO);
    }

    #[test]
    fn test_avg_frame_time() {
        let mut budget = FrameBudget::new(60);

        budget.record_frame_duration(Milliseconds::new(16.0));
        budget.record_frame_duration(Milliseconds::new(17.0));
        budget.record_frame_duration(Milliseconds::new(15.0));

        let avg = budget.avg_frame_time();
        assert!((avg.value() - 16.0).abs() < 0.1);
        assert!((budget.avg_fps() - 62.5).abs() < 0.1);
    }

    #[test]
    fn test_janky_frame_detection() {
        let mut budget = FrameBudget::new(60); // 16.67ms target

        budget.record_frame_duration(Milliseconds::new(15.0));
        assert!(!budget.is_janky());

        budget.record_frame_duration(Milliseconds::new(30.0)); // >50% over budget
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
        // Allow for rounding due to float conversions
        assert!((budget60.target_fps() as i32 - 60).abs() <= 1);

        let budget120 = FrameBudget::new(120);
        assert!((budget120.target_fps() as i32 - 120).abs() <= 1);
    }

    #[test]
    fn test_finish_frame() {
        let mut budget = FrameBudget::new(60);
        budget.reset();

        // Simulate some work
        std::thread::sleep(std::time::Duration::from_millis(5));

        budget.finish_frame();

        // Should have recorded frame time
        assert!(budget.last_frame_time().value() > 0.0);
        assert!(budget.avg_frame_time().value() > 0.0);
        assert_eq!(budget.frame_count(), 1);
    }

    #[test]
    fn test_policy_navigation() {
        assert_eq!(
            BudgetPolicy::Continue.more_restrictive(),
            Some(BudgetPolicy::SkipIdle)
        );
        assert_eq!(BudgetPolicy::StopAll.more_restrictive(), None);
        assert_eq!(
            BudgetPolicy::StopAll.less_restrictive(),
            Some(BudgetPolicy::SkipIdleAndBuild)
        );
        assert_eq!(BudgetPolicy::Continue.less_restrictive(), None);
    }

    #[test]
    fn test_all_phase_stats() {
        let mut budget = FrameBudget::new(60);
        budget.record_build_duration(Milliseconds::new(5.0));
        budget.record_layout_duration(Milliseconds::new(3.0));
        budget.record_paint_duration(Milliseconds::new(4.0));
        budget.record_composite_duration(Milliseconds::new(2.0));

        let stats = budget.all_phase_stats();
        assert!((stats.total_duration().value() - 14.0).abs() < 0.01);
    }

    #[test]
    fn test_jank_statistics() {
        let mut budget = FrameBudget::new(60); // 16.67ms target

        // Add some normal frames
        for _ in 0..8 {
            budget.record_frame_duration(Milliseconds::new(15.0));
        }
        // Add some janky frames (>25ms = >150% of 16.67ms)
        for _ in 0..2 {
            budget.record_frame_duration(Milliseconds::new(30.0));
        }

        assert_eq!(budget.jank_count(), 2);
        assert!((budget.jank_percentage().value() - 20.0).abs() < 0.1);
    }
}
