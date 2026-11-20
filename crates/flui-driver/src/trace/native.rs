//! A recording's native layer, written as one native recording per stream (ADR-0288 §11).
//!
//! Each stream the app's backend journals becomes `<dir>/<n>.jsonl`, the native recording
//! format (`flui_protocol::native_recording`) that `flui-native` replays: its header, then its
//! rows. A stream's header arrives naming no device, since its devices are known only once its
//! rows are, so the rows go to `<n>.rows.part` as they arrive, flushed line by line, and
//! [`NativeFiles::finish`] writes the file with the header naming every device the rows read,
//! then removes the part. A `gap` where the stream lost rows stays in place, spelled as the
//! input recording spells one, so a replay refuses the file there rather than run past it.
//!
//! A file is held to the bounds its reader holds it to, counted as the lines arrive with the
//! format's [`Budget`]. A header longer than a line may be opens no file. Each row is admitted
//! with the bytes the header grows by to declare the devices it reads first, and the first row
//! past [`MAX_ROWS`] or [`MAX_RECORDING_BYTES`] is replaced by a `cut` row that ends the file, so
//! a replay of it stops at the cut instead of the reader refusing the whole file. A row the file
//! cannot hold on its own (longer than a line, or declaring a device the header has no room
//! for) is lost in place of a `gap`.
//!
//! The counts follow one rule: every row the stream sent is counted once, as written, lost or
//! dropped at the cut; a `gap` line is no row, and the rows it stands for are lost; a line this
//! writer does not know is one row lost.
//!
//! `<dir>/index.json` names each file's stream, backend, what it journals, those counts and
//! where it was cut.
//!
//! [`read`] reads one such file back through the protocol's schema alone, without the core that
//! wrote its rows (ADR-0215 §4): the header's version, the line, row and byte bounds, the devices
//! each row reads declared in the header, and a `gap` kept in place.
//!
//! [`MAX_ROWS`]: flui_protocol::native_recording::MAX_ROWS
//! [`MAX_RECORDING_BYTES`]: flui_protocol::native_recording::MAX_RECORDING_BYTES

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use flui_protocol::native_recording::{
    Backend, Budget, CutReason, Device, Header, Line, Overrun, PointerKind, Reading, Row, Stream,
};
use serde::Serialize;

/// The native streams of one recording, as they are written.
#[derive(Debug)]
pub struct NativeFiles {
    dir: PathBuf,
    streams: BTreeMap<u32, StreamFile>,
}

/// One stream, as it is written.
#[derive(Debug)]
struct StreamFile {
    state: State,
    /// Every device its written rows read, with the kinds they read it as, in the order first
    /// read.
    devices: Vec<Device>,
    /// Where its rows go until [`NativeFiles::finish`].
    part: BufWriter<File>,
    /// The rows written, `gap` lines and the `cut` aside.
    rows: u64,
    /// The rows it lost: as its `gap` lines count them, the rows before its header or after a
    /// header no file holds, rows the file could not hold on their own, and lines this writer does
    /// not know.
    lost_rows: u64,
}

/// Where a stream's file stands.
#[derive(Debug)]
enum State {
    /// No header yet: a row is lost.
    Unopened,
    /// The header longer than a line may be: no file, and every row is lost.
    HeaderTooLong,
    /// Rows are written while the file's bounds admit them.
    Open {
        /// The header, its devices left for [`NativeFiles::finish`] to fill in.
        header: Header,
        budget: Budget,
    },
    /// Ended at a `cut` row: every row after it is dropped.
    Cut { header: Header, cut: StreamCut },
}

/// The bound a stream's file reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[expect(
    clippy::exhaustive_enums,
    reason = "the two bounds a native recording is full at, which the format fixes"
)]
pub enum FullBound {
    /// [`MAX_ROWS`](flui_protocol::native_recording::MAX_ROWS).
    Rows,
    /// [`MAX_RECORDING_BYTES`](flui_protocol::native_recording::MAX_RECORDING_BYTES).
    Bytes,
}

impl FullBound {
    /// The bound `overrun` says a recording is full at, if it says so.
    const fn of(overrun: Overrun) -> Option<Self> {
        match overrun {
            Overrun::Rows => Some(Self::Rows),
            Overrun::RecordingBytes => Some(Self::Bytes),
            _ => None,
        }
    }

