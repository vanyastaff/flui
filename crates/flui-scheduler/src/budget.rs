//! Frame budget management - enforce frame time limits
//!
//! Tracks time spent in each phase and enforces budget limits to maintain
//! a caller-chosen target framerate (e.g., ~16.7ms for a 60fps target).
//! `FrameBudget` carries no built-in default rate — callers state one
//! explicitly via [`FrameBudget::new`] or [`FrameBudgetBuilder`].
//!
//! ## Type-Safe Budget Management
//!
//! ```rust
//! use std::num::NonZeroU32;
//!
//! use flui_scheduler::{
//!     budget::{BudgetPolicy, FrameBudget},
//!     duration::Milliseconds,
//! };
//!
//! let start = flui_scheduler::Instant::now();
//! let mut budget = FrameBudget::new(NonZeroU32::new(60).expect("60 is non-zero"));
//! budget.reset(start);
//!
//! // Type-safe time recording
//! budget.record_build_duration(Milliseconds::new(5.0));
//!
//! // Check budget status
//! if budget.is_deadline_near(start) {
//!     budget.set_policy(BudgetPolicy::SkipIdle);
//! }
//! ```

use std::{collections::VecDeque, num::NonZeroU32, sync::Arc};

use parking_lot::Mutex;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use web_time::Instant;

use crate::{
    duration::{BudgetPercentage, FrameDuration, Milliseconds},
    frame::FramePhase,
};

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

    /// Share of the frame budget used
    pub budget_percent: BudgetPercentage,
}

impl PhaseStats {
    /// Create new phase stats
    pub fn new(duration: Milliseconds, budget_percent: BudgetPercentage) -> Self {
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
/// Tracks time spent in each phase and enforces budget limits. Instants are
/// supplied by the caller, read from whichever clock stamps its frames, so a
/// virtual clock drives the budget deterministically.
///
/// # Examples
///
/// ```
/// use std::num::NonZeroU32;
///
/// use flui_scheduler::{budget::FrameBudget, duration::Milliseconds};
///
/// let mut budget = FrameBudget::new(NonZeroU32::new(60).expect("60 is non-zero"));
/// budget.reset(flui_scheduler::Instant::now());
///
/// // Record phase timings
/// budget.record_build_duration(Milliseconds::new(5.0));
/// budget.record_layout_duration(Milliseconds::new(3.0));
///
/// // Check budget status
/// let stats = budget.build_stats();
/// assert_eq!(stats.duration.value(), 5.0);
/// ```
#[derive(Debug, Clone)]
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

    /// Average frame time over [`Self::frame_times`]
    avg_frame_time: Milliseconds,

    /// Frame time history (last [`FRAME_TIME_WINDOW`] frames), every entry
    /// finite and non-negative
    frame_times: VecDeque<Milliseconds>,

    /// Frame counter
    frame_count: u64,
}

/// Frames the rolling statistics cover.
const FRAME_TIME_WINDOW: usize = 60;

/// The frame duration of a non-zero rate.
fn fps_duration(fps: NonZeroU32) -> FrameDuration {
    FrameDuration::try_from_fps(fps.get())
        .expect("BUG: a non-zero frame rate is a valid frame duration")
}

impl FrameBudget {
    /// Create a new frame budget for target FPS
    pub fn new(target_fps: NonZeroU32) -> Self {
        Self::with_duration(fps_duration(target_fps))
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
            frame_times: VecDeque::with_capacity(FRAME_TIME_WINDOW + 1),
            frame_count: 0,
        }
    }

    /// Start a new frame at `now`
    pub fn reset(&mut self, now: Instant) {
        self.frame_start = Some(now);
        self.phase_timing.reset();
    }

    /// Time from the frame's start to `now`; zero before [`Self::reset`]
    pub fn elapsed(&self, now: Instant) -> Milliseconds {
        self.frame_start.map_or(Milliseconds::ZERO, |start| {
            Milliseconds::new(now.saturating_duration_since(start).as_secs_f64() * 1000.0)
        })
    }

    /// Check if over budget at `now`
    pub fn is_over_budget(&self, now: Instant) -> bool {
        self.frame_duration.is_over_budget(self.elapsed(now))
    }

    /// Get remaining budget at `now`
    pub fn remaining(&self, now: Instant) -> Milliseconds {
        self.frame_duration.remaining(self.elapsed(now))
    }

    /// Get budget utilization at `now` (0.0 to 1.0+)
    pub fn utilization(&self, now: Instant) -> f64 {
        self.frame_duration.utilization(self.elapsed(now))
    }

