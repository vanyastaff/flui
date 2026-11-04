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
//! `<dir>/index.json` names each file's stream, backend, what it journals, its rows and the
//! rows it lost.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use flui_protocol::native_recording::{Backend, Device, Header, Line, Reading, Row, Stream};
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
    /// Its header, once it arrived.
    header: Option<Header>,
    /// Every device its rows read, with the kinds they read it as, in the order first read.
    devices: Vec<Device>,
    /// Where its rows go until [`NativeFiles::finish`].
    part: BufWriter<File>,
    /// The rows written, `gap` lines aside.
    rows: u64,
    /// The rows it lost, as its `gap` lines count them, and the rows that came before its
    /// header.
    lost_rows: u64,
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
    /// Its rows, `gap` lines aside.
    pub(crate) rows: u64,
    /// The rows it lost.
    pub(crate) lost_rows: u64,
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
        let file = match self.streams.entry(stream) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                let path = self.dir.join(part_name(stream));
                let part = File::create(&path).map_err(|e| at(&path, e))?;
                entry.insert(StreamFile {
                    header: None,
                    devices: Vec::new(),
                    part: BufWriter::new(part),
                    rows: 0,
                    lost_rows: 0,
                })
            }
        };
        match line {
            // A stream sends its header once; a second one would restart it, which no stream
            // does, so the first stands.
            Line::Header(header) => {
                if file.header.is_none() {
                    file.header = Some((**header).clone());
                }
                Ok(())
            }
            Line::Row(_) | Line::Gap { .. } if file.header.is_none() => {
                file.lost_rows += match line {
                    Line::Gap { rows } => *rows,
                    _ => 1,
                };
                Ok(())
            }
            Line::Row(row) => {
                read_devices(row, &mut file.devices);
                file.rows += 1;
                let path = self.dir.join(part_name(stream));
                write_line(&mut file.part, &**row).map_err(|e| at(&path, e))
            }
            Line::Gap { rows } => {
                file.lost_rows += rows;
                let path = self.dir.join(part_name(stream));
                write_line(&mut file.part, line).map_err(|e| at(&path, e))
            }
            // A line this CLI does not know is not one it can place in the file.
            _ => {
                file.lost_rows += 1;
                Ok(())
            }
        }
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
                header,
                devices,
                part,
                rows,
                lost_rows,
            } = file;
            let part_path = self.dir.join(part_name(stream));
            part.into_inner()
                .map_err(io::IntoInnerError::into_error)
                .map_err(|e| at(&part_path, e))?;
            let Some(mut header) = header else {
                fs::remove_file(&part_path).map_err(|e| at(&part_path, e))?;
                written.push(WrittenStream {
                    stream,
                    file: None,
                    backend: None,
                    journals: None,
                    rows,
                    lost_rows,
                });
                continue;
            };
            header.devices = devices;
            let name = format!("{stream}.jsonl");
            let path = self.dir.join(&name);
            let mut out = BufWriter::new(File::create(&path).map_err(|e| at(&path, e))?);
            write_line(&mut out, &header).map_err(|e| at(&path, e))?;
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

/// The part file of `stream`.
fn part_name(stream: u32) -> String {
    format!("{stream}.rows.part")
}

/// Adds the devices `row`'s readings name to `devices`, with the kinds they read them as.
fn read_devices(row: &Row, devices: &mut Vec<Device>) {
    let readings: Vec<&Reading> = match row {
        Row::Reading { reading } => vec![reading],
        Row::Batch {
            coalesced,
            predicted,
            current,
        } => coalesced
            .iter()
            .chain(predicted)
            .chain(std::iter::once(current))
            .collect(),
        // The other rows read no device.
        _ => Vec::new(),
    };
    for reading in readings {
        let Some(device) = reading.device else {
            continue;
        };
        match devices.iter_mut().find(|entry| entry.device == device) {
            Some(entry) if entry.kinds.contains(&reading.kind) => {}
            Some(entry) => entry.kinds.push(reading.kind),
            None => devices.push(Device {
                device,
                kinds: vec![reading.kind],
            }),
        }
    }
}

/// Writes `value` as one line and flushes it.
fn write_line(out: &mut impl Write, value: &impl Serialize) -> io::Result<()> {
    serde_json::to_writer(&mut *out, value)?;
    out.write_all(b"\n")?;
    out.flush()
}

/// `error`, naming `path`.
fn at(path: &Path, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{}: {error}", path.display()))
}