    const fn cut_reason(self) -> CutReason {
        match self {
            Self::Rows => CutReason::Rows,
            Self::Bytes => CutReason::Bytes,
        }
    }
}

/// Where a stream's file ends at a `cut` row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct StreamCut {
    /// The bound the next line would have passed.
    pub bound: FullBound,
    /// The rows the stream sent from there that were not written: the one the cut replaced,
    /// when a row (not a `gap`) met the bound, and every row after it.
    pub dropped: u64,
}

/// One stream's file, as [`NativeFiles::finish`] wrote it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct WrittenStream {
    /// The stream's number, in the order the backend opened them.
    pub stream: u32,
    /// The file, relative to the directory; none for a stream whose header never came, or was
    /// longer than a line may be.
    pub file: Option<String>,
    /// The backend it was taken on, when it has a file.
    pub backend: Option<Backend>,
    /// What it journals, when it has a file.
    pub journals: Option<Stream>,
    /// Its rows written, `gap` lines and the `cut` aside.
    pub rows: u64,
    /// The rows it lost.
    pub lost_rows: u64,
    /// Where its file ends at a `cut` row, when it does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cut: Option<StreamCut>,
    /// Its header was longer than a line may be, so it has no file.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub header_too_long: bool,
}

/// The index file's shape.
#[derive(Serialize)]
struct Index<'a> {
    streams: &'a [WrittenStream],
}

impl NativeFiles {
    /// The streams of a recording written into `dir`, which is created when missing.
    ///
    /// # Errors
    ///
    /// When `dir` cannot be created.
    pub fn create(dir: &Path) -> io::Result<Self> {
        fs::create_dir_all(dir).map_err(|e| at(dir, e))?;
        Ok(Self {
            dir: dir.to_owned(),
            streams: BTreeMap::new(),
        })
    }

    /// The directory the streams go to.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Takes `line` of `stream`.
    ///
    /// # Errors
    ///
    /// When the stream's part file cannot be created or written.
    pub fn take(&mut self, stream: u32, line: &Line) -> io::Result<()> {
        let path = self.dir.join(part_name(stream));
        let file = match self.streams.entry(stream) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                let part = File::create(&path).map_err(|e| at(&path, e))?;
                entry.insert(StreamFile {
                    state: State::Unopened,
                    devices: Vec::new(),
                    part: BufWriter::new(part),
                    rows: 0,
                    lost_rows: 0,
                })
            }
        };
        file.take(line).map_err(|e| at(&path, e))
    }

    /// Takes a loss of `rows` lines the writer never saw, of any of the streams (events a
    /// connection dropped before they were taken): each stream it holds gets a `gap` of `rows`,
    /// since which of them lost the lines is not known, so no file reads as whole past it.
    ///
    /// # Errors
    ///
    /// When a stream's part file cannot be written.
    pub fn lose(&mut self, rows: u64) -> io::Result<()> {
        for (stream, file) in &mut self.streams {
            let path = self.dir.join(part_name(*stream));
            file.take(&Line::Gap { rows }).map_err(|e| at(&path, e))?;
        }
        Ok(())
    }

    /// Writes each stream's file and the index, removing the part files.
    ///
    /// # Errors
    ///
    /// The first file that cannot be read or written; the part files of the streams not yet
    /// written are kept.
    pub fn finish(self) -> io::Result<Vec<WrittenStream>> {
        let mut written = Vec::with_capacity(self.streams.len());
        for (stream, file) in self.streams {
            let StreamFile {
                state,
                devices,
                part,
                rows,
                lost_rows,
            } = file;
            let part_path = self.dir.join(part_name(stream));
            part.into_inner()
                .map_err(io::IntoInnerError::into_error)
                .map_err(|e| at(&part_path, e))?;
            let mut entry = WrittenStream {
                stream,
                file: None,
                backend: None,
                journals: None,
                rows,
                lost_rows,
                cut: None,
                header_too_long: matches!(state, State::HeaderTooLong),
            };
            let (mut header, cut) = match state {
                State::Open { header, .. } => (header, None),
                State::Cut { header, cut } => (header, Some(cut)),
                State::Unopened | State::HeaderTooLong => {
                    fs::remove_file(&part_path).map_err(|e| at(&part_path, e))?;
                    written.push(entry);
                    continue;
                }
            };
            header.devices = devices;
            let name = format!("{stream}.jsonl");
            let path = self.dir.join(&name);
            let mut out = BufWriter::new(File::create(&path).map_err(|e| at(&path, e))?);
            out.write_all(&encode_line(&header)?)
                .map_err(|e| at(&path, e))?;
            let mut rows_in = File::open(&part_path).map_err(|e| at(&part_path, e))?;
            io::copy(&mut rows_in, &mut out).map_err(|e| at(&path, e))?;
            out.flush().map_err(|e| at(&path, e))?;
            drop(rows_in);
            fs::remove_file(&part_path).map_err(|e| at(&part_path, e))?;
            entry.file = Some(name);
            entry.backend = Some(header.backend);
            entry.journals = Some(header.stream);
            entry.cut = cut;
            written.push(entry);
        }
        let path = self.dir.join("index.json");
        let mut index = BufWriter::new(File::create(&path).map_err(|e| at(&path, e))?);
        serde_json::to_writer_pretty(&mut index, &Index { streams: &written })
            .map_err(io::Error::from)
            .and_then(|()| index.write_all(b"\n"))
            .and_then(|()| index.flush())
            .map_err(|e| at(&path, e))?;
        Ok(written)
    }
}

