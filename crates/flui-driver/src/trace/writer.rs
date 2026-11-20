//! Writing a trace as a session runs (ADR-0241 §1–§4).
//!
//! Each row is one line, written whole and handed to the OS before the next command runs, so a
//! crash leaves every row that came before and no `end`: a reader reports such a trace
//! incomplete. A write that fails (a full disk) is kept as the trace's first failure and nothing
//! more is written, never an earlier row rewritten; the session goes on, and finishing the trace
//! answers that failure. Evidence is content-addressed under `evidence/`, so a capture taken
//! twice is stored once, and evidence past the trace's budget becomes a `gap` row instead.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use flui_protocol::events::Event;
use flui_protocol::input_recording::{self, REDACTED};
use flui_protocol::record::{RecordEvent, RecordNativeEvent, RecordSummary};
use flui_protocol::verdict::Verdict;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::format::{
    CommandRow, EndRow, EventRow, GapReason, GapRow, Header, Privacy, ResultRow, Row, TraceError,
};
use super::native::{NativeFiles, WrittenStream};
use super::reader::{EVIDENCE_DIR, INPUT_FILE, NATIVE_DIR, ROWS_FILE};
use super::redact;
use crate::error::{Error, Result};
use crate::pixels::{Image, codec};

/// The evidence a trace keeps by default before it writes `gap` rows instead: 256 MiB.
pub const DEFAULT_EVIDENCE_BUDGET: u64 = 256 << 20;

/// When a trace is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum TraceMode {
    /// Always.
    #[default]
    On,
    /// Unless the run passes: a failing or `cannot_verify` report keeps it, a passing one
    /// removes it.
    RetainOnFailure,
}

/// How a session records its trace: see the [module docs](self).
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub struct TraceOptions {
    pub(crate) dir: PathBuf,
    pub(crate) mode: TraceMode,
    pub(crate) keep_text: bool,
    pub(crate) evidence_budget: u64,
    pub(crate) record_layers: bool,
}

impl TraceOptions {
    /// A trace in the directory `dir` (by convention `<name>.flui-trace`), always kept, text
    /// redacted, with the default evidence budget, holding the app's recording layers where it
    /// serves them.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            mode: TraceMode::On,
            keep_text: false,
            evidence_budget: DEFAULT_EVIDENCE_BUDGET,
            record_layers: true,
        }
    }

    /// Kept as `mode` says.
    pub fn mode(mut self, mode: TraceMode) -> Self {
        self.mode = mode;
        self
    }

    /// Keeping typed text and the app's text when `keep_text` (ADR-0215 §5).
    pub fn keep_text(mut self, keep_text: bool) -> Self {
        self.keep_text = keep_text;
        self
    }

    /// Keeping at most `bytes` of evidence.
    pub fn evidence_budget(mut self, bytes: u64) -> Self {
        self.evidence_budget = bytes;
        self
    }

    /// Holding the app's input and native recordings beside the rows when `record`, where the
    /// app serves them.
    pub fn record_layers(mut self, record: bool) -> Self {
        self.record_layers = record;
        self
    }
}

/// What a finished trace came to.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct TraceSummary {
    /// Its directory; none when [`TraceMode::RetainOnFailure`] removed it.
    pub dir: Option<PathBuf>,
    /// The commands it holds.
    pub commands: u64,
    /// The rows and evidence files its `gap` rows stand for.
    pub gaps: u64,
    /// What the app's recording came to, when the trace held one.
    pub recording: Option<RecordSummary>,
    /// The native streams written, when the recording held the native layer.
    pub native: Vec<WrittenStream>,
}

/// The app's recording a trace holds.
#[derive(Debug)]
struct Recorder {
    number: u64,
    input: File,
    native: Option<NativeFiles>,
    ended: bool,
}

