//! What a recording holds: the [`HostInput`] a runtime applied, in order, with
//! the rows replay needs beside it (ADR-0215 §1).
//!
//! The runtime emits one [`InputRecord`] per applied input, per arena verdict
//! and per system-preference revision, on its owner thread and in the order it
//! applied them. A [`InputTap`](crate::InputTap) observes the stream; the
//! serialiser that turns it into a file lives in test and tool builds only.

use std::borrow::Cow;
use std::sync::Arc;

use flui_interaction::{GestureDisposition, PointerId};
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

/// One unit of host input and what the runtime made of it.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct InputRow {
    /// The window the input addressed.
    pub window: WindowId,
    /// The input's own time: the event's stamp, a frame's vsync, or the
    /// arrival time for input that carries none.
    pub t: EventTime,
    /// When the runtime applied it, on the same event timeline.
    pub arrival: EventTime,
    /// The input.
    pub input: HostInput,
    /// The dispatch result.
    pub outcome: InputOutcome,
}

impl InputRow {
    /// `input` addressed to `window`, stamped `t`, applied at `arrival` with
    /// `outcome`.
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
            t,
            arrival,
            input,
            outcome,
        }
    }
}

/// An arena verdict: which member won or lost a pointer's competition.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DecisionRow {
    /// The window whose arena decided.
    pub window: WindowId,
    /// The pointer whose competition was decided.
    pub pointer: PointerId,
    /// When, on the event timeline.
    pub t: EventTime,
    /// Whether the member won or lost.
    pub disposition: GestureDisposition,
    /// The member's concrete type name: a description to compare two runs
    /// of one application by, not an identity.
    pub member: Cow<'static, str>,
}

impl DecisionRow {
    /// `member` won or lost `pointer` in `window`'s arena at `t`.
    #[must_use]
    pub fn new(
        window: WindowId,
        pointer: PointerId,
        t: EventTime,
        disposition: GestureDisposition,
        member: impl Into<Cow<'static, str>>,
    ) -> Self {
        Self {
            window,
            pointer,
            t,
            disposition,
            member: member.into(),
        }
    }
}

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
