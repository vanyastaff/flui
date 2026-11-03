//! The input recording schema (ADR-0215 §2): the [`HostInput`] stream a UI runtime applied, the
//! gesture arena decisions and deadlines it ran, and the system preferences in effect, written
//! as JSON Lines.
//!
//! A recording is one [`Header`] object, then one [`Row`] object per row. The header names the
//! format's major version under [`VERSION_KEY`]; a reader skips unknown fields within one
//! version, and an unknown variant is a refusal, never a skipped row.
//!
//! The runtime's event vocabulary carries no serde, so it is not frozen as a wire format: these
//! types mirror it with pinned spellings, and `flui-runtime` converts between the two. This
//! module only declares the rows, so a reader that never links the runtime reads a recording
//! without it. The preference values are the schema of
//! [`native_recording`](crate::native_recording), so the two recordings never spell a
//! preference differently.
//!
//! The format is versioned by [`FORMAT_VERSION`], apart from
//! [`PROTOCOL_VERSION`](crate::PROTOCOL_VERSION).
//!
//! The module exists with the `serde` feature: its types are a serialization format and nothing
//! else.

use serde::{Deserialize, Serialize};

/// The input recording format's major version. It changes only with an incompatible change;
/// unknown fields are skipped within one version.
pub const FORMAT_VERSION: u32 = 3;

/// The header field that names the format and its major version.
pub const VERSION_KEY: &str = "flui_recording";

/// The text a redacted row carries in place of the original.
pub const REDACTED: &str = "\u{fffd}";

/// Which clock the recorded times count on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum Clock {
    /// A virtual clock a test advanced; times count from its start.
    #[serde(rename = "virtual")]
    Virtual,
    /// The process's monotonic event timeline.
    #[serde(rename = "monotonic")]
    Monotonic,
}

/// A recording's first line: the format version, the clock, the windows and devices, and the
/// system-preference revision in effect.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Header {
    /// The format's major version, under the [`VERSION_KEY`] field.
    pub flui_recording: u32,
    /// The clock the times count on.
    pub clock: Clock,
    /// The windows the recording addresses.
    #[serde(default)]
    pub windows: Vec<Window>,
    /// The pointer devices the recording saw.
    #[serde(default)]
    pub devices: Vec<Device>,
    /// The preference revision in effect when recording began, with its values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferences: Option<Preferences>,
    /// Whether the recording keeps text.
    #[serde(default)]
    pub keep_text: bool,
}

/// A window in the header: its id, its starting scale and the gesture values its contacts were
/// admitted with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Window {
    /// The window's id.
    pub window: u64,
    /// Device pixels per logical pixel, possibly fractional.
    pub scale_factor: f64,
    /// The window's title, when known; redacted unless text is kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The gesture values per pointer kind, when the recorder knew them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gestures: Option<Vec<Profile>>,
}

/// One recorded row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum Row {
    /// An input a window applied.
    #[serde(rename = "input")]
    Input(InputRow),
    /// A gesture arena decision.
    #[serde(rename = "decision")]
    Decision(Decision),
    /// A gesture arena deadline.
    #[serde(rename = "deadline")]
    Deadline(Deadline),
    /// A system-preferences revision.
    #[serde(rename = "preferences")]
    Preferences(Preferences),
    /// Rows the tap lost.
    #[serde(rename = "gap")]
    Gap {
        /// How many rows were lost.
        rows: u64,
    },
    /// The end of the recording.
    #[serde(rename = "end")]
    End {
        /// How many rows the tap lost in all.
        lost_rows: u64,
    },
}

/// One recorded input: what a window applied, when, and how its dispatch answered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct InputRow {
    /// The window's id.
    pub window: u64,
    /// The event time, in nanoseconds.
    pub t: u64,
    /// When the runtime received the input, in nanoseconds.
    pub arrival: u64,
    /// The input.
    pub input: HostInput,
    /// How the window's dispatch answered the input.
    pub outcome: Outcome,
    /// The embedded surface the input addressed; absent for the window's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedded_surface: Option<u64>,
    /// The input sequence dispatch placed the input in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u64>,
    /// Text in the payload was replaced before writing (ADR-0215 §5).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub redacted: bool,
}

