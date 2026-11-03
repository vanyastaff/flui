//! The input recording's wire conversions (ADR-0215 §2).
//!
//! The event vocabulary carries no serde, so it is not frozen as a wire
//! format: [`flui_protocol::input_recording`] mirrors it with pinned
//! spellings, and the conversions here are the only place the two meet.
//! Unknown fields are skipped within a major format version; an unknown
//! variant fails the row. Because the vocabulary is `#[non_exhaustive]`, a new
//! variant reaches the wildcard arms here instead of failing to compile:
//! writing it fails with a typed [`WireError::Unmirrored`], and the generated
//! variant table and its round-trip test in `flui-testing` catch it before
//! that.
//!
//! [`InputRecord::to_wire`] and [`InputRecord::from_wire`] convert a row; the
//! header's pieces have their own functions ([`profiles_to_wire`],
//! [`device_to_wire`], [`PreferencesRow::to_wire`] and their inverses).

use std::num::{NonZeroU8, NonZeroU32};
use std::sync::Arc;
use std::time::Duration;

use flui_foundation::DataTransferId;
use flui_foundation::geometry::{DevicePixelRatio, EdgeInsets, Point, Size, WindowMetrics};
use flui_interaction::gesture::{OneEuroTuning, VelocityEstimator};
use flui_interaction::{
    GestureDisposition, GestureProfiles, GestureSettings, GestureValues, RemainderHandoff,
};
use flui_platform::Availability;
use flui_platform::data_transfer::{
    DataTransferOffer, Mime, RepresentationDescriptor, TransferActions, TransferFormat,
};
use flui_platform::input_capabilities::{InputCapabilities, PointerKindSet};
use flui_platform::input_regions::{CaptionButton, InputRegionKind, ResizeEdge};
use flui_platform::keyboard::{
    Code, ImeComposition, Key, KeyEvent, KeyRelease, KeyRepeat, KeyState, Location, Modifiers,
    NamedKey,
};
use flui_platform::pointer::{
    ButtonChange, CancelReason, ContactSize, DeviceId, HoverDistance, PanZoomEvent, PanZoomPhase,
    PanZoomTransform, PenOrientation, PenSqueeze, PenSqueezePhase, PenTool, PointerButton,
    PointerButtonEvent, PointerButtons, PointerCancel, PointerDeviceChange, PointerEvent,
    PointerId, PointerInfo, PointerKind, PointerMove, PointerPosition, PointerRevision,
    PointerRole, PointerSample, PointerSignal, Pressure, ScrollDelta, ScrollEvent, ScrollPhase,
    ScrollPrecision, ScrollUnit, SmartZoomEvent, TangentialPressure, Twist,
};
use flui_platform::{
    Brightness, DragDropEvent, EventTime, FramePresented, GestureParts, HourCycle, ImeEvent,
    InsetCurve, InsetMotion, KeyboardInset, Locale, MeasurementSystem, MotionPreference,
    NativeMouseGeometry, NativeTouchGeometry, NumberingSystem, PlatformInput, PreferenceParts,
    RegionOverride, RegionalParts, SystemPreferences, TextWeightPreference, Weekday, WheelParts,
    WheelStep, WindowExecutionState, WindowInsets,
};
use flui_protocol::input_recording as rec;
use flui_protocol::native_recording as wire;

use crate::owner::{InputOutcome, WindowObservation};
#[cfg(feature = "testing")]
use crate::{DeadlineRow, DecisionRow, InputRow, RecordedMember, RecordedSequence};
use crate::{HostInput, InputRecord, PreferencesRow, RecordedSurface, RecordingStart};

