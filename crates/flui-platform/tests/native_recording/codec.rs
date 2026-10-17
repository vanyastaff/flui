//! The native recording codec (ADR-0215 §4).
//!
//! A native recording is the journal of one backend's [`ContactRegistry`]:
//! a header with the backend, its clock anchors (epoch, uptime and the
//! performance counter's count and frequency) and the registry's setup, then
//! one row per registry call — a reading, a coalesced batch, a device
//! hot-plug, a capture loss. Rows hold plain [`NativeReading`]s, so a Win32 or
//! Apple recording replays through the shared decoder on any host.
//!
//! The framing is the `HostInput` recording's: JSON Lines, one header object
//! and one object per row, a major version in the header, unknown fields
//! skipped and an unknown variant a typed error, never a skipped row.

use std::io::{BufRead, Write};
use std::num::{NonZeroU8, NonZeroU64};
use std::time::Duration;

use flui_platform::shared::direct_manipulation::{
    ContentTransform, TouchpadAssembler, TouchpadFrame, ViewportReading, ViewportStatus,
};
use flui_platform::shared::pointer_assembly::{
    ClickSettings, ContactRegistry, CounterAnchor, DecodeError, GestureComponent, GestureDelta,
    GesturePhase, GestureReading, MoveBatch, NativeCancel, NativeClock, NativeContact,
    NativeDevice, NativeReading, NativeSurface, NativeTime, PenAngles, QpcTicks, RawPoint,
    RawSensors, ReadingAction, ReadingTime, WheelAxis, WheelReading, decode,
};
use flui_platform::traits::PlatformInput;
use flui_platform_api::EventTime;
use flui_platform_api::keyboard::Modifiers;
use flui_platform_api::pointer::{
    PenTool, PointerButton, PointerButtons, PointerEvent, PointerKind,
};
use serde::{Deserialize, Serialize};

/// The native recording format's major version.
pub const FORMAT_VERSION: u32 = 1;

/// Why a native recording could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    /// Reading or writing the stream failed.
    #[error("native recording I/O failed: {0}")]
    Io(#[from] std::io::Error),
    /// Line `line` is not a well-formed object.
    #[error("line {line} of the native recording is malformed: {source}")]
    Json {
        /// The 1-based line, the header being line 1.
        line: u64,
        /// What the parser refused.
        source: serde_json::Error,
    },
    /// The header's major version is not this reader's.
    #[error("native recording version {found} is not supported (this reader reads {supported})")]
    Version {
        /// The header's version.
        found: u32,
        /// The version this reader reads.
        supported: u32,
    },
    /// Line `line` names a variant this reader does not know.
    #[error("line {line} names an unknown variant `{tag}`")]
    UnknownVariant {
        /// The 1-based line.
        line: u64,
        /// The unknown spelling.
        tag: String,
    },
    /// Line `line` holds a value outside its range (a zero frequency, a
    /// button number no button has).
    #[error("line {line} holds an invalid value: {what}")]
    Invalid {
        /// The 1-based line.
        line: u64,
        /// What was refused.
        what: String,
    },
    /// Line `line` reads a device the header does not declare with that kind.
    #[error("line {line} reads device {device} as a kind the header does not declare")]
    UndeclaredDevice {
        /// The 1-based line.
        line: u64,
        /// The device label.
        device: u64,
    },
    /// The value for line `line` has a variant this format does not mirror:
    /// the reading vocabulary grew and the codec has to follow.
    #[error("line {line} holds a {what} variant this native format does not mirror")]
    Unmirrored {
        /// The 1-based line the value was to be written to.
        line: u64,
        /// The type that grew.
        what: &'static str,
    },
}

/// The backend a recording was taken on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Backend {
    /// Win32 `WM_POINTER` and the legacy mouse route.
    #[serde(rename = "win32")]
    Win32,
    /// AppKit `NSEvent`s.
    #[serde(rename = "appkit")]
    AppKit,
    /// UIKit `UITouch`es.
    #[serde(rename = "uikit")]
    UiKit,
    /// Android `MotionEvent`s.
    #[serde(rename = "android")]
    Android,
    /// The winit fallback.
    #[serde(rename = "winit")]
    Winit,
    /// DOM `PointerEvent`s.
    #[serde(rename = "web")]
    Web,
    /// The headless backend.
    #[serde(rename = "headless")]
    Headless,
}

/// The clock and setup a registry started with.
#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    /// The backend.
    pub backend: Backend,
    /// The epoch instant.
    pub epoch: EventTime,
    /// System uptime at the epoch.
    pub uptime: Duration,
    /// The performance counter at the epoch, when the host has one.
    pub counter: Option<CounterAnchor>,
    /// The contact given pointer identity one, when the backend reserves one.
    pub reserved_contact: Option<NativeContact>,
    /// The first pointer identity, when not one.
    pub first_pointer: Option<NonZeroU64>,
    /// The first device identity, when not the registry's default.
    pub first_device: Option<NonZeroU64>,
    /// The OS click thresholds the registry started with.
    pub click_settings: Option<ClickSettings>,
    /// Where the readings came from, when not a capture: a hand-written
    /// recording says so here.
    pub provenance: Option<String>,
}

impl Header {
    /// A header for `backend` with the clock anchors `epoch`, `uptime` and
    /// `counter`, and a default registry.
    pub const fn new(
        backend: Backend,
        epoch: EventTime,
        uptime: Duration,
        counter: Option<CounterAnchor>,
    ) -> Self {
        Self {
            backend,
            epoch,
            uptime,
            counter,
            reserved_contact: None,
            first_pointer: None,
            first_device: None,
            click_settings: None,
            provenance: None,
        }
    }