/// How a window's dispatch answered an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum Outcome {
    /// The `handled` outcome.
    #[serde(rename = "handled")]
    Handled,
    /// The `unhandled` outcome.
    #[serde(rename = "unhandled")]
    Unhandled,
    /// The `queued` outcome.
    #[serde(rename = "queued")]
    Queued,
}

/// A gesture arena's decision on one member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Decision {
    /// The window's id.
    pub window: u64,
    /// The pointer's id.
    pub pointer: u64,
    /// The event time, in nanoseconds.
    pub t: u64,
    /// The input sequence dispatch placed it in, when it was in one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u64>,
    /// Whether the member was accepted or rejected.
    pub disposition: Disposition,
    /// The member's number in the recording, its identity.
    pub member: u64,
    /// The member's type name, an annotation for readers.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub member_type: String,
}

/// A gesture arena deadline that fired on one member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Deadline {
    /// The window's id.
    pub window: u64,
    /// The pointer's id, when the deadline belongs to one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pointer: Option<u64>,
    /// The input sequence dispatch placed it in, when it was in one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u64>,
    /// The event time, in nanoseconds.
    pub t: u64,
    /// The member's number in the recording, its identity.
    pub member: u64,
    /// The member's type name, an annotation for readers.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub member_type: String,
}

/// A gesture arena's verdict on a member.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum Disposition {
    /// The `accepted` gesture disposition.
    #[serde(rename = "accepted")]
    Accepted,
    /// The `rejected` gesture disposition.
    #[serde(rename = "rejected")]
    Rejected,
}

/// The input a window applied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum HostInput {
    /// An event the platform delivered.
    #[serde(rename = "platform")]
    Platform(PlatformInput),
    /// A window's observation of itself.
    #[serde(rename = "window")]
    Window(WindowObservation),
    /// A frame's vsync.
    #[serde(rename = "frame")]
    Frame {
        /// The vsync time, in nanoseconds.
        vsync: u64,
        /// The present time the frame was scheduled for, in nanoseconds.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_present: Option<u64>,
        /// The earlier frame that reached the screen, when reported.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        presented: Option<FramePresented>,
    },
    /// An assistive technology's action request.
    #[serde(rename = "accessibility")]
    Accessibility(ActionRequest),
    /// The screen reader's cursor: the node id it moved to, or none off the tree.
    #[serde(rename = "assistive_focus")]
    AssistiveFocus(Option<u64>),
}

/// A frame's report that an earlier frame reached the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct FramePresented {
    /// When the frame reached the screen, in nanoseconds.
    pub at: u64,
    /// The display's refresh period, in nanoseconds, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh: Option<u64>,
}

/// An event the platform delivered.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum PlatformInput {
    /// A pointer event.
    #[serde(rename = "pointer")]
    Pointer(PointerEvent),
    /// Pointer events delivered together.
    #[serde(rename = "pointer_frame")]
    PointerFrame(Vec<PointerEvent>),
    /// A keyboard event.
    #[serde(rename = "keyboard")]
    Keyboard(KeyEvent),
    /// An input method editor event.
    #[serde(rename = "ime")]
    Ime(ImeEvent),
    /// A drag-and-drop event.
    #[serde(rename = "drag_drop")]
    DragDrop(DragDrop),
    /// A change of the input capabilities.
    #[serde(rename = "capabilities")]
    Capabilities(Capabilities),
}

/// A pointer's kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum PointerKind {
    /// The `mouse` pointer kind.
    #[serde(rename = "mouse")]
    Mouse,
    /// The `touch` pointer kind.
    #[serde(rename = "touch")]
    Touch,
    /// The `pen` pointer kind.
    #[serde(rename = "pen")]
    Pen,
    /// The `eraser` pointer kind.
    #[serde(rename = "eraser")]
    Eraser,
    /// The `trackpad` pointer kind.
    #[serde(rename = "trackpad")]
    Trackpad,
    /// The `virtual` pointer kind.
    #[serde(rename = "virtual")]
    Virtual,
    /// The `unknown` pointer kind.
    #[serde(rename = "unknown")]
    Unknown,
}