/// A value the recording's wire form cannot convert in either direction.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum WireError {
    /// A spelling this reader does not know, such as a key or action name.
    #[error("unknown spelling `{0}`")]
    UnknownSpelling(String),
    /// A value the vocabulary's validating constructor refuses.
    #[error("invalid value: {0}")]
    Invalid(String),
    /// The vocabulary grew a variant of this type the wire form does not
    /// mirror yet: it is added to `flui_protocol::input_recording` and to the
    /// generated variant table.
    #[error("{0} has a variant the recording format does not mirror")]
    Unmirrored(&'static str),
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

type Result<T> = std::result::Result<T, WireError>;

/// `Self` built from `V`, as [`From`] does. A local trait, because the orphan
/// rules forbid implementing `From` between a vocabulary type and a wire type
/// that both live in other crates.
trait Convert<V>: Sized {
    fn convert(value: V) -> Self;
}

/// `Self` built from `V` when `V` holds a value `Self` refuses, as
/// [`TryFrom`] does.
trait TryConvert<V>: Sized {
    fn try_convert(value: V) -> Result<Self>;
}

/// [`Convert`] from the source's side, as [`Into`] is to [`From`].
trait ConvertInto: Sized {
    fn convert_into<T: Convert<Self>>(self) -> T {
        T::convert(self)
    }
}

impl<V> ConvertInto for V {}

/// [`TryConvert`] from the source's side, as [`TryInto`] is to [`TryFrom`].
trait TryConvertInto: Sized {
    fn try_convert_into<T: TryConvert<Self>>(self) -> Result<T> {
        T::try_convert(self)
    }
}

impl<V> TryConvertInto for V {}

fn invalid(what: impl std::fmt::Display) -> WireError {
    WireError::Invalid(what.to_string())
}

/// The vocabulary grew a variant of `what` this wire form does not mirror yet.
const fn unmirrored(what: &'static str) -> WireError {
    WireError::Unmirrored(what)
}

impl InputRecord {
    /// This row in the recording's wire form. Under [`Privacy::Redact`] the
    /// row's text is replaced and the row marked redacted.
    ///
    /// # Errors
    ///
    /// [`WireError::Unmirrored`] when the vocabulary grew a variant the wire
    /// form does not mirror, and [`WireError::Invalid`] for a value the form
    /// cannot hold.
    pub fn to_wire(&self, privacy: Privacy) -> Result<rec::Row> {
        let mut row = rec::Row::try_convert(self)?;
        if privacy == Privacy::Redact {
            row.redact();
        }
        Ok(row)
    }

    /// The row `row` records.
    ///
    /// # Errors
    ///
    /// [`WireError::UnknownSpelling`] for a spelling this build does not know
    /// and [`WireError::Invalid`] for a value the vocabulary refuses.
    #[cfg(feature = "testing")]
    pub fn from_wire(row: rec::Row) -> Result<Self> {
        Self::try_convert(row)
    }
}

impl PreferencesRow {
    /// This revision in the recording's wire form.
    ///
    /// # Errors
    ///
    /// [`WireError::Unmirrored`] when the preferences hold a variant the wire
    /// form does not mirror.
    pub fn to_wire(&self) -> Result<rec::Preferences> {
        rec::Preferences::try_convert(self)
    }

    /// The revision `preferences` records.
    ///
    /// # Errors
    ///
    /// [`WireError::Invalid`] for a value the preferences refuse.
    pub fn from_wire(preferences: rec::Preferences) -> Result<Self> {
        Self::try_convert(preferences)
    }
}

// ============================================================================
// Rows
// ============================================================================

impl TryConvert<&InputRecord> for rec::Row {
    fn try_convert(record: &InputRecord) -> Result<Self> {
        Ok(match record {
            InputRecord::Input(row) => Self::Input(rec::InputRow {
                window: row.window.get().get(),
                t: row.t.as_nanos(),
                arrival: row.arrival.as_nanos(),
                input: rec::HostInput::try_convert(&row.input)?,
                outcome: match row.outcome {
                    InputOutcome::Handled => rec::Outcome::Handled,
                    InputOutcome::Unhandled => rec::Outcome::Unhandled,
                    InputOutcome::Queued => rec::Outcome::Queued,
                },
                embedded_surface: match row.surface {
                    RecordedSurface::Window => None,
                    RecordedSurface::Embedded(surface) => Some(surface.get()),
                },
                sequence: row.sequence.map(|sequence| sequence.get().get()),
                redacted: false,
            }),
            InputRecord::Decision(row) => Self::Decision(rec::Decision {
                window: row.window.get().get(),
                pointer: row.pointer.get().get(),
                t: row.t.as_nanos(),
                disposition: match row.disposition {
                    GestureDisposition::Accepted => rec::Disposition::Accepted,
                    GestureDisposition::Rejected => rec::Disposition::Rejected,
                },
                sequence: row.sequence.map(|sequence| sequence.get().get()),
                member: row.member.get().get(),
                member_type: row.member_type.to_string(),
            }),
            InputRecord::Deadline(row) => Self::Deadline(rec::Deadline {
                window: row.window.get().get(),
                pointer: row.pointer.map(|pointer| pointer.get().get()),
                sequence: row.sequence.map(|sequence| sequence.get().get()),
                t: row.t.as_nanos(),
                member: row.member.get().get(),
                member_type: row.member_type.to_string(),
            }),
            InputRecord::Preferences(row) => Self::Preferences(rec::Preferences::try_convert(row)?),
            InputRecord::Gap { rows } => Self::Gap { rows: *rows },
            InputRecord::End { lost_rows } => Self::End {
                lost_rows: *lost_rows,
            },
        })
    }
}

#[cfg(feature = "testing")]
impl TryConvert<rec::Row> for InputRecord {
    fn try_convert(row: rec::Row) -> Result<Self> {
        Ok(match row {
            rec::Row::Input(row) => Self::Input(
                InputRow::new(
                    window_id(row.window)?,
                    EventTime::from_nanos(row.t),
                    EventTime::from_nanos(row.arrival),
                    HostInput::try_convert(row.input)?,
                    match row.outcome {
                        rec::Outcome::Handled => InputOutcome::Handled,
                        rec::Outcome::Unhandled => InputOutcome::Unhandled,
                        rec::Outcome::Queued => InputOutcome::Queued,
                    },
                )
                .on_surface(match row.embedded_surface {
                    None => RecordedSurface::Window,
                    Some(surface) => RecordedSurface::Embedded(nonzero(surface, "surface")?),
                })
                .in_sequence(recorded_sequence(row.sequence)?),
            ),
            rec::Row::Decision(row) => Self::Decision(
                DecisionRow::new(
                    window_id(row.window)?,
                    pointer_id(row.pointer)?,
                    EventTime::from_nanos(row.t),
                    match row.disposition {
                        rec::Disposition::Accepted => GestureDisposition::Accepted,
                        rec::Disposition::Rejected => GestureDisposition::Rejected,
                    },
                    RecordedMember::new(nonzero(row.member, "member")?),
                )
                .in_sequence(recorded_sequence(row.sequence)?)
                .with_member_type(row.member_type),
            ),
            rec::Row::Deadline(row) => Self::Deadline(
                DeadlineRow::new(
                    window_id(row.window)?,
                    row.pointer.map(pointer_id).transpose()?,
                    EventTime::from_nanos(row.t),
                    RecordedMember::new(nonzero(row.member, "member")?),
                )
                .in_sequence(recorded_sequence(row.sequence)?)
                .with_member_type(row.member_type),
            ),
            rec::Row::Preferences(row) => Self::Preferences(PreferencesRow::try_convert(row)?),
            rec::Row::Gap { rows } => Self::Gap { rows },
            rec::Row::End { lost_rows } => Self::End { lost_rows },
        })
    }
}

// ============================================================================
// Host input
// ============================================================================

impl TryConvert<&HostInput> for rec::HostInput {
    fn try_convert(input: &HostInput) -> Result<Self> {
        Ok(match input {
            HostInput::Platform(input) => Self::Platform(input.try_convert_into()?),
            HostInput::Window(observation) => Self::Window(observation.try_convert_into()?),
            HostInput::Frame {
                vsync,
                target_present,
                presented,
            } => Self::Frame {
                vsync: vsync.as_nanos(),
                target_present: target_present.map(EventTime::as_nanos),
                presented: presented.map(|presented| rec::FramePresented {
                    at: presented.at.as_nanos(),
                    refresh: presented.refresh.map(nanos),
                }),
            },
            HostInput::Accessibility(request) => Self::Accessibility(request.try_convert_into()?),
            HostInput::AssistiveFocus(node) => Self::AssistiveFocus(node.map(|node| node.0)),
        })
    }
}

impl TryConvert<rec::HostInput> for HostInput {
    fn try_convert(input: rec::HostInput) -> Result<Self> {
        Ok(match input {
            rec::HostInput::Platform(input) => Self::Platform(input.try_convert_into()?),
            rec::HostInput::Window(observation) => Self::Window(observation.try_convert_into()?),
            rec::HostInput::Frame {
                vsync,
                target_present,
                presented,
            } => Self::Frame {
                vsync: EventTime::from_nanos(vsync),
                target_present: target_present.map(EventTime::from_nanos),
                presented: presented.map(|presented| {
                    let shown = FramePresented::new(EventTime::from_nanos(presented.at));
                    match presented.refresh {
                        Some(refresh) => {
                            shown.with_refresh(std::time::Duration::from_nanos(refresh))
                        }
                        None => shown,
                    }
                }),
            },
            rec::HostInput::Accessibility(request) => {
                Self::Accessibility(request.try_convert_into()?)
            }
            rec::HostInput::AssistiveFocus(node) => {
                Self::AssistiveFocus(node.map(accesskit::NodeId))
            }
        })
    }
}

impl TryConvert<&PlatformInput> for rec::PlatformInput {
    fn try_convert(input: &PlatformInput) -> Result<Self> {
        Ok(match input {
            PlatformInput::Pointer(event) => Self::Pointer(event.try_convert_into()?),
            PlatformInput::PointerFrame(frame) => Self::PointerFrame(
                frame
                    .events()
                    .iter()
                    .map(rec::PointerEvent::try_convert)
                    .collect::<Result<_>>()?,
            ),
            PlatformInput::Keyboard(event) => Self::Keyboard(event.try_convert_into()?),
            PlatformInput::Ime(event) => Self::Ime(event.try_convert_into()?),
            PlatformInput::DragDrop(event) => Self::DragDrop(event.convert_into()),
            PlatformInput::Capabilities(capabilities) => {
                Self::Capabilities(capabilities.convert_into())
            }
        })
    }
}

impl TryConvert<rec::PlatformInput> for PlatformInput {
    fn try_convert(input: rec::PlatformInput) -> Result<Self> {
        Ok(match input {
            rec::PlatformInput::Pointer(event) => Self::Pointer(event.try_convert_into()?),
            rec::PlatformInput::PointerFrame(events) => Self::PointerFrame(
                flui_platform::PointerFrame::try_new(
                    events
                        .into_iter()
                        .map(PointerEvent::try_convert)
                        .collect::<Result<_>>()?,
                )
                .map_err(|_| invalid("empty pointer frame"))?,
            ),
            rec::PlatformInput::Keyboard(event) => Self::Keyboard(event.try_convert_into()?),
            rec::PlatformInput::Ime(event) => Self::Ime(event.convert_into()),
            rec::PlatformInput::DragDrop(event) => Self::DragDrop(event.try_convert_into()?),
            rec::PlatformInput::Capabilities(capabilities) => {
                Self::Capabilities(capabilities.convert_into())
            }
        })
    }
}

// ============================================================================
// Pointer
// ============================================================================

#[cfg(feature = "testing")]
fn nonzero(value: u64, what: &str) -> Result<std::num::NonZeroU64> {
    std::num::NonZeroU64::new(value).ok_or_else(|| invalid(format!("{what} 0")))
}

#[cfg(feature = "testing")]
fn recorded_sequence(value: Option<u64>) -> Result<Option<RecordedSequence>> {
    value
        .map(|value| nonzero(value, "sequence").map(RecordedSequence::new))
        .transpose()
}

/// The window a row names; 0 is no window any host issued. Decoding a window
/// id is a recording reader's, behind `testing` as `WindowId::try_from_raw` is.
#[cfg(feature = "testing")]
pub(crate) fn window_id(value: u64) -> Result<flui_platform::WindowId> {
    flui_platform::WindowId::try_from_raw(value).ok_or_else(|| invalid("window id 0"))
}

fn pointer_id(value: u64) -> Result<PointerId> {
    PointerId::try_from(value).map_err(|_| invalid("pointer id 0"))
}

fn device_id(value: u64) -> Result<DeviceId> {
    DeviceId::try_from(value).map_err(|_| invalid("device id 0"))
}

impl TryConvert<PointerKind> for rec::PointerKind {
    fn try_convert(kind: PointerKind) -> Result<Self> {
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
            PointerKind::Pen { .. } => return Err(unmirrored("PenTool")),
            PointerKind::Trackpad => Self::Trackpad,
            PointerKind::Virtual => Self::Virtual,
            PointerKind::Unknown => Self::Unknown,
            _ => return Err(unmirrored("PointerKind")),
        })
    }
}

impl Convert<rec::PointerKind> for PointerKind {
    fn convert(kind: rec::PointerKind) -> Self {
        match kind {
            rec::PointerKind::Mouse => Self::Mouse,
            rec::PointerKind::Touch => Self::Touch,
            rec::PointerKind::Pen => Self::Pen { tool: PenTool::Tip },
            rec::PointerKind::Eraser => Self::Pen {
                tool: PenTool::Eraser,
            },
            rec::PointerKind::Trackpad => Self::Trackpad,
            rec::PointerKind::Virtual => Self::Virtual,
            rec::PointerKind::Unknown => Self::Unknown,
        }
    }
}

impl TryConvert<&PointerInfo> for rec::PointerInfo {
    fn try_convert(info: &PointerInfo) -> Result<Self> {
        Ok(Self {
            id: info.id.get().get(),
            device: info.device.map(|device| device.get().get()),
            kind: info.kind.try_convert_into()?,
            primary: info.is_primary(),
        })
    }
}

impl TryConvert<rec::PointerInfo> for PointerInfo {
    fn try_convert(info: rec::PointerInfo) -> Result<Self> {
        let mut pointer =
            Self::new(pointer_id(info.id)?, info.kind.convert_into()).with_role(if info.primary {
                PointerRole::Primary
            } else {
                PointerRole::Additional
            });
        if let Some(device) = info.device {
            pointer = pointer.with_device(device_id(device)?);
        }
        Ok(pointer)
    }
}

/// Modifier names, pinned (the W3C `getModifierState` keys).
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

fn modifiers_dto(modifiers: Modifiers) -> Vec<String> {
    MODIFIERS
        .iter()
        .filter(|(modifier, _)| modifiers.contains(*modifier))
        .map(|(_, name)| (*name).to_owned())
        .collect()
}

fn modifiers(names: &[String]) -> Result<Modifiers> {
    names.iter().try_fold(Modifiers::NONE, |held, name| {
        MODIFIERS
            .iter()
            .find(|(_, spelling)| spelling == name)
            .map(|(modifier, _)| held | *modifier)
            .ok_or_else(|| WireError::UnknownSpelling(name.clone()))
    })
}

fn buttons_dto(buttons: PointerButtons) -> Vec<u8> {
    buttons.iter().map(|button| button.number().get()).collect()
}

fn button(number: u8) -> Result<PointerButton> {
    PointerButton::try_from(number).map_err(|_| invalid(format!("pointer button {number}")))
}

fn buttons(numbers: &[u8]) -> Result<PointerButtons> {
    numbers
        .iter()
        .try_fold(PointerButtons::NONE, |held, number| {
            Ok(held.with(button(*number)?))
        })
}

fn position(x: f64, y: f64) -> Result<PointerPosition> {
    PointerPosition::try_new(Point::new(x, y)).map_err(invalid)
}

