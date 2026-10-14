//! Private mirrors of the recorded vocabulary (ADR-0215 §2).
//!
//! The event vocabulary carries no serde, so it is not frozen as a wire
//! format: these types mirror it with pinned spellings, and the conversions
//! below are the only place the two meet. Unknown fields are skipped within a
//! major format version; an unknown variant fails the row. Because the
//! vocabulary is `#[non_exhaustive]`, a new variant reaches the wildcard arms
//! here instead of failing to compile; the generated variant table and its
//! round-trip test catch it.

use std::num::{NonZeroU8, NonZeroU32};
use std::sync::Arc;

use flui_foundation::DataTransferId;
use flui_foundation::geometry::{DevicePixelRatio, EdgeInsets, Point, Size};
use flui_interaction::GestureDisposition;
use flui_platform_api::data_transfer::{
    DataTransferOffer, Mime, RepresentationDescriptor, TransferActions, TransferFormat,
};
use flui_platform_api::input_capabilities::{Availability, InputCapabilities, PointerKindSet};
use flui_platform_api::keyboard::{
    Code, ImeComposition, Key, KeyEvent, KeyRepeat, KeyState, Location, Modifiers, NamedKey,
};
use flui_platform_api::pointer::{
    ButtonChange, CancelReason, ContactSize, DeviceId, HoverDistance, PanZoomEvent, PanZoomPhase,
    PanZoomTransform, PenOrientation, PenSqueeze, PenSqueezePhase, PenTool, PointerButton,
    PointerButtonEvent, PointerButtons, PointerCancel, PointerDeviceChange, PointerEvent,
    PointerId, PointerInfo, PointerKind, PointerMove, PointerPosition, PointerRevision,
    PointerRole, PointerSample, PointerSignal, Pressure, ScrollDelta, ScrollEvent, ScrollPhase,
    ScrollPrecision, ScrollUnit, TangentialPressure, Twist,
};
use flui_platform_api::{
    Brightness, DragDropEvent, EventTime, GesturePreferences, ImeEvent, Locale, MotionPreference,
    NativeMouseGeometry, NativeTouchGeometry, PlatformInput, SystemPreferences,
    TextWeightPreference, WheelPreferences, WheelStep, WindowExecutionState, WindowId,
};
use flui_runtime::owner::{InputOutcome, WindowObservation};
use flui_runtime::{DecisionRow, HostInput, InputRecord, InputRow, PreferencesRow};
use serde::{Deserialize, Serialize};

/// A value the DTO cannot turn back into the vocabulary.
#[derive(Debug)]
pub(crate) enum DtoError {
    /// A spelling this reader does not know, such as a key or action name.
    UnknownSpelling(String),
    /// A value the vocabulary's validating constructor refuses.
    Invalid(String),
}

type Result<T> = std::result::Result<T, DtoError>;

fn invalid(what: impl std::fmt::Display) -> DtoError {
    DtoError::Invalid(what.to_string())
}

/// The vocabulary grew a variant this DTO does not mirror yet.
fn unmirrored(what: &str) -> ! {
    panic!(
        "{what} has a variant the recording DTO does not mirror; add it to \
         flui-testing's recording DTO and regenerate the variant table"
    )
}

// ============================================================================
// Rows
// ============================================================================