/// The pointer an event belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct PointerInfo {
    /// The pointer's id.
    pub id: u64,
    /// The device that produced it, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<u64>,
    /// Its kind.
    pub kind: PointerKind,
    /// Whether it is the primary pointer of its kind.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub primary: bool,
}

/// One sampled pointer state.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Sample {
    /// The event time, in nanoseconds.
    pub t: u64,
    /// The x coordinate, in logical pixels.
    pub x: f64,
    /// The y coordinate, in logical pixels.
    pub y: f64,
    /// The pressure, when the device reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure: Option<f32>,
    /// The tangential pressure, when the device reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tangential_pressure: Option<f32>,
    /// The pen's altitude angle, when reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub altitude: Option<f64>,
    /// The pen's azimuth angle, when reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub azimuth: Option<f64>,
    /// The pen's twist, when reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub twist: Option<f64>,
    /// The distance above the surface, when reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hover_distance: Option<f32>,
    /// The contact's width and height, when reported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_size: Option<[f64; 2]>,
}

/// A pointer button press or release.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ButtonEvent {
    /// The pointer the event belongs to.
    pub pointer: PointerInfo,
    /// The button that changed, by number.
    pub button: u8,
    /// Every button held after the change, by number.
    pub buttons: Vec<u8>,
    /// The held modifiers, by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<String>,
    /// The click count, when the platform reports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub click_count: Option<u8>,
    /// The pointer's sample at the change.
    pub sample: Sample,
}

/// A pointer signal: an enter, a leave or a scroll-inertia cancel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Signal {
    /// The pointer the event belongs to.
    pub pointer: PointerInfo,
    /// The event time, in nanoseconds.
    pub t: u64,
    /// The pointer's position as x, y, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<[f64; 2]>,
}

/// Why a pointer was cancelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum CancelReason {
    /// The `platform` cancellation reason.
    #[serde(rename = "platform")]
    Platform,
    /// The `capture_lost` cancellation reason.
    #[serde(rename = "capture_lost")]
    CaptureLost,
    /// The `focus_lost` cancellation reason.
    #[serde(rename = "focus_lost")]
    FocusLost,
    /// The `device_removed` cancellation reason.
    #[serde(rename = "device_removed")]
    DeviceRemoved,
    /// The `invalid_input` cancellation reason.
    #[serde(rename = "invalid_input")]
    InvalidInput,
    /// The `claimed` cancellation reason.
    #[serde(rename = "claimed")]
    Claimed,
    /// The `superseded` cancellation reason.
    #[serde(rename = "superseded")]
    Superseded,
}

/// The unit of a scroll delta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum ScrollUnit {
    /// The `lines` scroll unit.
    #[serde(rename = "lines")]
    Lines,
    /// The `pixels` scroll unit.
    #[serde(rename = "pixels")]
    Pixels,
    /// The `pages` scroll unit.
    #[serde(rename = "pages")]
    Pages,
    /// The `detents` scroll unit.
    #[serde(rename = "detents")]
    Detents,
}

/// How precisely a scroll device reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum ScrollPrecision {
    /// The `unknown` scroll precision.
    #[serde(rename = "unknown")]
    Unknown,
    /// The `notched` scroll precision.
    #[serde(rename = "notched")]
    Notched,
    /// The `precise` scroll precision.
    #[serde(rename = "precise")]
    Precise,
}

/// A scroll gesture's phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum ScrollPhase {
    /// The `began` scroll phase.
    #[serde(rename = "began")]
    Began,
    /// The `changed` scroll phase.
    #[serde(rename = "changed")]
    Changed,
    /// The `ended` scroll phase.
    #[serde(rename = "ended")]
    Ended,
    /// The `cancelled` scroll phase.
    #[serde(rename = "cancelled")]
    Cancelled,
    /// The `momentum_began` scroll phase.
    #[serde(rename = "momentum_began")]
    MomentumBegan,
    /// The `momentum_changed` scroll phase.
    #[serde(rename = "momentum_changed")]
    MomentumChanged,
    /// The `momentum_ended` scroll phase.
    #[serde(rename = "momentum_ended")]
    MomentumEnded,
}

