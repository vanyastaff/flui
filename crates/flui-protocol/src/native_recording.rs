//! The native recording schema (ADR-0215 §4): the journal of one backend's pointer registry,
//! before FLUI translated it.
//!
//! A recording is JSON Lines: one [`Header`] object, then one [`Row`] object per registry
//! call — a reading, a coalesced batch, a device hot-plug, a capture loss. Rows hold the plain
//! fields a backend copies out of its native event, so a Win32 or Apple recording replays
//! through the shared decoder on any host. The header names the format's major version under
//! [`VERSION_KEY`]; a reader skips unknown fields, and an unknown variant is a refusal, never a
//! skipped row.
//!
//! `flui-native` writes and replays these records; this module only declares them, so a
//! reader such as the driver reads a recording without the core. The format is versioned by
//! [`FORMAT_VERSION`], apart from [`PROTOCOL_VERSION`](crate::PROTOCOL_VERSION).
//!
//! The module exists with the `serde` feature: its types are a serialization format and
//! nothing else.

use serde::{Deserialize, Serialize};

/// The native recording format's major version.
pub const FORMAT_VERSION: u32 = 1;

/// The header field that names the format and its major version.
pub const VERSION_KEY: &str = "flui_native_recording";

vocabulary! {
    /// The backend a recording was taken on.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum Backend {
        /// Win32 `WM_POINTER` and the legacy mouse route.
        #[serde(rename = "win32")]
        Win32 => "win32",
        /// AppKit `NSEvent`s.
        #[serde(rename = "appkit")]
        AppKit => "appkit",
        /// UIKit `UITouch`es.
        #[serde(rename = "uikit")]
        UiKit => "uikit",
        /// Android `MotionEvent`s.
        #[serde(rename = "android")]
        Android => "android",
        /// The winit fallback.
        #[serde(rename = "winit")]
        Winit => "winit",
        /// The native X11 host's XInput 2 events.
        #[serde(rename = "x11")]
        X11 => "x11",
        /// DOM `PointerEvent`s.
        #[serde(rename = "web")]
        Web => "web",
        /// A Wayland compositor's seats.
        #[serde(rename = "wayland")]
        Wayland => "wayland",
        /// The headless backend.
        #[serde(rename = "headless")]
        Headless => "headless",
    }
}

vocabulary! {
    /// The kind of device a reading or a hot-plug names.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum PointerKind {
        /// A mouse.
        Mouse => "mouse",
        /// A finger on a touch screen.
        Touch => "touch",
        /// A pen's tip.
        Pen => "pen",
        /// A pen's eraser.
        Eraser => "eraser",
        /// A precision touchpad.
        Trackpad => "trackpad",
        /// A pointer no physical device drives.
        Virtual => "virtual",
        /// A device the OS did not classify.
        Unknown => "unknown",
    }
}

vocabulary! {
    /// Why a contact ended without lifting.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum CancelReason {
        /// The OS cancelled it.
        Platform => "platform",
        /// The window lost the pointer capture.
        CaptureLost => "capture_lost",
        /// The window lost focus.
        FocusLost => "focus_lost",
        /// Its device was removed.
        DeviceRemoved => "device_removed",
        /// A reading of it could not be decoded.
        InvalidInput => "invalid_input",
    }
}

vocabulary! {
    /// A held modifier, as W3C `getModifierState` names it.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum Modifier {
        /// Alt, or Option.
        Alt => "alt",
        /// AltGr.
        AltGraph => "alt_graph",
        /// Caps Lock.
        CapsLock => "caps_lock",
        /// Control.
        Control => "control",
        /// Fn.
        Fn => "fn",
        /// Fn Lock.
        FnLock => "fn_lock",
        /// Meta: Windows, or Command.
        Meta => "meta",
        /// Num Lock.
        NumLock => "num_lock",
        /// Scroll Lock.
        ScrollLock => "scroll_lock",
        /// Shift.
        Shift => "shift",
        /// Symbol.
        Symbol => "symbol",
        /// Symbol Lock.
        SymbolLock => "symbol_lock",
    }
}

vocabulary! {
    /// Which part of a trackpad gesture a gesture reading carries.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum GestureComponent {
        /// The pinch.
        Scale => "scale",
        /// The rotation.
        Rotation => "rotation",
        /// The session both belong to.
        Session => "session",
    }
}

vocabulary! {
    /// Where a gesture is in its life.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum GesturePhase {
        /// It began.
        Began => "began",
        /// It changed.
        Changed => "changed",
        /// It ended.
        Ended => "ended",
        /// It was cancelled.
        Cancelled => "cancelled",
    }
}

vocabulary! {
    /// A touchpad viewport's status (Win32 DirectManipulation).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum ViewportStatus {
        /// Being built.
        Building => "building",
        /// Enabled, idle.
        Enabled => "enabled",
        /// Disabled.
        Disabled => "disabled",
        /// Fingers are moving the content.
        Running => "running",
        /// The content coasts after the fingers lifted.
        Inertia => "inertia",
        /// Ready for the next manipulation.
        Ready => "ready",
        /// Suspended by the OS.
        Suspended => "suspended",
    }
}

