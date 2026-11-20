//! Reading a trace back: its header and rows within their bounds, whether it is complete, and
//! the recordings and evidence beside it.

use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use flui_protocol::record::RecordLayer;

use super::format::{CommandRow, FORMAT_VERSION, Header, ResultRow, Row};
use super::lines::{BoundedLines, LineRead};
use super::native::{self, NativeRecording};
use crate::error::{Error, Result};

/// The longest line a trace may hold: a result can carry a whole tree, as long as the longest
/// answer the driver reads from an app.
pub const MAX_TRACE_LINE: usize = crate::connection::MAX_REPLY;

/// The most rows a trace may hold after its header.
pub const MAX_TRACE_ROWS: usize = 1 << 20;

/// The longest `trace.jsonl` read.
pub const MAX_TRACE_BYTES: u64 = 1 << 30;

/// The rows' file inside a trace's directory.
pub(crate) const ROWS_FILE: &str = "trace.jsonl";

/// The input recording's file.
pub(crate) const INPUT_FILE: &str = "input.jsonl";

/// The native recordings' directory.
pub(crate) const NATIVE_DIR: &str = "native";

/// The evidence directory.
pub(crate) const EVIDENCE_DIR: &str = "evidence";

/// A trace read from its directory.
#[derive(Debug, Clone, PartialEq)]
pub struct Trace {
    dir: PathBuf,
    header: Header,
    rows: Vec<Row>,
    complete: bool,
}

/// A command and what it came to.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Step<'a> {
    /// The command.
    pub command: &'a CommandRow,
    /// Its result; none when the trace ends before it answered.
    pub result: Option<&'a ResultRow>,
}

impl Trace {
    /// The trace in the directory `dir`.
    ///
    /// # Errors
    ///
    /// [`Error::NotSupported`] naming `trace_archive` for a file (a packed trace); [`Error::Io`]
    /// when `trace.jsonl` cannot be read; [`Error::Unreadable`] naming the line for a newer
    /// format, a line or a file past its bound, a row that does not read, a result to no command,
    /// and a row after the `end`.
    pub fn open(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        if dir.is_file() {
            return Err(Error::not_supported(
                "trace_archive",
                format!(
                    "{} is a file: the driver reads a trace's directory, and a packed one is \
                     unpacked first",
                    dir.display()
                ),
            ));
        }
        let path = dir.join(ROWS_FILE);
        let file = fs::File::open(&path)?;
        let length = file.metadata()?.len();
        if length > MAX_TRACE_BYTES {
            return Err(Error::unreadable(
                &path,
                format!("{length} bytes, past the {MAX_TRACE_BYTES} a trace may have"),
            ));
        }
        let mut lines = BoundedLines::new(BufReader::new(file), MAX_TRACE_LINE);
        let bad = |line: u64, message: String| Error::unreadable_line(&path, line, message);
        let header_line = match lines.next_line()? {
            LineRead::Line { bytes, .. } => bytes,
            LineRead::TooLong => return Err(bad(1, "the header is past the line bound".into())),
            LineRead::End => return Err(bad(1, "the trace is empty".into())),
        };
        let value: serde_json::Value = serde_json::from_slice(&header_line)
            .map_err(|error| bad(1, format!("the header is not JSON: {error}")))?;
        let version = value.get("flui_trace").and_then(serde_json::Value::as_u64);
        match version {
            Some(version) if version > u64::from(FORMAT_VERSION) => {
                return Err(bad(
                    1,
                    format!(
                        "a trace of format {version}; this driver reads format {FORMAT_VERSION}"
                    ),
                ));
            }
            Some(_) => {}
            None => return Err(bad(1, "the header names no `flui_trace` version".into())),
        }
        let header: Header = serde_json::from_value(value)
            .map_err(|error| bad(1, format!("the header does not read: {error}")))?;

        let mut rows = Vec::new();
        let mut complete = false;
        let mut open: Vec<u64> = Vec::new();
        let mut number: u64 = 1;
        loop {
            number += 1;
            let (bytes, terminated) = match lines.next_line()? {
                LineRead::Line {
                    bytes, terminated, ..
                } => (bytes, terminated),
                LineRead::TooLong => {
                    return Err(bad(number, "a row is past the line bound".into()));
                }
                LineRead::End => break,
            };
            // A writer that stopped in the middle of its last line left a trace cut there: what
            // came before reads, and the trace is incomplete.
            if !terminated && serde_json::from_slice::<Row>(&bytes).is_err() {
                complete = false;
                break;
            }
            if bytes.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            if complete {
                return Err(bad(number, "a row after the trace's `end`".into()));
            }
            if rows.len() >= MAX_TRACE_ROWS {
                return Err(bad(
                    number,
                    format!("past the {MAX_TRACE_ROWS} rows a trace may hold"),
                ));
            }
            let row: Row = serde_json::from_slice(&bytes)
                .map_err(|error| bad(number, format!("not a row: {error}")))?;
            match &row {
                Row::Command(command) => open.push(command.seq),
                Row::Result(result) => {
                    let Some(at) = open.iter().position(|seq| *seq == result.seq) else {
                        return Err(bad(
                            number,
                            format!("a result to command {}, which is not open", result.seq),
                        ));
                    };
                    open.remove(at);
                    if result.ok.is_some() == result.error.is_some() {
                        return Err(bad(
                            number,
                            "a result carries exactly one of `ok` and `error`".into(),
                        ));
                    }
                }
                Row::End(_) => complete = true,
                _ => {}
            }
            rows.push(row);
        }
        Ok(Self {
            dir: dir.to_owned(),
            header,
            rows,
            complete,
        })
    }

