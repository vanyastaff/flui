//! Frame management - lifecycle, timing, and callbacks
//!
//! A frame represents one render cycle from VSync to present:
//! ```text
//! VSync → BeginFrame → Build → Layout → Paint → EndFrame → Present
//! ```
//!
//! ## Type-Safe Frame IDs
//!
//! Frame IDs use the newtype pattern for type safety:
//! ```rust
//! use flui_scheduler::frame::FrameId;
//!
//! let frame1 = FrameId::new();
//! let frame2 = FrameId::new();
//! assert_ne!(frame1, frame2);
//! ```

use crate::duration::{FrameDuration, Milliseconds, Percentage, Seconds};
use crate::id::{FrameIdMarker, TypedId};
use std::fmt;
use std::sync::Arc;
use web_time::Instant;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Unique frame identifier using type-safe ID
///
/// Uses `NonZeroU64` internally for niche optimization -
/// `Option<FrameId>` is the same size as `FrameId` (8 bytes).
pub type FrameId = TypedId<FrameIdMarker>;

/// Frame phase - which part of the render pipeline is executing
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[repr(u8)]
pub enum FramePhase {
    /// Waiting for frame to start
    #[default]
    Idle = 0,
    /// Building widget tree (View → Element)
    Build = 1,
    /// Computing layout (constraints → sizes)
    Layout = 2,
    /// Painting to layers (Element → DisplayList)
    Paint = 3,
    /// Compositing layers to screen
    Composite = 4,
}

impl FramePhase {
    /// All phases in execution order
    pub const ALL: [FramePhase; 5] = [
        FramePhase::Idle,
        FramePhase::Build,
        FramePhase::Layout,
        FramePhase::Paint,
        FramePhase::Composite,
    ];

    /// Get the next phase in the pipeline
    #[inline]
    pub const fn next(self) -> Option<Self> {
        match self {
            Self::Idle => Some(Self::Build),
            Self::Build => Some(Self::Layout),
            Self::Layout => Some(Self::Paint),
            Self::Paint => Some(Self::Composite),
            Self::Composite => None,
        }
    }

    /// Get the previous phase in the pipeline
    #[inline]
    pub const fn prev(self) -> Option<Self> {
        match self {
            Self::Idle => None,
            Self::Build => Some(Self::Idle),
            Self::Layout => Some(Self::Build),
            Self::Paint => Some(Self::Layout),
            Self::Composite => Some(Self::Paint),
        }
    }

    /// Check if this is an active rendering phase
    #[inline]
    pub const fn is_rendering(self) -> bool {
        matches!(
            self,
            Self::Build | Self::Layout | Self::Paint | Self::Composite
        )
    }

    /// Get phase as numeric index
    #[inline]
    pub const fn as_index(self) -> usize {
        self as usize
    }
}

impl fmt::Display for FramePhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Idle => write!(f, "Idle"),
            Self::Build => write!(f, "Build"),
            Self::Layout => write!(f, "Layout"),
            Self::Paint => write!(f, "Paint"),
            Self::Composite => write!(f, "Composite"),
        }
    }
}

/// Frame timing information with type-safe durations
///
/// # Examples
///
/// ```
/// use flui_scheduler::frame::{FrameTiming, FramePhase};
/// use flui_scheduler::duration::FrameDuration;
///
/// let timing = FrameTiming::new(60);
/// assert_eq!(timing.phase, FramePhase::Idle);
///
/// // Check budget
/// assert!(!timing.is_over_budget());
/// let remaining = timing.remaining();
/// assert!(remaining.value() > 0.0);
///
/// // Using builder
/// use flui_scheduler::frame::FrameTimingBuilder;
/// let timing = FrameTimingBuilder::new()
///     .target_fps(120)
///     .initial_phase(FramePhase::Build)
///     .build();
/// ```
#[derive(Debug, Clone, Copy)]
pub struct FrameTiming {
    /// Frame identifier
    pub id: FrameId,

    /// When the frame started (vsync time)
    pub start_time: Instant,

    /// Frame duration configuration
    pub frame_duration: FrameDuration,

    /// Current phase
    pub phase: FramePhase,

    /// Target frame duration in milliseconds (for backwards compat)
    pub target_duration_ms: f64,
}

impl FrameTiming {
    /// Create a new frame timing
    pub fn new(target_fps: u32) -> Self {
        let frame_duration = FrameDuration::from_fps(target_fps);
        Self {
            id: FrameId::new(),
            start_time: Instant::now(),
            frame_duration,
            phase: FramePhase::Idle,
            target_duration_ms: frame_duration.as_ms().value(),
        }
    }

    /// Create with a specific frame duration
    pub fn with_duration(frame_duration: FrameDuration) -> Self {
        Self {
            id: FrameId::new(),
            start_time: Instant::now(),
            target_duration_ms: frame_duration.as_ms().value(),
            frame_duration,
            phase: FramePhase::Idle,
        }
    }

    /// Get elapsed time since frame start as type-safe Milliseconds
    #[inline]
    pub fn elapsed(&self) -> Milliseconds {
        Milliseconds::new(self.start_time.elapsed().as_secs_f64() * 1000.0)
    }

    /// Get elapsed time since frame start in milliseconds (raw f64)
    #[inline]
    pub fn elapsed_ms(&self) -> f64 {
        self.start_time.elapsed().as_secs_f64() * 1000.0
    }

    /// Get elapsed time since frame start as type-safe Seconds
    #[inline]
    pub fn elapsed_as_seconds(&self) -> Seconds {
        Seconds::new(self.start_time.elapsed().as_secs_f64())
    }