/// A trace being written: see the [module docs](self).
#[derive(Debug)]
pub(crate) struct TraceWriter {
    dir: PathBuf,
    mode: TraceMode,
    privacy: Privacy,
    rows: File,
    started: Duration,
    seq: u64,
    commands: u64,
    gaps: u64,
    evidence_left: u64,
    evidence_seen: BTreeSet<String>,
    pending_evidence: Vec<String>,
    pending_gap: u64,
    recorder: Option<Recorder>,
    /// The first write that failed; nothing is written after it.
    failure: Option<io::Error>,
    /// The recording's files this writer made, removed if the trace never starts.
    created: Vec<PathBuf>,
    /// Whether this writer made the trace's directory, which it then removes if the trace
    /// never starts.
    made_dir: bool,
}

impl TraceWriter {
    /// Begin the trace `options` describe with `header`, at `now` on the session's clock.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] when the directory already holds a trace; [`Error::Io`] when
    /// it cannot be made or the header written.
    pub(crate) fn create(options: &TraceOptions, header: &Header, now: Duration) -> Result<Self> {
        let rows_path = options.dir.join(ROWS_FILE);
        let made_dir = fs::symlink_metadata(&options.dir).is_err();
        fs::create_dir_all(&options.dir)?;
        let mut rows = create_new(&rows_path).map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                Error::invalid_argument(format!(
                    "{} already holds a trace; a trace never writes over another",
                    options.dir.display()
                ))
            } else {
                error.into()
            }
        })?;
        rows.write_all(&line(header)?)?;
        Ok(Self {
            dir: options.dir.clone(),
            mode: options.mode,
            privacy: header.privacy,
            rows,
            started: now,
            seq: 0,
            commands: 0,
            gaps: 0,
            evidence_left: options.evidence_budget,
            evidence_seen: BTreeSet::new(),
            pending_evidence: Vec::new(),
            pending_gap: 0,
            recorder: None,
            failure: None,
            created: Vec::new(),
            made_dir,
        })
    }

    /// Hold the app's recording number `number`, which starts from `header`, with its native
    /// layer when `native`.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] when its files cannot be made.
    pub(crate) fn hold_recording(
        &mut self,
        number: u64,
        header: &input_recording::Header,
        native: bool,
    ) -> Result<()> {
        // Each is made new, never opened: a file or a directory of the name that is there already
        // is another's, and is refused rather than written over.
        let input_path = self.dir.join(INPUT_FILE);
        let mut input = create_new(&input_path).map_err(|error| refused(&input_path, error))?;
        self.created.push(input_path);
        input.write_all(&line(header)?)?;
        let native = if native {
            let native_path = self.dir.join(NATIVE_DIR);
            fs::create_dir(&native_path).map_err(|error| refused(&native_path, error))?;
            self.created.push(native_path.clone());
            Some(NativeFiles::create(&native_path)?)
        } else {
            None
        };
        self.recorder = Some(Recorder {
            number,
            input,
            native,
            ended: false,
        });
        Ok(())
    }

    /// Remove what this writer made, the trace never started: its rows' file and the recording's
    /// files, and the directory when this writer made it and it is left empty. Nothing it did
    /// not make is touched, and a removal that fails leaves the file behind.
    pub(crate) fn abandon(self) {
        let Self {
            dir,
            rows,
            created,
            made_dir,
            ..
        } = self;
        drop(rows);
        for path in created.iter().rev() {
            let _ = if path.is_dir() {
                fs::remove_dir_all(path)
            } else {
                fs::remove_file(path)
            };
        }
        let _ = fs::remove_file(dir.join(ROWS_FILE));
        if made_dir {
            let _ = fs::remove_dir(&dir);
        }
    }

    /// The app's recording the trace holds, by its number.
    pub(crate) fn recording(&self) -> Option<u64> {
        self.recorder.as_ref().map(|recorder| recorder.number)
    }

    /// Whether the trace keeps text.
    pub(crate) fn keeps_text(&self) -> bool {
        self.privacy == Privacy::KeepText
    }

    /// Write the command `method` with `params`, sent at `now`; its number.
    pub(crate) fn command(&mut self, method: &str, mut params: Value, now: Duration) -> u64 {
        if !self.keeps_text() {
            redact::params(&mut params);
        }
        self.seq += 1;
        self.commands += 1;
        self.pending_evidence.clear();
        self.pending_gap = 0;
        let row = Row::Command(CommandRow {
            seq: self.seq,
            t_ms: self.millis(now),
            method: method.to_owned(),
            params,
        });
        self.write_row(&row);
        self.seq
    }

    /// Write what command `seq` came to at `now`, with the evidence it left.
    pub(crate) fn result(
        &mut self,
        seq: u64,
        outcome: std::result::Result<Value, &Error>,
        now: Duration,
    ) {
        if self.pending_gap > 0 {
            let rows = std::mem::take(&mut self.pending_gap);
            self.gaps = self.gaps.saturating_add(rows);
            self.write_row(&Row::Gap(GapRow {
                rows,
                reason: GapReason::EvidenceBudget,
            }));
        }
        let (ok, error) = match outcome {
            Ok(mut value) => {
                if !self.keeps_text() {
                    redact::output(&mut value);
                }
                (Some(value), None)
            }
            Err(error) => (None, Some(self.error(error))),
        };
        let row = Row::Result(ResultRow {
            seq,
            t_ms: self.millis(now),
            ok,
            error,
            evidence: std::mem::take(&mut self.pending_evidence),
        });
        self.write_row(&row);
    }

    /// `error` as the trace keeps it: its message redacted unless the trace keeps text, since a
    /// failure can quote what the app shows.
    pub(crate) fn error(&self, error: &Error) -> TraceError {
        TraceError {
            code: error.code(),
            message: if self.keeps_text() {
                error.to_string()
            } else {
                REDACTED.to_owned()
            },
        }
    }

    /// Take `events`, of which the connection dropped `dropped` before they were taken, at
    /// `now`: the app's recording rows go to its files, the rest become `event` rows. Whether
    /// each event was the trace's own recording's, in order.
    pub(crate) fn events(&mut self, events: &[Event], dropped: u64, now: Duration) -> Vec<bool> {
        if dropped > 0 {
            self.gaps = self.gaps.saturating_add(dropped);
            self.write_row(&Row::Gap(GapRow {
                rows: dropped,
                reason: GapReason::EventsDropped,
            }));
            // The rows of the recording may be among them, of its input layer and of any of its
            // native streams: each file says so where a replay reads it, rather than running
            // past the hole. Which events were lost is not known, so every stream the loss may
            // have touched gets the gap. Once the recording ended, no line of it is left to lose.
            if let Some(recorder) = self.recorder.as_mut()
                && !recorder.ended
            {
                let input = line(&input_recording::Row::Gap { rows: dropped })
                    .map_err(io::Error::other)
                    .and_then(|bytes| recorder.input.write_all(&bytes));
                let native = recorder
                    .native
                    .as_mut()
                    .map_or(Ok(()), |native| native.lose(dropped));
                self.keep_failure(input);
                self.keep_failure(native);
            }
        }
        let t_ms = self.millis(now);
        events
            .iter()
            .map(|event| match event {
                Event::Record(record) => self.input_row(record),
                Event::RecordNative(native) => self.native_line(native),
                other => {
                    self.event_row(other, t_ms);
                    false
                }
            })
            .collect()
    }

    fn input_row(&mut self, event: &RecordEvent) -> bool {
        let Some(recorder) = self.recorder.as_mut() else {
            return false;
        };
        if event.recording != recorder.number {
            return false;
        }
        if !recorder.ended {
            if matches!(event.row, input_recording::Row::End { .. }) {
                recorder.ended = true;
            }
            let written = line(&event.row)
                .map_err(io::Error::other)
                .and_then(|bytes| recorder.input.write_all(&bytes));
            self.keep_failure(written);
        }
        true
    }

    fn native_line(&mut self, event: &RecordNativeEvent) -> bool {
        let Some(recorder) = self.recorder.as_mut() else {
            return false;
        };
        if event.recording != recorder.number {
            return false;
        }
        if let Some(native) = recorder.native.as_mut() {
            let written = native.take(event.stream, &event.line);
            self.keep_failure(written);
        }
        true
    }

    fn event_row(&mut self, event: &Event, t_ms: u64) {
        let Ok(mut value) = serde_json::to_value(event) else {
            return;
        };
        let name = value
            .get("event")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_owned();
        let mut params = value
            .as_object_mut()
            .and_then(|fields| fields.remove("params"))
            .unwrap_or(Value::Null);
        if !self.keeps_text() {
            redact::output(&mut params);
        }
        self.write_row(&Row::Event(EventRow { t_ms, name, params }));
    }

    /// Keep `image` as evidence of the command being run, or count it in that command's `gap`
    /// when it is past the budget.
    pub(crate) fn evidence_png(&mut self, image: &Image) {
        let Ok(bytes) = codec::encode(image) else {
            self.pending_gap = self.pending_gap.saturating_add(1);
            return;
        };
        let name = format!("{EVIDENCE_DIR}/{}.png", sha256_hex(&bytes));
        if self.evidence_seen.contains(&name) {
            self.pending_evidence.push(name);
            return;
        }
        let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        if size > self.evidence_left || self.failure.is_some() {
            self.pending_gap = self.pending_gap.saturating_add(1);
            return;
        }
        let folder = self.dir.join(EVIDENCE_DIR);
        let written =
            fs::create_dir_all(&folder).and_then(|()| fs::write(self.dir.join(&name), &bytes));
        if written.is_ok() {
            self.evidence_left -= size;
            self.evidence_seen.insert(name.clone());
            self.pending_evidence.push(name);
        } else {
            self.pending_gap = self.pending_gap.saturating_add(1);
        }
        self.keep_failure(written);
    }

    /// End the trace with its `end` row after `stopped`, what the app's recording came to: its
    /// native streams' files are written, and a trace kept only on failure is removed when
    /// `verdict` is a pass.
    ///
    /// # Errors
    ///
    /// The trace's first failed write, with the trace left without its `end`; [`Error::Io`] when
    /// the native files or the removal fail.
    pub(crate) fn finish(
        mut self,
        stopped: Option<RecordSummary>,
        verdict: Verdict,
    ) -> Result<TraceSummary> {
        let native = match self.recorder.take().and_then(|recorder| recorder.native) {
            Some(native) => native.finish()?,
            None => Vec::new(),
        };
        self.write_row(&Row::End(EndRow::default()));
        if let Some(failure) = self.failure.take() {
            return Err(Error::Io(io::Error::new(
                failure.kind(),
                format!(
                    "the trace at {} stopped at a failed write: {failure}",
                    self.dir.display()
                ),
            )));
        }
        self.rows.flush()?;
        let kept = self.mode == TraceMode::On || verdict != Verdict::Pass;
        if !kept {
            fs::remove_dir_all(&self.dir)?;
        }
        Ok(TraceSummary {
            dir: kept.then(|| self.dir.clone()),
            commands: self.commands,
            gaps: self.gaps,
            recording: stopped,
            native,
        })
    }

    /// The directory.
    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }

    fn millis(&self, now: Duration) -> u64 {
        u64::try_from(now.saturating_sub(self.started).as_millis()).unwrap_or(u64::MAX)
    }

    fn write_row(&mut self, row: &Row) {
        if self.failure.is_some() {
            return;
        }
        let written = line(row)
            .map_err(io::Error::other)
            .and_then(|bytes| self.rows.write_all(&bytes));
        self.keep_failure(written);
    }

    fn keep_failure(&mut self, written: io::Result<()>) {
        if let Err(error) = written
            && self.failure.is_none()
        {
            self.failure = Some(error);
        }
    }
}

/// `value` as one line, its newline included.
fn line(value: &impl Serialize) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec(value).map_err(|error| Error::Protocol {
        message: format!("a trace row did not serialize: {error}"),
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// A file made at `path`, which must not be there: one that is (another's, or a link) fails with
/// [`io::ErrorKind::AlreadyExists`], never opened.
fn create_new(path: &Path) -> io::Result<File> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

/// `error` of making `path`: a refusal when something of the name is there already.
fn refused(path: &Path, error: io::Error) -> Error {
    if error.kind() == io::ErrorKind::AlreadyExists {
        Error::invalid_argument(format!(
            "{} is there already; a trace never writes over another's files",
            path.display()
        ))
    } else {
        error.into()
    }
}

/// The SHA-256 of `bytes`, in lowercase hex.
fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            write!(hex, "{byte:02x}").expect("BUG: writing to a String cannot fail");
            hex
        })
}
