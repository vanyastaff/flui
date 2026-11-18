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
//! format's [`Budget`]: each row is admitted with the bytes the header grows by to declare the
//! devices it reads first, and the first row past [`MAX_ROWS`] or [`MAX_RECORDING_BYTES`] is
//! replaced by a `cut` row that ends the file, so a replay of it stops at the cut instead of the
//! reader refusing the whole file. A row longer than a line may be is lost in place of a `gap`.
//!
//! `<dir>/index.json` names each file's stream, backend, what it journals, its rows, the rows
//! it lost and where it was cut.
//!
//! [`MAX_ROWS`]: flui_protocol::native_recording::MAX_ROWS
//! [`MAX_RECORDING_BYTES`]: flui_protocol::native_recording::MAX_RECORDING_BYTES

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use flui_protocol::native_recording::{
    Backend, Budget, CutReason, Device, Header, Line, PointerKind, Reading, Row, Stream,
};
use serde::Serialize;

/// The native streams of one recording, as they are written.
#[derive(Debug)]
pub(crate) struct NativeFiles {
    dir: PathBuf,
    streams: BTreeMap<u32, StreamFile>,
}

/// One stream, as it is written.
#[derive(Debug)]
struct StreamFile {
    /// Its header and what the file has taken of its bounds, once the header arrived.
    opened: Option<Opened>,
    /// Every device its written rows read, with the kinds they read it as, in the order first
    /// read.
    devices: Vec<Device>,
    /// Where its rows go until [`NativeFiles::finish`].
    part: BufWriter<File>,
    /// The rows written, `gap` lines and the `cut` aside.
    rows: u64,
    /// The rows it lost before its cut: as its `gap` lines count them, the rows that came before
    /// its header, and rows too long to write.
    lost_rows: u64,
    /// Where it was cut, once it was.
    cut: Option<StreamCut>,
}

/// A stream's header, once it arrived, and the bounds its file has taken.
#[derive(Debug)]
struct Opened {
    /// The header, its devices left for [`NativeFiles::finish`] to fill in.
    header: Header,
    budget: Budget,
}

/// Where a stream's file ends at a `cut` row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) struct StreamCut {
    /// The bound the next row would have passed.
    pub(crate) reason: CutReason,
    /// The rows not written from there: the one the cut replaced and every row after it.
    pub(crate) dropped: u64,
}