impl StreamFile {
    /// Takes `line`: a header opens the file, a row or a gap is written while the file's bounds
    /// admit it, and each is counted by the module's rule.
    fn take(&mut self, line: &Line) -> io::Result<()> {
        match line {
            // A stream sends its header once; a second one would restart it, which no stream
            // does, so the first stands.
            Line::Header(header) => {
                if matches!(self.state, State::Unopened) {
                    self.open(header)?;
                }
                Ok(())
            }
            Line::Row(row) => match self.state {
                State::Open { .. } => self.write_row(row),
                State::Cut { ref mut cut, .. } => {
                    cut.dropped = cut.dropped.saturating_add(1);
                    Ok(())
                }
                State::Unopened | State::HeaderTooLong => {
                    self.lost_rows = self.lost_rows.saturating_add(1);
                    Ok(())
                }
            },
            Line::Gap { rows } => {
                self.lost_rows = self.lost_rows.saturating_add(*rows);
                if matches!(self.state, State::Open { .. }) {
                    self.write_gap(line)?;
                }
                Ok(())
            }
            // A line this writer does not know is not one it can place in the file.
            _ => {
                self.lost_rows = self.lost_rows.saturating_add(1);
                Ok(())
            }
        }
    }

    /// Opens the file with `header`, unless it is longer than a line may be.
    fn open(&mut self, header: &Header) -> io::Result<()> {
        let mut header = header.clone();
        header.devices.clear();
        self.state = match Budget::for_header(encode_line(&header)?.len()) {
            Ok(budget) => State::Open { header, budget },
            Err(_) => State::HeaderTooLong,
        };
        Ok(())
    }

    /// Writes `row` and declares its devices, cuts the file where it is full, or writes a `gap`
    /// in place of a row the file cannot hold on its own.
    fn write_row(&mut self, row: &Row) -> io::Result<()> {
        let encoded = encode_line(row)?;
        let undeclared = undeclared(row, &self.devices);
        let declared = declared_bytes(&undeclared, &self.devices)?;
        let Err(overrun) = self.budget().admit_row(encoded.len(), declared, true) else {
            write_flushed(&mut self.part, &encoded)?;
            declare(&mut self.devices, &undeclared);
            self.rows += 1;
            return Ok(());
        };
        if let Some(bound) = FullBound::of(overrun) {
            return self.write_cut(bound, 1);
        }
        self.lost_rows = self.lost_rows.saturating_add(1);
        self.write_gap(&Line::Gap { rows: 1 })
    }

    /// Writes `gap`, whose rows are counted lost already, or cuts the file where it is full.
    fn write_gap(&mut self, gap: &Line) -> io::Result<()> {
        let encoded = encode_line(gap)?;
        match self.budget().admit_row(encoded.len(), 0, true) {
            Ok(()) => write_flushed(&mut self.part, &encoded),
            // A gap fits any line and declares nothing: only a full file refuses it, and a gap
            // is no row the cut drops.
            Err(overrun) => self.write_cut(FullBound::of(overrun).unwrap_or(FullBound::Bytes), 0),
        }
    }