/// A scroll event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Scroll {
    /// The pointer the event belongs to.
    pub pointer: PointerInfo,
    /// The event time, in nanoseconds.
    pub t: u64,
    /// The x coordinate, in logical pixels.
    pub x: f64,
    /// The y coordinate, in logical pixels.
    pub y: f64,
    /// The unit of `dx` and `dy`.
    pub unit: ScrollUnit,
    /// The horizontal delta.
    pub dx: f64,
    /// The vertical delta.
    pub dy: f64,
    /// How precisely the device reports.
    pub precision: ScrollPrecision,
    /// The scroll gesture's phase, when the device reports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<ScrollPhase>,
    /// The held modifiers, by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<String>,
}

/// A trackpad pan-zoom gesture's phase.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum PanZoomPhase {
    /// The gesture started.
    #[serde(rename = "start")]
    Start,
    /// The gesture moved to this transform.
    #[serde(rename = "update")]
    Update {
        /// The accumulated scale.
        scale: f64,
        /// The accumulated rotation.
        rotation: f64,
    },
    /// The gesture ended.
    #[serde(rename = "end")]
    End,
    /// The gesture was cancelled.
    #[serde(rename = "cancelled")]
    Cancelled,
}

/// A trackpad smart-zoom event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SmartZoom {
    /// The pointer the event belongs to.
    pub pointer: PointerInfo,
    /// The event time, in nanoseconds.
    pub t: u64,
    /// The x coordinate, in logical pixels.
    pub x: f64,
    /// The y coordinate, in logical pixels.
    pub y: f64,
    /// The held modifiers, by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<String>,
}

/// A trackpad pan-zoom event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct PanZoom {
    /// The pointer the event belongs to.
    pub pointer: PointerInfo,
    /// The event time, in nanoseconds.
    pub t: u64,
    /// The x coordinate, in logical pixels.
    pub x: f64,
    /// The y coordinate, in logical pixels.
    pub y: f64,
    /// The gesture's phase.
    pub phase: PanZoomPhase,
    /// The held modifiers, by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<String>,
}

/// A pen squeeze's phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum SqueezePhase {
    /// The `began` pen squeeze phase.
    #[serde(rename = "began")]
    Began,
    /// The `changed` pen squeeze phase.
    #[serde(rename = "changed")]
    Changed,
    /// The `ended` pen squeeze phase.
    #[serde(rename = "ended")]
    Ended,
    /// The `cancelled` pen squeeze phase.
    #[serde(rename = "cancelled")]
    Cancelled,
}

/// A pointer device appearing or disappearing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct DeviceChange {
    /// The device's id.
    pub device: u64,
    /// The device's kind.
    pub kind: PointerKind,
    /// The event time, in nanoseconds.
    pub t: u64,
}

