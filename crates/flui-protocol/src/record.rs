//! `record.start` and `record.stop` (ADR-0288 §11): a driver records what a window's UI runtime
//! applies, and is sent each line as an event on the connection that started it.
//!
//! A recording covers the UI runtime that hosts the window it was started on, every window that
//! runtime hosts: its gesture decisions and preference revisions are the runtime's, and a replay
//! needs them in order with the input. Its layers are the input recording (ADR-0215 §1) and,
//! where the backend journals it, the native one (§4). [`Recording::header`] is the input
//! recording's first line and each `record` event's row one line after it, so a driver writes a
//! file a replay reads with no other call. Each native stream is a native recording of its own,
//! sent as `record_native` events: its header first, then its rows. A recording ends with its
//! input `end` row: at `record.stop`, when the window it was started on closes, or when its
//! runtime goes. The connection that started it going ends it too, with no row sent.

use crate::input_recording::{Header, Row};
use crate::native_recording;
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
        /// What the platform backend was handed before the runtime heard it, one native
        /// recording per stream (ADR-0215 §4). A recording of it records `input` too.
        Native => "native",
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
    pub fn keep_text(mut self, keep_text: bool) -> Self {
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
    /// The input rows it sent, its `gap` and `end` rows aside.
    pub rows: u64,
    /// The input rows it lost: ones its `gap` rows stand for.
    pub lost_rows: u64,
    /// The native layer's rows it lost, over all its streams: ones their `gap` lines stand for.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "crate::skip::is_default")
    )]
    pub native_lost_rows: u64,
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
            native_lost_rows: 0,
            complete,
        }
    }

    /// This summary counting `native_lost_rows` of the native layer's rows lost.
    #[must_use]
    pub const fn with_native_lost_rows(mut self, native_lost_rows: u64) -> Self {
        self.native_lost_rows = native_lost_rows;
        self
    }
}

/// A line of one of a recording's layers, as its UI runtime hands it to the connection that
/// started it, which sends an input row as a [`RecordEvent`] and a native line as a
/// [`RecordNativeEvent`]. Exhaustive: a new layer is a new event, which a server sends only
/// once it handles it.
#[derive(Debug, Clone, PartialEq)]
#[expect(
    clippy::large_enum_variant,
    reason = "input rows are most of a recording and are queued by value, as before the native layer: boxing them would allocate per row"
)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a new layer is a new event, which a server sends only once it handles it"
)]
pub enum RecordRow {
    /// A row of the input layer, as the input recording holds it after its header: its `gap`
    /// rows count the input rows lost, and its `end` row ends the whole recording.
    Input(Row),
    /// A line of one of the native layer's streams, each a native recording of its own: its
    /// header first, then its rows, with a `gap` where that stream lost rows.
    Native {
        /// The stream, numbered in the order the backend opened them.
        stream: u32,
        /// The line.
        line: native_recording::Line,
    },
}

/// `record`: one input row of a recording, sent unasked to the connection that started it.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct RecordEvent {
    /// The recording's number on this connection.
    pub recording: u64,
    /// The window it was started on.
    pub window: WindowId,
    /// The row, as the input recording holds it after its header.
    pub row: Row,
}

impl RecordEvent {
    /// Input `row` of recording number `recording`, started on `window`.
    #[must_use]
    pub fn new(recording: u64, window: WindowId, row: Row) -> Self {
        Self {
            recording,
            window,
            row,
        }
    }
}

/// `record_native`: one line of a native stream of a recording that holds the native layer,
/// sent unasked to the connection that started it.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub struct RecordNativeEvent {
    /// The recording's number on this connection.
    pub recording: u64,
    /// The window it was started on.
    pub window: WindowId,
    /// The stream, numbered in the order the backend opened them: each is a native recording
    /// of its own.
    pub stream: u32,
    /// The line: the stream's header first, then its rows, with a `gap` where it lost rows.
    pub line: native_recording::Line,
}

impl RecordNativeEvent {
    /// `line` of native `stream` of recording number `recording`, started on `window`.
    #[must_use]
    pub fn new(
        recording: u64,
        window: WindowId,
        stream: u32,
        line: native_recording::Line,
    ) -> Self {
        Self {
            recording,
            window,
            stream,
            line,
        }
    }
}