    /// The registry this header describes.
    fn registry(&self) -> ContactRegistry {
        let mut registry =
            ContactRegistry::new(NativeClock::new(self.epoch, self.uptime, self.counter));
        if let Some(contact) = self.reserved_contact {
            registry = registry.with_reserved_contact(contact);
        }
        if let Some(first) = self.first_pointer {
            registry = registry.with_first_pointer(first);
        }
        if let Some(first) = self.first_device {
            registry = registry.with_first_device(first);
        }
        registry.set_click_settings(self.click_settings);
        registry
    }
}

/// One registry call.
#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    /// `decode(reading)`.
    Reading(NativeReading),
    /// A [`MoveBatch`]: the coalesced readings oldest first, the predicted
    /// ones, and the current reading they attach to.
    Batch {
        /// Readings coalesced before `current`.
        coalesced: Vec<NativeReading>,
        /// Readings predicted after `current`.
        predicted: Vec<NativeReading>,
        /// The reading the batch decodes as.
        current: NativeReading,
    },
    /// The OS click thresholds changed.
    ClickSettings(Option<ClickSettings>),
    /// A precision touchpad's viewport reported a status or a content
    /// transform (Win32 DirectManipulation), at `frame`. Replays through the
    /// shared [`TouchpadAssembler`] on the same registry: its scrolls are
    /// phased `Scroll`s and its pinches are gesture readings.
    Viewport {
        /// What the viewport reported.
        reading: ViewportReading,
        /// Where and when.
        frame: TouchpadFrame,
    },
    /// The touchpad's open manipulation ended as cancelled at `frame`: the
    /// window or its viewport went away, or a size-and-move loop paused it.
    ViewportAbandoned {
        /// Where and when.
        frame: TouchpadFrame,
    },
    /// A mouse wheel turned under the primary mouse.
    Wheel(WheelReading),
    /// A device was announced.
    AddDevice {
        /// Its label.
        device: NativeDevice,
        /// Its kind.
        kind: PointerKind,
        /// When.
        time: EventTime,
    },
    /// A device was removed.
    RemoveDevice {
        /// Its label.
        device: NativeDevice,
        /// When.
        time: EventTime,
    },
    /// Every contact on a surface ended.
    CancelSurface {
        /// The surface.
        surface: NativeSurface,
        /// When.
        time: EventTime,
        /// Why.
        reason: NativeCancel,
    },
    /// The contacts with one label ended (a capture loss).
    CancelContact {
        /// The surface.
        surface: NativeSurface,
        /// The label.
        contact: NativeContact,
        /// When.
        time: EventTime,
        /// Why.
        reason: NativeCancel,
    },
}

/// A header and its rows.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeRecording {
    /// The registry's clock and setup.
    pub header: Header,
    /// The registry calls, in order.
    pub rows: Vec<Row>,
}

impl NativeRecording {
    /// Read a whole recording.
    ///
    /// # Errors
    ///
    /// [`CodecError`] for the first line that cannot be read.
    pub fn read(mut input: impl BufRead) -> Result<Self, CodecError> {
        let mut buffer = String::new();
        input.read_line(&mut buffer)?;
        let value: serde_json::Value =
            serde_json::from_str(&buffer).map_err(|source| CodecError::Json { line: 1, source })?;
        let found = value
            .get("flui_native_recording")
            .and_then(serde_json::Value::as_u64)
            .and_then(|version| u32::try_from(version).ok())
            .unwrap_or(0);
        if found != FORMAT_VERSION {
            return Err(CodecError::Version {
                found,
                supported: FORMAT_VERSION,
            });
        }
        let dto: HeaderDto =
            serde_json::from_value(value).map_err(|source| json_error(1, source))?;
        let declared = dto.devices.clone();
        let header = dto.into_header().map_err(|error| error.at(1))?;
        let mut rows = Vec::new();
        let mut line = 1;
        loop {
            buffer.clear();
            if input.read_line(&mut buffer)? == 0 {
                break;
            }
            line += 1;
            if buffer.trim().is_empty() {
                continue;
            }
            let dto: RowDto =
                serde_json::from_str(&buffer).map_err(|source| json_error(line, source))?;
            let row = Row::try_from(dto).map_err(|error| error.at(line))?;
            for (device, kind) in row_devices(&row) {
                let kind = PointerKindDto::try_from(kind).map_err(|error| error.at(line))?;
                if !declared
                    .iter()
                    .any(|entry| entry.device == device.0 && entry.kinds.contains(&kind))
                {
                    return Err(CodecError::UndeclaredDevice {
                        line,
                        device: device.0,
                    });
                }
            }
            rows.push(row);
        }
        Ok(Self { header, rows })
    }

    /// Write the recording, declaring in the header every device its rows
    /// read with the kinds they read it as.
    ///
    /// # Errors
    ///
    /// [`CodecError::Unmirrored`] for a value this format does not mirror,
    /// and [`CodecError::Io`] when the sink fails.
    pub fn write(&self, mut out: impl Write) -> Result<(), CodecError> {
        let mut devices: Vec<DeviceDto> = Vec::new();
        let rows = self
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let line = index as u64 + 2;
                for (device, kind) in row_devices(row) {
                    let kind = PointerKindDto::try_from(kind).map_err(|error| error.at(line))?;
                    match devices.iter_mut().find(|entry| entry.device == device.0) {
                        Some(entry) if entry.kinds.contains(&kind) => {}
                        Some(entry) => entry.kinds.push(kind),
                        None => devices.push(DeviceDto {
                            device: device.0,
                            kinds: vec![kind],
                        }),
                    }
                }
                RowDto::try_from(row).map_err(|error| error.at(line))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let header = HeaderDto::new(&self.header, devices).map_err(|error| error.at(1))?;
        write_line(&mut out, 1, &header)?;
        for (index, row) in rows.iter().enumerate() {
            write_line(&mut out, index as u64 + 2, row)?;
        }
        out.flush()?;
        Ok(())
    }