/// A pointer event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum PointerEvent {
    /// A pointer made contact.
    #[serde(rename = "down")]
    Down(ButtonEvent),
    /// A button was pressed while the pointer was down.
    #[serde(rename = "button_pressed")]
    ButtonPressed(ButtonEvent),
    /// A button was released while the pointer stayed down.
    #[serde(rename = "button_released")]
    ButtonReleased(ButtonEvent),
    /// A pointer lost contact.
    #[serde(rename = "up")]
    Up(ButtonEvent),
    /// A pointer moved.
    #[serde(rename = "move")]
    Move {
        /// The pointer that moved.
        pointer: PointerInfo,
        /// Every button held, by number.
        buttons: Vec<u8>,
        /// The held modifiers, by name.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        modifiers: Vec<String>,
        /// The pointer's current sample.
        current: Sample,
        /// The samples coalesced into this event.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        coalesced: Vec<Sample>,
        /// The samples predicted past this event.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        predicted: Vec<Sample>,
    },
    /// A pointer was cancelled.
    #[serde(rename = "cancel")]
    Cancel {
        /// The pointer the event belongs to.
        pointer: PointerInfo,
        /// The event time, in nanoseconds.
        t: u64,
        /// Why the pointer was cancelled.
        reason: CancelReason,
    },
    /// A pointer entered the window.
    #[serde(rename = "enter")]
    Enter(Signal),
    /// A pointer left the window.
    #[serde(rename = "leave")]
    Leave(Signal),
    /// A scroll.
    #[serde(rename = "scroll")]
    Scroll(Scroll),
    /// A scroll's inertia was cancelled.
    #[serde(rename = "scroll_inertia_cancel")]
    ScrollInertiaCancel(Signal),
    /// A trackpad pan-zoom gesture.
    #[serde(rename = "pan_zoom")]
    PanZoom(PanZoom),
    /// A trackpad smart-zoom.
    #[serde(rename = "smart_zoom")]
    SmartZoom(SmartZoom),
    /// A pointer's earlier sample was revised.
    #[serde(rename = "revise")]
    Revise {
        /// The pointer the event belongs to.
        pointer: PointerInfo,
        /// The revised sample.
        sample: Sample,
    },
    /// A pen squeeze.
    #[serde(rename = "pen_squeeze")]
    PenSqueeze {
        /// The pointer the event belongs to.
        pointer: PointerInfo,
        /// The event time, in nanoseconds.
        t: u64,
        /// The squeeze's phase.
        phase: SqueezePhase,
        /// The pen's hover sample, when reported.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hover: Option<Sample>,
    },
    /// A pointer device appeared.
    #[serde(rename = "device_added")]
    DeviceAdded(DeviceChange),
    /// A pointer device disappeared.
    #[serde(rename = "device_removed")]
    DeviceRemoved(DeviceChange),
}

/// A logical key: its text, or its W3C name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum Key {
    /// A character key; its text.
    #[serde(rename = "character")]
    Character(String),
    /// A named key; its W3C name.
    #[serde(rename = "named")]
    Named(String),
}

/// Whether a key went down or up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum KeyState {
    /// The `down` key state.
    #[serde(rename = "down")]
    Down,
    /// The `up` key state.
    #[serde(rename = "up")]
    Up,
}

/// Where on the keyboard a key sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum Location {
    /// The `standard` key location.
    #[serde(rename = "standard")]
    Standard,
    /// The `left` key location.
    #[serde(rename = "left")]
    Left,
    /// The `right` key location.
    #[serde(rename = "right")]
    Right,
    /// The `numpad` key location.
    #[serde(rename = "numpad")]
    Numpad,
}

/// A keyboard event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct KeyEvent {
    /// Whether the key went down or up.
    pub state: KeyState,
    /// The logical key.
    pub key: Key,
    /// The physical key, by its W3C code.
    pub code: String,
    /// Where on the keyboard the key sits.
    pub location: Location,
    /// The held modifiers, by name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<String>,
    /// Whether the event is an auto-repeat.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub repeat: bool,
    /// Whether an input method composition was active.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub composing: bool,
    /// Whether the key's release is never delivered.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unreleased: bool,
    /// The event time, in nanoseconds.
    pub t: u64,
}

/// An input method editor event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum ImeEvent {
    /// The input method was enabled.
    #[serde(rename = "enabled")]
    Enabled,
    /// Composition text changed.
    #[serde(rename = "preedit")]
    Preedit {
        /// The composition text.
        text: String,
        /// The cursor's start and end in the text, when shown.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: Option<(usize, usize)>,
    },
    /// Text was committed.
    #[serde(rename = "commit")]
    Commit(String),
    /// The input method was disabled.
    #[serde(rename = "disabled")]
    Disabled,
}

/// A data transfer's id: its slot and generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct TransferId {
    /// The slot's index.
    pub index: u32,
    /// The slot's generation, never zero.
    pub generation: u32,
}

/// The format of one representation of a data transfer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum TransferFormat {
    /// Plain text.
    #[serde(rename = "text")]
    Text,
    /// HTML.
    #[serde(rename = "html")]
    Html,
    /// A list of URIs.
    #[serde(rename = "uri_list")]
    UriList,
    /// An image of this MIME type.
    #[serde(rename = "image")]
    Image {
        /// The image's MIME type.
        mime: String,
    },
    /// A custom format of this MIME type.
    #[serde(rename = "custom")]
    Custom {
        /// The format's MIME type.
        mime: String,
    },
}

