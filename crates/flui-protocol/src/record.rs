//! `record.start` and `record.stop` (ADR-0288 §11): a driver records what a window's UI runtime
//! applies, in the input recording's format (ADR-0215), and is sent each row as a `record`
//! event on the connection that started it.
//!
//! A recording covers the UI runtime that hosts the window it was started on, every window that
//! runtime hosts: its gesture decisions and preference revisions are the runtime's, and a replay
//! needs them in order with the input. [`Recording::header`] is the recording's first line and
//! each `record` event's row one line after it, so a driver writes a file a replay reads with no
//! other call. A recording ends with its `end` row: at `record.stop`, when the window it was
//! started on closes, or when its runtime goes. The connection that started it going ends it
//! too, with no row sent.

use crate::input_recording::{Header, Row};
use crate::tree::WindowId;

vocabulary! {
    /// What a recording holds. A server lists the layers it records in its capabilities
    /// ([`Capabilities::record`](crate::session::Capabilities::record)), and refuses any other
    /// with `not_supported`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(
        feature = "serde",
        derive(serde::Serialize, serde::Deserialize),
        serde(rename_all = "snake_case")
    )]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum RecordLayer {
        /// The input a UI runtime applied, with the gesture decisions it reached and the
        /// preference revisions it took (ADR-0215 §1).
        Input => "input",
    }
}

/// `record.start`: record the UI runtime of a window.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct RecordParams {
    /// The window whose runtime is recorded; the app's only window when absent.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub window: Option<WindowId>,
    /// What the recording holds.
    pub layers: Vec<RecordLayer>,
    /// Keep the text a row carries: key text, input-method strings, assistive action values,
    /// device names. Replaced and marked redacted when false (ADR-0215 §5).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "crate::skip::is_default")
    )]
    pub keep_text: bool,
}

impl RecordParams {
    /// The input of `window`'s runtime (the only window's when `None`), its text redacted.
    #[must_use]
    pub fn input(window: Option<WindowId>) -> Self {
        Self {
            window,
            layers: vec![RecordLayer::Input],
            keep_text: false,
        }
    }

    /// These parameters keeping the rows' text when `keep_text`.
    #[must_use]
    pub fn with_keep_text(mut self, keep_text: bool) -> Self {
        self.keep_text = keep_text;
        self
    }
}

/// A recording a connection started: its number, which `record.stop` and its `record` events
/// name, and its first line.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct Recording {
    /// The recording's number on this connection.
    pub recording: u64,
    /// What the recording starts from: its windows, their scales and gesture values, and the
    /// preferences in effect.
    pub header: Header,
}

impl Recording {
    /// Recording number `recording`, starting from `header`.
    #[must_use]
    pub fn new(recording: u64, header: Header) -> Self {
        Self { recording, header }
    }
}

/// `record.stop`: end a recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct RecordStopParams {
    /// The recording's number on this connection.
    pub recording: u64,
}

impl RecordStopParams {
    /// Stop recording number `recording`.
    #[must_use]
    pub const fn new(recording: u64) -> Self {
        Self { recording }
    }
}

/// What a stopped recording came to. Its `end` row was sent before this answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct RecordSummary {
    /// The rows it sent, its `gap` and `end` rows aside.
    pub rows: u64,
    /// The rows it lost: ones a `gap` row stands for.
    pub lost_rows: u64,
    /// Whether it ends with its `end` row, so a replay reads it.
    pub complete: bool,
}

impl RecordSummary {
    /// A recording of `rows` rows that lost `lost_rows`, `complete` when its `end` row went out.
    #[must_use]
    pub const fn new(rows: u64, lost_rows: u64, complete: bool) -> Self {
        Self {
            rows,
            lost_rows,
            complete,
        }
    }
}

/// `record`: one row of a recording, sent unasked to the connection that started it.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct RecordEvent {
    /// The recording's number on this connection.
    pub recording: u64,
    /// The window it was started on.
    pub window: WindowId,
    /// The row, as a recording's line holds it.
    pub row: Row,
}

impl RecordEvent {
    /// `row` of recording number `recording`, started on `window`.
    #[must_use]
    pub fn new(recording: u64, window: WindowId, row: Row) -> Self {
        Self {
            recording,
            window,
            row,
        }
    }
}
