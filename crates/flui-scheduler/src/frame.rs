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

/// Scheduler phase - which part of the frame lifecycle is executing
///
/// This follows Flutter's SchedulerPhase model for proper frame coordination.
/// The phases execute in order:
///
/// ```text
/// Idle → TransientCallbacks → MidFrameMicrotasks → PersistentCallbacks → PostFrameCallbacks → Idle
/// ```
///
/// - **TransientCallbacks**: Animation tickers fire here (one-time frame callbacks)
/// - **MidFrameMicrotasks**: Microtask queue flushes between animations and rendering
/// - **PersistentCallbacks**: Rendering pipeline runs here (build/layout/paint)
/// - **PostFrameCallbacks**: Cleanup and post-frame work
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[repr(u8)]
pub enum SchedulerPhase {
    /// No frame is being processed. Between frames.
    #[default]
    Idle = 0,

    /// Transient callbacks are being executed.
    /// Animation tickers fire during this phase.
    /// Corresponds to Flutter's `handleBeginFrame`.
    TransientCallbacks = 1,

    /// Microtasks scheduled during TransientCallbacks are being executed.
    /// Allows async work triggered by animations to complete.
    MidFrameMicrotasks = 2,

    /// Persistent callbacks are being executed.
    /// The rendering pipeline (build/layout/paint) runs during this phase.
    /// Corresponds to Flutter's `handleDrawFrame`.
    PersistentCallbacks = 3,

    /// Post-frame callbacks are being executed.
    /// Cleanup and one-time post-frame work happens here.
    PostFrameCallbacks = 4,
}

impl SchedulerPhase {
    /// Try to convert from u8 representation
    ///
    /// Returns `None` if the value is not a valid discriminant.
    #[inline]
    pub const fn try_from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Idle),
            1 => Some(Self::TransientCallbacks),
            2 => Some(Self::MidFrameMicrotasks),
            3 => Some(Self::PersistentCallbacks),
            4 => Some(Self::PostFrameCallbacks),
            _ => None,
        }
    }

    /// Convert from u8 representation (for atomic storage)
    ///
    /// # Panics
    /// Panics if the value is not a valid SchedulerPhase discriminant.
    /// For fallible conversion, use [`try_from_u8`](Self::try_from_u8).
    #[inline]
    pub const fn from_u8(value: u8) -> Self {
        match Self::try_from_u8(value) {
            Some(v) => v,
            None => panic!("Invalid SchedulerPhase value"),
        }
    }

    /// All phases in execution order
    pub const ALL: [SchedulerPhase; 5] = [
        SchedulerPhase::Idle,
        SchedulerPhase::TransientCallbacks,
        SchedulerPhase::MidFrameMicrotasks,
        SchedulerPhase::PersistentCallbacks,
        SchedulerPhase::PostFrameCallbacks,
    ];

    /// Check if valid transition from current phase to next phase
    #[inline]
    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Idle, Self::TransientCallbacks)
                | (Self::TransientCallbacks, Self::MidFrameMicrotasks)
                | (Self::MidFrameMicrotasks, Self::PersistentCallbacks)
                | (Self::PersistentCallbacks, Self::PostFrameCallbacks)
                | (Self::PostFrameCallbacks, Self::Idle)
                // Allow skipping MidFrameMicrotasks if no microtasks pending
                | (Self::TransientCallbacks, Self::PersistentCallbacks)
        )
    }

    /// Get the next phase in normal execution order
    #[inline]
    pub const fn next(self) -> Option<Self> {
        match self {
            Self::Idle => Some(Self::TransientCallbacks),
            Self::TransientCallbacks => Some(Self::MidFrameMicrotasks),
            Self::MidFrameMicrotasks => Some(Self::PersistentCallbacks),
            Self::PersistentCallbacks => Some(Self::PostFrameCallbacks),
            Self::PostFrameCallbacks => None,
        }
    }

    /// Check if currently in a frame (not idle)
    #[inline]
    pub const fn is_in_frame(self) -> bool {
        !matches!(self, Self::Idle)
    }

    /// Check if currently in animation phase
    #[inline]
    pub const fn is_animating(self) -> bool {
        matches!(self, Self::TransientCallbacks)
    }

    /// Check if currently in rendering phase
    #[inline]
    pub const fn is_rendering(self) -> bool {
        matches!(self, Self::PersistentCallbacks)
    }
}

impl fmt::Display for SchedulerPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Idle => write!(f, "Idle"),
            Self::TransientCallbacks => write!(f, "TransientCallbacks"),
            Self::MidFrameMicrotasks => write!(f, "MidFrameMicrotasks"),
            Self::PersistentCallbacks => write!(f, "PersistentCallbacks"),
            Self::PostFrameCallbacks => write!(f, "PostFrameCallbacks"),
        }
    }
}

