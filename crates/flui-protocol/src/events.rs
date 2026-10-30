//! Events an agent streams after `events.subscribe`, as `{"event": <name>, "params": {…}}`
//! (ADR-0238 §2).
//!
//! Each subscriber's queue is bounded: one that overflows gets a `gap` saying how many events
//! it missed, never a silently shortened stream.

use crate::input::JournalEntry;
use crate::tree::{TreeRevision, WindowId};

vocabulary! {
    /// The events a subscriber can ask for; the name is the event's `event` field.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum EventKind {
        /// A frame presented, or was skipped.
        Frame => "frame",
        /// Input was delivered: one journal entry.
        Input => "input",
        /// The semantics tree was published at a new revision.
        Semantics => "semantics",
        /// A window opened or closed.
        Window => "window",
        /// An announcement was spoken to assistive technology.
        Announcement => "announcement",
        /// The subscriber's queue overflowed. Sent unasked.
        Gap => "gap",
    }
}

/// Which events a wait or a subscription selects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct EventFilter {
    /// The kind of event.
    pub kind: EventKind,
    /// Only this window's; every window's when absent.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub window: Option<WindowId>,
}

impl EventFilter {
    /// Events of `kind` from every window.
    #[must_use]
    pub const fn new(kind: EventKind) -> Self {
        Self { kind, window: None }
    }
}

/// `events.subscribe`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct SubscribeParams {
    /// The events to stream.
    pub events: Vec<EventFilter>,
}

/// A live subscription, which `events.unsubscribe` ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct Subscription {
    /// The subscription's number on this connection.
    pub subscription: u64,
}

impl Subscription {
    /// Subscription number `subscription`.
    #[must_use]
    pub const fn new(subscription: u64) -> Self {
        Self { subscription }
    }
}

/// `frame`: a frame presented, or was skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct FrameEvent {
    /// The window.
    pub window: WindowId,
    /// The frame's number in the window.
    pub frame: u64,
    /// Whether it reached the screen.
    pub presented: bool,
    /// When it presented, in microseconds of the runtime's clock.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub time_us: Option<u64>,
}

/// `input`: one delivered input.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct InputEvent {
    /// The window.
    pub window: WindowId,
    /// What was delivered.
    pub entry: JournalEntry,
}

/// `semantics`: the tree was published at a new revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct SemanticsEvent {
    /// The window.
    pub window: WindowId,
    /// The new revision.
    pub revision: TreeRevision,
}

/// `window`: a window opened or closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct WindowEvent {
    /// The window.
    pub window: WindowId,
    /// Whether it opened; it closed otherwise.
    pub opened: bool,
}

/// `gap`: the subscriber's queue overflowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct GapEvent {
    /// Events dropped.
    pub missed: u64,
}

/// One event line: `{"event": "frame", "params": {…}}`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(tag = "event", content = "params", rename_all = "snake_case")
)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Event {
    /// A frame presented, or was skipped.
    Frame(FrameEvent),
    /// Input was delivered.
    Input(InputEvent),
    /// The tree was published.
    Semantics(SemanticsEvent),
    /// A window opened or closed.
    Window(WindowEvent),
    /// An announcement was spoken.
    Announcement(AnnouncementEvent),
    /// The queue overflowed.
    Gap(GapEvent),
}

impl Event {
    /// Which kind of event this is.
    #[must_use]
    pub const fn kind(&self) -> EventKind {
        match self {
            Self::Frame(_) => EventKind::Frame,
            Self::Input(_) => EventKind::Input,
            Self::Semantics(_) => EventKind::Semantics,
            Self::Window(_) => EventKind::Window,
            Self::Announcement(_) => EventKind::Announcement,
            Self::Gap(_) => EventKind::Gap,
        }
    }
}

vocabulary! {
    /// How an announcement interrupts what assistive technology is saying.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum AnnouncementUrgency {
        /// Spoken once the user is idle.
        Polite => "polite",
        /// Spoken at once, interrupting.
        Assertive => "assertive",
    }
}

/// One announcement the window delivered to assistive technology.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct Announcement {
    /// Its place in the window's announcement journal, from 1.
    pub seq: u64,
    /// How it interrupts.
    pub urgency: AnnouncementUrgency,
    /// What was said.
    pub message: String,
    /// The revision of the tree whose publication delivered it.
    pub revision: TreeRevision,
}

impl Announcement {
    /// Announcement `seq`: `message` at `urgency`, delivered with `revision`.
    #[must_use]
    pub fn new(
        seq: u64,
        urgency: AnnouncementUrgency,
        message: impl Into<String>,
        revision: TreeRevision,
    ) -> Self {
        Self {
            seq,
            urgency,
            message: message.into(),
            revision,
        }
    }
}

/// `semantics.announcements`: the announcements after `since`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct AnnouncementsParams {
    /// The window.
    pub window: WindowId,
    /// Announcements after this sequence number; every one still held when absent.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub since: Option<u64>,
    /// Announcements to answer at most.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub max_entries: Option<u32>,
}

impl AnnouncementsParams {
    /// Every announcement `window` still holds.
    #[must_use]
    pub const fn new(window: WindowId) -> Self {
        Self {
            window,
            since: None,
            max_entries: None,
        }
    }

    /// Only the announcements after `seq`.
    #[must_use]
    pub const fn since(mut self, seq: u64) -> Self {
        self.since = Some(seq);
        self
    }

    /// At most `max` announcements.
    #[must_use]
    pub const fn with_max_entries(mut self, max: u32) -> Self {
        self.max_entries = Some(max);
        self
    }
}

/// What `semantics.announcements` answers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct AnnouncementJournal {
    /// The announcements, oldest first.
    pub entries: Vec<Announcement>,
    /// Announcements after `since` the bounded journal no longer holds.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "crate::skip::is_default")
    )]
    pub dropped: u64,
}

impl AnnouncementJournal {
    /// `entries`, oldest first, after `dropped` the journal no longer holds.
    #[must_use]
    pub const fn new(entries: Vec<Announcement>, dropped: u64) -> Self {
        Self { entries, dropped }
    }
}

/// `announcement`: the window spoke an announcement.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct AnnouncementEvent {
    /// The window.
    pub window: WindowId,
    /// What it said.
    pub announcement: Announcement,
}
