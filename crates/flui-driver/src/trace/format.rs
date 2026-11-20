//! The rows of `trace.jsonl` (ADR-0241 §2): a header, then tagged rows. Unknown fields are
//! ignored and an unknown row tag is an error, as in ADR-0215.

use flui_protocol::error::ErrorCode;
use flui_protocol::record::RecordLayer;
use flui_protocol::session::Isolation;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The format's major version, `flui_trace` in the header: a change that is not additive bumps
/// it and names its ADR, and a reader refuses a newer one.
pub const FORMAT_VERSION: u32 = 1;

/// The first line of a trace.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Header {
    /// The format's major version, [`FORMAT_VERSION`].
    pub flui_trace: u32,
    /// The protocol the app spoke, `major.minor`.
    pub protocol: String,
    /// What was driven.
    pub target: TraceTarget,
    /// The app's main window when the trace began; a replay refuses a window of another size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<TraceWindow>,
    /// Where OS input landed.
    pub isolation: Isolation,
    /// What a replay compares of the input's positions.
    pub fidelity: Fidelity,
    /// The seed of an exploration run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
    /// When the trace began, in milliseconds since the Unix epoch.
    pub started_unix_ms: u64,
    /// Whether text was kept.
    pub privacy: Privacy,
    /// The app's recording layers the trace holds beside its rows: `input.jsonl` for `input`,
    /// `native/` for `native`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layers: Vec<RecordLayer>,
}

/// What a trace drove.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TraceTarget {
    /// `headless` (in process), `launched` (a process the session started) or `attached`.
    pub kind: String,
    /// The OS the driver ran on, as Rust names it.
    pub os: String,
    /// The app, as it introduced itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    /// The OS lane its OS input went through.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane: Option<String>,
}

/// The main window's surface.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TraceWindow {
    /// Logical width.
    pub width: f64,
    /// Logical height.
    pub height: f64,
    /// Device pixels per logical pixel.
    pub scale: f64,
}

/// What a replay compares of a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Fidelity {
    /// Whether positions are compared.
    pub positions: Positions,
}

/// Whether a lane reproduces positions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Positions {
    /// Exactly: a replay compares them.
    Exact,
    /// Only the order and kinds of events: a replay leaves positions out.
    Order,
}

/// Whether a trace kept text (ADR-0215 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Privacy {
    /// Typed text and text-bearing tree fields are replaced (the default).
    Redact,
    /// They are kept, as the session asked.
    KeepText,
}

/// One row after the header.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Row {
    /// A driver command, as the caller wrote it.
    Command(CommandRow),
    /// What a command came to.
    Result(ResultRow),
    /// An event the app sent.
    Event(EventRow),
    /// Rows or evidence the writer could not keep.
    Gap(GapRow),
    /// The end: a trace is complete only with it.
    End(EndRow),
}

/// `command{seq, t_ms, method, params}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CommandRow {
    /// The command's number, from 1.
    pub seq: u64,
    /// When it was sent, in milliseconds from the trace's start on the session's clock.
    pub t_ms: u64,
    /// Its wire name.
    pub method: String,
    /// Its parameters, locators included as written.
    pub params: Value,
}

/// `result{seq, t_ms, ok | error, evidence[]}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ResultRow {
    /// The command's number.
    pub seq: u64,
    /// When it answered.
    pub t_ms: u64,
    /// Its output, when it succeeded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ok: Option<Value>,
    /// Its failure, when it failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<TraceError>,
    /// The evidence files it left, relative to the trace's directory.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
}

/// A command's failure as a trace keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TraceError {
    /// The wire code; none for the driver's own failures.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<ErrorCode>,
    /// For people; redacted unless the trace keeps text.
    pub message: String,
}

/// `event{t_ms, name, params}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct EventRow {
    /// When the driver took it.
    pub t_ms: u64,
    /// The event's name.
    pub name: String,
    /// Its fields.
    #[serde(default)]
    pub params: Value,
}

/// `gap{rows, reason}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct GapRow {
    /// How many rows or evidence files are missing.
    pub rows: u64,
    /// Why.
    pub reason: GapReason,
}

/// Why a trace has a gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum GapReason {
    /// Evidence past the trace's budget: the files a result would have named.
    EvidenceBudget,
    /// Events the connection dropped before the trace took them.
    EventsDropped,
}

/// `end{}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[non_exhaustive]
pub struct EndRow {}
