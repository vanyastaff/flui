//! What a recording holds: the [`HostInput`] a runtime applied, in order, with
//! the rows replay needs beside it (ADR-0215 §1).
//!
//! The runtime emits one [`InputRecord`] per applied input, per arena verdict
//! and per system-preference revision, on its owner thread and in the order it
//! applied them. A [`InputTap`](crate::InputTap) observes the stream; the
//! serialiser that turns it into a file lives in test and tool builds only.
//!
//! Identities a recording names by value — the input sequence a row belongs
//! to, the surface it addressed, the arena member a verdict concerns — are
//! the recording's own types: a recording read back cannot mint an
//! [`InputSequenceId`] or an embedded surface, so it names them as the
//! runtime that recorded them did, and a replay compares by those values.

use std::borrow::Cow;
use std::num::NonZeroU64;
use std::sync::Arc;

use flui_foundation::InputSequenceId;
use flui_interaction::{GestureDisposition, InputSurface, PointerId};
use flui_platform_api::{EventTime, PlatformInput, SystemPreferences, WindowId};

use crate::HostInput;
use crate::owner::InputOutcome;

/// One row of the stream a runtime records.
///
/// A recording is complete only when it ends with [`End`](Self::End); a
/// [`Gap`](Self::Gap) marks rows a failing tap lost, so a reader never mistakes
/// a damaged stream for a whole one.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum InputRecord {
    /// One unit of host input the runtime applied.
    Input(InputRow),
    /// A gesture-arena verdict, including one a deadline made with no input
    /// of its own.
    Decision(DecisionRow),
    /// An arena member's deadline elapsed and the arena polled it; the
    /// verdicts it made follow as `Decision` rows.
    Deadline(DeadlineRow),
    /// A system-preference revision the runtime applied, at its position
    /// between inputs (ADR-0172).
    Preferences(PreferencesRow),
    /// `rows` rows were not delivered: the tap panicked on them.
    Gap {
        /// How many rows were lost since the previous delivered `Gap`.
        rows: u64,
    },
    /// The recording finished; `lost_rows` rows were lost in total.
    End {
        /// Every row a tap panicked on during this recording.
        lost_rows: u64,
    },
}

/// An input sequence as a recording names it: the value of the
/// [`InputSequenceId`] the recording runtime's binding minted. Two rows of one
/// recording with the same value belong to the same sequence; a pointer ID
/// reused by a later contact names a different one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordedSequence(NonZeroU64);

impl RecordedSequence {
    /// The sequence recorded as `value`.
    #[must_use]
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    /// The recorded value.
    #[must_use]
    pub const fn get(self) -> NonZeroU64 {
        self.0
    }
}

impl From<InputSequenceId> for RecordedSequence {
    fn from(id: InputSequenceId) -> Self {
        Self(id.into())
    }
}

/// The surface an input addressed, as a recording names it: the window
/// itself, or a surface embedded in it by the value its binding minted
/// (ADR-0207 §5). A replay resolves an embedded value against the surfaces
/// its own host embedded, in the same order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RecordedSurface {
    /// The window's own surface.
    #[default]
    Window,
    /// A surface embedded in the window.
    Embedded(NonZeroU64),
}

impl From<InputSurface> for RecordedSurface {
    fn from(surface: InputSurface) -> Self {
        match surface {
            InputSurface::Window => Self::Window,
            InputSurface::Embedded(id) => Self::Embedded(id.get()),
        }
    }
}