impl Convert<&PointerSample> for rec::Sample {
    fn convert(sample: &PointerSample) -> Self {
        let point = sample.position.get();
        Self {
            t: sample.time.as_nanos(),
            x: point.x,
            y: point.y,
            pressure: sample.pressure.map(Pressure::get),
            tangential_pressure: sample.tangential_pressure.map(TangentialPressure::get),
            altitude: sample.orientation.and_then(PenOrientation::altitude),
            azimuth: sample.orientation.and_then(PenOrientation::azimuth),
            twist: sample.twist.map(Twist::radians),
            hover_distance: sample.hover_distance.map(HoverDistance::get),
            contact_size: sample.contact_size.map(|size| {
                let size = size.get();
                [size.width, size.height]
            }),
        }
    }
}

impl TryConvert<rec::Sample> for PointerSample {
    fn try_convert(sample: rec::Sample) -> Result<Self> {
        let mut out = Self::new(
            EventTime::from_nanos(sample.t),
            position(sample.x, sample.y)?,
        );
        if let Some(pressure) = sample.pressure {
            out = out.with_pressure(Pressure::try_new(pressure).map_err(invalid)?);
        }
        if let Some(pressure) = sample.tangential_pressure {
            out = out
                .with_tangential_pressure(TangentialPressure::try_new(pressure).map_err(invalid)?);
        }
        let orientation = match (sample.altitude, sample.azimuth) {
            (Some(altitude), Some(azimuth)) => Some(PenOrientation::try_new(altitude, azimuth)),
            (Some(altitude), None) => Some(PenOrientation::try_altitude(altitude)),
            (None, Some(azimuth)) => Some(PenOrientation::try_azimuth(azimuth)),
            (None, None) => None,
        };
        if let Some(orientation) = orientation {
            out = out.with_orientation(orientation.map_err(invalid)?);
        }
        if let Some(twist) = sample.twist {
            out = out.with_twist(Twist::try_new(twist).map_err(invalid)?);
        }
        if let Some(distance) = sample.hover_distance {
            out = out.with_hover_distance(HoverDistance::try_new(distance).map_err(invalid)?);
        }
        if let Some([width, height]) = sample.contact_size {
            out = out.with_contact_size(
                ContactSize::try_new(Size::new(width, height)).map_err(invalid)?,
            );
        }
        Ok(out)
    }
}

fn samples(samples: Vec<rec::Sample>) -> Result<Vec<PointerSample>> {
    samples
        .into_iter()
        .map(PointerSample::try_convert)
        .collect()
}

impl<D: flui_platform::pointer::ButtonDirection> TryConvert<&PointerButtonEvent<D>>
    for rec::ButtonEvent
{
    fn try_convert(event: &PointerButtonEvent<D>) -> Result<Self> {
        Ok(Self {
            pointer: (&event.pointer).try_convert_into()?,
            button: event.button().number().get(),
            buttons: buttons_dto(event.buttons()),
            modifiers: modifiers_dto(event.modifiers),
            click_count: event.click_count.map(NonZeroU8::get),
            sample: (&event.sample).convert_into(),
        })
    }
}

/// The button event `event` records, in the direction `D` names.
fn button_event<D: flui_platform::pointer::ButtonDirection>(
    stored: rec::ButtonEvent,
) -> Result<PointerButtonEvent<D>> {
    let mut event = PointerButtonEvent::<D>::new(
        stored.pointer.try_convert_into()?,
        button(stored.button)?,
        buttons(&stored.buttons)?,
        stored.sample.try_convert_into()?,
    )
    .with_modifiers(modifiers(&stored.modifiers)?);
    if let Some(count) = stored.click_count {
        event =
            event.with_click_count(NonZeroU8::new(count).ok_or_else(|| invalid("click count 0"))?);
    }
    Ok(event)
}

impl TryConvert<&PointerSignal> for rec::Signal {
    fn try_convert(signal: &PointerSignal) -> Result<Self> {
        Ok(Self {
            pointer: (&signal.pointer).try_convert_into()?,
            t: signal.time.as_nanos(),
            position: signal.position.map(|position| {
                let point = position.get();
                [point.x, point.y]
            }),
        })
    }
}

impl TryConvert<rec::Signal> for PointerSignal {
    fn try_convert(signal: rec::Signal) -> Result<Self> {
        let mut out = Self::new(
            signal.pointer.try_convert_into()?,
            EventTime::from_nanos(signal.t),
        );
        if let Some([x, y]) = signal.position {
            out = out.with_position(position(x, y)?);
        }
        Ok(out)
    }
}

fn device_change_dto(change: &PointerDeviceChange) -> Result<rec::DeviceChange> {
    Ok(rec::DeviceChange {
        device: change.device.get().get(),
        kind: change.kind.try_convert_into()?,
        t: change.time.as_nanos(),
    })
}

fn device_change(change: rec::DeviceChange) -> Result<PointerDeviceChange> {
    Ok(PointerDeviceChange::new(
        device_id(change.device)?,
        change.kind.convert_into(),
        EventTime::from_nanos(change.t),
    ))
}

impl TryConvert<&PointerEvent> for rec::PointerEvent {
    fn try_convert(event: &PointerEvent) -> Result<Self> {
        Ok(match event {
            PointerEvent::Down(press) => Self::Down(press.try_convert_into()?),
            PointerEvent::ButtonChange(ButtonChange::Pressed(press)) => {
                Self::ButtonPressed(press.try_convert_into()?)
            }
            PointerEvent::ButtonChange(ButtonChange::Released(release)) => {
                Self::ButtonReleased(release.try_convert_into()?)
            }
            PointerEvent::Up(release) => Self::Up(release.try_convert_into()?),
            PointerEvent::Move(motion) => Self::Move {
                pointer: (&motion.pointer).try_convert_into()?,
                buttons: buttons_dto(motion.buttons),
                modifiers: modifiers_dto(motion.modifiers),
                current: motion.current().convert_into(),
                coalesced: motion
                    .coalesced()
                    .iter()
                    .map(rec::Sample::convert)
                    .collect(),
                predicted: motion
                    .predicted()
                    .iter()
                    .map(rec::Sample::convert)
                    .collect(),
            },
            PointerEvent::Cancel(cancel) => Self::Cancel {
                pointer: (&cancel.pointer).try_convert_into()?,
                t: cancel.time.as_nanos(),
                reason: match cancel.reason {
                    CancelReason::Platform => rec::CancelReason::Platform,
                    CancelReason::CaptureLost => rec::CancelReason::CaptureLost,
                    CancelReason::FocusLost => rec::CancelReason::FocusLost,
                    CancelReason::DeviceRemoved => rec::CancelReason::DeviceRemoved,
                    CancelReason::InvalidInput => rec::CancelReason::InvalidInput,
                    CancelReason::Claimed => rec::CancelReason::Claimed,
                    CancelReason::Superseded => rec::CancelReason::Superseded,
                    _ => return Err(unmirrored("CancelReason")),
                },
            },
            PointerEvent::Enter(signal) => Self::Enter(signal.try_convert_into()?),
            PointerEvent::Leave(signal) => Self::Leave(signal.try_convert_into()?),
            PointerEvent::Scroll(scroll) => {
                let point = scroll.position.get();
                Self::Scroll(rec::Scroll {
                    pointer: (&scroll.pointer).try_convert_into()?,
                    t: scroll.time.as_nanos(),
                    x: point.x,
                    y: point.y,
                    unit: match scroll.delta.unit() {
                        ScrollUnit::Lines => rec::ScrollUnit::Lines,
                        ScrollUnit::Pixels => rec::ScrollUnit::Pixels,
                        ScrollUnit::Pages => rec::ScrollUnit::Pages,
                        ScrollUnit::Detents => rec::ScrollUnit::Detents,
                        _ => return Err(unmirrored("ScrollUnit")),
                    },
                    dx: scroll.delta.x(),
                    dy: scroll.delta.y(),
                    precision: match scroll.precision {
                        ScrollPrecision::Unknown => rec::ScrollPrecision::Unknown,
                        ScrollPrecision::Notched => rec::ScrollPrecision::Notched,
                        ScrollPrecision::Precise => rec::ScrollPrecision::Precise,
                        _ => return Err(unmirrored("ScrollPrecision")),
                    },
                    phase: scroll
                        .phase
                        .map(|phase| {
                            Ok(match phase {
                                ScrollPhase::Began => rec::ScrollPhase::Began,
                                ScrollPhase::Changed => rec::ScrollPhase::Changed,
                                ScrollPhase::Ended => rec::ScrollPhase::Ended,
                                ScrollPhase::Cancelled => rec::ScrollPhase::Cancelled,
                                ScrollPhase::MomentumBegan => rec::ScrollPhase::MomentumBegan,
                                ScrollPhase::MomentumChanged => rec::ScrollPhase::MomentumChanged,
                                ScrollPhase::MomentumEnded => rec::ScrollPhase::MomentumEnded,
                                _ => return Err(unmirrored("ScrollPhase")),
                            })
                        })
                        .transpose()?,
                    modifiers: modifiers_dto(scroll.modifiers),
                })
            }
            PointerEvent::ScrollInertiaCancel(signal) => {
                Self::ScrollInertiaCancel(signal.try_convert_into()?)
            }
            PointerEvent::PanZoom(gesture) => {
                let point = gesture.position.get();
                Self::PanZoom(rec::PanZoom {
                    pointer: gesture.pointer().try_convert_into()?,
                    t: gesture.time.as_nanos(),
                    x: point.x,
                    y: point.y,
                    phase: match gesture.phase {
                        PanZoomPhase::Start => rec::PanZoomPhase::Start,
                        PanZoomPhase::Update(transform) => rec::PanZoomPhase::Update {
                            scale: transform.scale(),
                            rotation: transform.rotation(),
                        },
                        PanZoomPhase::End => rec::PanZoomPhase::End,
                        PanZoomPhase::Cancelled => rec::PanZoomPhase::Cancelled,
                        _ => return Err(unmirrored("PanZoomPhase")),
                    },
                    modifiers: modifiers_dto(gesture.modifiers),
                })
            }
            PointerEvent::SmartZoom(zoom) => {
                let point = zoom.position.get();
                Self::SmartZoom(rec::SmartZoom {
                    pointer: zoom.pointer().try_convert_into()?,
                    t: zoom.time.as_nanos(),
                    x: point.x,
                    y: point.y,
                    modifiers: modifiers_dto(zoom.modifiers),
                })
            }
            PointerEvent::Revise(revision) => Self::Revise {
                pointer: (&revision.pointer).try_convert_into()?,
                sample: (&revision.sample).convert_into(),
            },
            PointerEvent::PenSqueeze(squeeze) => Self::PenSqueeze {
                pointer: (&squeeze.pointer).try_convert_into()?,
                t: squeeze.time.as_nanos(),
                phase: match squeeze.phase {
                    PenSqueezePhase::Began => rec::SqueezePhase::Began,
                    PenSqueezePhase::Changed => rec::SqueezePhase::Changed,
                    PenSqueezePhase::Ended => rec::SqueezePhase::Ended,
                    PenSqueezePhase::Cancelled => rec::SqueezePhase::Cancelled,
                    _ => return Err(unmirrored("PenSqueezePhase")),
                },
                hover: squeeze.hover.as_ref().map(rec::Sample::convert),
            },
            PointerEvent::DeviceAdded(change) => Self::DeviceAdded(device_change_dto(change)?),
            PointerEvent::DeviceRemoved(change) => Self::DeviceRemoved(device_change_dto(change)?),
            _ => return Err(unmirrored("PointerEvent")),
        })
    }
}

