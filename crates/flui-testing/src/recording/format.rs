//! JSON Lines framing (ADR-0215 §3): one header object, then one object per
//! row, each followed by a newline.

use std::io::{BufRead, Write};

use flui_platform_api::WindowId;
use flui_platform_api::pointer::{DeviceId, PointerKind};
use flui_runtime::InputRecord;
use serde::{Deserialize, Serialize};

use super::dto::{DeviceDto, DtoError, PointerKindDto, RowDto};

/// The recording format's major version. It changes only with an
/// incompatible change; unknown fields are skipped within one version.
pub const FORMAT_VERSION: u32 = 1;

/// Why a recording could not be written, read or replayed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RecordingError {
    /// Reading or writing the underlying stream failed.
    #[error("recording I/O failed: {0}")]
    Io(#[from] std::io::Error),
    /// Line `line` is not a well-formed recording object.
    #[error("line {line} of the recording is malformed: {source}")]
    Json {
        /// The 1-based line, the header being line 1.
        line: u64,
        /// What the parser refused.
        source: serde_json::Error,
    },
    /// The recording's major version is not one this reader supports.
    #[error("recording format version {found} is not supported (this reader reads {supported})")]
    Version {
        /// The header's version.
        found: u32,
        /// The version this reader reads.
        supported: u32,
    },
    /// Line `line` names a variant this reader does not know: an error,
    /// never a skipped row.
    #[error("line {line} names an unknown variant `{tag}`")]
    UnknownVariant {
        /// The 1-based line.
        line: u64,
        /// The unknown spelling.
        tag: String,
    },
    /// Line `line` is a row replay cannot reproduce: a redacted row, a `Gap`
    /// marker, or the end of a stream that has no `End` row.
    #[error("line {line} cannot be replayed")]
    NonReplayable {
        /// The 1-based line.
        line: u64,
    },
    /// Finishing the recording did not deliver its final `Gap` or `End` row.
    #[error("the recording is incomplete: {lost_rows} rows were lost")]
    Incomplete {
        /// Every row the tap lost.
        lost_rows: u64,
    },
}

/// Whether text is kept in a recording (ADR-0215 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Privacy {
    /// Key text, IME strings, assistive action text, window titles and device
    /// names are replaced, and each row that lost text is marked redacted.
    #[default]
    Redact,
    /// Keep all text: the one explicit opt-in per recording, the
    /// `--keep-text` of a recording tool.
    KeepText,
}

/// Which clock the recorded times count on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RecordingClock {
    /// A virtual clock a test advanced; times count from its start.
    #[serde(rename = "virtual")]
    Virtual,
    /// The process's monotonic event timeline.
    #[serde(rename = "monotonic")]
    Monotonic,
}

/// A window the recording addresses, with the scale it started at.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct RecordedWindow {
    /// The window.
    pub window: WindowId,
    /// Device pixels per logical pixel, possibly fractional.
    pub scale_factor: f64,
    /// The window's title, when known; redacted unless text is kept.
    pub title: Option<String>,
}

/// A pointer device the recording saw.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct RecordedDevice {
    /// The device.
    pub device: DeviceId,
    /// Its kind.
    pub kind: PointerKind,
    /// Its name, when known; redacted unless text is kept.
    pub name: Option<String>,
}

/// What a recording holds besides its rows: the clock, the windows and
/// devices, and the behavior values in effect.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct RecordingHeader {
    /// The clock the times count on.
    pub clock: RecordingClock,
    /// The windows, with their starting scale factors.
    pub windows: Vec<RecordedWindow>,
    /// The pointer devices.
    pub devices: Vec<RecordedDevice>,
    /// The gesture preset in effect, by name.
    pub gesture_preset: Option<String>,
    /// The system-preference revision in effect when recording began.
    pub preferences_revision: Option<u64>,
    /// Whether the recording keeps text.
    pub privacy: Privacy,
}

impl RecordingHeader {
    /// A header for a recording on `clock`, with no windows or devices.
    #[must_use]
    pub fn new(clock: RecordingClock) -> Self {
        Self {
            clock,
            windows: Vec::new(),
            devices: Vec::new(),
            gesture_preset: None,
            preferences_revision: None,
            privacy: Privacy::Redact,
        }
    }

    /// This header with `window` at `scale_factor`.
    #[must_use]
    pub fn with_window(
        mut self,
        window: WindowId,
        scale_factor: f64,
        title: Option<String>,
    ) -> Self {
        self.windows.push(RecordedWindow {
            window,
            scale_factor,
            title,
        });
        self
    }