/// Application lifecycle state (follows Flutter's AppLifecycleState)
///
/// This tracks the overall state of the application as seen by the platform.
/// Different platforms may not support all states - the scheduler normalizes
/// to the closest supported state.
///
/// ## State Transitions
///
/// ```text
///                  ┌──────────┐
///          ┌──────►│ inactive │◄─────┐
///          │       └────┬─────┘      │
///          │            │            │
///     ┌────┴───┐        ▼        ┌───┴────┐
///     │ resumed│◄───────────────►│ hidden │
///     └────────┘                 └───┬────┘
///                                    │
///                               ┌────▼────┐
///                               │ paused  │
///                               └────┬────┘
///                                    │
///                               ┌────▼────┐
///                               │detached │
///                               └─────────┘
/// ```
///
/// - **Resumed**: App is visible, focused, and receiving events
/// - **Inactive**: App visible but not focused (modal dialog, split screen)
/// - **Hidden**: App hidden but still running (another app in front)
/// - **Paused**: App not visible, may be suspended soon
/// - **Detached**: App still hosted but detached from views (before exit)
///
/// ## Example
///
/// ```rust
/// use flui_scheduler::frame::AppLifecycleState;
///
/// let state = AppLifecycleState::Resumed;
///
/// if state.should_animate() {
///     // Run animations
/// }
///
/// if state.should_render() {
///     // Render frames
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[repr(u8)]
pub enum AppLifecycleState {
    /// The application is visible and responding to user input.
    ///
    /// This is the default running state. Animations should run,
    /// frames should be scheduled normally.
    #[default]
    Resumed = 0,

    /// The application is visible but not focused.
    ///
    /// Occurs when:
    /// - A modal dialog is shown
    /// - Split screen / multitasking
    /// - Phone call overlay
    ///
    /// Animations may continue but at reduced priority.
    /// User input may not be received.
    Inactive = 1,

    /// The application is not visible but still running.
    ///
    /// On desktop: Window minimized or completely covered.
    /// On mobile: Another app is in foreground.
    ///
    /// Frame scheduling should be paused to save resources.
    /// Background work may continue.
    Hidden = 2,

    /// The application is not visible and may be suspended.
    ///
    /// This is the last state before the OS may kill the app.
    /// Save state and release resources.
    Paused = 3,

    /// The application is still hosted but detached from views.
    ///
    /// This is the state before app termination or during
    /// engine warm-up before the first view is attached.
    Detached = 4,
}

impl AppLifecycleState {
    /// Try to convert from u8 representation
    ///
    /// Returns `None` if the value is not a valid discriminant.
    #[inline]
    pub const fn try_from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Resumed),
            1 => Some(Self::Inactive),
            2 => Some(Self::Hidden),
            3 => Some(Self::Paused),
            4 => Some(Self::Detached),
            _ => None,
        }
    }

    /// Convert from u8 representation (for atomic storage)
    ///
    /// # Panics
    /// Panics if the value is not a valid AppLifecycleState discriminant.
    /// For fallible conversion, use [`try_from_u8`](Self::try_from_u8).
    #[inline]
    pub const fn from_u8(value: u8) -> Self {
        match Self::try_from_u8(value) {
            Some(v) => v,
            None => panic!("Invalid AppLifecycleState value"),
        }
    }

    /// All states in typical lifecycle order
    pub const ALL: [AppLifecycleState; 5] = [
        AppLifecycleState::Detached,
        AppLifecycleState::Resumed,
        AppLifecycleState::Inactive,
        AppLifecycleState::Hidden,
        AppLifecycleState::Paused,
    ];

    /// Check if the app is currently visible to the user
    #[inline]
    pub const fn is_visible(self) -> bool {
        matches!(self, Self::Resumed | Self::Inactive)
    }

    /// Check if the app is currently focused and receiving input
    #[inline]
    pub const fn is_focused(self) -> bool {
        matches!(self, Self::Resumed)
    }

    /// Check if animations should run at full speed
    ///
    /// Returns true only when resumed (visible and focused).
    #[inline]
    pub const fn should_animate(self) -> bool {
        matches!(self, Self::Resumed)
    }

    /// Check if animations can run (possibly at reduced rate)
    ///
    /// Returns true when resumed or inactive (visible).
    #[inline]
    pub const fn can_animate(self) -> bool {
        matches!(self, Self::Resumed | Self::Inactive)
    }

    /// Check if frames should be rendered
    ///
    /// Returns true only when the app is visible.
    #[inline]
    pub const fn should_render(self) -> bool {
        matches!(self, Self::Resumed | Self::Inactive)
    }

    /// Check if the app should save state
    ///
    /// Returns true when transitioning away from resumed/inactive.
    #[inline]
    pub const fn should_save_state(self) -> bool {
        matches!(self, Self::Paused | Self::Detached)
    }

    /// Check if the app should release heavy resources
    ///
    /// Returns true when hidden or paused.
    #[inline]
    pub const fn should_release_resources(self) -> bool {
        matches!(self, Self::Hidden | Self::Paused | Self::Detached)
    }

    /// Check if the state transition is valid
    ///
    /// Most transitions are valid, but some are logically unusual.
    pub const fn can_transition_to(self, next: Self) -> bool {
        match (self, next) {
            // Same state is always valid (no-op)
            (a, b) if a as u8 == b as u8 => true,

            // Resumed can go to any state
            (Self::Resumed, _) => true,

            // Inactive can go to resumed, hidden, or paused
            (Self::Inactive, Self::Resumed | Self::Hidden | Self::Paused) => true,

            // Hidden can go to inactive, paused, or resumed (when coming back)
            (Self::Hidden, Self::Inactive | Self::Paused | Self::Resumed) => true,

            // Paused can resume through hidden/inactive or go to detached
            (Self::Paused, Self::Hidden | Self::Inactive | Self::Resumed | Self::Detached) => true,

            // Detached can transition to any state (app starting up)
            (Self::Detached, _) => true,

            // Other transitions are unusual but not forbidden
            _ => true,
        }
    }

    /// Get human-readable description
    pub const fn description(self) -> &'static str {
        match self {
            Self::Resumed => "Application is visible and focused",
            Self::Inactive => "Application is visible but not focused",
            Self::Hidden => "Application is not visible",
            Self::Paused => "Application is paused and may be suspended",
            Self::Detached => "Application is detached from views",
        }
    }
}