impl TryConvert<rec::PointerEvent> for PointerEvent {
    fn try_convert(event: rec::PointerEvent) -> Result<Self> {
        Ok(match event {
            rec::PointerEvent::Down(press) => Self::Down(button_event(press)?),
            rec::PointerEvent::ButtonPressed(press) => {
                Self::ButtonChange(ButtonChange::Pressed(button_event(press)?))
            }
            rec::PointerEvent::ButtonReleased(release) => {
                Self::ButtonChange(ButtonChange::Released(button_event(release)?))
            }
            rec::PointerEvent::Up(release) => Self::Up(button_event(release)?),
            rec::PointerEvent::Move {
                pointer,
                buttons: held,
                modifiers: names,
                current,
                coalesced,
                predicted,
            } => Self::Move(
                PointerMove::new(
                    pointer.try_convert_into()?,
                    buttons(&held)?,
                    current.try_convert_into()?,
                )
                .with_modifiers(modifiers(&names)?)
                .with_coalesced(samples(coalesced)?)
                .with_predicted(samples(predicted)?),
            ),
            rec::PointerEvent::Cancel { pointer, t, reason } => Self::Cancel(PointerCancel::new(
                pointer.try_convert_into()?,
                EventTime::from_nanos(t),
                match reason {
                    rec::CancelReason::Platform => CancelReason::Platform,
                    rec::CancelReason::CaptureLost => CancelReason::CaptureLost,
                    rec::CancelReason::FocusLost => CancelReason::FocusLost,
                    rec::CancelReason::DeviceRemoved => CancelReason::DeviceRemoved,
                    rec::CancelReason::InvalidInput => CancelReason::InvalidInput,
                    rec::CancelReason::Claimed => CancelReason::Claimed,
                    rec::CancelReason::Superseded => CancelReason::Superseded,
                },
            )),
            rec::PointerEvent::Enter(signal) => Self::Enter(signal.try_convert_into()?),
            rec::PointerEvent::Leave(signal) => Self::Leave(signal.try_convert_into()?),
            rec::PointerEvent::Scroll(scroll) => {
                let unit = match scroll.unit {
                    rec::ScrollUnit::Lines => ScrollUnit::Lines,
                    rec::ScrollUnit::Pixels => ScrollUnit::Pixels,
                    rec::ScrollUnit::Pages => ScrollUnit::Pages,
                    rec::ScrollUnit::Detents => ScrollUnit::Detents,
                };
                let mut event = ScrollEvent::new(
                    scroll.pointer.try_convert_into()?,
                    EventTime::from_nanos(scroll.t),
                    position(scroll.x, scroll.y)?,
                    ScrollDelta::try_new(unit, scroll.dx, scroll.dy).map_err(invalid)?,
                )
                .with_precision(match scroll.precision {
                    rec::ScrollPrecision::Unknown => ScrollPrecision::Unknown,
                    rec::ScrollPrecision::Notched => ScrollPrecision::Notched,
                    rec::ScrollPrecision::Precise => ScrollPrecision::Precise,
                })
                .with_modifiers(modifiers(&scroll.modifiers)?);
                if let Some(phase) = scroll.phase {
                    event = event.with_phase(match phase {
                        rec::ScrollPhase::Began => ScrollPhase::Began,
                        rec::ScrollPhase::Changed => ScrollPhase::Changed,
                        rec::ScrollPhase::Ended => ScrollPhase::Ended,
                        rec::ScrollPhase::Cancelled => ScrollPhase::Cancelled,
                        rec::ScrollPhase::MomentumBegan => ScrollPhase::MomentumBegan,
                        rec::ScrollPhase::MomentumChanged => ScrollPhase::MomentumChanged,
                        rec::ScrollPhase::MomentumEnded => ScrollPhase::MomentumEnded,
                    });
                }
                Self::Scroll(event)
            }
            rec::PointerEvent::ScrollInertiaCancel(signal) => {
                Self::ScrollInertiaCancel(signal.try_convert_into()?)
            }
            rec::PointerEvent::PanZoom(gesture) => Self::PanZoom(
                PanZoomEvent::new(
                    gesture.pointer.try_convert_into()?,
                    EventTime::from_nanos(gesture.t),
                    position(gesture.x, gesture.y)?,
                    match gesture.phase {
                        rec::PanZoomPhase::Start => PanZoomPhase::Start,
                        rec::PanZoomPhase::Update { scale, rotation } => PanZoomPhase::Update(
                            PanZoomTransform::try_new(scale, rotation).map_err(invalid)?,
                        ),
                        rec::PanZoomPhase::End => PanZoomPhase::End,
                        rec::PanZoomPhase::Cancelled => PanZoomPhase::Cancelled,
                    },
                )
                .with_modifiers(modifiers(&gesture.modifiers)?),
            ),
            rec::PointerEvent::SmartZoom(zoom) => Self::SmartZoom(
                SmartZoomEvent::new(
                    zoom.pointer.try_convert_into()?,
                    EventTime::from_nanos(zoom.t),
                    position(zoom.x, zoom.y)?,
                )
                .with_modifiers(modifiers(&zoom.modifiers)?),
            ),
            rec::PointerEvent::Revise { pointer, sample } => Self::Revise(PointerRevision::new(
                pointer.try_convert_into()?,
                sample.try_convert_into()?,
            )),
            rec::PointerEvent::PenSqueeze {
                pointer,
                t,
                phase,
                hover,
            } => {
                let mut squeeze = PenSqueeze::new(
                    pointer.try_convert_into()?,
                    EventTime::from_nanos(t),
                    match phase {
                        rec::SqueezePhase::Began => PenSqueezePhase::Began,
                        rec::SqueezePhase::Changed => PenSqueezePhase::Changed,
                        rec::SqueezePhase::Ended => PenSqueezePhase::Ended,
                        rec::SqueezePhase::Cancelled => PenSqueezePhase::Cancelled,
                    },
                );
                if let Some(hover) = hover {
                    squeeze = squeeze.with_hover(hover.try_convert_into()?);
                }
                Self::PenSqueeze(squeeze)
            }
            rec::PointerEvent::DeviceAdded(change) => Self::DeviceAdded(device_change(change)?),
            rec::PointerEvent::DeviceRemoved(change) => Self::DeviceRemoved(device_change(change)?),
        })
    }
}

// ============================================================================
// Keyboard and IME
// ============================================================================

impl TryConvert<&KeyEvent> for rec::KeyEvent {
    fn try_convert(event: &KeyEvent) -> Result<Self> {
        Ok(Self {
            state: match event.state() {
                KeyState::Down => rec::KeyState::Down,
                KeyState::Up => rec::KeyState::Up,
            },
            key: match &event.key {
                Key::Character(text) => rec::Key::Character(text.as_str().to_owned()),
                Key::Named(named) => rec::Key::Named(named.as_str().to_owned()),
                _ => return Err(unmirrored("Key")),
            },
            code: event.code.as_str().to_owned(),
            location: match event.location {
                Location::Standard => rec::Location::Standard,
                Location::Left => rec::Location::Left,
                Location::Right => rec::Location::Right,
                Location::Numpad => rec::Location::Numpad,
            },
            modifiers: modifiers_dto(event.modifiers),
            repeat: event.repeat() == KeyRepeat::AutoRepeat,
            composing: event.composition == ImeComposition::Active,
            unreleased: event.release() == KeyRelease::Never,
            t: event.time.as_nanos(),
        })
    }
}

impl TryConvert<rec::KeyEvent> for KeyEvent {
    fn try_convert(event: rec::KeyEvent) -> Result<Self> {
        let key = match event.key {
            rec::Key::Character(text) => Key::character(text),
            rec::Key::Named(name) => {
                Key::Named(NamedKey::from_w3c(&name).ok_or(WireError::UnknownSpelling(name))?)
            }
        };
        let code = Code::from_w3c(&event.code)
            .ok_or_else(|| WireError::UnknownSpelling(event.code.clone()))?;
        Ok(Self::new(
            match event.state {
                rec::KeyState::Down => KeyState::Down,
                rec::KeyState::Up => KeyState::Up,
            },
            key,
            code,
            EventTime::from_nanos(event.t),
        )
        .with_location(match event.location {
            rec::Location::Standard => Location::Standard,
            rec::Location::Left => Location::Left,
            rec::Location::Right => Location::Right,
            rec::Location::Numpad => Location::Numpad,
        })
        .with_modifiers(modifiers(&event.modifiers)?)
        .with_repeat(if event.repeat {
            KeyRepeat::AutoRepeat
        } else {
            KeyRepeat::First
        })
        .with_composition(if event.composing {
            ImeComposition::Active
        } else {
            ImeComposition::Inactive
        })
        .with_release(if event.unreleased {
            KeyRelease::Never
        } else {
            KeyRelease::Follows
        }))
    }
}