vocabulary! {
    /// The axis a mouse wheel turned on.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum WheelAxis {
        /// The vertical wheel.
        Vertical => "vertical",
        /// The horizontal wheel, or a tilt.
        Horizontal => "horizontal",
    }
}

vocabulary! {
    /// Where a scroll is in its gesture and momentum.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum ScrollPhase {
        /// The fingers began.
        Began => "began",
        /// The fingers moved.
        Changed => "changed",
        /// The fingers lifted.
        Ended => "ended",
        /// The gesture was cancelled.
        Cancelled => "cancelled",
        /// Momentum began.
        MomentumBegan => "momentum_began",
        /// Momentum continued.
        MomentumChanged => "momentum_changed",
        /// Momentum ended.
        MomentumEnded => "momentum_ended",
    }
}

vocabulary! {
    /// The unit a scroll's delta is in.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum ScrollUnit {
        /// Lines of text.
        Lines => "lines",
        /// Logical pixels.
        Pixels => "pixels",
        /// Pages.
        Pages => "pages",
        /// Wheel detents.
        Detents => "detents",
    }
}

/// The performance counter at the recording's epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Counter {
    /// The count.
    pub count: u64,
    /// Counts per second; never zero.
    pub frequency: u64,
}

/// The OS click thresholds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct ClickSettings {
    /// The longest gap between two clicks of one sequence, in nanoseconds.
    pub interval_ns: u64,
    /// How far apart two clicks of one sequence may be, across.
    pub width: f64,
    /// How far apart two clicks of one sequence may be, down.
    pub height: f64,
}

/// A device the rows read, with every kind they read it as.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Device {
    /// The backend's label for it.
    pub device: u64,
    /// The kinds the rows read it as.
    pub kinds: Vec<PointerKind>,
}

/// The first line: the backend, its clock anchors and the registry's setup.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Header {
    /// The format's major version, [`FORMAT_VERSION`]; the field is [`VERSION_KEY`].
    pub flui_native_recording: u32,
    /// The backend.
    pub backend: Backend,
    /// The epoch instant on the event timeline, in nanoseconds.
    pub epoch_ns: u64,
    /// System uptime at the epoch, in nanoseconds.
    pub uptime_ns: u64,
    /// The performance counter at the epoch, when the host has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counter: Option<Counter>,
    /// The contact given pointer identity one, when the backend reserves one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reserved_contact: Option<u64>,
    /// The first pointer identity, when not one; never zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_pointer: Option<u64>,
    /// The first device identity, when not the registry's default; never zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_device: Option<u64>,
    /// The OS click thresholds the registry started with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub click_settings: Option<ClickSettings>,
    /// Where the readings came from, when not a capture: a hand-written recording says so
    /// here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<String>,
    /// Every device the rows read, with the kinds they read it as.
    #[serde(default)]
    pub devices: Vec<Device>,
}

/// When a native event happened, in the clock its backend stamps.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Time {
    /// A wrapping millisecond tick and the uptime it was placed against.
    #[serde(rename = "tick_ms")]
    TickMillis {
        /// The tick.
        tick: u32,
        /// The uptime, in milliseconds, when it was observed.
        observed_ms: u64,
    },
    /// Seconds of system uptime.
    #[serde(rename = "uptime_s")]
    UptimeSeconds(f64),
    /// Nanoseconds on the event timeline.
    #[serde(rename = "epoch_ns")]
    Epoch(u64),
}

/// How much a gesture reading changed its component.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum GestureDelta {
    /// A scale factor.
    Scale(f64),
    /// A rotation, in radians.
    Rotation(f64),
    /// No change.
    None,
}

/// A trackpad gesture reading.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Gesture {
    /// The component it carries.
    pub component: GestureComponent,
    /// Where it is in its life.
    pub phase: GesturePhase,
    /// How much it changed.
    pub delta: GestureDelta,
}

/// What a reading did.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Action {
    /// The pointer entered the surface.
    Enter,
    /// The pointer left the surface.
    Leave,
    /// The pointer moved.
    Move,
    /// A button went down: its number, 1 for the primary.
    Press(u8),
    /// A button went up: its number.
    Release(u8),
    /// The contact ended without lifting.
    Cancel(CancelReason),
    /// A trackpad gesture.
    Gesture(Gesture),
}

/// A pen's angles, as its backend reports them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Angles {
    /// Tilt from the vertical along each axis, in degrees.
    TiltDegrees {
        /// Along x.
        x: f64,
        /// Along y.
        y: f64,
    },
    /// Altitude and azimuth, in radians.
    Spherical {
        /// The altitude.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        altitude: Option<f64>,
        /// The azimuth.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        azimuth: Option<f64>,
    },
}