    /// Ends the file with a `cut` row naming `bound`, in place of the line it could not take,
    /// which was `dropped` rows. The budget keeps room for it after every line it admits.
    fn write_cut(&mut self, bound: FullBound, dropped: u64) -> io::Result<()> {
        let State::Open { header, .. } = std::mem::replace(&mut self.state, State::Unopened) else {
            unreachable!("BUG: a stream cuts only an open file");
        };
        self.state = State::Cut {
            header,
            cut: StreamCut { bound, dropped },
        };
        let cut = Row::Cut {
            reason: bound.cut_reason(),
        };
        write_flushed(&mut self.part, &encode_line(&cut)?)
    }

    fn budget(&mut self) -> &mut Budget {
        let State::Open { budget, .. } = &mut self.state else {
            unreachable!("BUG: a stream writes lines only while its file is open");
        };
        budget
    }
}

/// The part file of `stream`.
fn part_name(stream: u32) -> String {
    format!("{stream}.rows.part")
}

/// The readings `row` holds.
fn readings(row: &Row) -> impl Iterator<Item = &Reading> {
    let (single, batch) = match row {
        Row::Reading { reading } => (Some(reading), None),
        Row::Batch {
            coalesced,
            predicted,
            current,
        } => (
            None,
            Some(
                coalesced
                    .iter()
                    .chain(predicted)
                    .chain(std::iter::once(current)),
            ),
        ),
        // The other rows read no device.
        _ => (None, None),
    };
    single.into_iter().chain(batch.into_iter().flatten())
}

/// Each device and kind `row` reads that `devices` does not declare yet, once, in the order
/// read.
fn undeclared(row: &Row, devices: &[Device]) -> Vec<(u64, PointerKind)> {
    let mut found: Vec<(u64, PointerKind)> = Vec::new();
    for reading in readings(row) {
        let Some(device) = reading.device else {
            continue;
        };
        let pair = (device, reading.kind);
        let declared = devices
            .iter()
            .any(|entry| entry.device == device && entry.kinds.contains(&reading.kind));
        if !declared && !found.contains(&pair) {
            found.push(pair);
        }
    }
    found
}

/// The bytes the header grows by to declare `undeclared` beside `devices`: a new device's entry
/// or a new kind of a known one, each with the comma before it (one byte more than the header
/// takes for its first device, so the count never falls short).
fn declared_bytes(undeclared: &[(u64, PointerKind)], devices: &[Device]) -> io::Result<usize> {
    let mut bytes = 0usize;
    for (index, &(device, kind)) in undeclared.iter().enumerate() {
        let known = devices.iter().any(|entry| entry.device == device)
            || undeclared[..index]
                .iter()
                .any(|&(earlier, _)| earlier == device);
        let entry = if known {
            serde_json::to_vec(&kind)?.len()
        } else {
            serde_json::to_vec(&Device {
                device,
                kinds: vec![kind],
            })?
            .len()
        };
        bytes = bytes.saturating_add(entry).saturating_add(1);
    }
    Ok(bytes)
}

/// Adds `undeclared` to `devices`.
fn declare(devices: &mut Vec<Device>, undeclared: &[(u64, PointerKind)]) {
    for &(device, kind) in undeclared {
        match devices.iter_mut().find(|entry| entry.device == device) {
            Some(entry) => entry.kinds.push(kind),
            None => devices.push(Device {
                device,
                kinds: vec![kind],
            }),
        }
    }
}

/// `value` as one line, its newline included.
fn encode_line(value: &impl Serialize) -> io::Result<Vec<u8>> {
    let mut line = serde_json::to_vec(value)?;
    line.push(b'\n');
    Ok(line)
}

/// Writes `line` and flushes it, so a recording cut short keeps it.
fn write_flushed(out: &mut impl Write, line: &[u8]) -> io::Result<()> {
    out.write_all(line)?;
    out.flush()
}

/// `error`, naming `path`.
fn at(path: &Path, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{}: {error}", path.display()))
}

/// One native recording read back: its header, with every device its rows read, and the lines
/// after it.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct NativeRecording {
    /// The header.
    pub header: Header,
    /// The lines after it, in order.
    pub lines: Vec<RecordedLine>,
}