/// One recorded row.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) enum RowDto {
    #[serde(rename = "input")]
    Input(InputRowDto),
    #[serde(rename = "decision")]
    Decision(DecisionDto),
    #[serde(rename = "preferences")]
    Preferences(PreferencesDto),
    #[serde(rename = "gap")]
    Gap { rows: u64 },
    #[serde(rename = "end")]
    End { lost_rows: u64 },
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct InputRowDto {
    window: u64,
    t: u64,
    arrival: u64,
    input: HostInputDto,
    outcome: OutcomeDto,
    /// Text in the payload was replaced before writing (ADR-0215 §5).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) redacted: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum OutcomeDto {
    #[serde(rename = "handled")]
    Handled,
    #[serde(rename = "unhandled")]
    Unhandled,
    #[serde(rename = "queued")]
    Queued,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct DecisionDto {
    window: u64,
    pointer: u64,
    t: u64,
    disposition: DispositionDto,
    member: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum DispositionDto {
    #[serde(rename = "accepted")]
    Accepted,
    #[serde(rename = "rejected")]
    Rejected,
}

impl RowDto {
    /// Replace every text-carrying payload with a placeholder and mark the
    /// row redacted; returns whether anything was replaced.
    pub(crate) fn redact(&mut self) {
        if let Self::Input(row) = self
            && row.input.redact()
        {
            row.redacted = true;
        }
    }

    pub(crate) fn is_redacted(&self) -> bool {
        matches!(self, Self::Input(row) if row.redacted)
    }
}

impl From<&InputRecord> for RowDto {
    fn from(record: &InputRecord) -> Self {
        match record {
            InputRecord::Input(row) => Self::Input(InputRowDto {
                window: row.window.0,
                t: row.t.as_nanos(),
                arrival: row.arrival.as_nanos(),
                input: HostInputDto::from(&row.input),
                outcome: match row.outcome {
                    InputOutcome::Handled => OutcomeDto::Handled,
                    InputOutcome::Unhandled => OutcomeDto::Unhandled,
                    InputOutcome::Queued => OutcomeDto::Queued,
                },
                redacted: false,
            }),
            InputRecord::Decision(row) => Self::Decision(DecisionDto {
                window: row.window.0,
                pointer: row.pointer.get().get(),
                t: row.t.as_nanos(),
                disposition: match row.disposition {
                    GestureDisposition::Accepted => DispositionDto::Accepted,
                    GestureDisposition::Rejected => DispositionDto::Rejected,
                },
                member: row.member.to_string(),
            }),
            InputRecord::Preferences(row) => Self::Preferences(PreferencesDto::from(row)),
            InputRecord::Gap { rows } => Self::Gap { rows: *rows },
            InputRecord::End { lost_rows } => Self::End {
                lost_rows: *lost_rows,
            },
            _ => unmirrored("InputRecord"),
        }
    }
}

impl TryFrom<RowDto> for InputRecord {
    type Error = DtoError;

    fn try_from(row: RowDto) -> Result<Self> {
        Ok(match row {
            RowDto::Input(row) => Self::Input(InputRow::new(
                WindowId(row.window),
                EventTime::from_nanos(row.t),
                EventTime::from_nanos(row.arrival),
                HostInput::try_from(row.input)?,
                match row.outcome {
                    OutcomeDto::Handled => InputOutcome::Handled,
                    OutcomeDto::Unhandled => InputOutcome::Unhandled,
                    OutcomeDto::Queued => InputOutcome::Queued,
                },
            )),
            RowDto::Decision(row) => Self::Decision(DecisionRow::new(
                WindowId(row.window),
                pointer_id(row.pointer)?,
                EventTime::from_nanos(row.t),
                match row.disposition {
                    DispositionDto::Accepted => GestureDisposition::Accepted,
                    DispositionDto::Rejected => GestureDisposition::Rejected,
                },
                row.member,
            )),
            RowDto::Preferences(row) => Self::Preferences(PreferencesRow::try_from(row)?),
            RowDto::Gap { rows } => Self::Gap { rows },
            RowDto::End { lost_rows } => Self::End { lost_rows },
        })
    }
}

// ============================================================================
// Host input
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
enum HostInputDto {
    #[serde(rename = "platform")]
    Platform(PlatformInputDto),
    #[serde(rename = "window")]
    Window(WindowObservationDto),
    #[serde(rename = "frame")]
    Frame { vsync: u64 },
    #[serde(rename = "accessibility")]
    Accessibility(ActionRequestDto),
}

impl HostInputDto {
    fn redact(&mut self) -> bool {
        match self {
            Self::Platform(input) => input.redact(),
            Self::Accessibility(request) => request.redact(),
            Self::Window(_) | Self::Frame { .. } => false,
        }
    }
}

impl From<&HostInput> for HostInputDto {
    fn from(input: &HostInput) -> Self {
        match input {
            HostInput::Platform(input) => Self::Platform(input.into()),
            HostInput::Window(observation) => Self::Window(observation.into()),
            HostInput::Frame { vsync } => Self::Frame {
                vsync: vsync.as_nanos(),
            },
            HostInput::Accessibility(request) => Self::Accessibility(request.into()),
            _ => unmirrored("HostInput"),
        }
    }
}

impl TryFrom<HostInputDto> for HostInput {
    type Error = DtoError;

    fn try_from(input: HostInputDto) -> Result<Self> {
        Ok(match input {
            HostInputDto::Platform(input) => Self::Platform(input.try_into()?),
            HostInputDto::Window(observation) => Self::Window(observation.try_into()?),
            HostInputDto::Frame { vsync } => Self::Frame {
                vsync: EventTime::from_nanos(vsync),
            },
            HostInputDto::Accessibility(request) => Self::Accessibility(request.try_into()?),
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
enum PlatformInputDto {
    #[serde(rename = "pointer")]
    Pointer(PointerEventDto),
    #[serde(rename = "keyboard")]
    Keyboard(KeyEventDto),
    #[serde(rename = "ime")]
    Ime(ImeEventDto),
    #[serde(rename = "drag_drop")]
    DragDrop(DragDropDto),
    #[serde(rename = "capabilities")]
    Capabilities(CapabilitiesDto),
}

impl PlatformInputDto {
    fn redact(&mut self) -> bool {
        match self {
            Self::Keyboard(event) => event.key.redact(),
            Self::Ime(event) => event.redact(),
            Self::Pointer(_) | Self::DragDrop(_) | Self::Capabilities(_) => false,
        }
    }
}

impl From<&PlatformInput> for PlatformInputDto {
    fn from(input: &PlatformInput) -> Self {
        match input {
            PlatformInput::Pointer(event) => Self::Pointer(event.into()),
            PlatformInput::Keyboard(event) => Self::Keyboard(event.into()),
            PlatformInput::Ime(event) => Self::Ime(event.into()),
            PlatformInput::DragDrop(event) => Self::DragDrop(event.into()),
            PlatformInput::Capabilities(capabilities) => Self::Capabilities(capabilities.into()),
        }
    }
}

impl TryFrom<PlatformInputDto> for PlatformInput {
    type Error = DtoError;

    fn try_from(input: PlatformInputDto) -> Result<Self> {
        Ok(match input {
            PlatformInputDto::Pointer(event) => Self::Pointer(event.try_into()?),
            PlatformInputDto::Keyboard(event) => Self::Keyboard(event.try_into()?),
            PlatformInputDto::Ime(event) => Self::Ime(event.into()),
            PlatformInputDto::DragDrop(event) => Self::DragDrop(event.try_into()?),
            PlatformInputDto::Capabilities(capabilities) => Self::Capabilities(capabilities.into()),
        })
    }
}

// ============================================================================
// Pointer
// ============================================================================

fn pointer_id(value: u64) -> Result<PointerId> {
    PointerId::try_from(value).map_err(|_| invalid("pointer id 0"))
}

fn device_id(value: u64) -> Result<DeviceId> {
    DeviceId::try_from(value).map_err(|_| invalid("device id 0"))
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) enum PointerKindDto {
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

impl From<PointerKind> for PointerKindDto {
    fn from(kind: PointerKind) -> Self {
        match kind {
            PointerKind::Mouse => Self::Mouse,
            PointerKind::Touch => Self::Touch,
            PointerKind::Pen {
                tool: PenTool::Tip, ..
            } => Self::Pen,
            PointerKind::Pen {
                tool: PenTool::Eraser,
                ..
            } => Self::Eraser,
            PointerKind::Pen { .. } => unmirrored("PenTool"),
            PointerKind::Trackpad => Self::Trackpad,
            PointerKind::Virtual => Self::Virtual,
            PointerKind::Unknown => Self::Unknown,
            _ => unmirrored("PointerKind"),
        }
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

#[derive(Debug, Serialize, Deserialize)]
struct PointerInfoDto {
    id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    device: Option<u64>,
    kind: PointerKindDto,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    primary: bool,
}

impl From<&PointerInfo> for PointerInfoDto {
    fn from(info: &PointerInfo) -> Self {
        Self {
            id: info.id.get().get(),
            device: info.device.map(|device| device.get().get()),
            kind: info.kind.into(),
            primary: info.is_primary(),
        }
    }
}

impl TryFrom<PointerInfoDto> for PointerInfo {
    type Error = DtoError;

    fn try_from(info: PointerInfoDto) -> Result<Self> {
        let mut pointer =
            Self::new(pointer_id(info.id)?, info.kind.into()).with_role(if info.primary {
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
            .ok_or_else(|| DtoError::UnknownSpelling(name.clone()))
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

#[derive(Debug, Serialize, Deserialize)]
struct SampleDto {
    t: u64,
    x: f64,
    y: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pressure: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tangential_pressure: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    altitude: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    azimuth: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    twist: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hover_distance: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    contact_size: Option<[f64; 2]>,
}

fn position(x: f64, y: f64) -> Result<PointerPosition> {
    PointerPosition::try_new(Point::new(x, y)).map_err(invalid)
}

impl From<&PointerSample> for SampleDto {
    fn from(sample: &PointerSample) -> Self {
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

impl TryFrom<SampleDto> for PointerSample {
    type Error = DtoError;

    fn try_from(sample: SampleDto) -> Result<Self> {
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

fn samples(samples: Vec<SampleDto>) -> Result<Vec<PointerSample>> {
    samples.into_iter().map(PointerSample::try_from).collect()
}

#[derive(Debug, Serialize, Deserialize)]
struct ButtonEventDto {
    pointer: PointerInfoDto,
    button: u8,
    buttons: Vec<u8>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    modifiers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    click_count: Option<u8>,
    sample: SampleDto,
}

impl<D: flui_platform_api::pointer::ButtonDirection> From<&PointerButtonEvent<D>>
    for ButtonEventDto
{
    fn from(event: &PointerButtonEvent<D>) -> Self {
        Self {
            pointer: (&event.pointer).into(),
            button: event.button().number().get(),
            buttons: buttons_dto(event.buttons()),
            modifiers: modifiers_dto(event.modifiers),
            click_count: event.click_count.map(NonZeroU8::get),
            sample: (&event.sample).into(),
        }
    }
}

impl ButtonEventDto {
    fn into_event<D: flui_platform_api::pointer::ButtonDirection>(
        self,
    ) -> Result<PointerButtonEvent<D>> {
        let mut event = PointerButtonEvent::<D>::new(
            self.pointer.try_into()?,
            button(self.button)?,
            buttons(&self.buttons)?,
            self.sample.try_into()?,
        )
        .with_modifiers(modifiers(&self.modifiers)?);
        if let Some(count) = self.click_count {
            event = event
                .with_click_count(NonZeroU8::new(count).ok_or_else(|| invalid("click count 0"))?);
        }
        Ok(event)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct SignalDto {
    pointer: PointerInfoDto,
    t: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    position: Option<[f64; 2]>,
}

impl From<&PointerSignal> for SignalDto {
    fn from(signal: &PointerSignal) -> Self {
        Self {
            pointer: (&signal.pointer).into(),
            t: signal.time.as_nanos(),
            position: signal.position.map(|position| {
                let point = position.get();
                [point.x, point.y]
            }),
        }
    }
}

impl TryFrom<SignalDto> for PointerSignal {
    type Error = DtoError;

    fn try_from(signal: SignalDto) -> Result<Self> {
        let mut out = Self::new(signal.pointer.try_into()?, EventTime::from_nanos(signal.t));
        if let Some([x, y]) = signal.position {
            out = out.with_position(position(x, y)?);
        }
        Ok(out)
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum ScrollUnitDto {
    #[serde(rename = "lines")]
    Lines,
    #[serde(rename = "pixels")]
    Pixels,
    #[serde(rename = "pages")]
    Pages,
    #[serde(rename = "detents")]
    Detents,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum ScrollPrecisionDto {
    #[serde(rename = "unknown")]
    Unknown,
    #[serde(rename = "notched")]
    Notched,
    #[serde(rename = "precise")]
    Precise,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum ScrollPhaseDto {
    #[serde(rename = "began")]
    Began,
    #[serde(rename = "changed")]
    Changed,
    #[serde(rename = "ended")]
    Ended,
    #[serde(rename = "cancelled")]
    Cancelled,
    #[serde(rename = "momentum_began")]
    MomentumBegan,
    #[serde(rename = "momentum_changed")]
    MomentumChanged,
    #[serde(rename = "momentum_ended")]
    MomentumEnded,
}

#[derive(Debug, Serialize, Deserialize)]
struct ScrollDto {
    pointer: PointerInfoDto,
    t: u64,
    x: f64,
    y: f64,
    unit: ScrollUnitDto,
    dx: f64,
    dy: f64,
    precision: ScrollPrecisionDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    phase: Option<ScrollPhaseDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    modifiers: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
enum PanZoomPhaseDto {
    #[serde(rename = "start")]
    Start,
    #[serde(rename = "update")]
    Update { scale: f64, rotation: f64 },
    #[serde(rename = "end")]
    End,
    #[serde(rename = "cancelled")]
    Cancelled,
}

#[derive(Debug, Serialize, Deserialize)]
struct PanZoomDto {
    pointer: PointerInfoDto,
    t: u64,
    x: f64,
    y: f64,
    phase: PanZoomPhaseDto,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    modifiers: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum SqueezePhaseDto {
    #[serde(rename = "began")]
    Began,
    #[serde(rename = "changed")]
    Changed,
    #[serde(rename = "ended")]
    Ended,
    #[serde(rename = "cancelled")]
    Cancelled,
}

#[derive(Debug, Serialize, Deserialize)]
struct DeviceChangeDto {
    device: u64,
    kind: PointerKindDto,
    t: u64,
}

#[derive(Debug, Serialize, Deserialize)]
enum PointerEventDto {
    #[serde(rename = "down")]
    Down(ButtonEventDto),
    #[serde(rename = "button_pressed")]
    ButtonPressed(ButtonEventDto),
    #[serde(rename = "button_released")]
    ButtonReleased(ButtonEventDto),
    #[serde(rename = "up")]
    Up(ButtonEventDto),
    #[serde(rename = "move")]
    Move {
        pointer: PointerInfoDto,
        buttons: Vec<u8>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        modifiers: Vec<String>,
        current: SampleDto,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        coalesced: Vec<SampleDto>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        predicted: Vec<SampleDto>,
    },
    #[serde(rename = "cancel")]
    Cancel {
        pointer: PointerInfoDto,
        t: u64,
        reason: CancelReasonDto,
    },
    #[serde(rename = "enter")]
    Enter(SignalDto),
    #[serde(rename = "leave")]
    Leave(SignalDto),
    #[serde(rename = "scroll")]
    Scroll(ScrollDto),
    #[serde(rename = "scroll_inertia_cancel")]
    ScrollInertiaCancel(SignalDto),
    #[serde(rename = "pan_zoom")]
    PanZoom(PanZoomDto),
    #[serde(rename = "revise")]
    Revise {
        pointer: PointerInfoDto,
        sample: SampleDto,
    },
    #[serde(rename = "pen_squeeze")]
    PenSqueeze {
        pointer: PointerInfoDto,
        t: u64,
        phase: SqueezePhaseDto,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hover: Option<SampleDto>,
    },
    #[serde(rename = "device_added")]
    DeviceAdded(DeviceChangeDto),
    #[serde(rename = "device_removed")]
    DeviceRemoved(DeviceChangeDto),
}

fn device_change_dto(change: &PointerDeviceChange) -> DeviceChangeDto {
    DeviceChangeDto {
        device: change.device.get().get(),
        kind: change.kind.into(),
        t: change.time.as_nanos(),
    }
}

fn device_change(change: DeviceChangeDto) -> Result<PointerDeviceChange> {
    Ok(PointerDeviceChange::new(
        device_id(change.device)?,
        change.kind.into(),
        EventTime::from_nanos(change.t),
    ))
}

impl From<&PointerEvent> for PointerEventDto {
    fn from(event: &PointerEvent) -> Self {
        match event {
            PointerEvent::Down(press) => Self::Down(press.into()),
            PointerEvent::ButtonChange(ButtonChange::Pressed(press)) => {
                Self::ButtonPressed(press.into())
            }
            PointerEvent::ButtonChange(ButtonChange::Released(release)) => {
                Self::ButtonReleased(release.into())
            }
            PointerEvent::Up(release) => Self::Up(release.into()),
            PointerEvent::Move(motion) => Self::Move {
                pointer: (&motion.pointer).into(),
                buttons: buttons_dto(motion.buttons),
                modifiers: modifiers_dto(motion.modifiers),
                current: motion.current().into(),
                coalesced: motion.coalesced().iter().map(SampleDto::from).collect(),
                predicted: motion.predicted().iter().map(SampleDto::from).collect(),
            },
            PointerEvent::Cancel(cancel) => Self::Cancel {
                pointer: (&cancel.pointer).into(),
                t: cancel.time.as_nanos(),
                reason: match cancel.reason {
                    CancelReason::Platform => CancelReasonDto::Platform,
                    CancelReason::CaptureLost => CancelReasonDto::CaptureLost,
                    CancelReason::FocusLost => CancelReasonDto::FocusLost,
                    CancelReason::DeviceRemoved => CancelReasonDto::DeviceRemoved,
                    CancelReason::InvalidInput => CancelReasonDto::InvalidInput,
                    _ => unmirrored("CancelReason"),
                },
            },
            PointerEvent::Enter(signal) => Self::Enter(signal.into()),
            PointerEvent::Leave(signal) => Self::Leave(signal.into()),
            PointerEvent::Scroll(scroll) => {
                let point = scroll.position.get();
                Self::Scroll(ScrollDto {
                    pointer: (&scroll.pointer).into(),
                    t: scroll.time.as_nanos(),
                    x: point.x,
                    y: point.y,
                    unit: match scroll.delta.unit() {
                        ScrollUnit::Lines => ScrollUnitDto::Lines,
                        ScrollUnit::Pixels => ScrollUnitDto::Pixels,
                        ScrollUnit::Pages => ScrollUnitDto::Pages,
                        ScrollUnit::Detents => ScrollUnitDto::Detents,
                        _ => unmirrored("ScrollUnit"),
                    },
                    dx: scroll.delta.x(),
                    dy: scroll.delta.y(),
                    precision: match scroll.precision {
                        ScrollPrecision::Unknown => ScrollPrecisionDto::Unknown,
                        ScrollPrecision::Notched => ScrollPrecisionDto::Notched,
                        ScrollPrecision::Precise => ScrollPrecisionDto::Precise,
                        _ => unmirrored("ScrollPrecision"),
                    },
                    phase: scroll.phase.map(|phase| match phase {
                        ScrollPhase::Began => ScrollPhaseDto::Began,
                        ScrollPhase::Changed => ScrollPhaseDto::Changed,
                        ScrollPhase::Ended => ScrollPhaseDto::Ended,
                        ScrollPhase::Cancelled => ScrollPhaseDto::Cancelled,
                        ScrollPhase::MomentumBegan => ScrollPhaseDto::MomentumBegan,
                        ScrollPhase::MomentumChanged => ScrollPhaseDto::MomentumChanged,
                        ScrollPhase::MomentumEnded => ScrollPhaseDto::MomentumEnded,
                        _ => unmirrored("ScrollPhase"),
                    }),
                    modifiers: modifiers_dto(scroll.modifiers),
                })
            }
            PointerEvent::ScrollInertiaCancel(signal) => Self::ScrollInertiaCancel(signal.into()),
            PointerEvent::PanZoom(gesture) => {
                let point = gesture.position.get();
                Self::PanZoom(PanZoomDto {
                    pointer: gesture.pointer().into(),
                    t: gesture.time.as_nanos(),
                    x: point.x,
                    y: point.y,
                    phase: match gesture.phase {
                        PanZoomPhase::Start => PanZoomPhaseDto::Start,
                        PanZoomPhase::Update(transform) => PanZoomPhaseDto::Update {
                            scale: transform.scale(),
                            rotation: transform.rotation(),
                        },
                        PanZoomPhase::End => PanZoomPhaseDto::End,
                        PanZoomPhase::Cancelled => PanZoomPhaseDto::Cancelled,
                        _ => unmirrored("PanZoomPhase"),
                    },
                    modifiers: modifiers_dto(gesture.modifiers),
                })
            }
            PointerEvent::Revise(revision) => Self::Revise {
                pointer: (&revision.pointer).into(),
                sample: (&revision.sample).into(),
            },
            PointerEvent::PenSqueeze(squeeze) => Self::PenSqueeze {
                pointer: (&squeeze.pointer).into(),
                t: squeeze.time.as_nanos(),
                phase: match squeeze.phase {
                    PenSqueezePhase::Began => SqueezePhaseDto::Began,
                    PenSqueezePhase::Changed => SqueezePhaseDto::Changed,
                    PenSqueezePhase::Ended => SqueezePhaseDto::Ended,
                    PenSqueezePhase::Cancelled => SqueezePhaseDto::Cancelled,
                    _ => unmirrored("PenSqueezePhase"),
                },
                hover: squeeze.hover.as_ref().map(SampleDto::from),
            },
            PointerEvent::DeviceAdded(change) => Self::DeviceAdded(device_change_dto(change)),
            PointerEvent::DeviceRemoved(change) => Self::DeviceRemoved(device_change_dto(change)),
            _ => unmirrored("PointerEvent"),
        }
    }
}

impl TryFrom<PointerEventDto> for PointerEvent {
    type Error = DtoError;

    fn try_from(event: PointerEventDto) -> Result<Self> {
        Ok(match event {
            PointerEventDto::Down(press) => Self::Down(press.into_event()?),
            PointerEventDto::ButtonPressed(press) => {
                Self::ButtonChange(ButtonChange::Pressed(press.into_event()?))
            }
            PointerEventDto::ButtonReleased(release) => {
                Self::ButtonChange(ButtonChange::Released(release.into_event()?))
            }
            PointerEventDto::Up(release) => Self::Up(release.into_event()?),
            PointerEventDto::Move {
                pointer,
                buttons: held,
                modifiers: names,
                current,
                coalesced,
                predicted,
            } => Self::Move(
                PointerMove::new(pointer.try_into()?, buttons(&held)?, current.try_into()?)
                    .with_modifiers(modifiers(&names)?)
                    .with_coalesced(samples(coalesced)?)
                    .with_predicted(samples(predicted)?),
            ),
            PointerEventDto::Cancel { pointer, t, reason } => Self::Cancel(PointerCancel::new(
                pointer.try_into()?,
                EventTime::from_nanos(t),
                match reason {
                    CancelReasonDto::Platform => CancelReason::Platform,
                    CancelReasonDto::CaptureLost => CancelReason::CaptureLost,
                    CancelReasonDto::FocusLost => CancelReason::FocusLost,
                    CancelReasonDto::DeviceRemoved => CancelReason::DeviceRemoved,
                    CancelReasonDto::InvalidInput => CancelReason::InvalidInput,
                },
            )),
            PointerEventDto::Enter(signal) => Self::Enter(signal.try_into()?),
            PointerEventDto::Leave(signal) => Self::Leave(signal.try_into()?),
            PointerEventDto::Scroll(scroll) => {
                let unit = match scroll.unit {
                    ScrollUnitDto::Lines => ScrollUnit::Lines,
                    ScrollUnitDto::Pixels => ScrollUnit::Pixels,
                    ScrollUnitDto::Pages => ScrollUnit::Pages,
                    ScrollUnitDto::Detents => ScrollUnit::Detents,
                };
                let mut event = ScrollEvent::new(
                    scroll.pointer.try_into()?,
                    EventTime::from_nanos(scroll.t),
                    position(scroll.x, scroll.y)?,
                    ScrollDelta::try_new(unit, scroll.dx, scroll.dy).map_err(invalid)?,
                )
                .with_precision(match scroll.precision {
                    ScrollPrecisionDto::Unknown => ScrollPrecision::Unknown,
                    ScrollPrecisionDto::Notched => ScrollPrecision::Notched,
                    ScrollPrecisionDto::Precise => ScrollPrecision::Precise,
                })
                .with_modifiers(modifiers(&scroll.modifiers)?);
                if let Some(phase) = scroll.phase {
                    event = event.with_phase(match phase {
                        ScrollPhaseDto::Began => ScrollPhase::Began,
                        ScrollPhaseDto::Changed => ScrollPhase::Changed,
                        ScrollPhaseDto::Ended => ScrollPhase::Ended,
                        ScrollPhaseDto::Cancelled => ScrollPhase::Cancelled,
                        ScrollPhaseDto::MomentumBegan => ScrollPhase::MomentumBegan,
                        ScrollPhaseDto::MomentumChanged => ScrollPhase::MomentumChanged,
                        ScrollPhaseDto::MomentumEnded => ScrollPhase::MomentumEnded,
                    });
                }
                Self::Scroll(event)
            }
            PointerEventDto::ScrollInertiaCancel(signal) => {
                Self::ScrollInertiaCancel(signal.try_into()?)
            }
            PointerEventDto::PanZoom(gesture) => Self::PanZoom(
                PanZoomEvent::new(
                    gesture.pointer.try_into()?,
                    EventTime::from_nanos(gesture.t),
                    position(gesture.x, gesture.y)?,
                    match gesture.phase {
                        PanZoomPhaseDto::Start => PanZoomPhase::Start,
                        PanZoomPhaseDto::Update { scale, rotation } => PanZoomPhase::Update(
                            PanZoomTransform::try_new(scale, rotation).map_err(invalid)?,
                        ),
                        PanZoomPhaseDto::End => PanZoomPhase::End,
                        PanZoomPhaseDto::Cancelled => PanZoomPhase::Cancelled,
                    },
                )
                .with_modifiers(modifiers(&gesture.modifiers)?),
            ),
            PointerEventDto::Revise { pointer, sample } => Self::Revise(PointerRevision::new(
                pointer.try_into()?,
                sample.try_into()?,
            )),
            PointerEventDto::PenSqueeze {
                pointer,
                t,
                phase,
                hover,
            } => {
                let mut squeeze = PenSqueeze::new(
                    pointer.try_into()?,
                    EventTime::from_nanos(t),
                    match phase {
                        SqueezePhaseDto::Began => PenSqueezePhase::Began,
                        SqueezePhaseDto::Changed => PenSqueezePhase::Changed,
                        SqueezePhaseDto::Ended => PenSqueezePhase::Ended,
                        SqueezePhaseDto::Cancelled => PenSqueezePhase::Cancelled,
                    },
                );
                if let Some(hover) = hover {
                    squeeze = squeeze.with_hover(hover.try_into()?);
                }
                Self::PenSqueeze(squeeze)
            }
            PointerEventDto::DeviceAdded(change) => Self::DeviceAdded(device_change(change)?),
            PointerEventDto::DeviceRemoved(change) => Self::DeviceRemoved(device_change(change)?),
        })
    }
}

// ============================================================================
// Keyboard and IME
// ============================================================================

/// The text a redacted row carries in place of the original.
pub(crate) const REDACTED: &str = "\u{fffd}";

#[derive(Debug, Serialize, Deserialize)]
enum KeyDto {
    #[serde(rename = "character")]
    Character(String),
    #[serde(rename = "named")]
    Named(String),
}

impl KeyDto {
    fn redact(&mut self) -> bool {
        match self {
            Self::Character(text) => {
                REDACTED.clone_into(text);
                true
            }
            Self::Named(_) => false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum KeyStateDto {
    #[serde(rename = "down")]
    Down,
    #[serde(rename = "up")]
    Up,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum LocationDto {
    #[serde(rename = "standard")]
    Standard,
    #[serde(rename = "left")]
    Left,
    #[serde(rename = "right")]
    Right,
    #[serde(rename = "numpad")]
    Numpad,
}

#[derive(Debug, Serialize, Deserialize)]
struct KeyEventDto {
    state: KeyStateDto,
    key: KeyDto,
    code: String,
    location: LocationDto,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    modifiers: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    repeat: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    composing: bool,
    t: u64,
}

impl From<&KeyEvent> for KeyEventDto {
    fn from(event: &KeyEvent) -> Self {
        Self {
            state: match event.state() {
                KeyState::Down => KeyStateDto::Down,
                KeyState::Up => KeyStateDto::Up,
            },
            key: match &event.key {
                Key::Character(text) => KeyDto::Character(text.as_str().to_owned()),
                Key::Named(named) => KeyDto::Named(named.as_str().to_owned()),
                _ => unmirrored("Key"),
            },
            code: event.code.as_str().to_owned(),
            location: match event.location {
                Location::Standard => LocationDto::Standard,
                Location::Left => LocationDto::Left,
                Location::Right => LocationDto::Right,
                Location::Numpad => LocationDto::Numpad,
            },
            modifiers: modifiers_dto(event.modifiers),
            repeat: event.repeat() == KeyRepeat::AutoRepeat,
            composing: event.composition == ImeComposition::Active,
            t: event.time.as_nanos(),
        }
    }
}

impl TryFrom<KeyEventDto> for KeyEvent {
    type Error = DtoError;

    fn try_from(event: KeyEventDto) -> Result<Self> {
        let key = match event.key {
            KeyDto::Character(text) => Key::character(text),
            KeyDto::Named(name) => {
                Key::Named(NamedKey::from_w3c(&name).ok_or(DtoError::UnknownSpelling(name))?)
            }
        };
        let code = Code::from_w3c(&event.code)
            .ok_or_else(|| DtoError::UnknownSpelling(event.code.clone()))?;
        Ok(Self::new(
            match event.state {
                KeyStateDto::Down => KeyState::Down,
                KeyStateDto::Up => KeyState::Up,
            },
            key,
            code,
            EventTime::from_nanos(event.t),
        )
        .with_location(match event.location {
            LocationDto::Standard => Location::Standard,
            LocationDto::Left => Location::Left,
            LocationDto::Right => Location::Right,
            LocationDto::Numpad => Location::Numpad,
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
        }))
    }
}

#[derive(Debug, Serialize, Deserialize)]
enum ImeEventDto {
    #[serde(rename = "enabled")]
    Enabled,
    #[serde(rename = "preedit")]
    Preedit {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: Option<(usize, usize)>,
    },
    #[serde(rename = "commit")]
    Commit(String),
    #[serde(rename = "disabled")]
    Disabled,
}

impl ImeEventDto {
    fn redact(&mut self) -> bool {
        match self {
            Self::Preedit { text, cursor } => {
                let had_text = !text.is_empty();
                if had_text {
                    REDACTED.clone_into(text);
                    // The cursor indexed the original text.
                    *cursor = cursor.map(|_| (0, REDACTED.len()));
                }
                had_text
            }
            Self::Commit(text) => {
                REDACTED.clone_into(text);
                true
            }
            Self::Enabled | Self::Disabled => false,
        }
    }
}

impl From<&ImeEvent> for ImeEventDto {
    fn from(event: &ImeEvent) -> Self {
        match event {
            ImeEvent::Enabled => Self::Enabled,
            ImeEvent::Preedit { text, cursor } => Self::Preedit {
                text: text.clone(),
                cursor: *cursor,
            },
            ImeEvent::Commit(text) => Self::Commit(text.clone()),
            ImeEvent::Disabled => Self::Disabled,
            _ => unmirrored("ImeEvent"),
        }
    }
}

impl From<ImeEventDto> for ImeEvent {
    fn from(event: ImeEventDto) -> Self {
        match event {
            ImeEventDto::Enabled => Self::Enabled,
            ImeEventDto::Preedit { text, cursor } => Self::Preedit { text, cursor },
            ImeEventDto::Commit(text) => Self::Commit(text),
            ImeEventDto::Disabled => Self::Disabled,
        }
    }
}

// ============================================================================
// Drag and drop, capabilities
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
struct TransferIdDto {
    index: u32,
    generation: u32,
}

impl From<DataTransferId> for TransferIdDto {
    fn from(id: DataTransferId) -> Self {
        Self {
            index: id.index(),
            generation: id.generation().get(),
        }
    }
}

impl TryFrom<TransferIdDto> for DataTransferId {
    type Error = DtoError;

    fn try_from(id: TransferIdDto) -> Result<Self> {
        Ok(Self::new_gen(
            id.index,
            NonZeroU32::new(id.generation).ok_or_else(|| invalid("transfer generation 0"))?,
        ))
    }
}

#[derive(Debug, Serialize, Deserialize)]
enum TransferFormatDto {
    #[serde(rename = "text")]
    Text,
    #[serde(rename = "html")]
    Html,
    #[serde(rename = "uri_list")]
    UriList,
    #[serde(rename = "image")]
    Image { mime: String },
    #[serde(rename = "custom")]
    Custom { mime: String },
}

#[derive(Debug, Serialize, Deserialize)]
struct RepresentationDto {
    format: TransferFormatDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    declared_len: Option<u64>,
}

fn actions(bits: u8) -> Result<TransferActions> {
    TransferActions::from_bits(bits).ok_or_else(|| invalid(format!("transfer actions {bits}")))
}

fn point_dto(point: Point<f64>) -> [f64; 2] {
    [point.x, point.y]
}

#[derive(Debug, Serialize, Deserialize)]
enum DragDropDto {
    #[serde(rename = "entered")]
    Entered {
        id: TransferIdDto,
        representations: Vec<RepresentationDto>,
        allowed: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<[f64; 2]>,
    },
    #[serde(rename = "moved")]
    Moved {
        id: TransferIdDto,
        allowed: u8,
        position: [f64; 2],
    },
    #[serde(rename = "dropped")]
    Dropped {
        id: TransferIdDto,
        action: u8,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<[f64; 2]>,
    },
    #[serde(rename = "exited")]
    Exited { id: TransferIdDto },
}

impl From<&DragDropEvent> for DragDropDto {
    fn from(event: &DragDropEvent) -> Self {
        match event {
            DragDropEvent::Entered {
                offer,
                allowed,
                position,
            } => Self::Entered {
                id: offer.id().into(),
                representations: offer
                    .representations()
                    .iter()
                    .map(|descriptor| RepresentationDto {
                        format: match &descriptor.format {
                            TransferFormat::Text => TransferFormatDto::Text,
                            TransferFormat::Html => TransferFormatDto::Html,
                            TransferFormat::UriList => TransferFormatDto::UriList,
                            TransferFormat::Image { mime } => TransferFormatDto::Image {
                                mime: mime.as_str().to_owned(),
                            },
                            TransferFormat::Custom { mime } => TransferFormatDto::Custom {
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
                id: (*id).into(),
                allowed: allowed.bits(),
                position: point_dto(*position),
            },
            DragDropEvent::Dropped {
                id,
                action,
                position,
            } => Self::Dropped {
                id: (*id).into(),
                action: action.bits(),
                position: position.map(point_dto),
            },
            DragDropEvent::Exited { id } => Self::Exited { id: (*id).into() },
        }
    }
}

impl TryFrom<DragDropDto> for DragDropEvent {
    type Error = DtoError;

    fn try_from(event: DragDropDto) -> Result<Self> {
        let point = |[x, y]: [f64; 2]| Point::new(x, y);
        Ok(match event {
            DragDropDto::Entered {
                id,
                representations,
                allowed,
                position,
            } => Self::Entered {
                offer: DataTransferOffer::new(
                    id.try_into()?,
                    representations
                        .into_iter()
                        .map(|representation| RepresentationDescriptor {
                            format: match representation.format {
                                TransferFormatDto::Text => TransferFormat::Text,
                                TransferFormatDto::Html => TransferFormat::Html,
                                TransferFormatDto::UriList => TransferFormat::UriList,
                                TransferFormatDto::Image { mime } => TransferFormat::Image {
                                    mime: Mime::new(&mime),
                                },
                                TransferFormatDto::Custom { mime } => TransferFormat::Custom {
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
            DragDropDto::Moved {
                id,
                allowed,
                position,
            } => Self::Moved {
                id: id.try_into()?,
                allowed: actions(allowed)?,
                position: point(position),
            },
            DragDropDto::Dropped {
                id,
                action,
                position,
            } => Self::Dropped {
                id: id.try_into()?,
                action: actions(action)?,
                position: position.map(point),
            },
            DragDropDto::Exited { id } => Self::Exited { id: id.try_into()? },
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum AvailabilityDto {
    #[serde(rename = "native")]
    Native,
    #[serde(rename = "emulated")]
    Emulated,
    #[serde(rename = "unavailable")]
    Unavailable,
}

impl From<Availability> for AvailabilityDto {
    fn from(availability: Availability) -> Self {
        match availability {
            Availability::Native => Self::Native,
            Availability::Emulated => Self::Emulated,
            Availability::Unavailable => Self::Unavailable,
        }
    }
}

impl From<AvailabilityDto> for Availability {
    fn from(availability: AvailabilityDto) -> Self {
        match availability {
            AvailabilityDto::Native => Self::Native,
            AvailabilityDto::Emulated => Self::Emulated,
            AvailabilityDto::Unavailable => Self::Unavailable,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct CapabilitiesDto {
    pen: AvailabilityDto,
    precise_scroll: AvailabilityDto,
    trackpad_gestures: AvailabilityDto,
    input_regions: AvailabilityDto,
    outgoing_drag: AvailabilityDto,
    present: Vec<PointerKindDto>,
}

/// Each present kind, pinned to its kind spelling.
const PRESENT_KINDS: [(PointerKindSet, PointerKindDto); 5] = [
    (PointerKindSet::MOUSE, PointerKindDto::Mouse),
    (PointerKindSet::TOUCH, PointerKindDto::Touch),
    (PointerKindSet::PEN, PointerKindDto::Pen),
    (PointerKindSet::TRACKPAD, PointerKindDto::Trackpad),
    (PointerKindSet::VIRTUAL, PointerKindDto::Virtual),
];

impl From<&InputCapabilities> for CapabilitiesDto {
    fn from(capabilities: &InputCapabilities) -> Self {
        Self {
            pen: capabilities.pen().into(),
            precise_scroll: capabilities.precise_scroll().into(),
            trackpad_gestures: capabilities.trackpad_gestures().into(),
            input_regions: capabilities.input_regions().into(),
            outgoing_drag: capabilities.outgoing_drag().into(),
            present: PRESENT_KINDS
                .iter()
                .filter(|(kind, _)| capabilities.present().contains(*kind))
                .map(|(_, dto)| *dto)
                .collect(),
        }
    }
}

impl From<CapabilitiesDto> for InputCapabilities {
    fn from(capabilities: CapabilitiesDto) -> Self {
        let present = capabilities
            .present
            .iter()
            .fold(PointerKindSet::empty(), |set, kind| {
                set | PointerKindSet::of((*kind).into())
            });
        Self::builder()
            .pen(capabilities.pen.into())
            .precise_scroll(capabilities.precise_scroll.into())
            .trackpad_gestures(capabilities.trackpad_gestures.into())
            .input_regions(capabilities.input_regions.into())
            .outgoing_drag(capabilities.outgoing_drag.into())
            .present(present)
            .build()
    }
}

// ============================================================================
// Window observations
// ============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum ExecutionDto {
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "suspended")]
    Suspended,
    #[serde(rename = "detached")]
    Detached,
}

impl From<WindowExecutionState> for ExecutionDto {
    fn from(state: WindowExecutionState) -> Self {
        match state {
            WindowExecutionState::Running => Self::Running,
            WindowExecutionState::Suspended => Self::Suspended,
            WindowExecutionState::Detached => Self::Detached,
            _ => unmirrored("WindowExecutionState"),
        }
    }
}

impl From<ExecutionDto> for WindowExecutionState {
    fn from(state: ExecutionDto) -> Self {
        match state {
            ExecutionDto::Running => Self::Running,
            ExecutionDto::Suspended => Self::Suspended,
            ExecutionDto::Detached => Self::Detached,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
enum WindowObservationDto {
    #[serde(rename = "metrics")]
    Metrics {
        width: f64,
        height: f64,
        scale_factor: f64,
    },
    #[serde(rename = "focus")]
    Focus(bool),
    #[serde(rename = "visibility")]
    Visibility(bool),
    #[serde(rename = "execution")]
    Execution(ExecutionDto),
    #[serde(rename = "snapshot")]
    Snapshot {
        execution: ExecutionDto,
        focused: bool,
        visible: bool,
    },
    #[serde(rename = "hover")]
    Hover(bool),
    #[serde(rename = "moved")]
    Moved,
    #[serde(rename = "safe_area")]
    SafeArea {
        top: f64,
        right: f64,
        bottom: f64,
        left: f64,
    },
    #[serde(rename = "brightness")]
    Brightness(BrightnessDto),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum BrightnessDto {
    #[serde(rename = "light")]
    Light,
    #[serde(rename = "dark")]
    Dark,
}

impl From<&WindowObservation> for WindowObservationDto {
    fn from(observation: &WindowObservation) -> Self {
        match *observation {
            WindowObservation::Metrics { size, scale_factor } => Self::Metrics {
                width: size.width,
                height: size.height,
                scale_factor,
            },
            WindowObservation::Focus(focused) => Self::Focus(focused),
            WindowObservation::Visibility(visible) => Self::Visibility(visible),
            WindowObservation::Execution(state) => Self::Execution(state.into()),
            WindowObservation::Snapshot {
                execution,
                focused,
                visible,
            } => Self::Snapshot {
                execution: execution.into(),
                focused,
                visible,
            },
            WindowObservation::Hover(inside) => Self::Hover(inside),
            WindowObservation::Moved => Self::Moved,
            WindowObservation::SafeArea(insets) => Self::SafeArea {
                top: insets.top,
                right: insets.right,
                bottom: insets.bottom,
                left: insets.left,
            },
            WindowObservation::Brightness(brightness) => Self::Brightness(match brightness {
                Brightness::Light => BrightnessDto::Light,
                Brightness::Dark => BrightnessDto::Dark,
            }),
            _ => unmirrored("WindowObservation"),
        }
    }
}

impl TryFrom<WindowObservationDto> for WindowObservation {
    type Error = DtoError;

    fn try_from(observation: WindowObservationDto) -> Result<Self> {
        Ok(match observation {
            WindowObservationDto::Metrics {
                width,
                height,
                scale_factor,
            } => Self::Metrics {
                size: Size::new(width, height),
                scale_factor,
            },
            WindowObservationDto::Focus(focused) => Self::Focus(focused),
            WindowObservationDto::Visibility(visible) => Self::Visibility(visible),
            WindowObservationDto::Execution(state) => Self::Execution(state.into()),
            WindowObservationDto::Snapshot {
                execution,
                focused,
                visible,
            } => Self::Snapshot {
                execution: execution.into(),
                focused,
                visible,
            },
            WindowObservationDto::Hover(inside) => Self::Hover(inside),
            WindowObservationDto::Moved => Self::Moved,
            WindowObservationDto::SafeArea {
                top,
                right,
                bottom,
                left,
            } => Self::SafeArea(EdgeInsets::new(top, right, bottom, left)),
            WindowObservationDto::Brightness(brightness) => Self::Brightness(match brightness {
                BrightnessDto::Light => Brightness::Light,
                BrightnessDto::Dark => Brightness::Dark,
            }),
        })
    }
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum ScrollHintDto {
    #[serde(rename = "top_left")]
    TopLeft,
    #[serde(rename = "bottom_right")]
    BottomRight,
    #[serde(rename = "top_edge")]
    TopEdge,
    #[serde(rename = "bottom_edge")]
    BottomEdge,
    #[serde(rename = "left_edge")]
    LeftEdge,
    #[serde(rename = "right_edge")]
    RightEdge,
}

#[derive(Debug, Serialize, Deserialize)]
struct TextPositionDto {
    node: u64,
    character_index: usize,
}

#[derive(Debug, Serialize, Deserialize)]
enum ActionDataDto {
    #[serde(rename = "custom_action")]
    CustomAction(i32),
    #[serde(rename = "value")]
    Value(String),
    #[serde(rename = "numeric_value")]
    NumericValue(f64),
    #[serde(rename = "scroll_unit")]
    ScrollUnit(AxScrollUnitDto),
    #[serde(rename = "scroll_hint")]
    ScrollHint(ScrollHintDto),
    #[serde(rename = "scroll_to_point")]
    ScrollToPoint([f64; 2]),
    #[serde(rename = "set_scroll_offset")]
    SetScrollOffset([f64; 2]),
    #[serde(rename = "set_text_selection")]
    SetTextSelection {
        anchor: TextPositionDto,
        focus: TextPositionDto,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
enum AxScrollUnitDto {
    #[serde(rename = "item")]
    Item,
    #[serde(rename = "page")]
    Page,
}

#[derive(Debug, Serialize, Deserialize)]
struct ActionRequestDto {
    action: String,
    /// The target tree's UUID, as 32 lowercase hex digits.
    tree: String,
    node: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    data: Option<ActionDataDto>,
}

impl ActionRequestDto {
    fn redact(&mut self) -> bool {
        match &mut self.data {
            Some(ActionDataDto::Value(text)) => {
                REDACTED.clone_into(text);
                true
            }
            _ => false,
        }
    }
}

fn text_position_dto(position: accesskit::TextPosition) -> TextPositionDto {
    TextPositionDto {
        node: position.node.0,
        character_index: position.character_index,
    }
}

fn text_position(position: TextPositionDto) -> accesskit::TextPosition {
    accesskit::TextPosition {
        node: accesskit::NodeId(position.node),
        character_index: position.character_index,
    }
}

impl From<&accesskit::ActionRequest> for ActionRequestDto {
    fn from(request: &accesskit::ActionRequest) -> Self {
        let action = ACTIONS
            .iter()
            .find(|(action, _)| *action == request.action)
            .map_or_else(|| unmirrored("accesskit::Action"), |(_, name)| *name);
        Self {
            action: action.to_owned(),
            tree: format!("{:032x}", request.target_tree.0.as_u128()),
            node: request.target_node.0,
            data: request.data.as_ref().map(|data| match data {
                accesskit::ActionData::CustomAction(id) => ActionDataDto::CustomAction(*id),
                accesskit::ActionData::Value(text) => ActionDataDto::Value(text.to_string()),
                accesskit::ActionData::NumericValue(value) => ActionDataDto::NumericValue(*value),
                accesskit::ActionData::ScrollUnit(unit) => ActionDataDto::ScrollUnit(match unit {
                    accesskit::ScrollUnit::Item => AxScrollUnitDto::Item,
                    accesskit::ScrollUnit::Page => AxScrollUnitDto::Page,
                }),
                accesskit::ActionData::ScrollHint(hint) => ActionDataDto::ScrollHint(match hint {
                    accesskit::ScrollHint::TopLeft => ScrollHintDto::TopLeft,
                    accesskit::ScrollHint::BottomRight => ScrollHintDto::BottomRight,
                    accesskit::ScrollHint::TopEdge => ScrollHintDto::TopEdge,
                    accesskit::ScrollHint::BottomEdge => ScrollHintDto::BottomEdge,
                    accesskit::ScrollHint::LeftEdge => ScrollHintDto::LeftEdge,
                    accesskit::ScrollHint::RightEdge => ScrollHintDto::RightEdge,
                }),
                accesskit::ActionData::ScrollToPoint(point) => {
                    ActionDataDto::ScrollToPoint([point.x, point.y])
                }
                accesskit::ActionData::SetScrollOffset(point) => {
                    ActionDataDto::SetScrollOffset([point.x, point.y])
                }
                accesskit::ActionData::SetTextSelection(selection) => {
                    ActionDataDto::SetTextSelection {
                        anchor: text_position_dto(selection.anchor),
                        focus: text_position_dto(selection.focus),
                    }
                }
            }),
        }
    }
}

impl TryFrom<ActionRequestDto> for accesskit::ActionRequest {
    type Error = DtoError;

    fn try_from(request: ActionRequestDto) -> Result<Self> {
        let action = ACTIONS
            .iter()
            .find(|(_, name)| *name == request.action)
            .map(|(action, _)| *action)
            .ok_or(DtoError::UnknownSpelling(request.action))?;
        let tree = u128::from_str_radix(&request.tree, 16)
            .map_err(|_| invalid(format!("tree id {}", request.tree)))?;
        Ok(Self {
            action,
            target_tree: accesskit::TreeId(accesskit::Uuid::from_u128(tree)),
            target_node: accesskit::NodeId(request.node),
            data: request.data.map(|data| match data {
                ActionDataDto::CustomAction(id) => accesskit::ActionData::CustomAction(id),
                ActionDataDto::Value(text) => accesskit::ActionData::Value(text.into()),
                ActionDataDto::NumericValue(value) => accesskit::ActionData::NumericValue(value),
                ActionDataDto::ScrollUnit(unit) => accesskit::ActionData::ScrollUnit(match unit {
                    AxScrollUnitDto::Item => accesskit::ScrollUnit::Item,
                    AxScrollUnitDto::Page => accesskit::ScrollUnit::Page,
                }),
                ActionDataDto::ScrollHint(hint) => accesskit::ActionData::ScrollHint(match hint {
                    ScrollHintDto::TopLeft => accesskit::ScrollHint::TopLeft,
                    ScrollHintDto::BottomRight => accesskit::ScrollHint::BottomRight,
                    ScrollHintDto::TopEdge => accesskit::ScrollHint::TopEdge,
                    ScrollHintDto::BottomEdge => accesskit::ScrollHint::BottomEdge,
                    ScrollHintDto::LeftEdge => accesskit::ScrollHint::LeftEdge,
                    ScrollHintDto::RightEdge => accesskit::ScrollHint::RightEdge,
                }),
                ActionDataDto::ScrollToPoint([x, y]) => {
                    accesskit::ActionData::ScrollToPoint(accesskit::Point { x, y })
                }
                ActionDataDto::SetScrollOffset([x, y]) => {
                    accesskit::ActionData::SetScrollOffset(accesskit::Point { x, y })
                }
                ActionDataDto::SetTextSelection { anchor, focus } => {
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

#[derive(Debug, Serialize, Deserialize)]
enum MotionDto {
    #[serde(rename = "no_preference")]
    NoPreference,
    #[serde(rename = "reduce")]
    Reduce,
    #[serde(rename = "scaled")]
    Scaled(f64),
}

#[derive(Debug, Serialize, Deserialize)]
enum TextWeightDto {
    #[serde(rename = "no_preference")]
    NoPreference,
    #[serde(rename = "bold")]
    Bold,
    #[serde(rename = "adjustment")]
    Adjustment(i32),
}

#[derive(Debug, Serialize, Deserialize)]
enum WheelStepDto {
    #[serde(rename = "lines")]
    Lines(u32),
    #[serde(rename = "page")]
    Page,
}

#[derive(Debug, Serialize, Deserialize)]
struct MouseGeometryDto {
    double_click_area: [i32; 2],
    drag_tolerance: [i32; 2],
    pixel_ratio: f64,
}

#[derive(Debug, Serialize, Deserialize)]
struct TouchGeometryDto {
    touch_slop: i32,
    double_tap_slop: i32,
    min_fling: i32,
    max_fling: i32,
    pixel_ratio: f64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct GesturesDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    double_click_interval_ns: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    double_tap_interval_ns: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    long_press_timeout_ns: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    native_mouse_geometry: Option<MouseGeometryDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    native_touch_geometry: Option<TouchGeometryDto>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct PreferencesDto {
    revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    text_scale: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    motion: Option<MotionDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    high_contrast: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    haptics: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    text_weight: Option<TextWeightDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    locales: Option<Vec<String>>,
    #[serde(default)]
    gestures: GesturesDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    wheel_vertical: Option<WheelStepDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    wheel_horizontal_characters: Option<u32>,
}

fn nanos(duration: std::time::Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

impl From<&PreferencesRow> for PreferencesDto {
    fn from(row: &PreferencesRow) -> Self {
        let values = &row.values;
        let gestures = values.gestures();
        Self {
            revision: row.revision,
            text_scale: values.text_scale(),
            motion: values.motion().map(|motion| match motion {
                MotionPreference::NoPreference => MotionDto::NoPreference,
                MotionPreference::Reduce => MotionDto::Reduce,
                MotionPreference::Scaled(scale) => MotionDto::Scaled(scale.get()),
                _ => unmirrored("MotionPreference"),
            }),
            high_contrast: values.high_contrast(),
            haptics: values.haptics(),
            text_weight: values.text_weight().map(|weight| match weight {
                TextWeightPreference::NoPreference => TextWeightDto::NoPreference,
                TextWeightPreference::Bold => TextWeightDto::Bold,
                TextWeightPreference::Adjustment(value) => TextWeightDto::Adjustment(value.get()),
                _ => unmirrored("TextWeightPreference"),
            }),
            locales: values
                .locales()
                .map(|locales| locales.iter().map(Locale::to_language_tag).collect()),
            gestures: GesturesDto {
                double_click_interval_ns: gestures.double_click_interval().map(nanos),
                double_tap_interval_ns: gestures.double_tap_interval().map(nanos),
                long_press_timeout_ns: gestures.long_press_timeout().map(nanos),
                native_mouse_geometry: gestures.native_mouse_geometry().map(|geometry| {
                    let area = geometry.double_click_area();
                    let tolerance = geometry.drag_tolerance();
                    MouseGeometryDto {
                        double_click_area: [area.width, area.height],
                        drag_tolerance: [tolerance.width, tolerance.height],
                        pixel_ratio: geometry.pixel_ratio().get(),
                    }
                }),
                native_touch_geometry: gestures.native_touch_geometry().map(|geometry| {
                    TouchGeometryDto {
                        touch_slop: geometry.touch_slop(),
                        double_tap_slop: geometry.double_tap_slop(),
                        min_fling: geometry.min_fling(),
                        max_fling: geometry.max_fling(),
                        pixel_ratio: geometry.pixel_ratio().get(),
                    }
                }),
            },
            wheel_vertical: values.wheel().vertical().map(|step| match step {
                WheelStep::Lines(lines) => WheelStepDto::Lines(lines),
                WheelStep::Page => WheelStepDto::Page,
                _ => unmirrored("WheelStep"),
            }),
            wheel_horizontal_characters: values.wheel().horizontal_characters(),
        }
    }
}

fn pixel_ratio(value: f64) -> Result<DevicePixelRatio> {
    DevicePixelRatio::new(value).ok_or_else(|| invalid(format!("pixel ratio {value}")))
}

impl TryFrom<PreferencesDto> for PreferencesRow {
    type Error = DtoError;

    fn try_from(dto: PreferencesDto) -> Result<Self> {
        let mut values = SystemPreferences::default();
        if let Some(scale) = dto.text_scale {
            values = values.with_text_scale(scale).map_err(invalid)?;
        }
        if let Some(motion) = dto.motion {
            values = values.with_motion(match motion {
                MotionDto::NoPreference => MotionPreference::NoPreference,
                MotionDto::Reduce => MotionPreference::Reduce,
                MotionDto::Scaled(scale) => {
                    MotionPreference::from_duration_scale(scale).map_err(invalid)?
                }
            });
        }
        if let Some(enabled) = dto.high_contrast {
            values = values.with_high_contrast(enabled);
        }
        if let Some(enabled) = dto.haptics {
            values = values.with_haptics(enabled);
        }
        if let Some(weight) = dto.text_weight {
            values = values.with_text_weight(match weight {
                TextWeightDto::NoPreference => TextWeightPreference::NoPreference,
                TextWeightDto::Bold => TextWeightPreference::Bold,
                TextWeightDto::Adjustment(value) => TextWeightPreference::from_adjustment(value),
            });
        }
        if let Some(tags) = dto.locales {
            let locales = tags
                .iter()
                .map(|tag| {
                    Locale::from_language_tag(tag).ok_or_else(|| invalid(format!("locale {tag}")))
                })
                .collect::<Result<Vec<_>>>()?;
            values = values.with_locales(locales);
        }
        let mut gestures = GesturePreferences::default();
        let duration = std::time::Duration::from_nanos;
        if let Some(value) = dto.gestures.double_click_interval_ns {
            gestures = gestures.with_double_click_interval(duration(value));
        }
        if let Some(value) = dto.gestures.double_tap_interval_ns {
            gestures = gestures.with_double_tap_interval(duration(value));
        }
        if let Some(value) = dto.gestures.long_press_timeout_ns {
            gestures = gestures.with_long_press_timeout(duration(value));
        }
        if let Some(geometry) = dto.gestures.native_mouse_geometry {
            let [area_w, area_h] = geometry.double_click_area;
            let [drag_w, drag_h] = geometry.drag_tolerance;
            gestures = gestures.with_native_mouse_geometry(
                NativeMouseGeometry::new(
                    Size::new(area_w, area_h),
                    Size::new(drag_w, drag_h),
                    pixel_ratio(geometry.pixel_ratio)?,
                )
                .map_err(invalid)?,
            );
        }
        if let Some(geometry) = dto.gestures.native_touch_geometry {
            gestures = gestures.with_native_touch_geometry(
                NativeTouchGeometry::new(
                    geometry.touch_slop,
                    geometry.double_tap_slop,
                    geometry.min_fling,
                    geometry.max_fling,
                    pixel_ratio(geometry.pixel_ratio)?,
                )
                .map_err(invalid)?,
            );
        }
        values = values.with_gestures(gestures);
        let mut wheel = WheelPreferences::default();
        if let Some(step) = dto.wheel_vertical {
            wheel = wheel.with_vertical(match step {
                WheelStepDto::Lines(lines) => WheelStep::Lines(lines),
                WheelStepDto::Page => WheelStep::Page,
            });
        }
        if let Some(characters) = dto.wheel_horizontal_characters {
            wheel = wheel.with_horizontal_characters(characters);
        }
        values = values.with_wheel(wheel);
        Ok(Self::new(dto.revision, Arc::new(values)))
    }
}

// ============================================================================
// Header pieces
// ============================================================================

/// A device in the header.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct DeviceDto {
    pub(crate) device: u64,
    pub(crate) kind: PointerKindDto,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
}

pub(crate) fn device_from(dto: &DeviceDto) -> Result<(DeviceId, PointerKind)> {
    Ok((device_id(dto.device)?, dto.kind.into()))
}