/// One reading, flat: the fields a backend copies out of its native event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Reading {
    /// The surface it happened on.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub surface: u64,
    /// The device, when the backend names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<u64>,
    /// The backend's label for the contact.
    pub contact: u64,
    /// The device kind.
    pub kind: PointerKind,
    /// Whether this is the primary pointer of its kind, when the backend says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary: Option<bool>,
    /// What it did.
    pub action: Action,
    /// The buttons held, by number, when the backend reports them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub buttons: Option<Vec<u8>>,
    /// The modifiers held.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<Modifier>,
    /// The OS click count, when the backend counts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub click_count: Option<u8>,
    /// When.
    pub time: Time,
    /// The performance count, when the device reports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counter: Option<u64>,
    /// Where, in device pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<[f64; 2]>,
    /// Device pixels per logical pixel.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub scale: f64,
    /// Pressure, 0 to 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure: Option<f64>,
    /// A pen's barrel pressure, -1 to 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tangential_pressure: Option<f64>,
    /// A pen's angles.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angles: Option<Angles>,
    /// A pen's rotation about its axis, in degrees.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub twist_degrees: Option<f64>,
    /// The contact's width and height, in device pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_size: Option<[f64; 2]>,
}

/// One viewport callback: a status, or the content's scale and offset.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Viewport {
    /// The viewport's status.
    Status(ViewportStatus),
    /// The content's scale, x and y.
    Content([f64; 3]),
}

/// Where and when a viewport call happened, flat in its row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Frame {
    /// When.
    pub time: Time,
    /// The performance count, when the device reports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counter: Option<u64>,
    /// The cursor, in device pixels.
    pub position: [f64; 2],
    /// Device pixels per logical pixel.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub scale: f64,
    /// The modifiers held.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<Modifier>,
}

/// One wheel message: its signed distance in 1/120 detents as Win32 signs it, the cursor in
/// device pixels and its time on the event timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Wheel {
    /// The axis.
    pub axis: WheelAxis,
    /// The distance, in 1/120 detents.
    pub distance: i16,
    /// The cursor, in device pixels.
    pub position: [f64; 2],
    /// Device pixels per logical pixel.
    #[serde(default = "one", skip_serializing_if = "is_one")]
    pub scale: f64,
    /// When, on the event timeline.
    pub time_ns: u64,
    /// The modifiers held.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<Modifier>,
}

/// One scroll step under an open contact, in the owned sign and unit, its phase resolved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Scroll {
    /// The surface.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub surface: u64,
    /// The device, when the backend names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<u64>,
    /// The contact it scrolls from.
    pub contact: u64,
    /// The phase, when the device has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<ScrollPhase>,
    /// The delta's unit.
    pub unit: ScrollUnit,
    /// The delta, x and y.
    pub delta: [f64; 2],
    /// Whether the device reports precise deltas.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub precise: bool,
    /// Whole notches of a wheel the device reports as such (X11's, Wayland's `value120`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub notched: bool,
    /// When, on the event timeline.
    pub time_ns: u64,
    /// The modifiers held.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<Modifier>,
}

/// One registry call, after the header.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "row", rename_all = "snake_case")]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Row {
    /// One reading, decoded.
    Reading {
        /// The reading.
        reading: Reading,
    },
    /// A coalesced batch: the readings oldest first, the predicted ones, and the current
    /// reading they attach to.
    Batch {
        /// Readings coalesced before `current`.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        coalesced: Vec<Reading>,
        /// Readings predicted after `current`.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        predicted: Vec<Reading>,
        /// The reading the batch decodes as.
        current: Reading,
    },
    /// The OS click thresholds changed.
    ClickSettings {
        /// The new thresholds; none restores the registry's default.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        settings: Option<ClickSettings>,
    },
    /// A precision touchpad's viewport reported a status or a content transform.
    Viewport {
        /// What it reported.
        viewport: Viewport,
        /// Where and when.
        #[serde(flatten)]
        frame: Frame,
    },
    /// The touchpad's open manipulation ended as cancelled.
    ViewportAbandoned {
        /// Where and when.
        #[serde(flatten)]
        frame: Frame,
    },
    /// A mouse wheel turned under the primary mouse.
    Wheel {
        /// The wheel message.
        wheel: Wheel,
    },
    /// An open contact scrolled.
    Scroll {
        /// The scroll step.
        scroll: Scroll,
    },
    /// A device was announced.
    AddDevice {
        /// Its label.
        device: u64,
        /// Its kind.
        kind: PointerKind,
        /// When, on the event timeline.
        time_ns: u64,
    },
    /// A device was removed.
    RemoveDevice {
        /// Its label.
        device: u64,
        /// When, on the event timeline.
        time_ns: u64,
    },
    /// Every contact on a surface ended.
    CancelSurface {
        /// The surface.
        #[serde(default, skip_serializing_if = "is_zero")]
        surface: u64,
        /// When, on the event timeline.
        time_ns: u64,
        /// Why.
        reason: CancelReason,
    },
    /// The contacts with one label ended (a capture loss).
    CancelContact {
        /// The surface.
        #[serde(default, skip_serializing_if = "is_zero")]
        surface: u64,
        /// The label.
        contact: u64,
        /// When, on the event timeline.
        time_ns: u64,
        /// Why.
        reason: CancelReason,
    },
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