    /// The trace's directory.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The header.
    #[must_use]
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// The rows after the header.
    #[must_use]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Whether the trace ends with its `end` row: one that does not was cut short (a crash, a
    /// writer that failed) and holds only what came before.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// Each command with its result, in the order they were sent.
    #[must_use]
    pub fn steps(&self) -> Vec<Step<'_>> {
        let mut steps: Vec<Step<'_>> = Vec::new();
        for row in &self.rows {
            match row {
                Row::Command(command) => steps.push(Step {
                    command,
                    result: None,
                }),
                Row::Result(result) => {
                    if let Some(step) = steps.iter_mut().find(|step| step.command.seq == result.seq)
                    {
                        step.result = Some(result);
                    }
                }
                _ => {}
            }
        }
        steps
    }

    /// The app's input recording (ADR-0215), when the trace holds one.
    #[must_use]
    pub fn input_recording(&self) -> Option<PathBuf> {
        let path = self.dir.join(INPUT_FILE);
        (self.header.layers.contains(&RecordLayer::Input) && path.is_file()).then_some(path)
    }

    /// The native recordings, one per stream in the order the backend opened them, read through
    /// the protocol's schema ([`native::read`]).
    ///
    /// # Errors
    ///
    /// As [`native::read`], for the first that does not read.
    pub fn native_recordings(&self) -> Result<Vec<NativeRecording>> {
        let dir = self.dir.join(NATIVE_DIR);
        if !self.header.layers.contains(&RecordLayer::Native) || !dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut streams: Vec<(u32, PathBuf)> = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            let stream = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_suffix(".jsonl"))
                .and_then(|stem| stem.parse::<u32>().ok());
            if let Some(stream) = stream {
                streams.push((stream, path));
            }
        }
        streams.sort_by_key(|(stream, _)| *stream);
        streams.iter().map(|(_, path)| native::read(path)).collect()
    }

    /// The file of the evidence `name` a result names.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for a name that is not `evidence/<sha256>.<png|json>`, so a
    /// trace from elsewhere names no file outside its directory.
    pub fn evidence(&self, name: &str) -> Result<PathBuf> {
        let file = name
            .strip_prefix("evidence/")
            .filter(|file| {
                let (hash, extension) = file.split_once('.').unwrap_or_default();
                hash.len() == 64
                    && hash
                        .bytes()
                        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
                    && matches!(extension, "png" | "json")
            })
            .ok_or_else(|| {
                Error::invalid_argument(format!("`{name}` is not evidence/<sha256>.<png|json>"))
            })?;
        Ok(self.dir.join(EVIDENCE_DIR).join(file))
    }
}
