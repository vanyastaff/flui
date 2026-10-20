//! Waits on the app's frame and event timeline (ADR-0238 §7, R18): the [`Condition`] a wait
//! holds out for, and what it observed when it answered.
//!
//! The app evaluates a condition after each frame's semantics publish and after each input
//! delivery, so a wait never polls. A timeout answers the last observation and why the app had
//! not settled.

use crate::events::EventFilter;
use crate::locator::{Locator, StateFilter, TextMatch};
use crate::tree::{Node, TreeRevision, WindowId};

/// What a matched element must show.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Expectation {
    /// Its name matches.
    Name(TextMatch),
    /// Its value matches.
    Value(TextMatch),
    /// Its states hold.
    State(StateFilter),
    /// The locator matches exactly this many elements.
    Count(u32),
    /// It holds keyboard focus.
    Focused,
}

/// What a wait holds out for.
///
/// `{"appears": {…}}`, `"settled"`, `{"frames": 3}`, `{"all": [ … ]}` and so on.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "snake_case")
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Condition {
    /// The locator matches at least one element.
    Appears(Locator),
    /// The locator matches nothing.
    Gone(Locator),
    /// The locator's element meets an expectation.
    Matches {
        /// The element.
        locator: Locator,
        /// What it must show.
        expect: Expectation,
    },
    /// The app has no frame pending, no transient callbacks, timers or image loads.
    Settled,
    /// This many frames have presented since the wait began.
    Frames(u32),
    /// An event the filter selects was emitted.
    Event(EventFilter),
    /// Every condition holds at once.
    All(Vec<Condition>),
    /// At least one condition holds.
    Any(Vec<Condition>),
}

/// `wait.until`: hold out for a condition in a window.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct WaitParams {
    /// The window whose frames the wait counts.
    pub window: WindowId,
    /// What to wait for.
    pub condition: Condition,
    /// How long, in milliseconds of the runtime's clock; 5000 when absent.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub timeout_ms: Option<u64>,
    /// On the headless target, the frames to advance at most before answering.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub max_frames: Option<u32>,
}

impl WaitParams {
    /// `condition` in `window`, with the default timeout.
    #[must_use]
    pub fn new(window: WindowId, condition: Condition) -> Self {
        Self {
            window,
            condition,
            timeout_ms: None,
            max_frames: None,
        }
    }

    /// Wait at most `timeout_ms`.
    #[must_use]
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }
}

/// `wait.settled`: hold out until a window settles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct SettledParams {
    /// The window.
    pub window: WindowId,
    /// How long, in milliseconds; 5000 when absent.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub timeout_ms: Option<u64>,
}

/// `wait.frames`: hold out until a window has presented `frames` more frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct FramesParams {
    /// The window.
    pub window: WindowId,
    /// Frames to present.
    pub frames: u32,
    /// How long, in milliseconds; 5000 when absent.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub timeout_ms: Option<u64>,
}

vocabulary! {
    /// Why a window had not settled.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum UnsettledReason {
        /// A redraw is requested.
        NeedsRedraw => "needs_redraw",
        /// A frame is scheduled.
        FrameScheduled => "frame_scheduled",
        /// The owner has queued work.
        PendingWork => "pending_work",
        /// Animations and other per-frame callbacks are registered.
        TransientCallbacks => "transient_callbacks",
        /// A timer is due.
        Timer => "timer",
        /// Images are still loading.
        ImageLoads => "image_loads",
    }
}

/// One reason a window had not settled, with how much of it there was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct UnsettledCause {
    /// The reason.
    pub reason: UnsettledReason,
    /// How many: callbacks registered, loads pending.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub count: Option<u32>,
    /// When the next timer is due, in milliseconds from the answer.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub due_in_ms: Option<u64>,
}

impl UnsettledCause {
    /// `reason`, with no count.
    #[must_use]
    pub const fn new(reason: UnsettledReason) -> Self {
        Self {
            reason,
            count: None,
            due_in_ms: None,
        }
    }
}

/// What a wait saw last: the tree revision and the elements its locators matched.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct Observation {
    /// The revision evaluated.
    pub revision: TreeRevision,
    /// Elements the condition's locators matched, without children; capped.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub matched: Vec<Node>,
    /// How many elements matched, including those past the cap.
    pub count: u32,
}

/// What a wait answers. A wait that ran out is still an answer, `satisfied: false`; the
/// `timeout` error is for a window that answered nothing at all.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct WaitOutcome {
    /// Whether the condition held.
    pub satisfied: bool,
    /// Frames presented while waiting.
    pub frames: u64,
    /// The observation that satisfied it, or the last one.
    pub observation: Observation,
    /// Why the window had not settled, on a wait that ran out.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub unsettled: Vec<UnsettledCause>,
}
