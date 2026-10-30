//! JSON Lines framing (ADR-0215 §3): one header object, then one object per
//! row, each followed by a newline.

use std::io::{BufRead, Write};

use flui_interaction::GestureProfiles;
use flui_platform::WindowId;
use flui_platform::pointer::{DeviceId, PointerKind};
use flui_runtime::{InputRecord, PreferencesRow};
use serde::{Deserialize, Serialize};

use super::dto::{
    DeviceDto, DtoError, PointerKindDto, PreferencesDto, ProfileDto, RowDto, profiles, profiles_dto,
};

/// The recording format's major version. It changes only with an
/// incompatible change; unknown fields are skipped within one version.
pub const FORMAT_VERSION: u32 = 2;

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
    /// The value for line `line` holds a variant of `what` this format does
    /// not mirror yet: the vocabulary grew, and the recording DTO and its
    /// generated variant table have to follow.
    #[error("line {line} holds a {what} variant this recording format does not mirror")]
    Unmirrored {
        /// The 1-based line the value was to be written to.
        line: u64,
        /// The vocabulary type that grew.
        what: &'static str,
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

/// A window the recording addresses, with the scale it started at and the
/// gesture presets its contacts were admitted with.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct RecordedWindow {
    /// The window.
    pub window: WindowId,
    /// Device pixels per logical pixel, possibly fractional.
    pub scale_factor: f64,
    /// The window's title, when known; redacted unless text is kept.
    pub title: Option<String>,
    /// The per-kind gesture values the host's preferences resolved over
    /// (ADR-0212 §4), as values, never a preset's name; `None` when the
    /// recorder did not know them.
    pub gesture_profiles: Option<GestureProfiles>,
}

impl RecordedWindow {
    /// `window` at `scale_factor`, untitled, its presets unknown.
    #[must_use]
    pub fn new(window: WindowId, scale_factor: f64) -> Self {
        Self {
            window,
            scale_factor,
            title: None,
            gesture_profiles: None,
        }
    }

    /// This window titled `title`.
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// This window admitting contacts with `profiles`.
    #[must_use]
    pub fn with_gesture_profiles(mut self, profiles: GestureProfiles) -> Self {
        self.gesture_profiles = Some(profiles);
        self
    }
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

/// What a recording holds besides its rows: the clock, the windows with
/// their scale factors and gesture values, the devices, and the
/// system-preference revision in effect. A replay restores the windows'
/// gesture values and the preferences before its first row.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct RecordingHeader {
    /// The clock the times count on.
    pub clock: RecordingClock,
    /// The windows, with their starting scale factors and gesture values.
    pub windows: Vec<RecordedWindow>,
    /// The pointer devices.
    pub devices: Vec<RecordedDevice>,
    /// The system-preference revision in effect when recording began, with
    /// its values.
    pub preferences: Option<PreferencesRow>,
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
            preferences: None,
            privacy: Privacy::Redact,
        }
    }

    /// A header on `clock` for a recording that starts from `start`: its
    /// windows with their scales and gesture values, and its preference
    /// revision. A runner's factory tap and a headless host build their
    /// headers this way, from the facts the runtime reports.
    #[must_use]
    pub fn starting(clock: RecordingClock, start: &flui_runtime::RecordingStart) -> Self {
        let mut header = Self::new(clock);
        for window in &start.windows {
            header = header.with_window(
                RecordedWindow::new(window.window, window.scale_factor)
                    .with_gesture_profiles(window.gesture_profiles.clone()),
            );
        }
        header.preferences.clone_from(&start.preferences);
        header
    }

    /// This header with `window`.
    #[must_use]
    pub fn with_window(mut self, window: RecordedWindow) -> Self {
        self.windows.push(window);
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

    /// This header with the system-preference revision `preferences` in
    /// effect.
    #[must_use]
    pub fn with_preferences(mut self, preferences: PreferencesRow) -> Self {
        self.preferences = Some(preferences);
        self
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct WindowDto {
    window: u64,
    scale_factor: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gestures: Option<Vec<ProfileDto>>,
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
    preferences: Option<PreferencesDto>,
    #[serde(default)]
    keep_text: bool,
}

const REDACTED_NAME: &str = super::dto::REDACTED;

impl HeaderDto {
    fn try_new(header: &RecordingHeader, privacy: Privacy) -> Result<Self, DtoError> {
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
        Ok(Self {
            flui_recording: FORMAT_VERSION,
            clock: header.clock,
            windows: header
                .windows
                .iter()
                .map(|window| {
                    Ok(WindowDto {
                        window: window.window.0,
                        scale_factor: window.scale_factor,
                        title: text(&window.title),
                        gestures: window
                            .gesture_profiles
                            .as_ref()
                            .map(profiles_dto)
                            .transpose()?,
                    })
                })
                .collect::<Result<_, DtoError>>()?,
            devices: header
                .devices
                .iter()
                .map(|device| {
                    Ok(DeviceDto {
                        device: device.device.get().get(),
                        kind: PointerKindDto::try_from(device.kind)?,
                        name: text(&device.name),
                    })
                })
                .collect::<Result<_, DtoError>>()?,
            preferences: header
                .preferences
                .as_ref()
                .map(PreferencesDto::try_from)
                .transpose()?,
            keep_text: keep,
        })
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
                .map(|window| {
                    Ok(RecordedWindow {
                        window: WindowId(window.window),
                        scale_factor: window.scale_factor,
                        title: window.title,
                        gesture_profiles: window.gestures.map(profiles).transpose()?,
                    })
                })
                .collect::<Result<_, DtoError>>()
                .map_err(|error| dto_error(line, error))?,
            devices,
            preferences: self
                .preferences
                .map(PreferencesRow::try_from)
                .transpose()
                .map_err(|error| dto_error(line, error))?,
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
    pub fn try_new(mut out: W, header: &RecordingHeader) -> Result<Self, RecordingError> {
        let privacy = header.privacy;
        let dto = HeaderDto::try_new(header, privacy).map_err(|error| dto_error(1, error))?;
        write_line(&mut out, 1, &dto)?;
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
        self.write_row(prepare(record, self.privacy))
    }

    pub(crate) fn privacy(&self) -> Privacy {
        self.privacy
    }

    /// Write a prepared row on the next line; a row that could not be
    /// prepared fails with the line it was to be written to.
    pub(crate) fn write_row(&mut self, row: PreparedRow) -> Result<(), RecordingError> {
        self.line += 1;
        let row = row.map_err(|error| dto_error(self.line, error))?;
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

/// A row converted for writing, or why it could not be.
pub(crate) type PreparedRow = Result<RowDto, DtoError>;

/// `record` as written under `privacy`.
pub(crate) fn prepare(record: &InputRecord, privacy: Privacy) -> PreparedRow {
    let mut row = RowDto::try_from(record)?;
    if privacy == Privacy::Redact {
        row.redact();
    }
    Ok(row)
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
    pub fn try_new(mut input: R) -> Result<Self, RecordingError> {
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
        DtoError::Unmirrored(what) => RecordingError::Unmirrored { line, what },
    }
}
