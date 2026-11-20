//! Traces (ADR-0241): what a session did, kept so a person can replay it, an agent's run can be
//! reviewed and a failed scenario reproduced.
//!
//! A trace is a directory, by convention `<name>.flui-trace/`:
//!
//! - `trace.jsonl`: a [`Header`], then one [`Row`] per line: every driver command as the caller
//!   wrote it (locators included, never only the handle they resolved to), its result and the
//!   evidence it left, the app's events, `gap` rows where something could not be kept, and `end`;
//! - `input.jsonl`: the app's input recording (ADR-0215), verbatim, where the app serves one;
//! - `native/<n>.jsonl`: one native recording per stream the app's backend journals, in the
//!   protocol's native recording format ([`native`]), with `native/index.json` naming each;
//! - `evidence/<sha256>.png`: captures, content-addressed.
//!
//! [`Driver::start_trace`](crate::Driver::start_trace) begins one and
//! [`Driver::finish_trace`](crate::Driver::finish_trace) ends it; a session that ends without
//! finishing leaves it incomplete, as a crash does. [`Trace::open`] reads one back, [`replay`]
//! sends its commands again and reports where the results diverge, and [`codegen`] writes a Rust
//! test from it. Text is redacted unless the session keeps it ([`TEXT_FIELDS`]).

mod codegen;
mod format;
mod lines;
pub mod native;
mod reader;
mod redact;
mod replay;
mod writer;

pub use codegen::{CodegenOptions, codegen};
pub use format::{
    CommandRow, EndRow, EventRow, FORMAT_VERSION, Fidelity, GapReason, GapRow, Header, Positions,
    Privacy, ResultRow, Row, TraceError, TraceTarget, TraceWindow,
};
pub use reader::{
    MAX_NATIVE_INDEX_BYTES, MAX_NATIVE_STREAMS, MAX_TRACE_BYTES, MAX_TRACE_LINE, MAX_TRACE_ROWS,
    Step, Trace,
};
pub use redact::TEXT_FIELDS;
pub use replay::{Divergence, HANDLE_FIELDS, ReplayReport, Skipped, replay};
pub(crate) use writer::TraceWriter;
pub use writer::{DEFAULT_EVIDENCE_BUDGET, TraceMode, TraceOptions, TraceSummary};