impl fmt::Display for AppLifecycleState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resumed => write!(f, "Resumed"),
            Self::Inactive => write!(f, "Inactive"),
            Self::Hidden => write!(f, "Hidden"),
            Self::Paused => write!(f, "Paused"),
            Self::Detached => write!(f, "Detached"),
        }
    }
}

/// Listener callback for lifecycle state changes
pub type LifecycleStateCallback = Box<dyn Fn(AppLifecycleState) + Send + Sync>;

/// Frame phase - which part of the render pipeline is executing
///
/// This is used within the PersistentCallbacks scheduler phase to track
/// rendering pipeline progress.
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

/// Transient frame callback - executed during TransientCallbacks phase.
///
/// These are one-time callbacks that fire during the animation phase.
/// Animation tickers use this to receive the vsync timestamp.
/// Receives the vsync timestamp for synchronized timing.
pub type OneShotFrameCallback = Box<dyn FnOnce(Instant) + Send>;

/// Frame callback - executed at frame boundaries (legacy, prefer OneShotFrameCallback)
pub type FrameCallback = Box<dyn FnOnce(&FrameTiming) + Send>;

/// Recurring frame callback (runs every frame)
///
/// These run during the PersistentCallbacks phase every frame.
/// The rendering pipeline (build/layout/paint) registers here.
/// Uses Arc for cheap cloning - recurring callbacks are cloned before execution
/// to avoid holding locks during callback invocation.
pub type RecurringFrameCallback = Arc<dyn Fn(&FrameTiming) + Send + Sync>;

/// Post-frame callback - executed after frame completes
///
/// These run during the PostFrameCallbacks phase, after rendering is complete.
/// Use for cleanup, analytics, or scheduling the next frame.
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

    // AppLifecycleState tests

    #[test]
    fn test_app_lifecycle_state_default() {
        let state = AppLifecycleState::default();
        assert_eq!(state, AppLifecycleState::Resumed);
    }

    #[test]
    fn test_app_lifecycle_state_display() {
        assert_eq!(format!("{}", AppLifecycleState::Resumed), "Resumed");
        assert_eq!(format!("{}", AppLifecycleState::Inactive), "Inactive");
        assert_eq!(format!("{}", AppLifecycleState::Hidden), "Hidden");
        assert_eq!(format!("{}", AppLifecycleState::Paused), "Paused");
        assert_eq!(format!("{}", AppLifecycleState::Detached), "Detached");
    }

    #[test]
    fn test_app_lifecycle_state_transitions() {
        // All transitions from Resumed should be valid
        assert!(AppLifecycleState::Resumed.can_transition_to(AppLifecycleState::Inactive));
        assert!(AppLifecycleState::Resumed.can_transition_to(AppLifecycleState::Hidden));
        assert!(AppLifecycleState::Resumed.can_transition_to(AppLifecycleState::Paused));
        assert!(AppLifecycleState::Resumed.can_transition_to(AppLifecycleState::Detached));

        // All transitions from Detached should be valid (app starting up)
        assert!(AppLifecycleState::Detached.can_transition_to(AppLifecycleState::Resumed));
        assert!(AppLifecycleState::Detached.can_transition_to(AppLifecycleState::Inactive));

        // Same state transition is valid (no-op)
        assert!(AppLifecycleState::Resumed.can_transition_to(AppLifecycleState::Resumed));
        assert!(AppLifecycleState::Hidden.can_transition_to(AppLifecycleState::Hidden));
    }

    #[test]
    fn test_app_lifecycle_state_all_array() {
        assert_eq!(AppLifecycleState::ALL.len(), 5);
        assert!(AppLifecycleState::ALL.contains(&AppLifecycleState::Resumed));
        assert!(AppLifecycleState::ALL.contains(&AppLifecycleState::Detached));
    }
}