    /// Get elapsed time since frame start in seconds (raw f64)
    #[inline]
    pub fn elapsed_secs(&self) -> f64 {
        self.start_time.elapsed().as_secs_f64()
    }

    /// Check if frame is over budget
    #[inline]
    pub fn is_over_budget(&self) -> bool {
        self.frame_duration.is_over_budget(self.elapsed())
    }

    /// Get remaining budget as type-safe Milliseconds
    #[inline]
    pub fn remaining(&self) -> Milliseconds {
        self.frame_duration.remaining(self.elapsed())
    }

    /// Get remaining budget in milliseconds (raw f64)
    #[inline]
    pub fn remaining_budget_ms(&self) -> f64 {
        self.remaining().value()
    }

    /// Calculate how much over/under budget (negative = over budget)
    #[inline]
    pub fn budget_delta_ms(&self) -> f64 {
        self.target_duration_ms - self.elapsed_ms()
    }

    /// Get budget utilization as percentage
    #[inline]
    pub fn utilization(&self) -> Percentage {
        Percentage::from_ratio(self.elapsed_ms() / self.target_duration_ms)
    }

    /// Check if deadline is near (>80% budget used)
    #[inline]
    pub fn is_deadline_near(&self) -> bool {
        self.frame_duration.is_deadline_near(self.elapsed())
    }

    /// Check if frame is janky (>150% budget used)
    #[inline]
    pub fn is_janky(&self) -> bool {
        self.frame_duration.is_janky(self.elapsed())
    }

    /// Advance to the next phase
    #[inline]
    pub fn advance_phase(&mut self) -> bool {
        if let Some(next) = self.phase.next() {
            self.phase = next;
            true
        } else {
            false
        }
    }

    /// Get target FPS
    #[inline]
    pub fn target_fps(&self) -> u32 {
        self.frame_duration.fps() as u32
    }
}

impl Default for FrameTiming {
    fn default() -> Self {
        Self::new(60)
    }
}

/// Frame callback - executed at frame boundaries
pub type FrameCallback = Box<dyn FnOnce(&FrameTiming) + Send>;

/// Persistent frame callback (can be called multiple times)
///
/// Uses Arc for cheap cloning - persistent callbacks are cloned before execution
/// to avoid holding locks during callback invocation.
pub type PersistentFrameCallback = Arc<dyn Fn(&FrameTiming) + Send + Sync>;

/// Post-frame callback - executed after frame completes
pub type PostFrameCallback = Box<dyn FnOnce(&FrameTiming) + Send>;

/// Builder for creating frame timing with custom configuration
#[derive(Debug, Clone)]
pub struct FrameTimingBuilder {
    target_fps: Option<u32>,
    frame_duration: Option<FrameDuration>,
    initial_phase: FramePhase,
}

impl FrameTimingBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            target_fps: None,
            frame_duration: None,
            initial_phase: FramePhase::Idle,
        }
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

    /// Set initial phase
    pub fn initial_phase(mut self, phase: FramePhase) -> Self {
        self.initial_phase = phase;
        self
    }

    /// Build the frame timing
    pub fn build(self) -> FrameTiming {
        let frame_duration = self
            .frame_duration
            .or_else(|| self.target_fps.map(FrameDuration::from_fps))
            .unwrap_or(FrameDuration::FPS_60);

        let mut timing = FrameTiming::with_duration(frame_duration);
        timing.phase = self.initial_phase;
        timing
    }
}

impl Default for FrameTimingBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_id_unique() {
        let id1 = FrameId::new();
        let id2 = FrameId::new();
        assert_ne!(id1, id2);
        assert!(id2.as_u64() > id1.as_u64());
    }

    #[test]
    fn test_frame_timing_budget() {
        let timing = FrameTiming::new(60); // 16.67ms target
        assert!(!timing.is_over_budget());
        assert!(timing.remaining().value() > 0.0);
        assert!((timing.target_duration_ms - 1000.0 / 60.0).abs() < 0.01);
    }

    #[test]
    fn test_frame_phase_display() {
        assert_eq!(format!("{}", FramePhase::Build), "Build");
        assert_eq!(format!("{}", FramePhase::Layout), "Layout");
        assert_eq!(format!("{}", FramePhase::Paint), "Paint");
    }

    #[test]
    fn test_frame_phase_navigation() {
        assert_eq!(FramePhase::Idle.next(), Some(FramePhase::Build));
        assert_eq!(FramePhase::Build.next(), Some(FramePhase::Layout));
        assert_eq!(FramePhase::Composite.next(), None);

        assert_eq!(FramePhase::Composite.prev(), Some(FramePhase::Paint));
        assert_eq!(FramePhase::Idle.prev(), None);
    }

    #[test]
    fn test_frame_timing_builder() {
        let timing = FrameTimingBuilder::new()
            .target_fps(120)
            .initial_phase(FramePhase::Build)
            .build();

        assert_eq!(timing.phase, FramePhase::Build);
        // Allow for rounding due to float conversions
        assert!((timing.target_fps() as i32 - 120).abs() <= 1);
    }

    #[test]
    fn test_frame_timing_with_duration() {
        let timing = FrameTiming::with_duration(FrameDuration::FPS_144);
        assert!((timing.frame_duration.fps() - 144.0).abs() < 0.1);
    }

    #[test]
    fn test_utilization() {
        let timing = FrameTiming::new(60);
        let util = timing.utilization();
        // Just started, should be very low
        assert!(util.value() < 10.0);
    }
}