/// One stream's file, as [`NativeFiles::finish`] wrote it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct WrittenStream {
    /// The stream's number, in the order the backend opened them.
    pub(crate) stream: u32,
    /// The file, relative to the directory; none for a stream whose header never came.
    pub(crate) file: Option<String>,
    /// The backend it was taken on, when its header came.
    pub(crate) backend: Option<Backend>,
    /// What it journals, when its header came.
    pub(crate) journals: Option<Stream>,
    /// Its rows, `gap` lines and the `cut` aside.
    pub(crate) rows: u64,
    /// The rows it lost before any cut.
    pub(crate) lost_rows: u64,
    /// Where its file ends at a `cut` row, when it does.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) cut: Option<StreamCut>,
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
    pub(crate) fn create(dir: &Path) -> io::Result<Self> {
        fs::create_dir_all(dir).map_err(|e| at(dir, e))?;
        Ok(Self {
            dir: dir.to_owned(),
            streams: BTreeMap::new(),
        })
    }

    /// The directory the streams go to.
    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }

    /// Takes `line` of `stream`.
    ///
    /// # Errors
    ///
    /// When the stream's part file cannot be created or written.
    pub(crate) fn take(&mut self, stream: u32, line: &Line) -> io::Result<()> {
        let path = self.dir.join(part_name(stream));
        let file = match self.streams.entry(stream) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                let part = File::create(&path).map_err(|e| at(&path, e))?;
                entry.insert(StreamFile {
                    opened: None,
                    devices: Vec::new(),
                    part: BufWriter::new(part),
                    rows: 0,
                    lost_rows: 0,
                    cut: None,
                })
            }
        };
        file.take(line).map_err(|e| at(&path, e))
    }

    /// Writes each stream's file and the index, removing the part files.
    ///
    /// # Errors
    ///
    /// The first file that cannot be read or written; the part files of the streams not yet
    /// written are kept.
    pub(crate) fn finish(self) -> io::Result<Vec<WrittenStream>> {
        let mut written = Vec::with_capacity(self.streams.len());
        for (stream, file) in self.streams {
            let StreamFile {
                opened,
                devices,
                part,
                rows,
                lost_rows,
                cut,
            } = file;
            let part_path = self.dir.join(part_name(stream));
            part.into_inner()
                .map_err(io::IntoInnerError::into_error)
                .map_err(|e| at(&part_path, e))?;
            let Some(Opened { mut header, .. }) = opened else {
                fs::remove_file(&part_path).map_err(|e| at(&part_path, e))?;
                written.push(WrittenStream {
                    stream,
                    file: None,
                    backend: None,
                    journals: None,
                    rows,
                    lost_rows,
                    cut,
                });
                continue;
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
            written.push(WrittenStream {
                stream,
                file: Some(name),
                backend: Some(header.backend),
                journals: Some(header.stream),
                rows,
                lost_rows,
                cut,
            });
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
    /// admit it.
    fn take(&mut self, line: &Line) -> io::Result<()> {
        let lost = match line {
            Line::Row(_) => 1,
            Line::Gap { rows } => *rows,
            _ => 0,
        };
        if let Some(cut) = &mut self.cut {
            // Nothing follows a cut.
            cut.dropped = cut.dropped.saturating_add(lost);
            return Ok(());
        }
        match line {
            // A stream sends its header once; a second one would restart it, which no stream
            // does, so the first stands.
            Line::Header(header) => {
                if self.opened.is_none() {
                    let mut header = (**header).clone();
                    header.devices.clear();
                    let budget = Budget::after_header(encode_line(&header)?.len());
                    self.opened = Some(Opened { header, budget });
                }
                Ok(())
            }
            Line::Row(_) | Line::Gap { .. } if self.opened.is_none() => {
                self.lost_rows = self.lost_rows.saturating_add(lost);
                Ok(())
            }
            Line::Row(row) => self.write_row(row),
            Line::Gap { .. } => {
                self.lost_rows = self.lost_rows.saturating_add(lost);
                self.write_gap(line)
            }
            // A line this CLI does not know is not one it can place in the file.
            _ => {
                self.lost_rows = self.lost_rows.saturating_add(1);
                Ok(())
            }
        }
    }

    /// Writes `row` and declares its devices, or cuts the file where it is full, or writes a
    /// `gap` in place of a row no file can hold.
    fn write_row(&mut self, row: &Row) -> io::Result<()> {
        let encoded = encode_line(row)?;
        let undeclared = undeclared(row, &self.devices);
        let declared = declared_bytes(&undeclared, &self.devices)?;
        let budget = &mut self.opened_mut().budget;
        let Err(overrun) = budget.admit_row(encoded.len(), declared, true) else {
            write_flushed(&mut self.part, &encoded)?;
            declare(&mut self.devices, &undeclared);
            self.rows += 1;
            return Ok(());
        };
        if let Some(reason) = overrun.cut_reason() {
            return self.write_cut(reason);
        }
        self.lost_rows = self.lost_rows.saturating_add(1);
        self.write_gap(&Line::Gap { rows: 1 })
    }

    /// Writes `gap`, or cuts the file where it is full.
    fn write_gap(&mut self, gap: &Line) -> io::Result<()> {
        let encoded = encode_line(gap)?;
        match self.opened_mut().budget.admit_row(encoded.len(), 0, true) {
            Ok(()) => write_flushed(&mut self.part, &encoded),
            Err(overrun) => match overrun.cut_reason() {
                Some(reason) => self.write_cut(reason),
                // A header past a line's length, which no line after it fixes: the rows are
                // counted lost all the same.
                None => Ok(()),
            },
        }
    }

    /// Ends the file with a `cut` row naming `reason`, in place of the row it could not take.
    /// The budget keeps room for it after every row it admits.
    fn write_cut(&mut self, reason: CutReason) -> io::Result<()> {
        self.cut = Some(StreamCut { reason, dropped: 1 });
        write_flushed(&mut self.part, &encode_line(&Row::Cut { reason })?)
    }

    fn opened_mut(&mut self) -> &mut Opened {
        self.opened
            .as_mut()
            .expect("BUG: a stream writes rows only once its header opened it")
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

#[cfg(test)]
mod tests {
    use std::io::BufRead as _;

    use flui_protocol::native_recording::{
        Backend, CutReason, Header, Ime, ImeEvent, Line, MAX_LINE_BYTES, MAX_RECORDING_BYTES,
        MAX_ROWS, Row, Stream,
    };

    use super::{NativeFiles, StreamCut};

    fn header(stream: Stream) -> Line {
        Line::Header(Box::new(Header {
            flui_native_recording: 3,
            backend: Backend::X11,
            stream,
            epoch_ns: None,
            uptime_ns: None,
            counter: None,
            reserved_contact: None,
            first_pointer: None,
            first_device: None,
            click_settings: None,
            provenance: None,
            joined_late: false,
            devices: Vec::new(),
        }))
    }

    fn commit(text: String) -> Line {
        Line::Row(Box::new(Row::Ime {
            ime: Ime::Event(ImeEvent::Commit(text)),
            redacted: false,
        }))
    }

    /// The lines of `file` in `dir`, each with its newline.
    fn lines(dir: &std::path::Path, file: &str) -> Vec<String> {
        let reader = std::io::BufReader::new(std::fs::File::open(dir.join(file)).unwrap());
        reader
            .split(b'\n')
            .map(|line| String::from_utf8(line.unwrap()).unwrap() + "\n")
            .collect()
    }

    /// A stream longer than a native recording may be is written up to the last row that fits
    /// with a `cut` row after it, so the file stays within what its reader takes; the rows past
    /// the cut are counted, not written.
    #[test]
    fn a_stream_past_the_byte_bound_ends_at_a_cut() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut files = NativeFiles::create(tmp.path()).unwrap();
        files.take(0, &header(Stream::Window)).unwrap();
        let text = "x".repeat(MAX_LINE_BYTES - 128);
        let sent = MAX_RECORDING_BYTES / text.len() + 3;
        for _ in 0..sent {
            files.take(0, &commit(text.clone())).unwrap();
        }
        let written = files.finish().unwrap();

        let lines = lines(tmp.path(), "0.jsonl");
        let bytes: usize = lines.iter().map(String::len).sum();
        assert!(bytes <= MAX_RECORDING_BYTES, "{bytes} bytes");
        let last = lines.last().unwrap();
        assert!(
            matches!(
                serde_json::from_str::<Row>(last),
                Ok(Row::Cut {
                    reason: CutReason::Bytes
                })
            ),
            "the last line, {} bytes, is not a byte cut",
            last.len()
        );
        let kept = lines.len() as u64 - 2;
        assert_eq!(written[0].rows, kept);
        assert_eq!(
            written[0].cut,
            Some(StreamCut {
                reason: CutReason::Bytes,
                dropped: sent as u64 - kept,
            })
        );
        assert_eq!(written[0].lost_rows, 0);
    }

    /// A stream with more rows than a native recording holds ends at a `cut` row that keeps the
    /// file within the row bound.
    #[test]
    fn a_stream_past_the_row_bound_ends_at_a_cut() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut files = NativeFiles::create(tmp.path()).unwrap();
        files.take(0, &header(Stream::App)).unwrap();
        let reopen = Line::Row(Box::new(Row::Reopen));
        for _ in 0..=MAX_ROWS {
            files.take(0, &reopen).unwrap();
        }
        let written = files.finish().unwrap();

        let lines = lines(tmp.path(), "0.jsonl");
        assert_eq!(lines.len() - 1, MAX_ROWS, "the rows, the cut last");
        let last: Row = serde_json::from_str(lines.last().unwrap()).unwrap();
        assert_eq!(
            last,
            Row::Cut {
                reason: CutReason::Rows
            }
        );
        assert_eq!(
            written[0].cut,
            Some(StreamCut {
                reason: CutReason::Rows,
                dropped: 2,
            })
        );
    }

    /// A row longer than a line may be is lost in place of a `gap`, and the rows after it are
    /// written.
    #[test]
    fn a_row_past_the_line_bound_leaves_a_gap() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut files = NativeFiles::create(tmp.path()).unwrap();
        files.take(0, &header(Stream::Window)).unwrap();
        files.take(0, &commit("x".repeat(MAX_LINE_BYTES))).unwrap();
        files.take(0, &commit("after".to_owned())).unwrap();
        let written = files.finish().unwrap();

        let lines = lines(tmp.path(), "0.jsonl");
        let sizes: Vec<usize> = lines.iter().map(String::len).collect();
        assert_eq!(sizes.len(), 3, "line sizes {sizes:?}");
        assert!(
            lines[1] == "{\"gap\":{\"rows\":1}}\n",
            "line 2, {} bytes, is not a gap of one row",
            sizes[1]
        );
        assert_eq!(written[0].rows, 1);
        assert_eq!(written[0].lost_rows, 1);
        assert_eq!(written[0].cut, None);
    }
}