/// One unit of host input and what the runtime made of it.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct InputRow {
    /// The window the input addressed.
    pub window: WindowId,
    /// The surface of that window the input addressed.
    pub surface: RecordedSurface,
    /// The input's own time: the event's stamp, a frame's vsync, or the
    /// arrival time for input that carries none.
    pub t: EventTime,
    /// When the input reached the runtime, on the same event timeline. Rows
    /// follow the order the runtime applied their input, so the row of
    /// pointer input a presentation held for its committed tree, recorded
    /// where it was dispatched or discarded, arrived before the rows ahead
    /// of it.
    pub arrival: EventTime,
    /// The input: for held pointer input that was dispatched, the event as
    /// dispatch delivered it, with the held moves folded into it.
    pub input: HostInput,
    /// The dispatch result. [`InputOutcome::Queued`] is held pointer input
    /// the runtime accepted and never dispatched (refused, folded into a later
    /// move, superseded, evicted or dropped with its presentation's held
    /// input), which a replay does not apply.
    pub outcome: InputOutcome,
    /// The input sequence the dispatch placed it in: a contact's, a wheel or
    /// trackpad latch's, an assistive action's. `None` for input outside any
    /// sequence (hover, frames, window observations), never dispatched, or
    /// whose dispatch failed.
    pub sequence: Option<RecordedSequence>,
}

impl InputRow {
    /// `input` addressed to `window`'s own surface, stamped `t`, applied at
    /// `arrival` with `outcome`, in no sequence.
    #[must_use]
    pub fn new(
        window: WindowId,
        t: EventTime,
        arrival: EventTime,
        input: HostInput,
        outcome: InputOutcome,
    ) -> Self {
        Self {
            window,
            surface: RecordedSurface::Window,
            t,
            arrival,
            input,
            outcome,
            sequence: None,
        }
    }

    /// This row addressed to `surface` of its window.
    #[must_use]
    pub fn on_surface(mut self, surface: impl Into<RecordedSurface>) -> Self {
        self.surface = surface.into();
        self
    }

    /// This row in `sequence`.
    #[must_use]
    pub fn in_sequence(mut self, sequence: Option<RecordedSequence>) -> Self {
        self.sequence = sequence;
        self
    }
}

/// An arena member as a recording names it: numbered in the order the
/// recording first saw each member, normally its join. Two runs of one
/// application over one input stream number their members alike, so two
/// members of the same type stay apart and a replay compares verdicts by
/// this number, never by a type name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordedMember(NonZeroU64);

impl RecordedMember {
    /// The member recorded as `value`.
    #[must_use]
    pub const fn new(value: NonZeroU64) -> Self {
        Self(value)
    }

    /// The recorded value.
    #[must_use]
    pub const fn get(self) -> NonZeroU64 {
        self.0
    }
}

/// An arena verdict: which member won or lost a contest.
///
/// Two rows are the same decision when their window, pointer, sequence, time,
/// verdict and member agree; [`member_type`](Self::member_type) is an
/// annotation for people reading the recording and is not compared.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DecisionRow {
    /// The window whose arena decided.
    pub window: WindowId,
    /// The pointer whose contest was decided.
    pub pointer: PointerId,
    /// The input sequence the contest decided; `None` for an arena no
    /// binding drives.
    pub sequence: Option<RecordedSequence>,
    /// When, on the event timeline.
    pub t: EventTime,
    /// Whether the member won or lost.
    pub disposition: GestureDisposition,
    /// The member, by its number in the recording.
    pub member: RecordedMember,
    /// The member's concrete type name, read when the verdict was committed.
    pub member_type: Cow<'static, str>,
}

impl DecisionRow {
    /// `member` won or lost `pointer`'s contest in `window`'s arena at `t`,
    /// in no sequence and with no type annotation.
    #[must_use]
    pub fn new(
        window: WindowId,
        pointer: PointerId,
        t: EventTime,
        disposition: GestureDisposition,
        member: RecordedMember,
    ) -> Self {
        Self {
            window,
            pointer,
            sequence: None,
            t,
            disposition,
            member,
            member_type: Cow::Borrowed(""),
        }
    }

    /// This verdict in `sequence`.
    #[must_use]
    pub fn in_sequence(mut self, sequence: Option<RecordedSequence>) -> Self {
        self.sequence = sequence;
        self
    }

    /// This verdict annotated with its member's type name.
    #[must_use]
    pub fn with_member_type(mut self, member_type: impl Into<Cow<'static, str>>) -> Self {
        self.member_type = member_type.into();
        self
    }
}