impl TryConvert<&ImeEvent> for rec::ImeEvent {
    fn try_convert(event: &ImeEvent) -> Result<Self> {
        Ok(match event {
            ImeEvent::Enabled => Self::Enabled,
            ImeEvent::Preedit { text, cursor } => Self::Preedit {
                text: text.clone(),
                cursor: *cursor,
            },
            ImeEvent::Commit(text) => Self::Commit(text.clone()),
            ImeEvent::Disabled => Self::Disabled,
            _ => return Err(unmirrored("ImeEvent")),
        })
    }
}

impl Convert<rec::ImeEvent> for ImeEvent {
    fn convert(event: rec::ImeEvent) -> Self {
        match event {
            rec::ImeEvent::Enabled => Self::Enabled,
            rec::ImeEvent::Preedit { text, cursor } => Self::Preedit { text, cursor },
            rec::ImeEvent::Commit(text) => Self::Commit(text),
            rec::ImeEvent::Disabled => Self::Disabled,
        }
    }
}

// ============================================================================
// Drag and drop, capabilities
// ============================================================================

impl Convert<DataTransferId> for rec::TransferId {
    fn convert(id: DataTransferId) -> Self {
        Self {
            index: id.index(),
            generation: id.generation().get(),
        }
    }
}

impl TryConvert<rec::TransferId> for DataTransferId {
    fn try_convert(id: rec::TransferId) -> Result<Self> {
        Ok(Self::new_gen(
            id.index,
            NonZeroU32::new(id.generation).ok_or_else(|| invalid("transfer generation 0"))?,
        ))
    }
}

fn actions(bits: u8) -> Result<TransferActions> {
    TransferActions::from_bits(bits).ok_or_else(|| invalid(format!("transfer actions {bits}")))
}

fn point_dto(point: Point<f64>) -> [f64; 2] {
    [point.x, point.y]
}

impl Convert<&DragDropEvent> for rec::DragDrop {
    fn convert(event: &DragDropEvent) -> Self {
        match event {
            DragDropEvent::Entered {
                offer,
                allowed,
                position,
            } => Self::Entered {
                id: offer.id().convert_into(),
                representations: offer
                    .representations()
                    .iter()
                    .map(|descriptor| rec::Representation {
                        format: match &descriptor.format {
                            TransferFormat::Text => rec::TransferFormat::Text,
                            TransferFormat::Html => rec::TransferFormat::Html,
                            TransferFormat::UriList => rec::TransferFormat::UriList,
                            TransferFormat::Image { mime } => rec::TransferFormat::Image {
                                mime: mime.as_str().to_owned(),
                            },
                            TransferFormat::Custom { mime } => rec::TransferFormat::Custom {
                                mime: mime.as_str().to_owned(),
                            },
                        },
                        declared_len: descriptor.declared_len,
                    })
                    .collect(),
                allowed: allowed.bits(),
                position: position.map(point_dto),
            },
            DragDropEvent::Moved {
                id,
                allowed,
                position,
            } => Self::Moved {
                id: (*id).convert_into(),
                allowed: allowed.bits(),
                position: point_dto(*position),
            },
            DragDropEvent::Dropped {
                id,
                action,
                position,
            } => Self::Dropped {
                id: (*id).convert_into(),
                action: action.bits(),
                position: position.map(point_dto),
            },
            DragDropEvent::Exited { id } => Self::Exited {
                id: (*id).convert_into(),
            },
        }
    }
}

impl TryConvert<rec::DragDrop> for DragDropEvent {
    fn try_convert(event: rec::DragDrop) -> Result<Self> {
        let point = |[x, y]: [f64; 2]| Point::new(x, y);
        Ok(match event {
            rec::DragDrop::Entered {
                id,
                representations,
                allowed,
                position,
            } => Self::Entered {
                offer: DataTransferOffer::new(
                    id.try_convert_into()?,
                    representations
                        .into_iter()
                        .map(|representation| RepresentationDescriptor {
                            format: match representation.format {
                                rec::TransferFormat::Text => TransferFormat::Text,
                                rec::TransferFormat::Html => TransferFormat::Html,
                                rec::TransferFormat::UriList => TransferFormat::UriList,
                                rec::TransferFormat::Image { mime } => TransferFormat::Image {
                                    mime: Mime::new(&mime),
                                },
                                rec::TransferFormat::Custom { mime } => TransferFormat::Custom {
                                    mime: Mime::new(&mime),
                                },
                            },
                            declared_len: representation.declared_len,
                        })
                        .collect::<Arc<[_]>>(),
                ),
                allowed: actions(allowed)?,
                position: position.map(point),
            },
            rec::DragDrop::Moved {
                id,
                allowed,
                position,
            } => Self::Moved {
                id: id.try_convert_into()?,
                allowed: actions(allowed)?,
                position: point(position),
            },
            rec::DragDrop::Dropped {
                id,
                action,
                position,
            } => Self::Dropped {
                id: id.try_convert_into()?,
                action: actions(action)?,
                position: position.map(point),
            },
            rec::DragDrop::Exited { id } => Self::Exited {
                id: id.try_convert_into()?,
            },
        })
    }
}

impl Convert<Availability> for rec::Availability {
    fn convert(availability: Availability) -> Self {
        match availability {
            Availability::Native => Self::Native,
            Availability::Emulated => Self::Emulated,
            Availability::Unavailable => Self::Unavailable,
        }
    }
}

impl Convert<rec::Availability> for Availability {
    fn convert(availability: rec::Availability) -> Self {
        match availability {
            rec::Availability::Native => Self::Native,
            rec::Availability::Emulated => Self::Emulated,
            rec::Availability::Unavailable => Self::Unavailable,
        }
    }
}

/// Each present kind, pinned to its kind spelling.
const PRESENT_KINDS: [(PointerKindSet, rec::PointerKind); 5] = [
    (PointerKindSet::MOUSE, rec::PointerKind::Mouse),
    (PointerKindSet::TOUCH, rec::PointerKind::Touch),
    (PointerKindSet::PEN, rec::PointerKind::Pen),
    (PointerKindSet::TRACKPAD, rec::PointerKind::Trackpad),
    (PointerKindSet::VIRTUAL, rec::PointerKind::Virtual),
];

impl Convert<&InputCapabilities> for rec::Capabilities {
    fn convert(capabilities: &InputCapabilities) -> Self {
        // Every edge or button of a kind shares its availability: one stands for all.
        let region = |kind| rec::Availability::convert(capabilities.input_region(kind));
        Self {
            pen: rec::Availability::convert(capabilities.pen()),
            precise_scroll: rec::Availability::convert(capabilities.precise_scroll()),
            trackpad_gestures: rec::Availability::convert(capabilities.trackpad_gestures()),
            system_gesture_exclusion: region(InputRegionKind::SystemGestureExclusion),
            window_move: region(InputRegionKind::WindowMove),
            window_resize: region(InputRegionKind::WindowResize(ResizeEdge::Top)),
            caption_buttons: region(InputRegionKind::CaptionButton(CaptionButton::Close)),
            outgoing_drag: rec::Availability::convert(capabilities.outgoing_drag()),
            present: PRESENT_KINDS
                .iter()
                .filter(|(kind, _)| capabilities.present().contains(*kind))
                .map(|(_, dto)| *dto)
                .collect(),
        }
    }
}

impl Convert<rec::Capabilities> for InputCapabilities {
    fn convert(capabilities: rec::Capabilities) -> Self {
        let present = capabilities
            .present
            .iter()
            .fold(PointerKindSet::empty(), |set, kind| {
                set | PointerKindSet::of((*kind).convert_into())
            });
        Self::builder()
            .pen(Availability::convert(capabilities.pen))
            .precise_scroll(Availability::convert(capabilities.precise_scroll))
            .trackpad_gestures(Availability::convert(capabilities.trackpad_gestures))
            .system_gesture_exclusion(Availability::convert(capabilities.system_gesture_exclusion))
            .window_move(Availability::convert(capabilities.window_move))
            .window_resize(Availability::convert(capabilities.window_resize))
            .caption_buttons(Availability::convert(capabilities.caption_buttons))
            .outgoing_drag(Availability::convert(capabilities.outgoing_drag))
            .present(present)
            .build()
    }
}

// ============================================================================
// Window observations
// ============================================================================

impl TryConvert<WindowExecutionState> for rec::Execution {
    fn try_convert(state: WindowExecutionState) -> Result<Self> {
        Ok(match state {
            WindowExecutionState::Running => Self::Running,
            WindowExecutionState::Suspended => Self::Suspended,
            WindowExecutionState::Detached => Self::Detached,
            _ => return Err(unmirrored("WindowExecutionState")),
        })
    }
}

impl Convert<rec::Execution> for WindowExecutionState {
    fn convert(state: rec::Execution) -> Self {
        match state {
            rec::Execution::Running => Self::Running,
            rec::Execution::Suspended => Self::Suspended,
            rec::Execution::Detached => Self::Detached,
        }
    }
}