/// One representation a data transfer offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Representation {
    /// The representation's format.
    pub format: TransferFormat,
    /// The length the source declared, in bytes, when it did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_len: Option<u64>,
}

/// A drag-and-drop event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum DragDrop {
    /// A drag entered the window.
    #[serde(rename = "entered")]
    Entered {
        /// The transfer's id.
        id: TransferId,
        /// The representations the offer lists.
        representations: Vec<Representation>,
        /// The allowed drop actions, as a bit set.
        allowed: u8,
        /// The pointer's position as x, y, in logical pixels, when known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<[f64; 2]>,
    },
    /// A drag moved over the window.
    #[serde(rename = "moved")]
    Moved {
        /// The transfer's id.
        id: TransferId,
        /// The allowed drop actions, as a bit set.
        allowed: u8,
        /// The pointer's position as x, y, in logical pixels.
        position: [f64; 2],
    },
    /// A drag was dropped on the window.
    #[serde(rename = "dropped")]
    Dropped {
        /// The transfer's id.
        id: TransferId,
        /// The chosen drop action, as a bit set.
        action: u8,
        /// The pointer's position as x, y, in logical pixels, when known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<[f64; 2]>,
    },
    /// A drag left the window.
    #[serde(rename = "exited")]
    Exited {
        /// The transfer's id.
        id: TransferId,
    },
}

/// How a platform provides a capability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum Availability {
    /// The `native` availability.
    #[serde(rename = "native")]
    Native,
    /// The `emulated` availability.
    #[serde(rename = "emulated")]
    Emulated,
    /// The `unavailable` availability.
    #[serde(rename = "unavailable")]
    Unavailable,
}

/// What the platform's input stack provides.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Capabilities {
    /// Pen support.
    pub pen: Availability,
    /// Precise scrolling.
    pub precise_scroll: Availability,
    /// Trackpad gestures.
    pub trackpad_gestures: Availability,
    /// System gesture exclusion regions.
    pub system_gesture_exclusion: Availability,
    /// Window move regions.
    pub window_move: Availability,
    /// Window resize regions.
    pub window_resize: Availability,
    /// Caption button regions.
    pub caption_buttons: Availability,
    /// Outgoing drags.
    pub outgoing_drag: Availability,
    /// The pointer kinds present.
    pub present: Vec<PointerKind>,
}

/// A window's execution state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum Execution {
    /// The `running` execution state.
    #[serde(rename = "running")]
    Running,
    /// The `suspended` execution state.
    #[serde(rename = "suspended")]
    Suspended,
    /// The `detached` execution state.
    #[serde(rename = "detached")]
    Detached,
}

/// What a window reported about itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum WindowObservation {
    #[serde(rename = "metrics")]
    /// The drawable area in device pixels and its pixel ratio.
    Metrics {
        /// The drawable width, in device pixels.
        width: i32,
        /// The drawable height, in device pixels.
        height: i32,
        /// Device pixels per logical pixel.
        scale_factor: f64,
    },
    /// Whether the window has focus.
    #[serde(rename = "focus")]
    Focus(bool),
    /// Whether the window is visible.
    #[serde(rename = "visibility")]
    Visibility(bool),
    /// The window's execution state.
    #[serde(rename = "execution")]
    Execution(Execution),
    /// The window's execution state, focus and visibility together.
    #[serde(rename = "snapshot")]
    Snapshot {
        /// The execution state.
        execution: Execution,
        /// Whether the window has focus.
        focused: bool,
        /// Whether the window is visible.
        visible: bool,
    },
    /// Whether a pointer is inside the window.
    #[serde(rename = "hover")]
    Hover(bool),
    /// The window moved.
    #[serde(rename = "moved")]
    Moved,
    #[serde(rename = "insets")]
    /// What covers the window: the safe area, the keyboard and the system
    /// gesture regions, in logical pixels as top, right, bottom, left.
    Insets {
        /// The safe area, as top, right, bottom, left.
        safe_area: [f64; 4],
        /// The keyboard's inset; absent for a hidden keyboard at rest.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        keyboard: Option<crate::native_recording::KeyboardInset>,
        /// The system gesture regions, as top, right, bottom, left.
        #[serde(default)]
        system_gestures: [f64; 4],
    },
    /// The system appearance.
    #[serde(rename = "brightness")]
    Brightness(Brightness),
}

