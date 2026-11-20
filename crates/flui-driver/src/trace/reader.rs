//! Reading a trace back: its header and rows within their bounds, whether it is complete, and
//! the recordings and evidence beside it.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufReader, Read as _};
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

/// The index of the native recordings, inside their directory.
const NATIVE_INDEX: &str = "index.json";

/// The most native streams a trace may hold: a backend opens a handful.
pub const MAX_NATIVE_STREAMS: usize = 256;

/// The longest `native/index.json` read.
pub const MAX_NATIVE_INDEX_BYTES: u64 = 1 << 20;

/// The most bytes a trace's native streams may hold together: four streams at their own bound.
pub const MAX_NATIVE_TOTAL_BYTES: u64 = 256 << 20;

/// What `native/index.json` says of each stream, as far as a reader needs it.
#[derive(serde::Deserialize)]
struct NativeIndex {
    streams: Vec<NativeIndexEntry>,
}

#[derive(serde::Deserialize)]
struct NativeIndexEntry {
    stream: u32,
    #[serde(default)]
    file: Option<String>,
}

/// Whether `path` is no symbolic link or junction. A trace's files have fixed names under its own
/// directory, each checked from the top down, so a path that passes resolves inside the trace.
fn no_link(path: &Path) -> Result<()> {
    if fs::symlink_metadata(path)?.file_type().is_symlink() {
        return Err(Error::unreadable(
            path,
            "a symbolic link or a junction, which a trace never holds",
        ));
    }
    Ok(())
}

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
        // The length read above is of the file when it was opened; the reading itself stops one
        // byte past the bound, so a file that grows as it is read (or a pipe) never passes it.
        let mut lines = BoundedLines::new(
            BufReader::new(file.take(MAX_TRACE_BYTES.saturating_add(1))),
            MAX_TRACE_LINE,
        );
        let bad = |line: u64, message: String| Error::unreadable_line(&path, line, message);
        let mut total: u64 = 0;
        let mut counted = |len: usize, line: u64| -> Result<()> {
            total = total.saturating_add(u64::try_from(len).unwrap_or(u64::MAX));
            if total > MAX_TRACE_BYTES {
                return Err(Error::unreadable_line(
                    &path,
                    line,
                    format!("past the {MAX_TRACE_BYTES} bytes a trace may have"),
                ));
            }
            Ok(())
        };
        let header_line = match lines.next_line()? {
            LineRead::Line { bytes, len, .. } => {
                counted(len, 1)?;
                bytes
            }
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
        let mut open: BTreeSet<u64> = BTreeSet::new();
        let mut sent: BTreeSet<u64> = BTreeSet::new();
        let mut number: u64 = 1;
        loop {
            number += 1;
            let (bytes, terminated) = match lines.next_line()? {
                LineRead::Line {
                    bytes,
                    len,
                    terminated,
                } => {
                    counted(len, number)?;
                    (bytes, terminated)
                }
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
                Row::Command(command) => {
                    if !sent.insert(command.seq) {
                        return Err(bad(
                            number,
                            format!("a second command numbered {}", command.seq),
                        ));
                    }
                    open.insert(command.seq);
                }
                Row::Result(result) => {
                    if !open.remove(&result.seq) {
                        return Err(bad(
                            number,
                            format!("a result to command {}, which is not open", result.seq),
                        ));
                    }
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
        let results: BTreeMap<u64, &ResultRow> = self
            .rows
            .iter()
            .filter_map(|row| match row {
                Row::Result(result) => Some((result.seq, result)),
                _ => None,
            })
            .collect();
        self.rows
            .iter()
            .filter_map(|row| match row {
                Row::Command(command) => Some(Step {
                    command,
                    result: results.get(&command.seq).copied(),
                }),
                _ => None,
            })
            .collect()
    }

    /// The app's input recording (ADR-0215), when the trace holds one.
    #[must_use]
    pub fn input_recording(&self) -> Option<PathBuf> {
        let path = self.dir.join(INPUT_FILE);
        (self.header.layers.contains(&RecordLayer::Input) && path.is_file()).then_some(path)
    }

    /// The native recordings `native/index.json` declares, one per stream in the order the
    /// backend opened them, read through the protocol's schema ([`native::read`]). The index is
    /// the list: a file it does not name is not read, at most [`MAX_NATIVE_STREAMS`] are, each
    /// under its one name (`<stream>.jsonl`), and none through a symbolic link or outside the
    /// trace's directory.
    ///
    /// # Errors
    ///
    /// [`Error::Unreadable`] for a trace whose header holds the native layer and has no index
    /// (the run ended before the trace did), an index past [`MAX_NATIVE_INDEX_BYTES`] or
    /// [`MAX_NATIVE_STREAMS`] or naming a stream twice or a file by another name, and a symbolic
    /// link or junction, and streams past [`MAX_NATIVE_TOTAL_BYTES`] together; as
    /// [`native::read`] for the first stream
    /// that does not read.
    pub fn native_recordings(&self) -> Result<Vec<NativeRecording>> {
        if !self.header.layers.contains(&RecordLayer::Native) {
            return Ok(Vec::new());
        }
        let dir = self.dir.join(NATIVE_DIR);
        let index_path = dir.join(NATIVE_INDEX);
        if fs::symlink_metadata(&index_path).is_err() {
            return Err(Error::unreadable(
                &index_path,
                "the trace holds the native layer and no index of its streams: the run ended \
                 before the trace was finished, so the streams were never written",
            ));
        }
        no_link(&dir)?;
        no_link(&index_path)?;
        let index = fs::File::open(&index_path)?;
        let length = index.metadata()?.len();
        let mut text = Vec::new();
        index
            .take(MAX_NATIVE_INDEX_BYTES.saturating_add(1))
            .read_to_end(&mut text)?;
        if length > MAX_NATIVE_INDEX_BYTES
            || u64::try_from(text.len()).map_or(true, |read| read > MAX_NATIVE_INDEX_BYTES)
        {
            return Err(Error::unreadable(
                &index_path,
                format!("past the {MAX_NATIVE_INDEX_BYTES} bytes an index may have"),
            ));
        }
        let index: NativeIndex = serde_json::from_slice(&text)
            .map_err(|error| Error::unreadable(&index_path, format!("not an index: {error}")))?;
        if index.streams.len() > MAX_NATIVE_STREAMS {
            return Err(Error::unreadable(
                &index_path,
                format!(
                    "{} streams, past the {MAX_NATIVE_STREAMS} a trace may hold",
                    index.streams.len()
                ),
            ));
        }
        // Every file the index names is checked, and their sizes added up, before any is read.
        let mut seen = BTreeSet::new();
        let mut paths = Vec::new();
        let mut total: u64 = 0;
        for entry in &index.streams {
            if !seen.insert(entry.stream) {
                return Err(Error::unreadable(
                    &index_path,
                    format!("stream {} is named twice", entry.stream),
                ));
            }
            let Some(file) = &entry.file else {
                // A stream whose header never came, or was longer than a line: no file.
                continue;
            };
            let canonical = format!("{}.jsonl", entry.stream);
            if *file != canonical {
                return Err(Error::unreadable(
                    &index_path,
                    format!("stream {} is in `{canonical}`, not `{file}`", entry.stream),
                ));
            }
            let path = dir.join(&canonical);
            no_link(&path)?;
            total = total.saturating_add(fs::metadata(&path)?.len());
            if total > MAX_NATIVE_TOTAL_BYTES {
                return Err(Error::unreadable(
                    &dir,
                    format!(
                        "the streams hold more than the {MAX_NATIVE_TOTAL_BYTES} bytes a trace's \
                         native layer may"
                    ),
                ));
            }
            paths.push(path);
        }
        paths.iter().map(|path| native::read(path)).collect()
    }

    /// The file of the evidence `name` a result names.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidArgument`] for a name that is not `evidence/<sha256>.png`, and
    /// [`Error::Unreadable`] for an `evidence/` directory or a file that is a symbolic link or a
    /// junction, so a trace from elsewhere names no file outside its directory; [`Error::Io`]
    /// when the file is not there.
    pub fn evidence(&self, name: &str) -> Result<PathBuf> {
        let file = name
            .strip_prefix("evidence/")
            .filter(|file| {
                let (hash, extension) = file.split_once('.').unwrap_or_default();
                hash.len() == 64
                    && hash
                        .bytes()
                        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
                    && extension == "png"
            })
            .ok_or_else(|| {
                Error::invalid_argument(format!("`{name}` is not evidence/<sha256>.png"))
            })?;
        let folder = self.dir.join(EVIDENCE_DIR);
        no_link(&folder)?;
        let path = folder.join(file);
        no_link(&path)?;
        Ok(path)
    }
}