impl TryConvert<&WindowObservation> for rec::WindowObservation {
    fn try_convert(observation: &WindowObservation) -> Result<Self> {
        Ok(match *observation {
            WindowObservation::Metrics(metrics) => Self::Metrics {
                width: metrics.physical().width,
                height: metrics.physical().height,
                scale_factor: metrics.scale().get(),
            },
            WindowObservation::Focus(focused) => Self::Focus(focused),
            WindowObservation::Visibility(visible) => Self::Visibility(visible),
            WindowObservation::Execution(state) => Self::Execution(state.try_convert_into()?),
            WindowObservation::Snapshot {
                execution,
                focused,
                visible,
            } => Self::Snapshot {
                execution: execution.try_convert_into()?,
                focused,
                visible,
            },
            WindowObservation::Hover(inside) => Self::Hover(inside),
            WindowObservation::Moved => Self::Moved,
            WindowObservation::Insets(insets) => Self::Insets {
                safe_area: edges_to_wire(insets.safe_area()),
                keyboard: keyboard_to_wire(insets.keyboard())?,
                system_gestures: edges_to_wire(insets.system_gestures()),
            },
            WindowObservation::Brightness(brightness) => Self::Brightness(match brightness {
                Brightness::Light => rec::Brightness::Light,
                Brightness::Dark => rec::Brightness::Dark,
            }),
            _ => return Err(unmirrored("WindowObservation")),
        })
    }
}

impl TryConvert<rec::WindowObservation> for WindowObservation {
    fn try_convert(observation: rec::WindowObservation) -> Result<Self> {
        Ok(match observation {
            rec::WindowObservation::Metrics {
                width,
                height,
                scale_factor,
            } => Self::Metrics(
                WindowMetrics::try_new(
                    flui_foundation::geometry::DeviceSize::new(width, height),
                    scale_factor,
                )
                .map_err(invalid)?,
            ),
            rec::WindowObservation::Focus(focused) => Self::Focus(focused),
            rec::WindowObservation::Visibility(visible) => Self::Visibility(visible),
            rec::WindowObservation::Execution(state) => Self::Execution(state.convert_into()),
            rec::WindowObservation::Snapshot {
                execution,
                focused,
                visible,
            } => Self::Snapshot {
                execution: execution.convert_into(),
                focused,
                visible,
            },
            rec::WindowObservation::Hover(inside) => Self::Hover(inside),
            rec::WindowObservation::Moved => Self::Moved,
            rec::WindowObservation::Insets {
                safe_area,
                keyboard,
                system_gestures,
            } => Self::Insets(
                WindowInsets::try_new(
                    edges_from_wire(safe_area),
                    keyboard
                        .map(keyboard_from_wire)
                        .transpose()?
                        .unwrap_or(KeyboardInset::HIDDEN),
                    edges_from_wire(system_gestures),
                )
                .map_err(invalid)?,
            ),
            rec::WindowObservation::Brightness(brightness) => Self::Brightness(match brightness {
                rec::Brightness::Light => Brightness::Light,
                rec::Brightness::Dark => Brightness::Dark,
            }),
        })
    }
}

fn edges_to_wire(edges: EdgeInsets) -> [f64; 4] {
    [edges.top, edges.right, edges.bottom, edges.left]
}

fn edges_from_wire([top, right, bottom, left]: [f64; 4]) -> EdgeInsets {
    EdgeInsets::new(top, right, bottom, left)
}

/// `keyboard` in the recording; `None` for a hidden keyboard at rest.
fn keyboard_to_wire(keyboard: KeyboardInset) -> Result<Option<wire::KeyboardInset>> {
    if keyboard == KeyboardInset::HIDDEN {
        return Ok(None);
    }
    let motion = keyboard
        .motion()
        .map(|motion| {
            Ok(wire::InsetMotion {
                from: edges_to_wire(motion.from()),
                start_ns: motion.start().as_nanos(),
                duration_ns: u64::try_from(motion.duration().as_nanos())
                    .map_err(|_| invalid("a keyboard motion longer than the timeline"))?,
                curve: match motion.curve() {
                    InsetCurve::Linear => wire::InsetCurve::Linear,
                    InsetCurve::CubicBezier { x1, y1, x2, y2 } => {
                        wire::InsetCurve::CubicBezier { x1, y1, x2, y2 }
                    }
                    InsetCurve::SystemKeyboard => wire::InsetCurve::SystemKeyboard,
                    _ => return Err(unmirrored("InsetCurve")),
                },
            })
        })
        .transpose()?;
    Ok(Some(wire::KeyboardInset {
        target: edges_to_wire(keyboard.target()),
        motion,
    }))
}

/// The keyboard `wire` describes, validated as its types validate it.
fn keyboard_from_wire(wire: wire::KeyboardInset) -> Result<KeyboardInset> {
    let target = edges_from_wire(wire.target);
    let Some(motion) = wire.motion else {
        return KeyboardInset::try_at_rest(target).map_err(invalid);
    };
    let curve = match motion.curve {
        wire::InsetCurve::Linear => InsetCurve::Linear,
        wire::InsetCurve::CubicBezier { x1, y1, x2, y2 } => {
            InsetCurve::CubicBezier { x1, y1, x2, y2 }
        }
        wire::InsetCurve::SystemKeyboard => InsetCurve::SystemKeyboard,
        _ => return Err(invalid("a keyboard curve this build does not read")),
    };
    let motion = InsetMotion::try_new(
        edges_from_wire(motion.from),
        EventTime::from_nanos(motion.start_ns),
        Duration::from_nanos(motion.duration_ns),
        curve,
    )
    .map_err(invalid)?;
    KeyboardInset::try_moving(target, motion).map_err(invalid)
}

// ============================================================================
// Accessibility
// ============================================================================

/// accesskit's actions, pinned to these spellings.
const ACTIONS: [(accesskit::Action, &str); 22] = [
    (accesskit::Action::Click, "click"),
    (accesskit::Action::Focus, "focus"),
    (accesskit::Action::Blur, "blur"),
    (accesskit::Action::Collapse, "collapse"),
    (accesskit::Action::Expand, "expand"),
    (accesskit::Action::CustomAction, "custom_action"),
    (accesskit::Action::Decrement, "decrement"),
    (accesskit::Action::Increment, "increment"),
    (accesskit::Action::HideTooltip, "hide_tooltip"),
    (accesskit::Action::ShowTooltip, "show_tooltip"),
    (
        accesskit::Action::ReplaceSelectedText,
        "replace_selected_text",
    ),
    (accesskit::Action::ScrollDown, "scroll_down"),
    (accesskit::Action::ScrollLeft, "scroll_left"),
    (accesskit::Action::ScrollRight, "scroll_right"),
    (accesskit::Action::ScrollUp, "scroll_up"),
    (accesskit::Action::ScrollIntoView, "scroll_into_view"),
    (accesskit::Action::ScrollToPoint, "scroll_to_point"),
    (accesskit::Action::SetScrollOffset, "set_scroll_offset"),
    (accesskit::Action::SetTextSelection, "set_text_selection"),
    (
        accesskit::Action::SetSequentialFocusNavigationStartingPoint,
        "set_sequential_focus_navigation_starting_point",
    ),
    (accesskit::Action::SetValue, "set_value"),
    (accesskit::Action::ShowContextMenu, "show_context_menu"),
];

fn text_position_dto(position: accesskit::TextPosition) -> rec::TextPosition {
    rec::TextPosition {
        node: position.node.0,
        character_index: position.character_index,
    }
}

fn text_position(position: rec::TextPosition) -> accesskit::TextPosition {
    accesskit::TextPosition {
        node: accesskit::NodeId(position.node),
        character_index: position.character_index,
    }
}