/// The system's light or dark appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum Brightness {
    /// The `light` appearance.
    #[serde(rename = "light")]
    Light,
    /// The `dark` appearance.
    #[serde(rename = "dark")]
    Dark,
}

/// Where a scroll-into-view action places its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum ScrollHint {
    /// The `top_left` scroll hint.
    #[serde(rename = "top_left")]
    TopLeft,
    /// The `bottom_right` scroll hint.
    #[serde(rename = "bottom_right")]
    BottomRight,
    /// The `top_edge` scroll hint.
    #[serde(rename = "top_edge")]
    TopEdge,
    /// The `bottom_edge` scroll hint.
    #[serde(rename = "bottom_edge")]
    BottomEdge,
    /// The `left_edge` scroll hint.
    #[serde(rename = "left_edge")]
    LeftEdge,
    /// The `right_edge` scroll hint.
    #[serde(rename = "right_edge")]
    RightEdge,
}

/// A position in a node's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct TextPosition {
    /// The node's id.
    pub node: u64,
    /// The character's index in the node's text.
    pub character_index: usize,
}

/// The payload of an assistive action request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum ActionData {
    /// A custom action's id.
    #[serde(rename = "custom_action")]
    CustomAction(i32),
    /// A text value to set.
    #[serde(rename = "value")]
    Value(String),
    /// A numeric value to set.
    #[serde(rename = "numeric_value")]
    NumericValue(f64),
    /// The unit of a scroll action.
    #[serde(rename = "scroll_unit")]
    ScrollUnit(AxScrollUnit),
    /// Where to place the target.
    #[serde(rename = "scroll_hint")]
    ScrollHint(ScrollHint),
    /// The point to scroll to, as x, y.
    #[serde(rename = "scroll_to_point")]
    ScrollToPoint([f64; 2]),
    /// The scroll offset to set, as x, y.
    #[serde(rename = "set_scroll_offset")]
    SetScrollOffset([f64; 2]),
    /// The text selection to set.
    #[serde(rename = "set_text_selection")]
    SetTextSelection {
        /// Where the selection starts.
        anchor: TextPosition,
        /// Where the selection ends.
        focus: TextPosition,
    },
}

/// The unit of an assistive scroll action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum AxScrollUnit {
    /// The `item` assistive scroll unit.
    #[serde(rename = "item")]
    Item,
    /// The `page` assistive scroll unit.
    #[serde(rename = "page")]
    Page,
}

/// An assistive technology's action request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ActionRequest {
    /// The action, by its pinned spelling.
    pub action: String,
    /// The target tree's UUID, as 32 lowercase hex digits.
    pub tree: String,
    /// The target node's id.
    pub node: u64,
    /// The action's payload, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<ActionData>,
}

/// A preferences row: its revision beside the preferences, in the schema a
/// native recording's `preferences` row shares (`flui-protocol`), so the two
/// never spell a preference differently.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Preferences {
    /// The preferences' revision.
    pub revision: u64,
    /// The preferences, flattened into the row's object.
    #[serde(flatten)]
    pub values: crate::native_recording::Preferences,
}

/// A device in the header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Device {
    /// The device's id.
    pub device: u64,
    /// The device's kind.
    pub kind: PointerKind,
    /// The device's name, when known; redacted unless text is kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// One pointer kind's gesture values in a window's header entry.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Profile {
    /// The pointer kind the values belong to.
    pub kind: PointerKind,
    /// The kind's gesture values.
    pub values: GestureValues,
}