    /// This header with `device` of `kind`.
    #[must_use]
    pub fn with_device(
        mut self,
        device: DeviceId,
        kind: PointerKind,
        name: Option<String>,
    ) -> Self {
        self.devices.push(RecordedDevice { device, kind, name });
        self
    }

    /// This header naming the gesture preset in effect.
    #[must_use]
    pub fn with_gesture_preset(mut self, preset: impl Into<String>) -> Self {
        self.gesture_preset = Some(preset.into());
        self
    }

    /// This header naming the preference revision in effect.
    #[must_use]
    pub fn with_preferences_revision(mut self, revision: u64) -> Self {
        self.preferences_revision = Some(revision);
        self
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct WindowDto {
    window: u64,
    scale_factor: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    title: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct HeaderDto {
    flui_recording: u32,
    clock: RecordingClock,
    #[serde(default)]
    windows: Vec<WindowDto>,
    #[serde(default)]
    devices: Vec<DeviceDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gesture_preset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    preferences_revision: Option<u64>,
    #[serde(default)]
    keep_text: bool,
}

const REDACTED_NAME: &str = super::dto::REDACTED;

impl HeaderDto {
    fn new(header: &RecordingHeader, privacy: Privacy) -> Self {
        let keep = privacy == Privacy::KeepText;
        let text = |text: &Option<String>| {
            text.as_ref().map(|text| {
                if keep {
                    text.clone()
                } else {
                    REDACTED_NAME.to_owned()
                }
            })
        };
        Self {
            flui_recording: FORMAT_VERSION,
            clock: header.clock,
            windows: header
                .windows
                .iter()
                .map(|window| WindowDto {
                    window: window.window.0,
                    scale_factor: window.scale_factor,
                    title: text(&window.title),
                })
                .collect(),
            devices: header
                .devices
                .iter()
                .map(|device| DeviceDto {
                    device: device.device.get().get(),
                    kind: PointerKindDto::from(device.kind),
                    name: text(&device.name),
                })
                .collect(),
            gesture_preset: header.gesture_preset.clone(),
            preferences_revision: header.preferences_revision,
            keep_text: keep,
        }
    }

    fn into_header(self, line: u64) -> Result<RecordingHeader, RecordingError> {
        let devices = self
            .devices
            .iter()
            .map(|dto| {
                super::dto::device_from(dto)
                    .map(|(device, kind)| RecordedDevice {
                        device,
                        kind,
                        name: dto.name.clone(),
                    })
                    .map_err(|error| dto_error(line, error))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(RecordingHeader {
            clock: self.clock,
            windows: self
                .windows
                .into_iter()
                .map(|window| RecordedWindow {
                    window: WindowId(window.window),
                    scale_factor: window.scale_factor,
                    title: window.title,
                })
                .collect(),
            devices,
            gesture_preset: self.gesture_preset,
            preferences_revision: self.preferences_revision,
            privacy: if self.keep_text {
                Privacy::KeepText
            } else {
                Privacy::Redact
            },
        })
    }
}

/// One row read back, with where it was and whether text was redacted.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Record {
    /// The 1-based line, the header being line 1.
    pub line: u64,
    /// The row. A redacted row carries placeholder text.
    pub record: InputRecord,
    /// Whether the row lost text when it was written.
    pub redacted: bool,
}

/// Writes a recording to any [`Write`]: a file, a buffer, or on wasm, any
/// sink with no file system behind it.
#[derive(Debug)]
pub struct RecordingWriter<W: Write> {
    out: W,
    privacy: Privacy,
    line: u64,
}

impl<W: Write> RecordingWriter<W> {
    /// Start a recording on `out` with `header`, keeping text only when
    /// `header.privacy` says so.
    ///
    /// # Errors
    ///
    /// [`RecordingError::Io`] or [`RecordingError::Json`] when the header
    /// cannot be written.
    pub fn new(mut out: W, header: &RecordingHeader) -> Result<Self, RecordingError> {
        let privacy = header.privacy;
        write_line(&mut out, 1, &HeaderDto::new(header, privacy))?;
        Ok(Self {
            out,
            privacy,
            line: 1,
        })
    }

    /// Append `record`.
    ///
    /// # Errors
    ///
    /// [`RecordingError::Io`] or [`RecordingError::Json`] when the row cannot
    /// be written.
    pub fn write(&mut self, record: &InputRecord) -> Result<(), RecordingError> {
        self.write_row(self.prepare(record))
    }

    /// The owned row `record` is written as, text already redacted.
    pub(crate) fn prepare(&self, record: &InputRecord) -> RowDto {
        prepare(record, self.privacy)
    }

    pub(crate) fn privacy(&self) -> Privacy {
        self.privacy
    }

    pub(crate) fn write_row(&mut self, row: RowDto) -> Result<(), RecordingError> {
        self.line += 1;
        write_line(&mut self.out, self.line, &row)
    }

    /// Flush and return the sink.
    ///
    /// # Errors
    ///
    /// [`RecordingError::Io`] when flushing fails.
    pub fn finish(mut self) -> Result<W, RecordingError> {
        self.out.flush()?;
        Ok(self.out)
    }
}

/// `record` as written under `privacy`.
pub(crate) fn prepare(record: &InputRecord, privacy: Privacy) -> RowDto {
    let mut row = RowDto::from(record);
    if privacy == Privacy::Redact {
        row.redact();
    }
    row
}

fn write_line(
    out: &mut impl Write,
    line: u64,
    value: &impl Serialize,
) -> Result<(), RecordingError> {
    serde_json::to_writer(&mut *out, value).map_err(|source| {
        if source.is_io() {
            RecordingError::Io(source.into())
        } else {
            RecordingError::Json { line, source }
        }
    })?;
    out.write_all(b"\n")?;
    Ok(())
}

/// Reads a recording: the header on construction, then each row as an
/// iterator item. A row with an unknown variant is an error, never skipped.
#[derive(Debug)]
pub struct RecordingReader<R: BufRead> {
    input: R,
    header: RecordingHeader,
    line: u64,
    buffer: String,
}

impl<R: BufRead> RecordingReader<R> {
    /// Read the header of the recording on `input`.
    ///
    /// # Errors
    ///
    /// [`RecordingError::Version`] for another major version, and
    /// [`RecordingError::Io`] or [`RecordingError::Json`] for a missing or
    /// malformed header.
    pub fn new(mut input: R) -> Result<Self, RecordingError> {
        let mut buffer = String::new();
        input.read_line(&mut buffer)?;
        let value: serde_json::Value = serde_json::from_str(&buffer)
            .map_err(|source| RecordingError::Json { line: 1, source })?;
        let found = value
            .get("flui_recording")
            .and_then(serde_json::Value::as_u64)
            .and_then(|version| u32::try_from(version).ok())
            .unwrap_or(0);
        if found != FORMAT_VERSION {
            return Err(RecordingError::Version {
                found,
                supported: FORMAT_VERSION,
            });
        }
        let header = serde_json::from_value::<HeaderDto>(value)
            .map_err(|source| json_error(1, source))?
            .into_header(1)?;
        Ok(Self {
            input,
            header,
            line: 1,
            buffer,
        })
    }

    /// The recording's header.
    #[must_use]
    pub fn header(&self) -> &RecordingHeader {
        &self.header
    }

    /// The line the next row will be read from.
    pub(crate) fn next_line(&self) -> u64 {
        self.line + 1
    }
}

impl<R: BufRead> Iterator for RecordingReader<R> {
    type Item = Result<Record, RecordingError>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            self.buffer.clear();
            match self.input.read_line(&mut self.buffer) {
                Ok(0) => return None,
                Ok(_) => {}
                Err(error) => return Some(Err(error.into())),
            }
            self.line += 1;
            if self.buffer.trim().is_empty() {
                continue;
            }
            let line = self.line;
            return Some(
                serde_json::from_str::<RowDto>(&self.buffer)
                    .map_err(|source| json_error(line, source))
                    .and_then(|row| {
                        let redacted = row.is_redacted();
                        InputRecord::try_from(row)
                            .map(|record| Record {
                                line,
                                record,
                                redacted,
                            })
                            .map_err(|error| dto_error(line, error))
                    }),
            );
        }
    }
}

/// A parse failure, with serde's unknown-variant refusal typed.
fn json_error(line: u64, source: serde_json::Error) -> RecordingError {
    let message = source.to_string();
    if let Some(rest) = message.strip_prefix("unknown variant `")
        && let Some((tag, _)) = rest.split_once('`')
    {
        return RecordingError::UnknownVariant {
            line,
            tag: tag.to_owned(),
        };
    }
    RecordingError::Json { line, source }
}

fn dto_error(line: u64, error: DtoError) -> RecordingError {
    match error {
        DtoError::UnknownSpelling(tag) => RecordingError::UnknownVariant { line, tag },
        DtoError::Invalid(what) => RecordingError::Json {
            line,
            source: <serde_json::Error as serde::de::Error>::custom(what),
        },
    }
}