    /// The input the backend's registry assembles from these rows, as the
    /// backends deliver it: a reading that fails delivers its cancel.
    pub fn replay(&self) -> Vec<PlatformInput> {
        let mut registry = self.header.registry();
        let mut batch = MoveBatch::new();
        let mut touchpad = TouchpadAssembler::new(NativeSurface(0));
        let mut out = Vec::new();
        let pointer = |event| PlatformInput::Pointer(event);
        for row in &self.rows {
            match row {
                Row::Reading(reading) => out.extend(
                    decode(&mut registry, reading).unwrap_or_else(DecodeError::into_inputs),
                ),
                Row::Batch {
                    coalesced,
                    predicted,
                    current,
                } => {
                    for reading in coalesced {
                        batch.push_coalesced(*reading);
                    }
                    for reading in predicted {
                        batch.push_predicted(*reading);
                    }
                    out.extend(
                        batch
                            .decode(&mut registry, current)
                            .unwrap_or_else(DecodeError::into_inputs),
                    );
                }
                Row::ClickSettings(settings) => registry.set_click_settings(*settings),
                Row::Viewport { reading, frame } => {
                    out.extend(touchpad.assemble(&mut registry, *reading, *frame).inputs);
                }
                Row::ViewportAbandoned { frame } => {
                    out.extend(touchpad.abandon(&mut registry, *frame).inputs);
                }
                Row::Wheel(reading) => out.extend(registry.wheel(reading)),
                Row::AddDevice { device, kind, time } => out.extend(
                    registry
                        .add_device(*device, *kind, *time)
                        .map(|change| pointer(PointerEvent::DeviceAdded(change))),
                ),
                Row::RemoveDevice { device, time } => {
                    let (ended, removed) = registry.remove_device(*device, *time);
                    out.extend(ended.into_iter().map(|(_, input)| input));
                    out.extend(removed.map(|change| pointer(PointerEvent::DeviceRemoved(change))));
                }
                Row::CancelSurface {
                    surface,
                    time,
                    reason,
                } => out.extend(registry.cancel_surface(*surface, *time, *reason)),
                Row::CancelContact {
                    surface,
                    contact,
                    time,
                    reason,
                } => out.extend(registry.cancel_contact(*surface, *contact, *time, *reason)),
            }
        }
        out
    }
}

/// The devices `row`'s readings name, with the kind each reads it as.
fn row_devices(row: &Row) -> Vec<(NativeDevice, PointerKind)> {
    let readings: Vec<&NativeReading> = match row {
        Row::Reading(reading) => vec![reading],
        Row::Batch {
            coalesced,
            predicted,
            current,
        } => coalesced
            .iter()
            .chain(predicted)
            .chain(std::iter::once(current))
            .collect(),
        Row::AddDevice { .. }
        | Row::RemoveDevice { .. }
        | Row::ClickSettings(_)
        | Row::Viewport { .. }
        | Row::ViewportAbandoned { .. }
        | Row::Wheel(_)
        | Row::CancelSurface { .. }
        | Row::CancelContact { .. } => Vec::new(),
    };
    readings
        .into_iter()
        .filter_map(|reading| Some((reading.device?, reading.kind)))
        .collect()
}

