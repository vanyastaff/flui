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
#[non_exhaustive]
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
    /// The state behind a discriminant the scheduler stored in an atomic;
    /// `None` for a byte no variant has.
    #[inline]
    pub(crate) const fn try_from_u8(value: u8) -> Option<Self> {
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

    /// Whether the phase machine may step from `self` to `next`: the order a
    /// frame runs its phases in, the microtask flush being optional.
    pub(crate) const fn can_transition_to(self, next: Self) -> bool {
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

    /// Check if currently in a frame (not idle)
    #[inline]
    pub const fn is_in_frame(self) -> bool {
        !matches!(self, Self::Idle)
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
/// if state.is_visible() {
///     // Keep the window's content current
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[repr(u8)]
#[expect(
    clippy::exhaustive_enums,
    reason = "each state decides what may run in it, and a `_` arm would run a new state as some other"
)]
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
    /// The most active of `states`, as the state of something they make up
    /// together (an application of several windows, a runtime of several
    /// presentations): `Resumed` over `Inactive` over `Hidden` over `Paused`,
    /// and `Detached` when `states` is empty or holds nothing else.
    #[must_use]
    pub fn most_active(states: impl IntoIterator<Item = Self>) -> Self {
        states
            .into_iter()
            .max_by_key(|state| state.activity())
            .unwrap_or(Self::Detached)
    }

    /// How active the state is: the higher, the more of the application the
    /// user sees and reaches.
    const fn activity(self) -> u8 {
        match self {
            Self::Resumed => 4,
            Self::Inactive => 3,
            Self::Hidden => 2,
            Self::Paused => 1,
            Self::Detached => 0,
        }
    }

    /// The state behind a discriminant the scheduler stored in an atomic;
    /// `None` for a byte no variant has.
    #[inline]
    pub(crate) const fn try_from_u8(value: u8) -> Option<Self> {
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

/// Listener for application lifecycle state changes, invoked on the UI owner.
pub type LifecycleStateCallback = Rc<dyn Fn(AppLifecycleState)>;

/// Frame phase - which part of the render pipeline is executing
///
/// This is used within the PersistentCallbacks scheduler phase to track
/// rendering pipeline progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[repr(u8)]
#[non_exhaustive]
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

/// One frame's timing as its scheduler sees it: which frame and when it began.
///
/// Issued by the scheduler and handed to persistent and post-frame callbacks.
/// [`Self::start_time`] is read from the scheduler's own clock source, which a
/// headless driver replaces with a virtual one; a duration measured against it
/// is measured on that clock, not on the wall clock. What a frame cost is
/// recorded in its [`FrameSnapshot`](crate::frame_telemetry::FrameSnapshot).
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
}

impl FrameTiming {
    /// The timing of frame `id`, begun at `start_time`.
    pub(crate) fn begun(id: FrameId, start_time: Instant) -> Self {
        Self { id, start_time }
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
