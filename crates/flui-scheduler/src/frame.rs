//! Frame management - lifecycle, timing, and callbacks
//!
//! A frame represents one render cycle from VSync to present:
//! ```text
//! VSync → BeginFrame → Build → Layout → Paint → EndFrame → Present
//! ```

use std::{fmt, num::NonZeroU64, rc::Rc};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use web_time::Instant;

use crate::duration::{BudgetPercentage, FrameDuration, Milliseconds};

/// A frame's number within the sequence that minted it: one scheduler's
/// frames, or one presentation's produced frames.
///
/// 64 bits on every target, so a 32-bit host (wasm32) does not run out
/// within a session; `Option<FrameId>` is the size of a `u64`. Each sequence
/// starts at 1 and refuses to continue past `u64::MAX`, so a number is never
/// reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FrameId(NonZeroU64);

impl FrameId {
    /// The first frame of a sequence.
    pub const FIRST: Self = Self(NonZeroU64::MIN);

    /// The frame numbered `count` in its sequence, or `None` for zero.
    #[must_use]
    pub const fn from_count(count: u64) -> Option<Self> {
        match NonZeroU64::new(count) {
            Some(count) => Some(Self(count)),
            None => None,
        }
    }

    /// This frame's number in its sequence, counting from 1.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }

    /// The frame after this one, or `None` once the sequence is exhausted.
    #[must_use]
    pub const fn next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(next) => Some(Self(next)),
            None => None,
        }
    }
}

impl fmt::Display for FrameId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

/// UpdateScheduler phase - which part of the frame lifecycle is executing
///
/// The phases execute in order:
///
/// ```text
/// Idle → TransientCallbacks → MidFrameMicrotasks → PersistentCallbacks → PostFrameCallbacks → Idle
/// ```
///
/// - **TransientCallbacks**: Animation tickers fire here (one-time frame
///   callbacks)
/// - **MidFrameMicrotasks**: Microtask queue flushes between animations and
///   rendering
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
    TransientCallbacks = 1,

    /// Microtasks scheduled during TransientCallbacks are being executed.
    /// Allows async work triggered by animations to complete.
    MidFrameMicrotasks = 2,

    /// Persistent callbacks are being executed.
    /// The rendering pipeline (build/layout/paint) runs during this phase.
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
                | (
                    Self::TransientCallbacks,
                    Self::MidFrameMicrotasks | Self::PersistentCallbacks
                )
                | (Self::MidFrameMicrotasks, Self::PersistentCallbacks)
                | (Self::PersistentCallbacks, Self::PostFrameCallbacks)
                | (Self::PostFrameCallbacks, Self::Idle)
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

impl TryFrom<u8> for SchedulerPhase {
    type Error = u8;

    #[inline]
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::try_from_u8(value).ok_or(value)
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

/// Application lifecycle state
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
    #[expect(
        clippy::match_same_arms,
        reason = "deliberate transition table: each state pair is listed explicitly \
                  so a future tightening edits one arm instead of reconstructing \
                  the table"
    )]
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

impl TryFrom<u8> for AppLifecycleState {
    type Error = u8;

    #[inline]
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::try_from_u8(value).ok_or(value)
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

impl TryFrom<u8> for FramePhase {
    type Error = u8;

    #[inline]
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Idle),
            1 => Ok(Self::Build),
            2 => Ok(Self::Layout),
            3 => Ok(Self::Paint),
            4 => Ok(Self::Composite),
            v => Err(v),
        }
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

/// One frame's timing as its scheduler sees it: which frame, when it began,
/// and the target it is judged against.
///
/// Issued by the scheduler and handed to persistent and post-frame callbacks.
/// Elapsed time is measured against an instant the caller supplies, read from
/// the clock that stamped [`Self::start_time`] — the scheduler's own clock
/// source, which a headless driver replaces with a virtual one. Measuring
/// against the wall clock instead would mix two timelines.
///
/// # Examples
///
/// ```
/// use flui_scheduler::UpdateScheduler;
///
/// let scheduler = UpdateScheduler::new();
/// assert!(scheduler.current_frame().is_none(), "no frame is open");
/// ```
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct FrameTiming {
    /// Frame identifier, unique within its scheduler.
    pub id: FrameId,

    /// When the frame started (its vsync or frame time).
    pub start_time: Instant,

    /// The target the frame is judged against.
    pub frame_duration: FrameDuration,
}

impl FrameTiming {
    /// The timing of frame `id`, begun at `start_time`.
    pub(crate) fn begun(id: FrameId, start_time: Instant, frame_duration: FrameDuration) -> Self {
        Self {
            id,
            start_time,
            frame_duration,
        }
    }

    /// Get target frame duration in milliseconds
    #[inline]
    pub fn target_duration_ms(&self) -> f64 {
        self.frame_duration.as_ms().value()
    }

    /// Time from the frame's start to `now`, zero when `now` precedes it.
    #[inline]
    pub fn elapsed_at(&self, now: Instant) -> Milliseconds {
        Milliseconds::new(now.saturating_duration_since(self.start_time).as_secs_f64() * 1000.0)
    }

    /// Whether the frame has used its whole budget by `now`.
    #[inline]
    pub fn is_over_budget_at(&self, now: Instant) -> bool {
        self.frame_duration.is_over_budget(self.elapsed_at(now))
    }

    /// Budget left at `now`.
    #[inline]
    pub fn remaining_at(&self, now: Instant) -> Milliseconds {
        self.frame_duration.remaining(self.elapsed_at(now))
    }

    /// Budget used by `now`, as a percentage.
    #[inline]
    pub fn utilization_at(&self, now: Instant) -> BudgetPercentage {
        BudgetPercentage::from_ratio(self.elapsed_at(now).value() / self.target_duration_ms())
    }

    /// Whether more than 80% of the budget is used by `now`.
    #[inline]
    pub fn is_deadline_near_at(&self, now: Instant) -> bool {
        self.frame_duration.is_deadline_near(self.elapsed_at(now))
    }

    /// Whether more than 150% of the budget is used by `now`.
    #[inline]
    pub fn is_janky_at(&self, now: Instant) -> bool {
        self.frame_duration.is_janky(self.elapsed_at(now))
    }

    /// Get target FPS
    #[inline]
    pub fn target_fps(&self) -> u32 {
        self.frame_duration.fps() as u32
    }
}

/// Transient frame callback - executed during TransientCallbacks phase.
///
/// These are one-time callbacks that fire during the animation phase.
/// Animation tickers use this to receive the vsync timestamp.
/// Receives the vsync timestamp for synchronized timing.
pub type OneShotFrameCallback = Box<dyn FnOnce(Instant)>;

/// Recurring frame callback (runs every frame)
///
/// These run during the PersistentCallbacks phase every frame.
/// The rendering pipeline (build/layout/paint) registers here.
/// Uses Arc for cheap cloning - recurring callbacks are cloned before execution
/// to avoid holding locks during callback invocation.
pub type RecurringFrameCallback = Rc<dyn Fn(&FrameTiming)>;

/// Post-frame callback - executed after frame completes
///
/// These run during the PostFrameCallbacks phase, after rendering is complete.
/// Use for cleanup, analytics, or scheduling the next frame.
pub type PostFrameCallback = Box<dyn FnOnce(&FrameTiming)>;