impl NativeRecording {
    /// The rows, `gap` lines left out.
    pub fn rows(&self) -> impl Iterator<Item = &Row> {
        self.lines.iter().filter_map(|line| match line {
            RecordedLine::Row(row) => Some(&**row),
            RecordedLine::Gap { .. } => None,
        })
    }

    /// The rows the writer lost, as its `gap` lines count them.
    #[must_use]
    pub fn lost_rows(&self) -> u64 {
        self.lines
            .iter()
            .map(|line| match line {
                RecordedLine::Gap { rows } => *rows,
                RecordedLine::Row(_) => 0,
            })
            .fold(0, u64::saturating_add)
    }
}

/// A line of a native recording after its header.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum RecordedLine {
    /// A row.
    Row(Box<Row>),
    /// Rows the writer lost here: a replay stops at it.
    Gap {
        /// How many.
        rows: u64,
    },
}

/// The native recording at `path`, read through the protocol's schema: see the
/// [module docs](self).
///
/// # Errors
///
/// [`Error::Io`](crate::Error::Io) when the file cannot be read; [`Error::Unreadable`] naming the
/// line for a header of a version this driver does not read, a line past a bound, a line that is
/// neither a row nor a `gap`, and a row reading a device the header does not declare.
///
/// [`Error::Unreadable`]: crate::Error::Unreadable
pub fn read(path: &Path) -> crate::Result<NativeRecording> {
    use flui_protocol::native_recording::{
        FORMAT_VERSION, MAX_LINE_BYTES, OLDEST_READABLE_VERSION, VERSION_KEY,
    };

    use super::lines::{BoundedLines, LineRead};
    use crate::Error;

    let file = File::open(path)?;
    let mut lines = BoundedLines::new(io::BufReader::new(file), MAX_LINE_BYTES);
    let bad = |line: u64, message: String| Error::unreadable_line(path, line, message);
    let (header_line, header_len) = match lines.next_line()? {
        LineRead::Line { bytes, len, .. } => (bytes, len),
        LineRead::TooLong => {
            return Err(bad(
                1,
                format!("a header longer than {MAX_LINE_BYTES} bytes"),
            ));
        }
        LineRead::End => return Err(bad(1, "the file is empty".into())),
    };
    let value: serde_json::Value = serde_json::from_slice(&header_line)
        .map_err(|error| bad(1, format!("the header is not JSON: {error}")))?;
    let found = value
        .get(VERSION_KEY)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    if !(u64::from(OLDEST_READABLE_VERSION)..=u64::from(FORMAT_VERSION)).contains(&found) {
        return Err(bad(
            1,
            format!(
                "a native recording of version {found}; this driver reads versions \
                 {OLDEST_READABLE_VERSION} to {FORMAT_VERSION}"
            ),
        ));
    }
    let header: Header = serde_json::from_value(value)
        .map_err(|error| bad(1, format!("the header does not read: {error}")))?;
    let mut budget = Budget::after_header(header_len);
    let mut recorded = Vec::new();
    let mut number: u64 = 1;
    loop {
        number += 1;
        let (bytes, len) = match lines.next_line()? {
            LineRead::Line { bytes, len, .. } => (bytes, len),
            LineRead::TooLong => {
                return Err(bad(
                    number,
                    format!("a line longer than {MAX_LINE_BYTES} bytes"),
                ));
            }
            LineRead::End => break,
        };
        budget
            .read_line(len)
            .map_err(|overrun| bad(number, overrun.to_string()))?;
        if bytes.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let line = match serde_json::from_slice::<Row>(&bytes) {
            Ok(row) => RecordedLine::Row(Box::new(row)),
            Err(row_error) => match serde_json::from_slice::<Line>(&bytes) {
                Ok(Line::Gap { rows }) => RecordedLine::Gap { rows },
                _ => return Err(bad(number, format!("not a row: {row_error}"))),
            },
        };
        if let RecordedLine::Row(row) = &line
            && let Some((device, kind)) = undeclared(row, &header.devices).into_iter().next()
        {
            return Err(bad(
                number,
                format!(
                    "the row reads device {device} as {kind:?}, which the header does not declare"
                ),
            ));
        }
        recorded.push(line);
    }
    Ok(NativeRecording {
        header,
        lines: recorded,
    })
}