/// A conversion failure, before its line is known.
#[derive(Debug)]
enum DtoError {
    UnknownSpelling(String),
    Invalid(String),
    Unmirrored(&'static str),
}

impl DtoError {
    fn at(self, line: u64) -> CodecError {
        match self {
            Self::UnknownSpelling(tag) => CodecError::UnknownVariant { line, tag },
            Self::Invalid(what) => CodecError::Invalid { line, what },
            Self::Unmirrored(what) => CodecError::Unmirrored { line, what },
        }
    }
}

type DtoResult<T> = Result<T, DtoError>;

fn invalid(what: impl Into<String>) -> DtoError {
    DtoError::Invalid(what.into())
}

/// A parse failure, with serde's unknown-variant refusal typed.
fn json_error(line: u64, source: serde_json::Error) -> CodecError {
    let message = source.to_string();
    if let Some(rest) = message.strip_prefix("unknown variant `")
        && let Some((tag, _)) = rest.split_once('`')
    {
        return CodecError::UnknownVariant {
            line,
            tag: tag.to_owned(),
        };
    }
    CodecError::Json { line, source }
}

fn write_line(out: &mut impl Write, line: u64, value: &impl Serialize) -> Result<(), CodecError> {
    serde_json::to_writer(&mut *out, value).map_err(|source| {
        if source.is_io() {
            CodecError::Io(source.into())
        } else {
            CodecError::Json { line, source }
        }
    })?;
    out.write_all(b"\n")?;
    Ok(())
}

const fn nanos(time: EventTime) -> u64 {
    time.as_nanos()
}

fn duration_nanos(duration: Duration) -> DtoResult<u64> {
    u64::try_from(duration.as_nanos()).map_err(|_| invalid("duration past u64 nanoseconds"))
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct CounterDto {
    count: u64,
    frequency: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct ClickSettingsDto {
    interval_ns: u64,
    width: f64,
    height: f64,
}

impl ClickSettingsDto {
    fn new(settings: ClickSettings) -> DtoResult<Self> {
        Ok(Self {
            interval_ns: duration_nanos(settings.interval)?,
            width: settings.area.0,
            height: settings.area.1,
        })
    }
}

impl From<ClickSettingsDto> for ClickSettings {
    fn from(dto: ClickSettingsDto) -> Self {
        Self {
            interval: Duration::from_nanos(dto.interval_ns),
            area: (dto.width, dto.height),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeviceDto {
    device: u64,
    kinds: Vec<PointerKindDto>,
}

#[derive(Debug, Serialize, Deserialize)]
struct HeaderDto {
    flui_native_recording: u32,
    backend: Backend,
    epoch_ns: u64,
    uptime_ns: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    counter: Option<CounterDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reserved_contact: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    first_pointer: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    first_device: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    click_settings: Option<ClickSettingsDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    provenance: Option<String>,
    #[serde(default)]
    devices: Vec<DeviceDto>,
}

impl HeaderDto {
    fn new(header: &Header, devices: Vec<DeviceDto>) -> DtoResult<Self> {
        Ok(Self {
            flui_native_recording: FORMAT_VERSION,
            backend: header.backend,
            epoch_ns: nanos(header.epoch),
            uptime_ns: duration_nanos(header.uptime)?,
            counter: header.counter.map(|anchor| CounterDto {
                count: anchor.count.0,
                frequency: anchor.frequency.get(),
            }),
            reserved_contact: header.reserved_contact.map(|contact| contact.0),
            first_pointer: header.first_pointer.map(NonZeroU64::get),
            first_device: header.first_device.map(NonZeroU64::get),
            click_settings: header
                .click_settings
                .map(ClickSettingsDto::new)
                .transpose()?,
            provenance: header.provenance.clone(),
            devices,
        })
    }

    fn into_header(self) -> DtoResult<Header> {
        let nonzero = |value: u64, what: &str| {
            NonZeroU64::new(value).ok_or_else(|| invalid(format!("{what} 0")))
        };
        let counter = self
            .counter
            .map(|counter| {
                Ok::<_, DtoError>(CounterAnchor {
                    count: QpcTicks(counter.count),
                    frequency: nonzero(counter.frequency, "counter frequency")?,
                })
            })
            .transpose()?;
        Ok(Header {
            reserved_contact: self.reserved_contact.map(NativeContact),
            first_pointer: self
                .first_pointer
                .map(|first| nonzero(first, "first pointer"))
                .transpose()?,
            first_device: self
                .first_device
                .map(|first| nonzero(first, "first device"))
                .transpose()?,
            click_settings: self.click_settings.map(ClickSettings::from),
            provenance: self.provenance,
            ..Header::new(
                self.backend,
                EventTime::from_nanos(self.epoch_ns),
                Duration::from_nanos(self.uptime_ns),
                counter,
            )
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum PointerKindDto {
    #[serde(rename = "mouse")]
    Mouse,
    #[serde(rename = "touch")]
    Touch,
    #[serde(rename = "pen")]
    Pen,
    #[serde(rename = "eraser")]
    Eraser,
    #[serde(rename = "trackpad")]
    Trackpad,
    #[serde(rename = "virtual")]
    Virtual,
    #[serde(rename = "unknown")]
    Unknown,
}

impl TryFrom<PointerKind> for PointerKindDto {
    type Error = DtoError;

    fn try_from(kind: PointerKind) -> DtoResult<Self> {
        Ok(match kind {
            PointerKind::Mouse => Self::Mouse,
            PointerKind::Touch => Self::Touch,
            PointerKind::Pen {
                tool: PenTool::Tip, ..
            } => Self::Pen,
            PointerKind::Pen {
                tool: PenTool::Eraser,
                ..
            } => Self::Eraser,
            PointerKind::Pen { .. } => return Err(DtoError::Unmirrored("PenTool")),
            PointerKind::Trackpad => Self::Trackpad,
            PointerKind::Virtual => Self::Virtual,
            PointerKind::Unknown => Self::Unknown,
            _ => return Err(DtoError::Unmirrored("PointerKind")),
        })
    }
}

impl From<PointerKindDto> for PointerKind {
    fn from(kind: PointerKindDto) -> Self {
        match kind {
            PointerKindDto::Mouse => Self::Mouse,
            PointerKindDto::Touch => Self::Touch,
            PointerKindDto::Pen => Self::Pen { tool: PenTool::Tip },
            PointerKindDto::Eraser => Self::Pen {
                tool: PenTool::Eraser,
            },
            PointerKindDto::Trackpad => Self::Trackpad,
            PointerKindDto::Virtual => Self::Virtual,
            PointerKindDto::Unknown => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum CancelReasonDto {
    #[serde(rename = "platform")]
    Platform,
    #[serde(rename = "capture_lost")]
    CaptureLost,
    #[serde(rename = "focus_lost")]
    FocusLost,
    #[serde(rename = "device_removed")]
    DeviceRemoved,
    #[serde(rename = "invalid_input")]
    InvalidInput,
}

impl TryFrom<NativeCancel> for CancelReasonDto {
    type Error = DtoError;

    fn try_from(reason: NativeCancel) -> DtoResult<Self> {
        Ok(match reason {
            NativeCancel::Platform => Self::Platform,
            NativeCancel::CaptureLost => Self::CaptureLost,
            NativeCancel::FocusLost => Self::FocusLost,
            NativeCancel::DeviceRemoved => Self::DeviceRemoved,
            NativeCancel::InvalidInput => Self::InvalidInput,
            _ => return Err(DtoError::Unmirrored("NativeCancel")),
        })
    }
}

impl From<CancelReasonDto> for NativeCancel {
    fn from(reason: CancelReasonDto) -> Self {
        match reason {
            CancelReasonDto::Platform => Self::Platform,
            CancelReasonDto::CaptureLost => Self::CaptureLost,
            CancelReasonDto::FocusLost => Self::FocusLost,
            CancelReasonDto::DeviceRemoved => Self::DeviceRemoved,
            CancelReasonDto::InvalidInput => Self::InvalidInput,
        }
    }
}

/// Modifier names, pinned (the W3C `getModifierState` keys, as the
/// `HostInput` recording spells them).
const MODIFIERS: [(Modifiers, &str); 12] = [
    (Modifiers::ALT, "alt"),
    (Modifiers::ALT_GRAPH, "alt_graph"),
    (Modifiers::CAPS_LOCK, "caps_lock"),
    (Modifiers::CONTROL, "control"),
    (Modifiers::FN, "fn"),
    (Modifiers::FN_LOCK, "fn_lock"),
    (Modifiers::META, "meta"),
    (Modifiers::NUM_LOCK, "num_lock"),
    (Modifiers::SCROLL_LOCK, "scroll_lock"),
    (Modifiers::SHIFT, "shift"),
    (Modifiers::SYMBOL, "symbol"),
    (Modifiers::SYMBOL_LOCK, "symbol_lock"),
];

fn modifiers_dto(modifiers: Modifiers) -> DtoResult<Vec<String>> {
    let named = MODIFIERS
        .iter()
        .filter(|(modifier, _)| modifiers.contains(*modifier))
        .fold(Modifiers::NONE, |held, (modifier, _)| held | *modifier);
    if named != modifiers {
        return Err(DtoError::Unmirrored("Modifiers"));
    }
    Ok(MODIFIERS
        .iter()
        .filter(|(modifier, _)| modifiers.contains(*modifier))
        .map(|(_, name)| (*name).to_owned())
        .collect())
}

fn modifiers(names: &[String]) -> DtoResult<Modifiers> {
    names.iter().try_fold(Modifiers::NONE, |held, name| {
        MODIFIERS
            .iter()
            .find(|(_, spelling)| spelling == name)
            .map(|(modifier, _)| held | *modifier)
            .ok_or_else(|| DtoError::UnknownSpelling(name.clone()))
    })
}

fn button(number: u8) -> DtoResult<PointerButton> {
    PointerButton::try_from(number).map_err(|_| invalid(format!("pointer button {number}")))
}

fn buttons(numbers: &[u8]) -> DtoResult<PointerButtons> {
    numbers
        .iter()
        .try_fold(PointerButtons::NONE, |held, number| {
            Ok(held.with(button(*number)?))
        })
}

fn buttons_dto(buttons: PointerButtons) -> Vec<u8> {
    buttons.iter().map(|button| button.number().get()).collect()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum TimeDto {
    /// A wrapping millisecond tick and the uptime it was placed against.
    #[serde(rename = "tick_ms")]
    TickMillis { tick: u32, observed_ms: u64 },
    #[serde(rename = "uptime_s")]
    UptimeSeconds(f64),
    #[serde(rename = "epoch_ns")]
    Epoch(u64),
}

impl TryFrom<NativeTime> for TimeDto {
    type Error = DtoError;

    fn try_from(time: NativeTime) -> DtoResult<Self> {
        Ok(match time {
            NativeTime::TickMillis { tick, observed_ms } => Self::TickMillis { tick, observed_ms },
            NativeTime::UptimeSeconds(seconds) => Self::UptimeSeconds(seconds),
            NativeTime::Epoch(time) => Self::Epoch(nanos(time)),
            _ => return Err(DtoError::Unmirrored("NativeTime")),
        })
    }
}

impl From<TimeDto> for NativeTime {
    fn from(time: TimeDto) -> Self {
        match time {
            TimeDto::TickMillis { tick, observed_ms } => Self::TickMillis { tick, observed_ms },
            TimeDto::UptimeSeconds(seconds) => Self::UptimeSeconds(seconds),
            TimeDto::Epoch(nanos) => Self::Epoch(EventTime::from_nanos(nanos)),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum GestureComponentDto {
    #[serde(rename = "scale")]
    Scale,
    #[serde(rename = "rotation")]
    Rotation,
    #[serde(rename = "session")]
    Session,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum GesturePhaseDto {
    #[serde(rename = "began")]
    Began,
    #[serde(rename = "changed")]
    Changed,
    #[serde(rename = "ended")]
    Ended,
    #[serde(rename = "cancelled")]
    Cancelled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum GestureDeltaDto {
    #[serde(rename = "scale")]
    Scale(f64),
    #[serde(rename = "rotation")]
    Rotation(f64),
    #[serde(rename = "none")]
    None,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct GestureDto {
    component: GestureComponentDto,
    phase: GesturePhaseDto,
    delta: GestureDeltaDto,
}

impl TryFrom<GestureReading> for GestureDto {
    type Error = DtoError;

    fn try_from(gesture: GestureReading) -> DtoResult<Self> {
        Ok(Self {
            component: match gesture.component {
                GestureComponent::Scale => GestureComponentDto::Scale,
                GestureComponent::Rotation => GestureComponentDto::Rotation,
                GestureComponent::Session => GestureComponentDto::Session,
                _ => return Err(DtoError::Unmirrored("GestureComponent")),
            },
            phase: match gesture.phase {
                GesturePhase::Began => GesturePhaseDto::Began,
                GesturePhase::Changed => GesturePhaseDto::Changed,
                GesturePhase::Ended => GesturePhaseDto::Ended,
                GesturePhase::Cancelled => GesturePhaseDto::Cancelled,
            },
            delta: match gesture.delta {
                GestureDelta::Scale(factor) => GestureDeltaDto::Scale(factor),
                GestureDelta::Rotation(radians) => GestureDeltaDto::Rotation(radians),
                GestureDelta::None => GestureDeltaDto::None,
                _ => return Err(DtoError::Unmirrored("GestureDelta")),
            },
        })
    }
}

impl From<GestureDto> for GestureReading {
    fn from(gesture: GestureDto) -> Self {
        Self {
            component: match gesture.component {
                GestureComponentDto::Scale => GestureComponent::Scale,
                GestureComponentDto::Rotation => GestureComponent::Rotation,
                GestureComponentDto::Session => GestureComponent::Session,
            },
            phase: match gesture.phase {
                GesturePhaseDto::Began => GesturePhase::Began,
                GesturePhaseDto::Changed => GesturePhase::Changed,
                GesturePhaseDto::Ended => GesturePhase::Ended,
                GesturePhaseDto::Cancelled => GesturePhase::Cancelled,
            },
            delta: match gesture.delta {
                GestureDeltaDto::Scale(factor) => GestureDelta::Scale(factor),
                GestureDeltaDto::Rotation(radians) => GestureDelta::Rotation(radians),
                GestureDeltaDto::None => GestureDelta::None,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum ViewportStatusDto {
    #[serde(rename = "building")]
    Building,
    #[serde(rename = "enabled")]
    Enabled,
    #[serde(rename = "disabled")]
    Disabled,
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "inertia")]
    Inertia,
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "suspended")]
    Suspended,
}

/// One viewport callback: a status, or the content's scale and offset.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum ViewportDto {
    #[serde(rename = "status")]
    Status(ViewportStatusDto),
    #[serde(rename = "content")]
    Content([f64; 3]),
}

impl TryFrom<ViewportReading> for ViewportDto {
    type Error = DtoError;

    fn try_from(reading: ViewportReading) -> DtoResult<Self> {
        Ok(match reading {
            ViewportReading::Status(status) => Self::Status(match status {
                ViewportStatus::Building => ViewportStatusDto::Building,
                ViewportStatus::Enabled => ViewportStatusDto::Enabled,
                ViewportStatus::Disabled => ViewportStatusDto::Disabled,
                ViewportStatus::Running => ViewportStatusDto::Running,
                ViewportStatus::Inertia => ViewportStatusDto::Inertia,
                ViewportStatus::Ready => ViewportStatusDto::Ready,
                ViewportStatus::Suspended => ViewportStatusDto::Suspended,
                _ => return Err(DtoError::Unmirrored("ViewportStatus")),
            }),
            ViewportReading::Content(transform) => {
                Self::Content([transform.scale(), transform.x(), transform.y()])
            }
            _ => return Err(DtoError::Unmirrored("ViewportReading")),
        })
    }
}

impl TryFrom<ViewportDto> for ViewportReading {
    type Error = DtoError;

    fn try_from(dto: ViewportDto) -> DtoResult<Self> {
        Ok(match dto {
            ViewportDto::Status(status) => Self::Status(match status {
                ViewportStatusDto::Building => ViewportStatus::Building,
                ViewportStatusDto::Enabled => ViewportStatus::Enabled,
                ViewportStatusDto::Disabled => ViewportStatus::Disabled,
                ViewportStatusDto::Running => ViewportStatus::Running,
                ViewportStatusDto::Inertia => ViewportStatus::Inertia,
                ViewportStatusDto::Ready => ViewportStatus::Ready,
                ViewportStatusDto::Suspended => ViewportStatus::Suspended,
            }),
            ViewportDto::Content([scale, x, y]) => Self::Content(
                ContentTransform::try_new(scale, x, y)
                    .ok_or_else(|| invalid("a content transform that is not finite"))?,
            ),
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum ActionDto {
    #[serde(rename = "enter")]
    Enter,
    #[serde(rename = "leave")]
    Leave,
    #[serde(rename = "move")]
    Move,
    #[serde(rename = "press")]
    Press(u8),
    #[serde(rename = "release")]
    Release(u8),
    #[serde(rename = "cancel")]
    Cancel(CancelReasonDto),
    #[serde(rename = "gesture")]
    Gesture(GestureDto),
}

impl TryFrom<ReadingAction> for ActionDto {
    type Error = DtoError;

    fn try_from(action: ReadingAction) -> DtoResult<Self> {
        Ok(match action {
            ReadingAction::Enter => Self::Enter,
            ReadingAction::Leave => Self::Leave,
            ReadingAction::Move => Self::Move,
            ReadingAction::Press(button) => Self::Press(button.number().get()),
            ReadingAction::Release(button) => Self::Release(button.number().get()),
            ReadingAction::Cancel(reason) => Self::Cancel(reason.try_into()?),
            ReadingAction::Gesture(gesture) => Self::Gesture(gesture.try_into()?),
            _ => return Err(DtoError::Unmirrored("ReadingAction")),
        })
    }
}

impl TryFrom<ActionDto> for ReadingAction {
    type Error = DtoError;

    fn try_from(action: ActionDto) -> DtoResult<Self> {
        Ok(match action {
            ActionDto::Enter => Self::Enter,
            ActionDto::Leave => Self::Leave,
            ActionDto::Move => Self::Move,
            ActionDto::Press(number) => Self::Press(button(number)?),
            ActionDto::Release(number) => Self::Release(button(number)?),
            ActionDto::Cancel(reason) => Self::Cancel(reason.into()),
            ActionDto::Gesture(gesture) => Self::Gesture(gesture.into()),
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum AnglesDto {
    #[serde(rename = "tilt_degrees")]
    TiltDegrees { x: f64, y: f64 },
    #[serde(rename = "spherical")]
    Spherical {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        altitude: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        azimuth: Option<f64>,
    },
}

impl TryFrom<PenAngles> for AnglesDto {
    type Error = DtoError;

    fn try_from(angles: PenAngles) -> DtoResult<Self> {
        Ok(match angles {
            PenAngles::TiltDegrees { x, y } => Self::TiltDegrees { x, y },
            PenAngles::Spherical { altitude, azimuth } => Self::Spherical { altitude, azimuth },
            _ => return Err(DtoError::Unmirrored("PenAngles")),
        })
    }
}

impl From<AnglesDto> for PenAngles {
    fn from(angles: AnglesDto) -> Self {
        match angles {
            AnglesDto::TiltDegrees { x, y } => Self::TiltDegrees { x, y },
            AnglesDto::Spherical { altitude, azimuth } => Self::Spherical { altitude, azimuth },
        }
    }
}

const fn is_zero(value: &u64) -> bool {
    *value == 0
}

const fn is_one(value: &f64) -> bool {
    *value == 1.0
}

const fn one() -> f64 {
    1.0
}

/// Where and when a viewport call happened, flat in its row.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct FrameDto {
    time: TimeDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    counter: Option<u64>,
    position: [f64; 2],
    #[serde(default = "one", skip_serializing_if = "is_one")]
    scale: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    modifiers: Vec<String>,
}

impl TryFrom<&TouchpadFrame> for FrameDto {
    type Error = DtoError;

    fn try_from(frame: &TouchpadFrame) -> DtoResult<Self> {
        Ok(Self {
            time: frame.time.coarse.try_into()?,
            counter: frame.time.counter.map(|count| count.0),
            position: [frame.position.0, frame.position.1],
            scale: frame.scale,
            modifiers: modifiers_dto(frame.modifiers)?,
        })
    }
}

impl TryFrom<FrameDto> for TouchpadFrame {
    type Error = DtoError;

    fn try_from(dto: FrameDto) -> DtoResult<Self> {
        Ok(Self {
            time: ReadingTime {
                coarse: dto.time.into(),
                counter: dto.counter.map(QpcTicks),
            },
            position: (dto.position[0], dto.position[1]),
            scale: dto.scale,
            modifiers: modifiers(&dto.modifiers)?,
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum WheelAxisDto {
    #[serde(rename = "vertical")]
    Vertical,
    #[serde(rename = "horizontal")]
    Horizontal,
}

/// One wheel message: its signed distance in 1/120 detents as Win32 signs
/// it, the cursor in device pixels and its time on the event timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct WheelDto {
    axis: WheelAxisDto,
    distance: i16,
    position: [f64; 2],
    #[serde(default = "one", skip_serializing_if = "is_one")]
    scale: f64,
    time_ns: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    modifiers: Vec<String>,
}

impl TryFrom<&WheelReading> for WheelDto {
    type Error = DtoError;

    fn try_from(reading: &WheelReading) -> DtoResult<Self> {
        Ok(Self {
            axis: match reading.axis {
                WheelAxis::Vertical => WheelAxisDto::Vertical,
                WheelAxis::Horizontal => WheelAxisDto::Horizontal,
                _ => return Err(DtoError::Unmirrored("WheelAxis")),
            },
            distance: reading.distance,
            position: [reading.position.0, reading.position.1],
            scale: reading.scale,
            time_ns: nanos(reading.time),
            modifiers: modifiers_dto(reading.modifiers)?,
        })
    }
}

impl TryFrom<WheelDto> for WheelReading {
    type Error = DtoError;

    fn try_from(dto: WheelDto) -> DtoResult<Self> {
        Ok(Self {
            axis: match dto.axis {
                WheelAxisDto::Vertical => WheelAxis::Vertical,
                WheelAxisDto::Horizontal => WheelAxis::Horizontal,
            },
            distance: dto.distance,
            position: (dto.position[0], dto.position[1]),
            scale: dto.scale,
            time: EventTime::from_nanos(dto.time_ns),
            modifiers: modifiers(&dto.modifiers)?,
        })
    }
}

/// One reading, flat: the fields a backend copies out of its native event.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReadingDto {
    #[serde(default, skip_serializing_if = "is_zero")]
    surface: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    device: Option<u64>,
    contact: u64,
    kind: PointerKindDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    primary: Option<bool>,
    action: ActionDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    buttons: Option<Vec<u8>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    modifiers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    click_count: Option<u8>,
    time: TimeDto,
    /// The performance count, when the device reports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    counter: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    position: Option<[f64; 2]>,
    #[serde(default = "one", skip_serializing_if = "is_one")]
    scale: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pressure: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tangential_pressure: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    angles: Option<AnglesDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    twist_degrees: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    contact_size: Option<[f64; 2]>,
}

impl TryFrom<&NativeReading> for ReadingDto {
    type Error = DtoError;

    fn try_from(reading: &NativeReading) -> DtoResult<Self> {
        let sensors = reading.sensors;
        Ok(Self {
            surface: reading.surface.0,
            device: reading.device.map(|device| device.0),
            contact: reading.contact.0,
            kind: reading.kind.try_into()?,
            primary: reading.primary,
            action: reading.action.try_into()?,
            buttons: reading.buttons.map(buttons_dto),
            modifiers: modifiers_dto(reading.modifiers)?,
            click_count: reading.click_count.map(NonZeroU8::get),
            time: reading.time.coarse.try_into()?,
            counter: reading.time.counter.map(|count| count.0),
            position: reading.position.map(|point| [point.x, point.y]),
            scale: reading.scale,
            pressure: sensors.pressure,
            tangential_pressure: sensors.tangential_pressure,
            angles: sensors.angles.map(AnglesDto::try_from).transpose()?,
            twist_degrees: sensors.twist_degrees,
            contact_size: sensors.contact_size.map(|(width, height)| [width, height]),
        })
    }
}

impl TryFrom<ReadingDto> for NativeReading {
    type Error = DtoError;

    fn try_from(dto: ReadingDto) -> DtoResult<Self> {
        let time = ReadingTime {
            coarse: dto.time.into(),
            counter: dto.counter.map(QpcTicks),
        };
        let mut reading = Self::new(
            NativeContact(dto.contact),
            dto.kind.into(),
            dto.action.try_into()?,
            time,
        )
        .on(NativeSurface(dto.surface))
        .with_modifiers(modifiers(&dto.modifiers)?)
        .with_sensors(RawSensors {
            pressure: dto.pressure,
            tangential_pressure: dto.tangential_pressure,
            angles: dto.angles.map(PenAngles::from),
            twist_degrees: dto.twist_degrees,
            contact_size: dto.contact_size.map(|[width, height]| (width, height)),
        });
        reading.scale = dto.scale;
        reading.position = dto.position.map(|[x, y]| RawPoint { x, y });
        reading.primary = dto.primary;
        if let Some(device) = dto.device {
            reading = reading.from_device(NativeDevice(device));
        }
        if let Some(numbers) = dto.buttons {
            reading = reading.with_buttons(buttons(&numbers)?);
        }
        if let Some(count) = dto.click_count {
            reading = reading
                .with_click_count(NonZeroU8::new(count).ok_or_else(|| invalid("click count 0"))?);
        }
        Ok(reading)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "row")]
enum RowDto {
    #[serde(rename = "reading")]
    Reading { reading: ReadingDto },
    #[serde(rename = "batch")]
    Batch {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        coalesced: Vec<ReadingDto>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        predicted: Vec<ReadingDto>,
        current: ReadingDto,
    },
    #[serde(rename = "click_settings")]
    ClickSettings {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        settings: Option<ClickSettingsDto>,
    },
    #[serde(rename = "viewport")]
    Viewport {
        viewport: ViewportDto,
        #[serde(flatten)]
        frame: FrameDto,
    },
    #[serde(rename = "viewport_abandoned")]
    ViewportAbandoned {
        #[serde(flatten)]
        frame: FrameDto,
    },
    #[serde(rename = "wheel")]
    Wheel { wheel: WheelDto },
    #[serde(rename = "add_device")]
    AddDevice {
        device: u64,
        kind: PointerKindDto,
        time_ns: u64,
    },
    #[serde(rename = "remove_device")]
    RemoveDevice { device: u64, time_ns: u64 },
    #[serde(rename = "cancel_surface")]
    CancelSurface {
        #[serde(default, skip_serializing_if = "is_zero")]
        surface: u64,
        time_ns: u64,
        reason: CancelReasonDto,
    },
    #[serde(rename = "cancel_contact")]
    CancelContact {
        #[serde(default, skip_serializing_if = "is_zero")]
        surface: u64,
        contact: u64,
        time_ns: u64,
        reason: CancelReasonDto,
    },
}

impl TryFrom<&Row> for RowDto {
    type Error = DtoError;

    fn try_from(row: &Row) -> DtoResult<Self> {
        let readings = |readings: &[NativeReading]| {
            readings
                .iter()
                .map(ReadingDto::try_from)
                .collect::<DtoResult<Vec<_>>>()
        };
        Ok(match row {
            Row::Reading(reading) => Self::Reading {
                reading: reading.try_into()?,
            },
            Row::Batch {
                coalesced,
                predicted,
                current,
            } => Self::Batch {
                coalesced: readings(coalesced)?,
                predicted: readings(predicted)?,
                current: current.try_into()?,
            },
            Row::ClickSettings(settings) => Self::ClickSettings {
                settings: settings.map(ClickSettingsDto::new).transpose()?,
            },
            Row::Viewport { reading, frame } => Self::Viewport {
                viewport: (*reading).try_into()?,
                frame: frame.try_into()?,
            },
            Row::ViewportAbandoned { frame } => Self::ViewportAbandoned {
                frame: frame.try_into()?,
            },
            Row::Wheel(reading) => Self::Wheel {
                wheel: reading.try_into()?,
            },
            Row::AddDevice { device, kind, time } => Self::AddDevice {
                device: device.0,
                kind: (*kind).try_into()?,
                time_ns: nanos(*time),
            },
            Row::RemoveDevice { device, time } => Self::RemoveDevice {
                device: device.0,
                time_ns: nanos(*time),
            },
            Row::CancelSurface {
                surface,
                time,
                reason,
            } => Self::CancelSurface {
                surface: surface.0,
                time_ns: nanos(*time),
                reason: (*reason).try_into()?,
            },
            Row::CancelContact {
                surface,
                contact,
                time,
                reason,
            } => Self::CancelContact {
                surface: surface.0,
                contact: contact.0,
                time_ns: nanos(*time),
                reason: (*reason).try_into()?,
            },
        })
    }
}

impl TryFrom<RowDto> for Row {
    type Error = DtoError;

    fn try_from(row: RowDto) -> DtoResult<Self> {
        let readings = |readings: Vec<ReadingDto>| {
            readings
                .into_iter()
                .map(NativeReading::try_from)
                .collect::<DtoResult<Vec<_>>>()
        };
        Ok(match row {
            RowDto::Reading { reading } => Self::Reading(reading.try_into()?),
            RowDto::Batch {
                coalesced,
                predicted,
                current,
            } => Self::Batch {
                coalesced: readings(coalesced)?,
                predicted: readings(predicted)?,
                current: current.try_into()?,
            },
            RowDto::ClickSettings { settings } => {
                Self::ClickSettings(settings.map(ClickSettings::from))
            }
            RowDto::Viewport { viewport, frame } => Self::Viewport {
                reading: viewport.try_into()?,
                frame: frame.try_into()?,
            },
            RowDto::ViewportAbandoned { frame } => Self::ViewportAbandoned {
                frame: frame.try_into()?,
            },
            RowDto::Wheel { wheel } => Self::Wheel(wheel.try_into()?),
            RowDto::AddDevice {
                device,
                kind,
                time_ns,
            } => Self::AddDevice {
                device: NativeDevice(device),
                kind: kind.into(),
                time: EventTime::from_nanos(time_ns),
            },
            RowDto::RemoveDevice { device, time_ns } => Self::RemoveDevice {
                device: NativeDevice(device),
                time: EventTime::from_nanos(time_ns),
            },
            RowDto::CancelSurface {
                surface,
                time_ns,
                reason,
            } => Self::CancelSurface {
                surface: NativeSurface(surface),
                time: EventTime::from_nanos(time_ns),
                reason: reason.into(),
            },
            RowDto::CancelContact {
                surface,
                contact,
                time_ns,
                reason,
            } => Self::CancelContact {
                surface: NativeSurface(surface),
                contact: NativeContact(contact),
                time: EventTime::from_nanos(time_ns),
                reason: reason.into(),
            },
        })
    }
}