impl PartialEq for DecisionRow {
    fn eq(&self, other: &Self) -> bool {
        self.window == other.window
            && self.pointer == other.pointer
            && self.sequence == other.sequence
            && self.t == other.t
            && self.disposition == other.disposition
            && self.member == other.member
    }
}

impl Eq for DecisionRow {}

/// A deadline the arena polled: which member, for which contest, when.
///
/// Compared as [`DecisionRow`] is: the member by its number in the recording,
/// never by [`member_type`](Self::member_type).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DeadlineRow {
    /// The window whose arena polled.
    pub window: WindowId,
    /// The contact of the contest the member was polled for; `None` for a
    /// member that registered its deadline outside any contest.
    pub pointer: Option<PointerId>,
    /// That contest's input sequence.
    pub sequence: Option<RecordedSequence>,
    /// When the arena polled, on the event timeline.
    pub t: EventTime,
    /// The member, by its number in the recording.
    pub member: RecordedMember,
    /// The member's concrete type name, an annotation for readers.
    pub member_type: Cow<'static, str>,
}

impl DeadlineRow {
    /// `member`'s deadline polled in `window`'s arena at `t`, for `pointer`'s
    /// contest, in no sequence and with no type annotation.
    #[must_use]
    pub fn new(
        window: WindowId,
        pointer: Option<PointerId>,
        t: EventTime,
        member: RecordedMember,
    ) -> Self {
        Self {
            window,
            pointer,
            sequence: None,
            t,
            member,
            member_type: Cow::Borrowed(""),
        }
    }

    /// This poll for a contest in `sequence`.
    #[must_use]
    pub fn in_sequence(mut self, sequence: Option<RecordedSequence>) -> Self {
        self.sequence = sequence;
        self
    }

    /// This poll annotated with its member's type name.
    #[must_use]
    pub fn with_member_type(mut self, member_type: impl Into<Cow<'static, str>>) -> Self {
        self.member_type = member_type.into();
        self
    }
}

impl PartialEq for DeadlineRow {
    fn eq(&self, other: &Self) -> bool {
        self.window == other.window
            && self.pointer == other.pointer
            && self.sequence == other.sequence
            && self.t == other.t
            && self.member == other.member
    }
}

impl Eq for DeadlineRow {}

/// A system-preference revision, with the values it carried.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PreferencesRow {
    /// The host's revision number.
    pub revision: u64,
    /// The observed values.
    pub values: Arc<SystemPreferences>,
}

impl PreferencesRow {
    /// Revision `revision` carrying `values`.
    #[must_use]
    pub fn new(revision: u64, values: Arc<SystemPreferences>) -> Self {
        Self { revision, values }
    }
}

/// What a recording of a runtime starts from: each open window with the
/// scale it is at and the per-kind gesture values its contacts are admitted
/// with, and the system-preference revision in effect (ADR-0215 §1). A
/// recording's header holds it, so a replay restores it before its first row;
/// the runtime produces it, so a recording made by a runner and one made by a
/// headless host start from the same facts.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RecordingStart {
    /// The open windows, the primary first.
    pub windows: Vec<RecordingWindow>,
    /// The system-preference revision in effect, with its values; `None`
    /// before the host reported one.
    pub preferences: Option<PreferencesRow>,
}

/// One window a recording starts with.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RecordingWindow {
    /// The window.
    pub window: WindowId,
    /// Device pixels per logical pixel, possibly fractional.
    pub scale_factor: f64,
    /// The per-kind gesture values the window's contacts are admitted with.
    pub gesture_profiles: flui_interaction::GestureProfiles,
}

/// The time `input` carries, if any: a pointer or key event's stamp or a
/// frame's vsync.
pub(crate) fn input_time(input: &HostInput) -> Option<EventTime> {
    match input {
        HostInput::Platform(PlatformInput::Pointer(event)) => {
            flui_interaction::PointerEventExt::time(event)
        }
        HostInput::Platform(PlatformInput::Keyboard(event)) => Some(event.time),
        HostInput::Frame { vsync } => Some(*vsync),
        HostInput::Platform(_) | HostInput::Window(_) | HostInput::Accessibility(_) => None,
    }
}