/// The pinned spelling of `action` in `table`.
fn action_name(
    table: &[(accesskit::Action, &'static str)],
    action: accesskit::Action,
) -> Result<&'static str> {
    table
        .iter()
        .find(|(known, _)| *known == action)
        .map(|(_, name)| *name)
        .ok_or(unmirrored("accesskit::Action"))
}

impl TryConvert<&accesskit::ActionRequest> for rec::ActionRequest {
    fn try_convert(request: &accesskit::ActionRequest) -> Result<Self> {
        let action = action_name(&ACTIONS, request.action)?;
        Ok(Self {
            action: action.to_owned(),
            tree: format!("{:032x}", request.target_tree.0.as_u128()),
            node: request.target_node.0,
            data: request.data.as_ref().map(|data| match data {
                accesskit::ActionData::CustomAction(id) => rec::ActionData::CustomAction(*id),
                accesskit::ActionData::Value(text) => rec::ActionData::Value(text.to_string()),
                accesskit::ActionData::NumericValue(value) => rec::ActionData::NumericValue(*value),
                accesskit::ActionData::ScrollUnit(unit) => {
                    rec::ActionData::ScrollUnit(match unit {
                        accesskit::ScrollUnit::Item => rec::AxScrollUnit::Item,
                        accesskit::ScrollUnit::Page => rec::AxScrollUnit::Page,
                    })
                }
                accesskit::ActionData::ScrollHint(hint) => {
                    rec::ActionData::ScrollHint(match hint {
                        accesskit::ScrollHint::TopLeft => rec::ScrollHint::TopLeft,
                        accesskit::ScrollHint::BottomRight => rec::ScrollHint::BottomRight,
                        accesskit::ScrollHint::TopEdge => rec::ScrollHint::TopEdge,
                        accesskit::ScrollHint::BottomEdge => rec::ScrollHint::BottomEdge,
                        accesskit::ScrollHint::LeftEdge => rec::ScrollHint::LeftEdge,
                        accesskit::ScrollHint::RightEdge => rec::ScrollHint::RightEdge,
                    })
                }
                accesskit::ActionData::ScrollToPoint(point) => {
                    rec::ActionData::ScrollToPoint([point.x, point.y])
                }
                accesskit::ActionData::SetScrollOffset(point) => {
                    rec::ActionData::SetScrollOffset([point.x, point.y])
                }
                accesskit::ActionData::SetTextSelection(selection) => {
                    rec::ActionData::SetTextSelection {
                        anchor: text_position_dto(selection.anchor),
                        focus: text_position_dto(selection.focus),
                    }
                }
            }),
        })
    }
}

impl TryConvert<rec::ActionRequest> for accesskit::ActionRequest {
    fn try_convert(request: rec::ActionRequest) -> Result<Self> {
        let action = ACTIONS
            .iter()
            .find(|(_, name)| *name == request.action)
            .map(|(action, _)| *action)
            .ok_or(WireError::UnknownSpelling(request.action))?;
        let tree = u128::from_str_radix(&request.tree, 16)
            .map_err(|_| invalid(format!("tree id {}", request.tree)))?;
        Ok(Self {
            action,
            target_tree: accesskit::TreeId(accesskit::Uuid::from_u128(tree)),
            target_node: accesskit::NodeId(request.node),
            data: request.data.map(|data| match data {
                rec::ActionData::CustomAction(id) => accesskit::ActionData::CustomAction(id),
                rec::ActionData::Value(text) => accesskit::ActionData::Value(text.into()),
                rec::ActionData::NumericValue(value) => accesskit::ActionData::NumericValue(value),
                rec::ActionData::ScrollUnit(unit) => {
                    accesskit::ActionData::ScrollUnit(match unit {
                        rec::AxScrollUnit::Item => accesskit::ScrollUnit::Item,
                        rec::AxScrollUnit::Page => accesskit::ScrollUnit::Page,
                    })
                }
                rec::ActionData::ScrollHint(hint) => {
                    accesskit::ActionData::ScrollHint(match hint {
                        rec::ScrollHint::TopLeft => accesskit::ScrollHint::TopLeft,
                        rec::ScrollHint::BottomRight => accesskit::ScrollHint::BottomRight,
                        rec::ScrollHint::TopEdge => accesskit::ScrollHint::TopEdge,
                        rec::ScrollHint::BottomEdge => accesskit::ScrollHint::BottomEdge,
                        rec::ScrollHint::LeftEdge => accesskit::ScrollHint::LeftEdge,
                        rec::ScrollHint::RightEdge => accesskit::ScrollHint::RightEdge,
                    })
                }
                rec::ActionData::ScrollToPoint([x, y]) => {
                    accesskit::ActionData::ScrollToPoint(accesskit::Point { x, y })
                }
                rec::ActionData::SetScrollOffset([x, y]) => {
                    accesskit::ActionData::SetScrollOffset(accesskit::Point { x, y })
                }
                rec::ActionData::SetTextSelection { anchor, focus } => {
                    accesskit::ActionData::SetTextSelection(accesskit::TextSelection {
                        anchor: text_position(anchor),
                        focus: text_position(focus),
                    })
                }
            }),
        })
    }
}

// ============================================================================
// System preferences
// ============================================================================

fn nanos(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

/// `values` in the shared schema, through [`PreferenceParts`]: a field added
/// there fails to compile here until it is mirrored.
fn preferences_wire(values: SystemPreferences) -> Result<wire::Preferences> {
    let PreferenceParts {
        text_scale,
        motion,
        high_contrast,
        haptics,
        text_weight,
        locales,
        regional,
        gestures:
            GestureParts {
                double_click_interval,
                double_tap_interval,
                long_press_timeout,
                native_mouse_geometry,
                native_touch_geometry,
            },
        wheel: WheelParts {
            vertical,
            horizontal_characters,
        },
    } = values.into_parts();
    Ok(wire::Preferences {
        text_scale,
        motion: motion
            .map(|motion| {
                Ok(match motion {
                    MotionPreference::NoPreference => wire::Motion::NoPreference,
                    MotionPreference::Reduce => wire::Motion::Reduce,
                    MotionPreference::Scaled(scale) => wire::Motion::Scaled(scale.get()),
                    _ => return Err(unmirrored("MotionPreference")),
                })
            })
            .transpose()?,
        high_contrast,
        haptics,
        text_weight: text_weight
            .map(|weight| {
                Ok(match weight {
                    TextWeightPreference::NoPreference => wire::TextWeight::NoPreference,
                    TextWeightPreference::Bold => wire::TextWeight::Bold,
                    TextWeightPreference::Adjustment(value) => {
                        wire::TextWeight::Adjustment(value.get())
                    }
                    _ => return Err(unmirrored("TextWeightPreference")),
                })
            })
            .transpose()?,
        locales: locales.map(|locales| locales.iter().map(Locale::to_language_tag).collect()),
        regional: regional_wire(regional),
        gestures: wire::Gestures {
            double_click_interval_ns: double_click_interval.map(nanos),
            double_tap_interval_ns: double_tap_interval.map(nanos),
            long_press_timeout_ns: long_press_timeout.map(nanos),
            native_mouse_geometry: native_mouse_geometry.map(|geometry| {
                let area = geometry.double_click_area();
                let tolerance = geometry.drag_tolerance();
                wire::MouseGeometry {
                    double_click_area: [area.width, area.height],
                    drag_tolerance: [tolerance.width, tolerance.height],
                    pixel_ratio: geometry.pixel_ratio().get(),
                }
            }),
            native_touch_geometry: native_touch_geometry.map(|geometry| wire::TouchGeometry {
                touch_slop: geometry.touch_slop(),
                double_tap_slop: geometry.double_tap_slop(),
                min_fling: geometry.min_fling(),
                max_fling: geometry.max_fling(),
                pixel_ratio: geometry.pixel_ratio().get(),
            }),
        },
        wheel_vertical: vertical
            .map(|step| {
                Ok(match step {
                    WheelStep::Lines(lines) => wire::WheelStep::Lines(lines),
                    WheelStep::Page => wire::WheelStep::Page,
                    _ => return Err(unmirrored("WheelStep")),
                })
            })
            .transpose()?,
        wheel_horizontal_characters: horizontal_characters,
    })
}

/// `parts` in the shared schema, each format as its Unicode value; none when
/// no format was observed.
fn regional_wire(parts: RegionalParts) -> Option<Box<wire::Regional>> {
    if parts == RegionalParts::default() {
        return None;
    }
    let RegionalParts {
        first_weekday,
        hour_cycle,
        measurement,
        numbering,
        region,
    } = parts;
    Some(Box::new(wire::Regional {
        first_weekday: first_weekday.map(|day| day.unicode_value().to_owned()),
        hour_cycle: hour_cycle.map(|cycle| cycle.unicode_value().to_owned()),
        measurement: measurement.map(|system| system.unicode_value().to_owned()),
        numbering: numbering.map(|numbering| numbering.unicode_value().to_owned()),
        region: region.map(|region| region.code().to_owned()),
    }))
}

/// The formats `wire` names, each checked as its type checks it.
fn regional_from_wire(wire: Option<Box<wire::Regional>>) -> Result<RegionalParts> {
    let Some(wire) = wire else {
        return Ok(RegionalParts::default());
    };
    let wire::Regional {
        first_weekday,
        hour_cycle,
        measurement,
        numbering,
        region,
    } = *wire;
    Ok(RegionalParts {
        first_weekday: first_weekday
            .map(|value| {
                Weekday::from_unicode_value(&value)
                    .ok_or_else(|| invalid(format!("first weekday {value}")))
            })
            .transpose()?,
        hour_cycle: hour_cycle
            .map(|value| {
                HourCycle::from_unicode_value(&value)
                    .ok_or_else(|| invalid(format!("hour cycle {value}")))
            })
            .transpose()?,
        measurement: measurement
            .map(|value| {
                MeasurementSystem::from_unicode_value(&value)
                    .ok_or_else(|| invalid(format!("measurement system {value}")))
            })
            .transpose()?,
        numbering: numbering
            .map(|value| NumberingSystem::try_new(&value).map_err(invalid))
            .transpose()?,
        region: region
            .map(|value| RegionOverride::try_new(&value).map_err(invalid))
            .transpose()?,
    })
}

fn pixel_ratio(value: f64) -> Result<DevicePixelRatio> {
    DevicePixelRatio::try_new(value).ok_or_else(|| invalid(format!("pixel ratio {value}")))
}

/// The preferences `wire` describes, validated as the `with_*` methods
/// validate them.
fn preferences_from_wire(wire: wire::Preferences) -> Result<SystemPreferences> {
    let wire::Preferences {
        text_scale,
        motion,
        high_contrast,
        haptics,
        text_weight,
        locales,
        regional,
        gestures,
        wheel_vertical,
        wheel_horizontal_characters,
    } = wire;
    let duration = std::time::Duration::from_nanos;
    PreferenceParts {
        text_scale,
        motion: motion
            .map(|motion| {
                Ok(match motion {
                    wire::Motion::NoPreference => MotionPreference::NoPreference,
                    wire::Motion::Reduce => MotionPreference::Reduce,
                    wire::Motion::Scaled(scale) => {
                        MotionPreference::from_duration_scale(scale).map_err(invalid)?
                    }
                    _ => return Err(unmirrored("native_recording::Motion")),
                })
            })
            .transpose()?,
        high_contrast,
        haptics,
        text_weight: text_weight
            .map(|weight| {
                Ok(match weight {
                    wire::TextWeight::NoPreference => TextWeightPreference::NoPreference,
                    wire::TextWeight::Bold => TextWeightPreference::Bold,
                    wire::TextWeight::Adjustment(value) => {
                        TextWeightPreference::from_adjustment(value)
                    }
                    _ => return Err(unmirrored("native_recording::TextWeight")),
                })
            })
            .transpose()?,
        locales: locales
            .map(|tags| {
                tags.iter()
                    .map(|tag| {
                        Locale::from_language_tag(tag)
                            .ok_or_else(|| invalid(format!("locale {tag}")))
                    })
                    .collect::<Result<Arc<[Locale]>>>()
            })
            .transpose()?,
        regional: regional_from_wire(regional)?,
        gestures: GestureParts {
            double_click_interval: gestures.double_click_interval_ns.map(duration),
            double_tap_interval: gestures.double_tap_interval_ns.map(duration),
            long_press_timeout: gestures.long_press_timeout_ns.map(duration),
            native_mouse_geometry: gestures
                .native_mouse_geometry
                .map(|geometry| {
                    let [area_w, area_h] = geometry.double_click_area;
                    let [drag_w, drag_h] = geometry.drag_tolerance;
                    NativeMouseGeometry::try_new(
                        Size::new(area_w, area_h),
                        Size::new(drag_w, drag_h),
                        pixel_ratio(geometry.pixel_ratio)?,
                    )
                    .map_err(invalid)
                })
                .transpose()?,
            native_touch_geometry: gestures
                .native_touch_geometry
                .map(|geometry| {
                    NativeTouchGeometry::try_new(
                        geometry.touch_slop,
                        geometry.double_tap_slop,
                        geometry.min_fling,
                        geometry.max_fling,
                        pixel_ratio(geometry.pixel_ratio)?,
                    )
                    .map_err(invalid)
                })
                .transpose()?,
        },
        wheel: WheelParts {
            vertical: wheel_vertical
                .map(|step| {
                    Ok(match step {
                        wire::WheelStep::Lines(lines) => WheelStep::Lines(lines),
                        wire::WheelStep::Page => WheelStep::Page,
                        _ => return Err(unmirrored("native_recording::WheelStep")),
                    })
                })
                .transpose()?,
            horizontal_characters: wheel_horizontal_characters,
        },
    }
    .try_into_preferences()
    .map_err(invalid)
}

impl TryConvert<&PreferencesRow> for rec::Preferences {
    fn try_convert(row: &PreferencesRow) -> Result<Self> {
        Ok(Self {
            revision: row.revision,
            values: preferences_wire(SystemPreferences::clone(&row.values))?,
        })
    }
}

impl TryConvert<rec::Preferences> for PreferencesRow {
    fn try_convert(dto: rec::Preferences) -> Result<Self> {
        Ok(Self::new(
            dto.revision,
            Arc::new(preferences_from_wire(dto.values)?),
        ))
    }
}

// ============================================================================
// Header pieces
// ============================================================================

impl RecordingStart {
    /// The first line of a recording on `clock` that starts from here, keeping text under
    /// `privacy`: each window with its scale and gesture values, untitled, and the preference
    /// revision in effect. It names no device: the rows carry the devices they saw.
    ///
    /// # Errors
    ///
    /// [`WireError::Unmirrored`] when a window's gesture values or the preferences hold a
    /// variant the wire form does not mirror.
    pub fn to_wire(&self, clock: rec::Clock, privacy: Privacy) -> Result<rec::Header> {
        Ok(rec::Header {
            flui_recording: rec::FORMAT_VERSION,
            clock,
            windows: self
                .windows
                .iter()
                .map(|window| {
                    Ok(rec::Window {
                        window: window.window.get().get(),
                        scale_factor: window.scale_factor,
                        title: None,
                        gestures: Some(profiles_to_wire(&window.gesture_profiles)?),
                    })
                })
                .collect::<Result<Vec<rec::Window>>>()?,
            devices: Vec::new(),
            preferences: self
                .preferences
                .as_ref()
                .map(PreferencesRow::to_wire)
                .transpose()?,
            keep_text: privacy == Privacy::KeepText,
        })
    }
}

/// A header's entry for `device`, a pointer device of `kind` named `name`.
///
/// # Errors
///
/// [`WireError::Unmirrored`] when `kind` is a variant the wire form does not mirror.
pub fn device_to_wire(
    device: DeviceId,
    kind: PointerKind,
    name: Option<String>,
) -> Result<rec::Device> {
    Ok(rec::Device {
        device: device.get().get(),
        kind: kind.try_convert_into()?,
        name,
    })
}

/// The device and kind a header's device entry records; its name stays on the entry.
///
/// # Errors
///
/// [`WireError::Invalid`] for a device id of 0.
pub fn device_from_wire(device: &rec::Device) -> Result<(DeviceId, PointerKind)> {
    Ok((device_id(device.device)?, device.kind.convert_into()))
}

/// Every kind's gesture values of `profiles`, as a window's header entry stores them.
///
/// # Errors
///
/// [`WireError::Unmirrored`] when the values hold a variant the wire form does not mirror.
pub fn profiles_to_wire(profiles: &GestureProfiles) -> Result<Vec<rec::Profile>> {
    profiles
        .iter()
        .map(|(kind, settings)| {
            let values = settings.values();
            Ok(rec::Profile {
                kind: kind.try_convert_into()?,
                values: rec::GestureValues {
                    touch_slop: values.touch_slop,
                    pan_slop: values.pan_slop,
                    pan_slop_vertical: values.pan_slop_vertical,
                    pan_slop_horizontal: values.pan_slop_horizontal,
                    scale_slop: values.scale_slop,
                    double_tap_slop: values.double_tap_slop,
                    double_tap_timeout_ns: nanos(values.double_tap_timeout),
                    long_press_timeout_ns: nanos(values.long_press_timeout),
                    min_fling_velocity: values.min_fling_velocity,
                    max_fling_velocity: values.max_fling_velocity,
                    velocity_estimator: match values.velocity_estimator {
                        VelocityEstimator::LeastSquares => rec::VelocityEstimator::LeastSquares,
                        VelocityEstimator::Impulse => rec::VelocityEstimator::Impulse,
                        VelocityEstimator::Ios => rec::VelocityEstimator::Ios,
                        VelocityEstimator::Macos => rec::VelocityEstimator::Macos,
                        _ => return Err(unmirrored("VelocityEstimator")),
                    },
                    position_smoothing: values.position_smoothing.map(|tuning| rec::Smoothing {
                        min_cutoff: tuning.min_cutoff(),
                        beta: tuning.beta(),
                        d_cutoff: tuning.d_cutoff(),
                    }),
                    min_target: values.min_target,
                    spring_loading_delay_ns: nanos(values.spring_loading_delay),
                    swipe_edge_width: values.swipe_edge_width,
                    remainder_handoff: match values.remainder_handoff {
                        RemainderHandoff::NextSequence => rec::RemainderHandoff::NextSequence,
                        RemainderHandoff::AtEdgeWithinSequence => {
                            rec::RemainderHandoff::AtEdgeWithinSequence
                        }
                        _ => return Err(unmirrored("RemainderHandoff")),
                    },
                    contactless_latch_idle_ns: nanos(values.contactless_latch_idle),
                    phased_latch_idle_ns: nanos(values.phased_latch_idle),
                    max_resolution_delay_ns: nanos(values.max_resolution_delay),
                    haptic_frame_deadline_factor: values.haptic_frame_deadline_factor.get(),
                },
            })
        })
        .collect()
}

/// The profiles a window's header entry stores; a kind it does not list keeps the default
/// values.
///
/// # Errors
///
/// [`WireError::Invalid`] for a value the gesture settings refuse.
pub fn profiles_from_wire(stored: Vec<rec::Profile>) -> Result<GestureProfiles> {
    let duration = std::time::Duration::from_nanos;
    stored.into_iter().try_fold(
        GestureProfiles::uniform(GestureSettings::default()),
        |profiles, profile| {
            let values = profile.values;
            let settings = GestureSettings::try_from_values(GestureValues {
                touch_slop: values.touch_slop,
                pan_slop: values.pan_slop,
                pan_slop_vertical: values.pan_slop_vertical,
                pan_slop_horizontal: values.pan_slop_horizontal,
                scale_slop: values.scale_slop,
                double_tap_slop: values.double_tap_slop,
                double_tap_timeout: duration(values.double_tap_timeout_ns),
                long_press_timeout: duration(values.long_press_timeout_ns),
                min_fling_velocity: values.min_fling_velocity,
                max_fling_velocity: values.max_fling_velocity,
                velocity_estimator: match values.velocity_estimator {
                    rec::VelocityEstimator::LeastSquares => VelocityEstimator::LeastSquares,
                    rec::VelocityEstimator::Impulse => VelocityEstimator::Impulse,
                    rec::VelocityEstimator::Ios => VelocityEstimator::Ios,
                    rec::VelocityEstimator::Macos => VelocityEstimator::Macos,
                },
                position_smoothing: values
                    .position_smoothing
                    .map(|tuning| {
                        OneEuroTuning::try_new(tuning.min_cutoff, tuning.beta, tuning.d_cutoff)
                            .map_err(invalid)
                    })
                    .transpose()?,
                min_target: values.min_target,
                spring_loading_delay: duration(values.spring_loading_delay_ns),
                swipe_edge_width: values.swipe_edge_width,
                remainder_handoff: match values.remainder_handoff {
                    rec::RemainderHandoff::NextSequence => RemainderHandoff::NextSequence,
                    rec::RemainderHandoff::AtEdgeWithinSequence => {
                        RemainderHandoff::AtEdgeWithinSequence
                    }
                },
                contactless_latch_idle: duration(values.contactless_latch_idle_ns),
                phased_latch_idle: duration(values.phased_latch_idle_ns),
                max_resolution_delay: duration(values.max_resolution_delay_ns),
                haptic_frame_deadline_factor: NonZeroU32::new(values.haptic_frame_deadline_factor)
                    .ok_or_else(|| invalid("haptic frame deadline factor 0"))?,
            })
            .map_err(invalid)?;
            Ok(profiles.with_kind(profile.kind.convert_into(), settings))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{ACTIONS, WireError, action_name};

    /// Every vocabulary variant is mirrored today, so an action reaches an
    /// unmirrored spelling through a narrowed table.
    #[test]
    fn an_action_missing_from_the_spelling_table_is_unmirrored() {
        let refused = action_name(&ACTIONS[..1], accesskit::Action::Focus);
        assert!(
            matches!(refused, Err(WireError::Unmirrored("accesskit::Action"))),
            "{refused:?}"
        );
        let known = action_name(&ACTIONS, accesskit::Action::Focus);
        assert_eq!(known.expect("a pinned spelling"), "focus");
    }
}