    /// Get budget utilization at `now` as percentage
    pub fn utilization_percent(&self, now: Instant) -> BudgetPercentage {
        BudgetPercentage::from_ratio(self.utilization(now))
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

    /// Record build phase duration (type-safe)
    pub fn record_build_duration(&mut self, duration: Milliseconds) {
        self.phase_timing.build = duration;
    }

    /// Record layout phase duration (type-safe)
    pub fn record_layout_duration(&mut self, duration: Milliseconds) {
        self.phase_timing.layout = duration;
    }

    /// Record paint phase duration (type-safe)
    pub fn record_paint_duration(&mut self, duration: Milliseconds) {
        self.phase_timing.paint = duration;
    }

    /// Record composite phase duration (type-safe)
    pub fn record_composite_duration(&mut self, duration: Milliseconds) {
        self.phase_timing.composite = duration;
    }

    /// Record total frame duration.
    ///
    /// Returns `false`, recording nothing, for a non-finite or negative
    /// duration: one such sample would otherwise poison the rolling average.
    pub fn record_frame_duration(&mut self, total: Milliseconds) -> bool {
        if !total.value().is_finite() || total.value() < 0.0 {
            return false;
        }
        self.last_frame_time = total;
        self.frame_count += 1;
        self.frame_times.push_back(total);
        if self.frame_times.len() > FRAME_TIME_WINDOW {
            self.frame_times.pop_front();
        }
        // Summed from the window rather than kept as a running sum, which
        // accumulates rounding error with every eviction.
        let sum: f64 = self.frame_times.iter().map(|time| time.value()).sum();
        self.avg_frame_time = Milliseconds::new(sum / self.frame_times.len() as f64);
        true
    }

    /// Get phase stats
    pub fn phase_stats(&self, phase: FramePhase) -> PhaseStats {
        let duration = self.phase_timing.get(phase);
        let budget_percent =
            BudgetPercentage::from_ratio(duration.value() / self.frame_duration.as_ms().value());
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

    /// Set the frame duration the budget judges frames against, keeping a
    /// fractional rate exact (see [`FrameDuration::try_from_period`]).
    pub fn set_frame_duration(&mut self, frame_duration: FrameDuration) {
        self.frame_duration = frame_duration;
    }

    /// Set the target FPS
    pub fn set_target_fps(&mut self, target_fps: NonZeroU32) {
        self.frame_duration = fps_duration(target_fps);
    }

    /// Get target FPS
    pub fn target_fps(&self) -> u32 {
        self.frame_duration.fps() as u32
    }

    /// Check if deadline is approaching at `now` (>80% of budget used)
    pub fn is_deadline_near(&self, now: Instant) -> bool {
        self.frame_duration.is_deadline_near(self.elapsed(now))
    }

    /// Finish the current frame at `now` and record its total time
    ///
    /// This is a convenience that calls
    /// [`record_frame_duration`](Self::record_frame_duration)
    /// with the time from [`reset`](Self::reset) to `now`.
    pub fn finish_frame(&mut self, now: Instant) {
        if self.frame_start.is_some() {
            self.record_frame_duration(self.elapsed(now));
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
    /// A frame is considered "janky" if it took longer than the configured
    /// target frame duration.
    pub fn jank_count(&self) -> usize {
        let threshold = self.frame_duration.as_ms().value();
        self.frame_times
            .iter()
            .filter(|t| t.value() > threshold)
            .count()
    }

    /// Get jank percentage
    pub fn jank_percentage(&self) -> BudgetPercentage {
        if self.frame_times.is_empty() {
            return BudgetPercentage::ZERO;
        }
        BudgetPercentage::from_ratio(self.jank_count() as f64 / self.frame_times.len() as f64)
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
    pub fn total_budget_percent(&self) -> BudgetPercentage {
        BudgetPercentage::new(
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
/// use flui_scheduler::{
///     budget::{BudgetPolicy, FrameBudgetBuilder},
///     duration::FrameDuration,
/// };
///
/// let budget = FrameBudgetBuilder::new()
///     .target_fps(std::num::NonZeroU32::new(120).expect("120 is non-zero"))
///     .policy(BudgetPolicy::SkipIdleAndBuild)
///     .build();
///
/// assert_eq!(budget.policy(), BudgetPolicy::SkipIdleAndBuild);
/// ```
#[derive(Debug, Clone, Default)]
pub struct FrameBudgetBuilder {
    target_fps: Option<NonZeroU32>,
    frame_duration: Option<FrameDuration>,
    policy: Option<BudgetPolicy>,
}

impl FrameBudgetBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self::default()
    }

    /// Set target FPS
    pub fn target_fps(mut self, fps: NonZeroU32) -> Self {
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
    ///
    /// A caller that specifies neither [`Self::target_fps`] nor
    /// [`Self::frame_duration`] gets a 60fps fallback — an ordinary builder
    /// default, not a scheduler-wide assumption: `flui-scheduler` itself
    /// makes no frame-rate assumption anywhere (see `UpdateScheduler::new`).
    pub fn build(self) -> FrameBudget {
        let frame_duration = self
            .frame_duration
            .or_else(|| self.target_fps.map(fps_duration))
            .unwrap_or_else(|| {
                FrameDuration::try_from_fps(60).expect("BUG: 60 fps is always valid")
            });

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
pub fn shared_budget(target_fps: NonZeroU32) -> SharedBudget {
    Arc::new(Mutex::new(FrameBudget::new(target_fps)))
}