/// One pointer kind's resolved gesture values.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct GestureValues {
    /// Touch slop.
    pub touch_slop: f64,
    /// Pan slop.
    pub pan_slop: f64,
    /// Vertical pan slop.
    pub pan_slop_vertical: f64,
    /// Horizontal pan slop.
    pub pan_slop_horizontal: f64,
    /// Scale slop.
    pub scale_slop: f64,
    /// Double-tap slop.
    pub double_tap_slop: f64,
    /// Double-tap timeout, in nanoseconds.
    pub double_tap_timeout_ns: u64,
    /// Long-press timeout, in nanoseconds.
    pub long_press_timeout_ns: u64,
    /// Minimum fling velocity.
    pub min_fling_velocity: f64,
    /// Maximum fling velocity.
    pub max_fling_velocity: f64,
    /// How fling velocity is estimated.
    pub velocity_estimator: VelocityEstimator,
    /// The position smoothing tuning, when smoothing is on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position_smoothing: Option<Smoothing>,
    /// Minimum target size.
    pub min_target: f64,
    /// Spring-loading delay, in nanoseconds.
    pub spring_loading_delay_ns: u64,
    /// Swipe edge width.
    pub swipe_edge_width: f64,
    /// Where a scroll's remainder is handed off.
    pub remainder_handoff: RemainderHandoff,
    /// How long a contactless latch idles before it releases, in nanoseconds.
    pub contactless_latch_idle_ns: u64,
    /// How long a phased latch idles before it releases, in nanoseconds.
    pub phased_latch_idle_ns: u64,
    /// The longest a gesture may wait to resolve, in nanoseconds.
    pub max_resolution_delay_ns: u64,
    /// The haptic frame deadline factor, never zero.
    pub haptic_frame_deadline_factor: u32,
}

/// How fling velocity is estimated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum VelocityEstimator {
    /// The `least_squares` velocity estimator.
    #[serde(rename = "least_squares")]
    LeastSquares,
    /// The `impulse` velocity estimator.
    #[serde(rename = "impulse")]
    Impulse,
    /// The `ios` velocity estimator.
    #[serde(rename = "ios")]
    Ios,
    /// The `macos` velocity estimator.
    #[serde(rename = "macos")]
    Macos,
}

/// Where a scroll's remainder is handed off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum RemainderHandoff {
    /// The `next_sequence` remainder handoff.
    #[serde(rename = "next_sequence")]
    NextSequence,
    /// The `at_edge_within_sequence` remainder handoff.
    #[serde(rename = "at_edge_within_sequence")]
    AtEdgeWithinSequence,
}

/// The tuning of the position smoothing filter.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Smoothing {
    /// The minimum cutoff frequency.
    pub min_cutoff: f64,
    /// The speed coefficient.
    pub beta: f64,
    /// The derivative cutoff frequency.
    pub d_cutoff: f64,
}

impl Row {
    /// Replace every text-carrying payload with a placeholder and mark the row redacted when
    /// anything was replaced.
    pub fn redact(&mut self) {
        if let Self::Input(row) = self
            && row.input.redact()
        {
            row.redacted = true;
        }
    }

    /// Whether the row lost text when it was written.
    #[must_use]
    pub fn is_redacted(&self) -> bool {
        matches!(self, Self::Input(row) if row.redacted)
    }
}

impl HostInput {
    fn redact(&mut self) -> bool {
        match self {
            Self::Platform(input) => input.redact(),
            Self::Accessibility(request) => request.redact(),
            Self::Window(_) | Self::Frame { .. } | Self::AssistiveFocus(_) => false,
        }
    }
}

impl PlatformInput {
    fn redact(&mut self) -> bool {
        match self {
            Self::Keyboard(event) => event.key.redact(),
            Self::Ime(event) => event.redact(),
            Self::Pointer(_)
            | Self::PointerFrame(_)
            | Self::DragDrop(_)
            | Self::Capabilities(_) => false,
        }
    }
}

impl Key {
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

impl ImeEvent {
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

impl ActionRequest {
    fn redact(&mut self) -> bool {
        match &mut self.data {
            Some(ActionData::Value(text)) => {
                REDACTED.clone_into(text);
                true
            }
            _ => false,
        }
    }
}
