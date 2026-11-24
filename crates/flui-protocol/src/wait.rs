//! Waits on the app's frame and event timeline (ADR-0238 §7, R18): the [`Condition`] a wait
//! holds out for, and what it observed when it answered.
//!
//! The app evaluates a condition after each frame's semantics publish and after each input
//! delivery, so a wait never polls. A timeout answers the last observation and why the app had
//! not settled.

use crate::build::SourceLocation;
use crate::events::EventFilter;
use crate::locator::{Locator, StateFilter, TextMatch};
use crate::tree::{ElementRef, Node, TreeRevision, WindowId};

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
    /// It is shown: included in the published tree, and its bounds keep a non-empty part inside
    /// the window and every clipping ancestor, the `visible` fact of actionability.
    Visible,
    /// Its description matches.
    Description(TextMatch),
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
    /// How long, in milliseconds of the runtime's clock; 5000 when absent and
    /// at most 24 hours ([`Self::effective_timeout_ms`]).
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
    /// How long a wait holds out when it names no timeout, in milliseconds.
    pub const DEFAULT_TIMEOUT_MS: u64 = 5_000;

    /// The longest a wait holds out, in milliseconds (24 hours); a longer
    /// timeout is this long.
    pub const MAX_TIMEOUT_MS: u64 = 24 * 60 * 60 * 1_000;

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
    pub fn timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }

    /// How long the app holds the wait out, in milliseconds: the timeout
    /// named, at most [`Self::MAX_TIMEOUT_MS`], or [`Self::DEFAULT_TIMEOUT_MS`].
    #[must_use]
    pub fn effective_timeout_ms(&self) -> u64 {
        self.timeout_ms
            .map_or(Self::DEFAULT_TIMEOUT_MS, |ms| ms.min(Self::MAX_TIMEOUT_MS))
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
    /// What keeps an application from being idle (ADR-0400 §3): part of it the runtime's, part
    /// its host's, which runs the runtime's turns.
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
        /// A timer is due.
        Timer => "timer",
        /// A ticker is running: an animation asks for every frame.
        Ticker => "ticker",
        /// Background work is held or running, which the host's executor owns.
        Background => "background",
        /// The host owes a turn for a close or a quit it was asked for.
        OwedTurn => "owed_turn",
        /// The host has an observation of a window (a resize, a focus change, its insets) not yet
        /// delivered to the runtime.
        WindowObservation => "window_observation",
        /// The host holds a frame request due at a time of its own.
        FrameRequest => "frame_request",
        /// A scenario the host plays has a step still to come.
        ScenarioStep => "scenario_step",
        /// A covered window owes its surface a frame.
        SurfaceFrame => "surface_frame",
        /// A reply the platform owes is in flight.
        PlatformReply => "platform_reply",
    }
}

vocabulary! {
    /// What discharges a cause, which decides when its host's loop wakes for it (ADR-0400 §3).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum ClearedBy {
        /// The next turn serves it.
        NextFrame => "next_frame",
        /// A turn at its deadline serves it: a timer, a frame's retry, a scenario step.
        Deadline => "deadline",
        /// Only the test discharges it: held background work.
        Test => "test",
        /// Something outside the application answers it: a platform reply in flight.
        Outside => "outside",
    }
}

vocabulary! {
    /// Which halves of what keeps an application from being idle an answer counts (ADR-0400 §3).
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum SettleScope {
        /// The runtime's half and its host's: the answer saw every cause.
        Complete => "complete",
        /// The runtime's half alone: the host composes none of its own, so a cause only the host
        /// sees (a held background job, an observation not yet delivered) is not in it. What an
        /// answer is until its host says otherwise.
        #[default]
        RuntimeOnly => "runtime_only",
    }
}

/// One cause that kept an application from being idle when the answer was made.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct UnsettledCause {
    /// The reason.
    pub reason: UnsettledReason,
    /// The window it belongs to; `None` for a cause of the whole application.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub window: Option<WindowId>,
    /// The element that owns it, where the runtime records one.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub owner: Option<ElementRef>,
    /// Where in the source it was created, where the runtime records it.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub site: Option<SourceLocation>,
    /// What discharges it.
    pub cleared_by: ClearedBy,
    /// How long until it is due, in whole milliseconds from the answer, rounded down: for the
    /// report. The wake its host sleeps until is the exact instant whoever armed it holds.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub due_in_ms: Option<u64>,
}

impl UnsettledCause {
    /// `reason`, which `cleared_by` discharges, of the whole application, with no owner, site or
    /// due time.
    #[must_use]
    pub const fn new(reason: UnsettledReason, cleared_by: ClearedBy) -> Self {
        Self {
            reason,
            window: None,
            owner: None,
            site: None,
            cleared_by,
            due_in_ms: None,
        }
    }

    /// The cause, of `window`.
    #[must_use]
    pub const fn window(mut self, window: WindowId) -> Self {
        self.window = Some(window);
        self
    }

    /// The cause, owned by `owner`.
    #[must_use]
    pub const fn owner(mut self, owner: ElementRef) -> Self {
        self.owner = Some(owner);
        self
    }

    /// The cause, created at `site`.
    #[must_use]
    pub fn site(mut self, site: SourceLocation) -> Self {
        self.site = Some(site);
        self
    }

    /// The cause, due in `due_in_ms` whole milliseconds.
    #[must_use]
    pub const fn due_in_ms(mut self, due_in_ms: u64) -> Self {
        self.due_in_ms = Some(due_in_ms);
        self
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
    /// Which halves of the causes the answer counted; an app that names none counted its
    /// runtime's alone.
    #[cfg_attr(feature = "serde", serde(default))]
    pub scope: SettleScope,
}
