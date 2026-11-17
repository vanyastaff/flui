//! The pointer registry's journal: a backend's readings of its pointers, gestures and wheels.

use super::Time;

vocabulary! {
    /// The kind of device a reading or a hot-plug names.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema), schemars(rename = "NativePointerKind"))]
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
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema), schemars(rename = "NativeCancelReason"))]
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
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
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
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
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
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
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
    /// Where fingers resting on a touchpad are.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum HoldPhase {
        /// They came to rest.
        Begin => "begin",
        /// They lifted without moving.
        End => "end",
        /// They started a gesture, or the platform took the hold back.
        Cancelled => "cancelled",
    }
}

vocabulary! {
    /// A touchpad viewport's status (Win32 DirectManipulation).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
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
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
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
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema), schemars(rename = "NativeScrollPhase"))]
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
    /// Whether a scroll's delta runs the way the device moved or the reverse.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema), schemars(rename = "NativeScrollInversion"))]
    pub enum ScrollInversion {
        /// The way the device moved.
        FromDevice => "from_device",
        /// The platform reversed it (natural scrolling).
        InvertedFromDevice => "inverted_from_device",
    }
}

vocabulary! {
    /// The unit a scroll's delta is in.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema), schemars(rename = "NativeScrollUnit"))]
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

/// How much a gesture reading changed its component.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
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
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
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
    /// Fingers resting on a touchpad.
    Hold(HoldPhase),
    /// A touchpad swipe of several fingers.
    Swipe(Swipe),
}

/// A touchpad swipe reading.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Swipe {
    /// How many fingers swipe, at least one.
    pub fingers: u8,
    /// Where it is in its life.
    pub phase: GesturePhase,
    /// The horizontal step since the swipe's previous reading, in the reading's raw unit.
    pub dx: f64,
    /// The vertical step since the swipe's previous reading, in the reading's raw unit.
    pub dy: f64,
}

/// A pen's angles, as its backend reports them.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
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
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        altitude: Option<f64>,
        /// The azimuth.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        azimuth: Option<f64>,
    },
}

/// One reading, flat: the fields a backend copies out of its native event.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Reading {
    /// The surface it happened on.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "super::is_zero")
    )]
    pub surface: u64,
    /// The device, when the backend names one.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub device: Option<u64>,
    /// The backend's label for the contact.
    pub contact: u64,
    /// The device kind.
    pub kind: PointerKind,
    /// Whether this is the primary pointer of its kind, when the backend says.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub primary: Option<bool>,
    /// What it did.
    pub action: Action,
    /// The buttons held, by number, when the backend reports them.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub buttons: Option<Vec<u8>>,
    /// The modifiers held.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub modifiers: Vec<Modifier>,
    /// The OS click count, when the backend counts.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub click_count: Option<u8>,
    /// When.
    pub time: Time,
    /// The performance count, when the device reports one.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub counter: Option<u64>,
    /// Where, in device pixels.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub position: Option<[f64; 2]>,
    /// Device pixels per logical pixel.
    #[cfg_attr(
        feature = "serde",
        serde(default = "super::one", skip_serializing_if = "super::is_one")
    )]
    pub scale: f64,
    /// Pressure, 0 to 1.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub pressure: Option<f64>,
    /// A pen's barrel pressure, -1 to 1.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub tangential_pressure: Option<f64>,
    /// A pen's angles.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub angles: Option<Angles>,
    /// A pen's rotation about its axis, in degrees.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub twist_degrees: Option<f64>,
    /// The contact's width and height, in device pixels.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub contact_size: Option<[f64; 2]>,
}

/// One viewport callback: a status, or the content's scale and offset.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Viewport {
    /// The viewport's status.
    Status(ViewportStatus),
    /// The content's scale, x and y.
    Content([f64; 3]),
}

/// Where and when a viewport call happened, flat in its row.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Frame {
    /// When.
    pub time: Time,
    /// The performance count, when the device reports one.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub counter: Option<u64>,
    /// The cursor, in device pixels.
    pub position: [f64; 2],
    /// Device pixels per logical pixel.
    #[cfg_attr(
        feature = "serde",
        serde(default = "super::one", skip_serializing_if = "super::is_one")
    )]
    pub scale: f64,
    /// The modifiers held.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub modifiers: Vec<Modifier>,
}

/// One wheel message: its signed distance in 1/120 detents as Win32 signs it, the cursor in
/// device pixels and its time on the event timeline.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Wheel {
    /// The axis.
    pub axis: WheelAxis,
    /// The distance, in 1/120 detents.
    pub distance: i16,
    /// The cursor, in device pixels.
    pub position: [f64; 2],
    /// Device pixels per logical pixel.
    #[cfg_attr(
        feature = "serde",
        serde(default = "super::one", skip_serializing_if = "super::is_one")
    )]
    pub scale: f64,
    /// When, on the event timeline.
    pub time_ns: u64,
    /// The modifiers held.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub modifiers: Vec<Modifier>,
}

/// One scroll step under an open contact, in the owned sign and unit, its phase resolved.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "schemars",
    derive(schemars::JsonSchema),
    schemars(rename = "NativeScroll")
)]
pub struct Scroll {
    /// The surface.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "super::is_zero")
    )]
    pub surface: u64,
    /// The device, when the backend names one.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub device: Option<u64>,
    /// The contact it scrolls from.
    pub contact: u64,
    /// The phase, when the device has one.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub phase: Option<ScrollPhase>,
    /// The delta's unit.
    pub unit: ScrollUnit,
    /// The delta, x and y.
    pub delta: [f64; 2],
    /// Whether the device reports precise deltas.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub precise: bool,
    /// Whole notches of a wheel the device reports as such (X11's, Wayland's `value120`).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub notched: bool,
    /// Whether the platform reversed the device's direction, when it says.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub inversion: Option<ScrollInversion>,
    /// When, on the event timeline.
    pub time_ns: u64,
    /// The modifiers held.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub modifiers: Vec<Modifier>,
}
