//! The native recording schema (ADR-0215 §4): the journal of one backend's pointer registry,
//! loop delivery, window delivery, system reads or window snapshots, before the application
//! heard it.
//!
//! A recording is JSON Lines: one [`Header`] object, which names the [`Stream`] it journals,
//! then one [`Row`] object per call — a reading, a coalesced batch, a device hot-plug, a
//! capture loss; a system notice, a session end, a reopen; an input method's event, a lock it
//! held on a text store, a close request and a close; a read of
//! the system preferences or the displays; a window's snapshot. Rows hold the plain fields a
//! backend copies out of its native event, so a Win32 or Apple recording replays through the
//! shared decoder on any host. The header names the format's major version under
//! [`VERSION_KEY`]; a reader skips unknown fields, and an unknown variant is a refusal, never a
//! skipped row. A recording is untrusted input: a reader refuses a line longer than
//! [`MAX_LINE_BYTES`] before it parses it, a row past [`MAX_ROWS`] and a line that takes the
//! recording past [`MAX_RECORDING_BYTES`].
//!
//! `flui-native` writes and replays these records; this module only declares them, so a
//! reader such as the driver reads a recording without the core. The format is versioned by
//! [`FORMAT_VERSION`], apart from [`PROTOCOL_VERSION`](crate::PROTOCOL_VERSION).
//!
//! With the `serde` feature its types read and write the recording; without it they are plain
//! data, which the input recording's preference rows share.

/// The native recording format's major version.
pub const FORMAT_VERSION: u32 = 2;

/// The header field that names the format and its major version.
pub const VERSION_KEY: &str = "flui_native_recording";

vocabulary! {
    /// The backend a recording was taken on.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum Backend {
        /// Win32 `WM_POINTER` and the legacy mouse route.
        #[cfg_attr(feature = "serde", serde(rename = "win32"))]
        Win32 => "win32",
        /// AppKit `NSEvent`s.
        #[cfg_attr(feature = "serde", serde(rename = "appkit"))]
        AppKit => "appkit",
        /// UIKit `UITouch`es.
        #[cfg_attr(feature = "serde", serde(rename = "uikit"))]
        UiKit => "uikit",
        /// Android `MotionEvent`s.
        #[cfg_attr(feature = "serde", serde(rename = "android"))]
        Android => "android",
        /// The native X11 host's XInput 2 events.
        #[cfg_attr(feature = "serde", serde(rename = "x11"))]
        X11 => "x11",
        /// DOM `PointerEvent`s.
        #[cfg_attr(feature = "serde", serde(rename = "web"))]
        Web => "web",
        /// A Wayland compositor's seats.
        #[cfg_attr(feature = "serde", serde(rename = "wayland"))]
        Wayland => "wayland",
        /// The headless backend.
        #[cfg_attr(feature = "serde", serde(rename = "headless"))]
        Headless => "headless",
    }
}

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

/// The performance counter at the recording's epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Counter {
    /// The count.
    pub count: u64,
    /// Counts per second; never zero.
    pub frequency: u64,
}

/// The OS click thresholds.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "schemars",
    derive(schemars::JsonSchema),
    schemars(rename = "NativeDevice")
)]
pub struct Device {
    /// The backend's label for it.
    pub device: u64,
    /// The kinds the rows read it as.
    pub kinds: Vec<PointerKind>,
}

vocabulary! {
    /// What a recording journals: one pointer registry, one loop's application delivery, one
    /// window's delivery, one platform's reads of the system, or one window's published
    /// snapshots.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum Stream {
        /// A pointer registry: readings, batches, devices, cancels, wheels and scrolls.
        #[default]
        Registry => "registry",
        /// A loop's application delivery: system notices, session ends and reopens.
        App => "app",
        /// A window's delivery: its input method's events and text store locks, its close
        /// requests and its close.
        Window => "window",
        /// A platform's reads of the system: its preferences and displays.
        Platform => "platform",
        /// A window's published snapshots.
        WindowState => "window_state",
    }
}

/// The first line: the backend, what the recording journals, its clock anchors and the
/// registry's setup.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "schemars",
    derive(schemars::JsonSchema),
    schemars(rename = "NativeHeader")
)]
pub struct Header {
    /// The format's major version, [`FORMAT_VERSION`]; the field is [`VERSION_KEY`].
    pub flui_native_recording: u32,
    /// The backend.
    pub backend: Backend,
    /// What the recording journals.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "crate::skip::is_default")
    )]
    pub stream: Stream,
    /// The epoch instant on the event timeline, in nanoseconds; a registry's clock, so only a
    /// registry's recording has it.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub epoch_ns: Option<u64>,
    /// System uptime at the epoch, in nanoseconds; with `epoch_ns`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub uptime_ns: Option<u64>,
    /// The performance counter at the epoch, when the host has one.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub counter: Option<Counter>,
    /// The contact given pointer identity one, when the backend reserves one.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub reserved_contact: Option<u64>,
    /// The first pointer identity, when not one; never zero.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub first_pointer: Option<u64>,
    /// The first device identity, when not the registry's default; never zero.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub first_device: Option<u64>,
    /// The OS click thresholds the registry started with.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub click_settings: Option<ClickSettings>,
    /// Where the readings came from, when not a capture: a hand-written recording says so
    /// here.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub provenance: Option<String>,
    /// The recording joined a live stream late: it begins with the stream's setup, not its
    /// first row, so a row may name a text store whose focus or document it never saw.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub joined_late: bool,
    /// Every device the rows read, with the kinds they read it as.
    #[cfg_attr(feature = "serde", serde(default))]
    pub devices: Vec<Device>,
}

/// When a native event happened, in the clock its backend stamps.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Time {
    /// A wrapping millisecond tick and the uptime it was placed against.
    #[cfg_attr(feature = "serde", serde(rename = "tick_ms"))]
    TickMillis {
        /// The tick.
        tick: u32,
        /// The uptime, in milliseconds, when it was observed.
        observed_ms: u64,
    },
    /// Seconds of system uptime.
    #[cfg_attr(feature = "serde", serde(rename = "uptime_s"))]
    UptimeSeconds(f64),
    /// Nanoseconds on the event timeline.
    #[cfg_attr(feature = "serde", serde(rename = "epoch_ns"))]
    Epoch(u64),
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
    #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "is_zero"))]
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
        serde(default = "one", skip_serializing_if = "is_one")
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
        serde(default = "one", skip_serializing_if = "is_one")
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
        serde(default = "one", skip_serializing_if = "is_one")
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
    #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "is_zero"))]
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

vocabulary! {
    /// How much memory the system wants back.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum MemoryPressureLevel {
        /// Free what is easy to restore.
        Moderate => "moderate",
        /// Free everything the current frame does not need.
        Critical => "critical",
    }
}

vocabulary! {
    /// A phase of the user's session ending.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum SessionEndPhase {
        /// The session is about to end; the application is asked whether to wait.
        Query => "query",
        /// The session end was cancelled.
        Cancelled => "cancelled",
        /// The session is ending now.
        Ending => "ending",
    }
}

/// A one-off system event the host reported to the application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Notice {
    /// The machine is about to sleep.
    Sleep,
    /// The machine woke from sleep.
    Wake,
    /// The user's session was locked.
    SessionLocked,
    /// The user's session was unlocked.
    SessionUnlocked,
    /// The system wants memory back.
    MemoryPressure(MemoryPressureLevel),
}

/// An input method's event, as the window delivered it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(
    feature = "schemars",
    derive(schemars::JsonSchema),
    schemars(rename = "NativeImeEvent")
)]
#[non_exhaustive]
pub enum ImeEvent {
    /// Composition became available for the focused input.
    Enabled,
    /// The composition text changed.
    Preedit {
        /// The composition text.
        text: String,
        /// The cursor or selection, as byte offsets into `text`; none hides the caret.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        cursor: Option<[usize; 2]>,
    },
    /// Composition finished with this text.
    Commit(String),
    /// Composition ended.
    Disabled,
}

/// What a window's input method did through an event. An input method that reads and edits the
/// focused text store instead is recorded as [`Row::StoreFocus`] and [`Row::StoreLock`] rows.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Ime {
    /// An event the window delivered.
    Event(ImeEvent),
}

/// The system's motion preference.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Motion {
    /// No restriction.
    NoPreference,
    /// The user asked for reduced motion.
    Reduce,
    /// Animation durations are multiplied by this finite, positive factor.
    Scaled(f64),
}

/// The system's text-weight request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum TextWeight {
    /// The authored weight, unadjusted.
    NoPreference,
    /// Heavier, more legible text.
    Bold,
    /// A signed, nonzero adjustment to every resolved weight.
    Adjustment(i32),
}

/// What one discrete vertical wheel notch scrolls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum WheelStep {
    /// This many lines; zero disables the axis.
    Lines(u32),
    /// One page.
    Page,
}

/// The native mouse geometry, in device pixels at `pixel_ratio`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct MouseGeometry {
    /// The double-click rectangle, width and height.
    pub double_click_area: [i32; 2],
    /// The drag threshold, horizontal and vertical.
    pub drag_tolerance: [i32; 2],
    /// The device pixel ratio the values were sampled at.
    pub pixel_ratio: f64,
}

/// The native touch geometry, in device pixels at `pixel_ratio`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct TouchGeometry {
    /// The distance a touch travels before it is a drag.
    pub touch_slop: i32,
    /// The distance between two taps of a double tap.
    pub double_tap_slop: i32,
    /// The slowest fling, per second.
    pub min_fling: i32,
    /// The fastest fling, per second.
    pub max_fling: i32,
    /// The device pixel ratio the values were sampled at.
    pub pixel_ratio: f64,
}

/// The system's gesture timing and geometry; an absent field was not observed.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Gestures {
    /// The mouse double-click interval, in nanoseconds.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub double_click_interval_ns: Option<u64>,
    /// The touch double-tap interval, in nanoseconds.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub double_tap_interval_ns: Option<u64>,
    /// The shortest long press, in nanoseconds.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub long_press_timeout_ns: Option<u64>,
    /// The native mouse geometry.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub native_mouse_geometry: Option<MouseGeometry>,
    /// The native touch geometry.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub native_touch_geometry: Option<TouchGeometry>,
}

/// The user's regional formats, each as the value of its Unicode `u` extension key (UTS #35); an
/// absent field was not observed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Regional {
    /// The first day of the week, as an `fw` value such as `mon`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub first_weekday: Option<String>,
    /// The hour cycle, as an `hc` value such as `h23`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub hour_cycle: Option<String>,
    /// The measurement system, as an `ms` value such as `metric`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub measurement: Option<String>,
    /// The numbering system, as an `nu` value such as `arab`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub numbering: Option<String>,
    /// The region override, as its region code such as `GB`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub region: Option<String>,
}

/// The host's system preferences (ADR-0172); an absent field was not observed.
///
/// The same object is a native `platform` recording's `preferences` row and, beside its
/// `revision`, a host input recording's preferences row, so the two never spell a preference
/// differently.
#[derive(Debug, Clone, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "schemars",
    derive(schemars::JsonSchema),
    schemars(rename = "NativePreferences")
)]
pub struct Preferences {
    /// The text-size multiplier, in `1/64..=64`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub text_scale: Option<f64>,
    /// The motion preference.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub motion: Option<Motion>,
    /// Whether high contrast is on.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub high_contrast: Option<bool>,
    /// Whether system haptics are on.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub haptics: Option<bool>,
    /// The text-weight request.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub text_weight: Option<TextWeight>,
    /// The preferred UI locales as BCP 47 tags, in priority order.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub locales: Option<Vec<String>>,
    /// The regional formats; absent when none was observed. Boxed, since a preferences read is
    /// carried by value and most hosts observe none.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub regional: Option<Box<Regional>>,
    /// Gesture timing and geometry; always written, empty when nothing was observed.
    #[cfg_attr(feature = "serde", serde(default))]
    pub gestures: Gestures,
    /// What one vertical wheel notch scrolls.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub wheel_vertical: Option<WheelStep>,
    /// The characters one horizontal wheel notch scrolls; zero disables the axis.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub wheel_horizontal_characters: Option<u32>,
    /// The accent colour: red, green, blue and alpha, sRGB-encoded with straight alpha, each
    /// finite and in `0..=1`.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub accent_color: Option<[f32; 4]>,
    /// When the system shows a scroll bar.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub scroll_bar_visibility: Option<ScrollBarVisibility>,
    /// What a click in a scroll bar's track does.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub scroll_track_click: Option<ScrollTrackClick>,
}

vocabulary! {
    /// When the system shows a scroll bar.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum ScrollBarVisibility {
        /// While the content scrolls, fading once it stops.
        WhenScrolling => "when_scrolling",
        /// Whenever the content is longer than its viewport.
        Always => "always",
    }
}

vocabulary! {
    /// What a click in a scroll bar's track, off the thumb, does.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum ScrollTrackClick {
        /// Scroll by a page toward the click.
        Page => "page",
        /// Move the thumb to the spot clicked.
        JumpToSpot => "jump_to_spot",
    }
}

vocabulary! {
    /// Which error a failed read of the system returned, as the platform's error names it.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum FailureKind {
        /// An observation was already running or waiting for its retry, so none was available.
        Deferred => "deferred",
        /// The preference source could not complete an observation.
        Preferences => "preferences",
        /// The display source could not complete a sample.
        Displays => "displays",
        /// The host has no source for what was read.
        Unsupported => "unsupported",
        /// The event loop failed.
        EventLoop => "event_loop",
        /// Another error, which only its message describes.
        Other => "other",
    }
}

/// Why a read of the system failed: the error's kind, and its message when it has one.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Failure {
    /// Which error.
    pub kind: FailureKind,
    /// What failed, in the host's words.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub message: Option<String>,
}

/// What one read of the system preferences returned.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum PreferenceRead {
    /// The preferences the system reported.
    Observed(Preferences),
    /// The read failed; a failure is not an observation.
    Failed(Failure),
}

vocabulary! {
    /// Why the platform reports no value for a capability.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum Absence {
        /// The platform has no such capability.
        NotOnThisPlatform => "not_on_this_platform",
        /// The platform has it, this device does not.
        NotOnThisDevice => "not_on_this_device",
        /// The platform has it, the host does not read it yet.
        NotYetOnThisHost => "not_yet_on_this_host",
        /// The window withdrew its capabilities: it can publish no more of
        /// what it honours.
        Withdrawn => "withdrawn",
    }
}

vocabulary! {
    /// The primaries a display shows surfaces in.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum Gamut {
        /// sRGB.
        Srgb => "srgb",
        /// Display P3.
        DisplayP3 => "display_p3",
        /// Rec. 2020.
        Rec2020 => "rec2020",
    }
}

vocabulary! {
    /// The range a display shows surfaces in.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum DynamicRange {
        /// Standard dynamic range.
        Standard => "standard",
        /// High dynamic range.
        High => "high",
    }
}

/// How far above SDR white a display can drive a surface, or why the platform does not say.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Headroom {
    /// A finite multiple of SDR white, at least 1.
    Multiple(f32),
    /// The platform gives none.
    Absent(Absence),
}

/// A display's colour: its colour space and headroom.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Color {
    /// The primaries.
    pub gamut: Gamut,
    /// The range.
    pub dynamic_range: DynamicRange,
    /// The headroom over SDR white.
    pub headroom: Headroom,
}

/// A display's colour, or why the platform gives none.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum DisplayColor {
    /// The colour the platform reports.
    Known(Color),
    /// The platform gives none.
    Absent(Absence),
}

/// One display as a read of the system reported it.
///
/// `label` is the display's id as its platform issued it: it names the display within this
/// recording and nowhere else, so a replay issues its own id for each label.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct Display {
    /// The display within this recording.
    pub label: u64,
    /// Its name, a label, never an identity.
    pub name: String,
    /// Where its top-left corner sits in the window system's desktop, in that window system's
    /// own units: x, y. It orders the displays and is never a logical position.
    pub origin: [i32; 2],
    /// Its extent in its own logical pixels: width, height.
    pub extent: [f64; 2],
    /// The area windows can occupy, in its own logical pixels from its top-left corner: left,
    /// top, right, bottom.
    pub usable: [f64; 4],
    /// Device pixels per logical pixel.
    pub scale_factor: f64,
    /// The period of its mode, in nanoseconds, when known.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub refresh_period_ns: Option<u64>,
    /// Whether it is the primary display.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub primary: bool,
    /// The string that names it again after a restart, when the platform has one.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub persistent_key: Option<String>,
    /// Its colour.
    pub color: DisplayColor,
}

/// What one read of the displays returned.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum DisplayRead {
    /// The displays the system reported, at the platform's snapshot revision.
    Observed {
        /// The snapshot's revision, as the platform counted it.
        revision: u64,
        /// The displays.
        displays: Vec<Display>,
    },
    /// The read failed.
    Failed(Failure),
}

vocabulary! {
    /// How a window occupies its display.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum WindowMode {
        /// Its own bounds.
        Normal => "normal",
        /// Minimized.
        Minimized => "minimized",
        /// Filling the display's usable area.
        Maximized => "maximized",
        /// Borderless on the whole display.
        Fullscreen => "fullscreen",
    }
}

vocabulary! {
    /// The user's theme as applied to a window.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum Appearance {
        /// Light.
        Light => "light",
        /// Dark.
        Dark => "dark",
        /// Light, with vibrancy.
        VibrantLight => "vibrant_light",
        /// Dark, with vibrancy.
        VibrantDark => "vibrant_dark",
    }
}

vocabulary! {
    /// Whether the platform lets a window run its UI.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema), schemars(rename = "NativeExecution"))]
    pub enum Execution {
        /// Running.
        Running => "running",
        /// Suspended.
        Suspended => "suspended",
        /// Detached from its surface.
        Detached => "detached",
    }
}

vocabulary! {
    /// How a window honours a command or makes a report.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema), schemars(rename = "NativeAvailability"))]
    pub enum Availability {
        /// The window system does it.
        Native => "native",
        /// The backend builds it from what the window system provides.
        Emulated => "emulated",
        /// Not at all.
        Unavailable => "unavailable",
    }
}

/// How a window honours a command it is admitted against, or why it does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Capability {
    /// The window system does it.
    Native,
    /// The backend builds it from what the window system provides.
    Emulated,
    /// Not at all, for this reason.
    Unavailable(Absence),
}

impl Capability {
    /// The capability a recording that predates a field reads as.
    #[must_use]
    pub const fn platform_absence() -> Self {
        Self::Unavailable(Absence::NotOnThisPlatform)
    }

    /// Whether this is [`platform_absence`](Self::platform_absence), which a
    /// writer leaves out.
    #[must_use]
    pub const fn is_platform_absence(&self) -> bool {
        matches!(self, Self::Unavailable(Absence::NotOnThisPlatform))
    }
}

/// Which commands a window honours, and how.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(
    feature = "schemars",
    derive(schemars::JsonSchema),
    schemars(rename = "NativeCapabilities")
)]
pub struct Capabilities {
    /// Setting the title.
    pub sets_title: Capability,
    /// Moving the window.
    pub sets_position: Capability,
    /// Resizing the window.
    pub sets_size: Capability,
    /// Setting its size limits.
    pub sets_constraints: Capability,
    /// Making it resizable or not.
    pub sets_resizable: Capability,
    /// Giving it the system's frame or none.
    pub sets_frame: Capability,
    /// Giving it a title bar the application draws.
    pub app_title_bar: Capability,
    /// Minimizing it.
    pub minimizes: Capability,
    /// Maximizing it.
    pub maximizes: Capability,
    /// Making it fullscreen.
    pub enters_fullscreen: Capability,
    /// Hiding it.
    pub hides: Capability,
    /// Showing it.
    pub shows: Capability,
    /// Activating it.
    pub activates: Capability,
    /// Closing it.
    pub closes: Capability,
    /// Setting its material.
    pub material: Capability,
    /// Holding the pointer inside it; a recording made before the field
    /// existed reads as the platform's absence, as the fields below do.
    #[cfg_attr(
        feature = "serde",
        serde(
            default = "Capability::platform_absence",
            skip_serializing_if = "Capability::is_platform_absence"
        )
    )]
    pub confines_cursor: Capability,
    /// Holding the pointer in place.
    #[cfg_attr(
        feature = "serde",
        serde(
            default = "Capability::platform_absence",
            skip_serializing_if = "Capability::is_platform_absence"
        )
    )]
    pub locks_cursor: Capability,
    /// Keeping it above the other windows.
    #[cfg_attr(
        feature = "serde",
        serde(
            default = "Capability::platform_absence",
            skip_serializing_if = "Capability::is_platform_absence"
        )
    )]
    pub keeps_on_top: Capability,
    /// Keeping it below the other windows.
    #[cfg_attr(
        feature = "serde",
        serde(
            default = "Capability::platform_absence",
            skip_serializing_if = "Capability::is_platform_absence"
        )
    )]
    pub keeps_on_bottom: Capability,
    /// Resizing it in steps.
    #[cfg_attr(
        feature = "serde",
        serde(
            default = "Capability::platform_absence",
            skip_serializing_if = "Capability::is_platform_absence"
        )
    )]
    pub sets_resize_increments: Capability,
    /// Keeping its proportion while resized.
    #[cfg_attr(
        feature = "serde",
        serde(
            default = "Capability::platform_absence",
            skip_serializing_if = "Capability::is_platform_absence"
        )
    )]
    pub sets_aspect_ratio: Capability,
    /// Reporting that its frames reached the screen.
    pub reports_presentation: Capability,
    /// Reporting where the soft keyboard covers it; a recording made before
    /// the field existed reads as the platform's absence.
    #[cfg_attr(
        feature = "serde",
        serde(
            default = "Capability::platform_absence",
            skip_serializing_if = "Capability::is_platform_absence"
        )
    )]
    pub reports_keyboard_inset: Capability,
    /// Reporting the edges system gestures take; read as
    /// [`reports_keyboard_inset`](Self::reports_keyboard_inset) is.
    #[cfg_attr(
        feature = "serde",
        serde(
            default = "Capability::platform_absence",
            skip_serializing_if = "Capability::is_platform_absence"
        )
    )]
    pub reports_system_gestures: Capability,
    /// Reporting a covering by other windows as occluded; read as
    /// [`reports_keyboard_inset`](Self::reports_keyboard_inset) is.
    #[cfg_attr(
        feature = "serde",
        serde(
            default = "Capability::platform_absence",
            skip_serializing_if = "Capability::is_platform_absence"
        )
    )]
    pub reports_occlusion: Capability,
    /// Reporting its position.
    pub observes_position: Availability,
    /// Reporting that it is minimized.
    pub observes_minimized: Availability,
    /// Placing it where asked when it opens.
    pub places_at_open: Availability,
    /// Taking a frame rate hint.
    pub frame_rate_hint: Availability,
    /// Showing a cursor image of the application's; a recording made before
    /// the field existed reads as unavailable.
    #[cfg_attr(
        feature = "serde",
        serde(
            default = "Availability::unavailable",
            skip_serializing_if = "Availability::is_unavailable"
        )
    )]
    pub cursor_image: Availability,
}

impl Availability {
    /// What a recording that predates an availability field reads as.
    #[must_use]
    pub const fn unavailable() -> Self {
        Self::Unavailable
    }

    /// Whether this is [`Unavailable`](Self::Unavailable), which a writer
    /// leaves out of a field older recordings lack.
    #[must_use]
    pub const fn is_unavailable(&self) -> bool {
        matches!(self, Self::Unavailable)
    }
}

/// Where a window's outer top-left corner is: a point of one display, in that display's logical
/// pixels from its top-left corner.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct WindowPosition {
    /// The display, by label, as a `platform` recording of the same session writes it.
    pub display: u64,
    /// The point: x, y.
    pub at: [f64; 2],
}

/// A window rectangle: its position on a display, and its size in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct WindowGeometry {
    /// The outer top-left corner, where the OS reports one.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub position: Option<WindowPosition>,
    /// Width and height.
    pub size: [f64; 2],
}

/// A window's inner-size limits in logical pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct SizeConstraints {
    /// The smallest inner size, if limited.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub min: Option<[f64; 2]>,
    /// The largest inner size, if limited.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub max: Option<[f64; 2]>,
}

/// What frames a window.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[expect(
    clippy::exhaustive_enums,
    reason = "a pinned mirror of the host's window vocabulary: a new variant is a format version, and a `_` arm would drop a recorded line"
)]
pub enum WindowFrame {
    /// The system's title bar and borders.
    System,
    /// A title bar the application draws, inside a frame the system keeps.
    AppTitleBar {
        /// The system's own controls over it.
        system_controls: SystemControls,
    },
    /// No frame.
    None,
}

/// The controls a system draws over a title bar the application draws.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[expect(
    clippy::exhaustive_enums,
    reason = "a pinned mirror of the host's window vocabulary: a new variant is a format version, and a `_` arm would drop a recorded line"
)]
pub enum SystemControls {
    /// Where the system puts them.
    Shown,
    /// Moved from there, in logical pixels: along the bar toward its trailing edge, and down.
    Offset([f64; 2]),
    /// Not drawn.
    Hidden,
}

/// A window's drawable area and scale.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct WindowMetrics {
    /// The drawable area in device pixels: width and height.
    pub physical: [i32; 2],
    /// Device pixels per logical pixel.
    pub scale: f64,
}

/// What the operating system reported about one window, at one revision.
///
/// `display` is the display's label, as a `platform` recording of the same session writes it.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct WindowState {
    /// The revision the window published it at.
    pub revision: u64,
    /// The outer rectangle.
    pub outer: WindowGeometry,
    /// The content area's size: width and height.
    pub content_size: [f64; 2],
    /// How it occupies its display.
    pub mode: WindowMode,
    /// The rectangle it returns to from another mode, where known.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub normal_geometry: Option<WindowGeometry>,
    /// Whether it is shown.
    pub visible: bool,
    /// Whether the window system says nothing of it reaches the screen.
    pub occluded: bool,
    /// Whether it has keyboard focus.
    pub focused: bool,
    /// Its title.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub title: Option<String>,
    /// Its inner-size limits.
    #[cfg_attr(feature = "serde", serde(default))]
    pub constraints: SizeConstraints,
    /// Whether the user can resize it.
    pub resizable: bool,
    /// What frames it.
    pub frame: WindowFrame,
    /// The display it is on, by label.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub display: Option<u64>,
    /// Its drawable area and scale, once reported.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub metrics: Option<WindowMetrics>,
    /// The theme applied to it.
    pub appearance: Appearance,
    /// The safe-area insets in logical pixels: top, right, bottom, left.
    pub safe_area: [f64; 4],
    /// The soft keyboard's occlusion and its motion; absent while it is hidden
    /// and at rest.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub keyboard: Option<KeyboardInset>,
    /// The edges system gestures take, in logical pixels: top, right, bottom,
    /// left; absent while zero.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "is_zero_insets")
    )]
    pub system_gestures: [f64; 4],
    /// Whether the platform lets it run its UI.
    pub execution: Execution,
    /// Whether the pointer is over it.
    pub hovered: bool,
    /// Whether it is the active window of its desktop.
    pub active: bool,
    /// Whether the user is placing it through the window system, where the
    /// host can tell; absent where it cannot, and in recordings older than
    /// the fact, which a reader takes as not the user.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub user_placing: Option<bool>,
    /// Whether the placing the user does is a resize by the frame; absent
    /// while it is not, and in recordings older than the fact.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub user_resizing: bool,
    /// Whether it holds the pointer; absent while it holds none, and in
    /// recordings older than the fact.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub cursor_grab: Option<CursorGrab>,
    /// Where it stands among the other windows; absent at the normal level,
    /// and in recordings older than the fact.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub level: Option<WindowLevel>,
    /// Which commands it honours.
    pub capabilities: Capabilities,
}

vocabulary! {
    /// How a window holds the pointer, when it holds it.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema), schemars(rename = "NativeCursorGrab"))]
    pub enum CursorGrab {
        /// Inside its content area.
        Confined => "confined",
        /// In place, its motion reported raw.
        Locked => "locked",
    }
}

vocabulary! {
    /// Where a window stands among the others, when not at the normal level.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema), schemars(rename = "NativeWindowLevel"))]
    pub enum WindowLevel {
        /// Above every normal window.
        AlwaysOnTop => "always_on_top",
        /// Below every normal window.
        AlwaysOnBottom => "always_on_bottom",
    }
}

/// Whether all four insets are zero, which a writer leaves out.
#[cfg(feature = "serde")]
fn is_zero_insets(insets: &[f64; 4]) -> bool {
    insets.iter().all(|inset| *inset == 0.0)
}

/// Where the soft keyboard covers a window, and how it gets there.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct KeyboardInset {
    /// The occlusion once any motion ends, in logical pixels: top, right,
    /// bottom, left.
    pub target: [f64; 4],
    /// The motion toward `target`; absent at rest.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub motion: Option<InsetMotion>,
}

/// A motion of the keyboard on the event timeline.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct InsetMotion {
    /// The occlusion it starts from: top, right, bottom, left.
    pub from: [f64; 4],
    /// When it starts on the event timeline, in nanoseconds.
    pub start_ns: u64,
    /// How long it lasts, in nanoseconds.
    pub duration_ns: u64,
    /// Its timing curve.
    pub curve: InsetCurve,
}

/// A keyboard motion's timing curve.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "curve", rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum InsetCurve {
    /// Progress proportional to time.
    Linear,
    /// A cubic Bézier through (0,0), the control points and (1,1).
    CubicBezier {
        /// The first control point's time.
        x1: f64,
        /// The first control point's progress.
        y1: f64,
        /// The second control point's time.
        x2: f64,
        /// The second control point's progress.
        y2: f64,
    },
    /// UIKit's keyboard curve.
    SystemKeyboard,
}

vocabulary! {
    /// What a window's handler answered when the window system asked to close the window.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum CloseAnswer {
        /// The handler granted it: the window closes.
        Close => "close",
        /// The handler vetoed it (the application's close guard held it): the window stays
        /// open.
        KeepOpen => "keep_open",
        /// An earlier request was granted and the window is closing: the handler was not asked.
        Closing => "closing",
        /// The window had let go of its handler: the window closes.
        NoHandler => "no_handler",
        /// The handler panicked while it answered: the panic went on and nothing was decided.
        Panicked => "panicked",
    }
}

vocabulary! {
    /// Why a window closed.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum CloseCause {
        /// Its handler granted a close request.
        GrantedRequest => "granted_request",
        /// Nobody was asked: the application's close command, or the system taking the window
        /// away.
        NoRequest => "no_request",
    }
}

vocabulary! {
    /// Why a writer stopped a recording ([`Row::Cut`]).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum CutReason {
        /// The next row would have taken it past [`MAX_RECORDING_BYTES`].
        Bytes => "bytes",
        /// The next row would have taken it past [`MAX_ROWS`].
        Rows => "rows",
        /// The window's text store journal could not be kept (the process has no owner slot
        /// left for it), so the window's stores are not journaled from here.
        StoreJournal => "store_journal",
    }
}

/// How many calls one [`StoreLock`] row holds; a session that makes more is recorded
/// [`incomplete`](StoreLock::incomplete).
pub const MAX_STORE_STEPS: usize = 256;

/// How many bytes of text one [`StoreLock`] row holds, its opened document and every call's
/// text together; a session that reads or writes more is recorded
/// [`incomplete`](StoreLock::incomplete).
pub const MAX_STORE_TEXT_BYTES: usize = 64 * 1024;

/// How long one line of a native recording may be, in bytes, its newline excluded: a reader
/// refuses a longer line before it parses it. A [`StoreLock`] row within its two bounds fits.
pub const MAX_LINE_BYTES: usize = 1024 * 1024;

/// How many rows one native recording may hold: a reader refuses the row past it.
pub const MAX_ROWS: usize = 1 << 20;

/// How many bytes one native recording may hold, its lines and their endings together: a reader
/// refuses the line that would pass it, as it reads, before it parses or keeps that line. The two
/// bounds above alone admit about a tebibyte held at once; the largest recording a device run has
/// written is a few mebibytes, so this leaves room for long sessions and keeps what one read
/// holds within what a test host and a CI runner have.
pub const MAX_RECORDING_BYTES: usize = 64 * 1024 * 1024;

vocabulary! {
    /// The kind of lock an input method was granted on a text store.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum StoreLockKind {
        /// Reads only.
        Read => "read",
        /// Reads and edits.
        ReadWrite => "read_write",
    }
}

vocabulary! {
    /// Why a text store refused a call, by kind.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum StoreError {
        /// A synchronous lock was unavailable.
        SyncLockUnavailable => "sync_lock_unavailable",
        /// The deferred-lock queue was full.
        DeferredQueueFull => "deferred_queue_full",
        /// An offset or range named no position in the document.
        Offset => "offset",
        /// The text must not be read out.
        Protected => "protected",
        /// The field had no layout for its text.
        NoLayout => "no_layout",
        /// The point missed the text.
        PointOutside => "point_outside",
        /// The field was gone.
        Detached => "detached",
    }
}

vocabulary! {
    /// What a text field is for.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum InputPurpose {
        /// Free text.
        #[default]
        Text => "text",
        /// An email address.
        Email => "email",
        /// A web address.
        Url => "url",
        /// A telephone number.
        Phone => "phone",
        /// A number; [`StoreStatus::signed`] and [`StoreStatus::decimal`] say
        /// what it holds beside the digits.
        Number => "number",
        /// A search query.
        Search => "search",
    }
}

vocabulary! {
    /// What a text field's on-screen Enter key says.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum EnterAction {
        /// The keyboard's own label.
        Default => "default",
        /// Done.
        Done => "done",
        /// Go.
        Go => "go",
        /// Next.
        Next => "next",
        /// Previous.
        Previous => "previous",
        /// Search.
        Search => "search",
        /// Send.
        SendMessage => "send_message",
    }
}

vocabulary! {
    /// Which letters a text field's on-screen keyboard shifts on its own.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum Capitalization {
        /// None.
        #[default]
        None => "none",
        /// The first letter of each sentence.
        Sentences => "sentences",
        /// The first letter of each word.
        Words => "words",
        /// Every letter.
        Characters => "characters",
    }
}

vocabulary! {
    /// Whether a text field's input method suggests and corrects words.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum Correction {
        /// Neither.
        Off => "off",
        /// Suggestions and the spell check, no word replaced on its own.
        Suggestions => "suggestions",
        /// Suggestions, and misspelled words replaced as the user types.
        Autocorrect => "autocorrect",
    }
}

vocabulary! {
    /// What the system may fill a text field with, beyond its purpose.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    #[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
    #[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
    pub enum Autofill {
        /// An account's user name.
        Username => "username",
        /// An account's existing password.
        CurrentPassword => "current_password",
        /// A password being chosen.
        NewPassword => "new_password",
        /// A one-time code.
        OneTimeCode => "one_time_code",
    }
}

/// A text store's standing properties, as the window was told to focus it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct StoreStatus {
    /// Its text must not be read out (a password field).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub protected: bool,
    /// It holds one line.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub single_line: bool,
    /// What it is for.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "crate::skip::is_default")
    )]
    pub purpose: InputPurpose,
    /// A number purpose's minus sign: the number may be negative. Set only with
    /// [`InputPurpose::Number`].
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub signed: bool,
    /// A number purpose's decimal separator: the number may have a fractional
    /// part. Set only with [`InputPurpose::Number`].
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub decimal: bool,
    /// What its Enter key says; absent, the default its purpose picks (Search
    /// for a search, the keyboard's own otherwise).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub enter_action: Option<EnterAction>,
    /// Which letters its keyboard shifts on its own.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "crate::skip::is_default")
    )]
    pub capitalization: Capitalization,
    /// Whether its input method suggests and corrects; absent, the default its
    /// purpose picks (prose corrected, an address or a number not).
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub correction: Option<Correction>,
    /// What the system may fill it with.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub autofill: Option<Autofill>,
}

/// The text store a window's input method was told to serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct StoreFocus {
    /// The store, numbered within the recording each time the window is told to serve a store
    /// it was not serving; never zero, never reused.
    pub store: u64,
    /// Its standing properties.
    pub status: StoreStatus,
}

/// A composition in a text store: its span in UTF-16 code units, and whether the input method
/// hides the caret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct StoreComposition {
    /// Its start and end.
    pub range: [u64; 2],
    /// The input method draws no caret.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub hides_caret: bool,
}

/// A text store's document: its text, its selection as anchor and active end in UTF-16 code
/// units, and its composition.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct StoreDocument {
    /// The text.
    pub text: String,
    /// The selection's anchor and active end.
    pub selection: [u64; 2],
    /// The composition, when one is in progress.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub composition: Option<StoreComposition>,
}

/// One call an input method made inside a lock on a text store, with the store's answer. An
/// offset is in UTF-16 code units; a call the store refused names its error instead of an
/// answer.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "call", rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum StoreStep {
    /// It read the document's length.
    DocumentLen {
        /// The length.
        len: u64,
    },
    /// It read the text of `range`: the context of a reconversion or a surrounding-text query.
    Text {
        /// The range read.
        range: [u64; 2],
        /// The text answered.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        text: Option<String>,
        /// Why the store refused.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        error: Option<StoreError>,
    },
    /// It read the selection.
    Selection {
        /// The anchor and active end answered.
        selection: [u64; 2],
    },
    /// It read the composition.
    Composition {
        /// The composition answered; absent when none was in progress.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        composition: Option<StoreComposition>,
    },
    /// It asked which offset lies at a point, in window-root logical pixels. The answer
    /// depends on the device's layout, so a replay keeps it and does not ask again.
    IndexAtPoint {
        /// The point.
        point: [f64; 2],
        /// A point inside a character's box answers its nearer edge.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "std::ops::Not::not")
        )]
        round_to_nearest_edge: bool,
        /// A point outside every box answers the nearest offset.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "std::ops::Not::not")
        )]
        nearest_when_outside: bool,
        /// The offset answered.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        index: Option<u64>,
        /// Why the store refused.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        error: Option<StoreError>,
    },
    /// It replaced `range` with `text`.
    Replace {
        /// The range replaced.
        range: [u64; 2],
        /// The text put there.
        text: String,
        /// The edit's start, the replaced span's end and the inserted text's end.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        change: Option<[u64; 3]>,
        /// Why the store refused.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        error: Option<StoreError>,
    },
    /// It replaced the selection with `text`.
    InsertAtSelection {
        /// The text put there.
        text: String,
        /// The edit's start, the replaced span's end and the inserted text's end.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        change: Option<[u64; 3]>,
        /// Why the store refused.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        error: Option<StoreError>,
    },
    /// It set the selection.
    SetSelection {
        /// The anchor and active end.
        selection: [u64; 2],
        /// Why the store refused.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        error: Option<StoreError>,
    },
    /// It started, moved or ended the composition.
    SetComposition {
        /// The composition; absent to end it.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        composition: Option<StoreComposition>,
        /// Why the store refused.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        error: Option<StoreError>,
    },
}

/// One lock an input method was granted on a text store, and its calls inside it in order. A
/// geometry read (a range's rect, the document's bounds) places the input method's window and
/// changes nothing in the store, so it is not recorded.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub struct StoreLock {
    /// The store, as its [`StoreFocus`] numbered it.
    pub store: u64,
    /// The lock's kind.
    pub kind: StoreLockKind,
    /// The document as the lock found it, when the recording did not hold it yet: at the
    /// first lock after the store was focused, and at the first after the application changed
    /// it.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub opened: Option<StoreDocument>,
    /// The calls, at most [`MAX_STORE_STEPS`].
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Vec::is_empty")
    )]
    pub steps: Vec<StoreStep>,
    /// Part of the session is missing: it made more calls than [`MAX_STORE_STEPS`] or carried
    /// more text than [`MAX_STORE_TEXT_BYTES`], the store would not read its document out (a
    /// protected field), or the input method's code panicked inside the lock. A replay refuses
    /// the row.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub incomplete: bool,
    /// The store was protected (a password field): one such row stands for every lock until a
    /// lock finds it unprotected, and holds nothing derived from the field's content (no text,
    /// range, offset, length or count of keys), so it has no `opened` and no `steps`; a replay
    /// refuses it.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub protected: bool,
}

/// One line of a native recording as a live stream of it carries it: the header first, then
/// the rows, and a `gap` where the stream lost rows, which a replay stops at.
///
/// A header a stream sends names no device: the stream's devices are known once its rows are,
/// so whoever writes the recording fills them in.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[non_exhaustive]
pub enum Line {
    /// The recording's header.
    Header(Box<Header>),
    /// One of its rows.
    Row(Box<Row>),
    /// Rows the stream lost here.
    Gap {
        /// How many.
        rows: u64,
    },
}

/// One registry call, after the header.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "row", rename_all = "snake_case"))]
#[cfg_attr(
    feature = "schemars",
    derive(schemars::JsonSchema),
    schemars(rename = "NativeRow")
)]
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
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Vec::is_empty")
        )]
        coalesced: Vec<Reading>,
        /// Readings predicted after `current`.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Vec::is_empty")
        )]
        predicted: Vec<Reading>,
        /// The reading the batch decodes as.
        current: Reading,
    },
    /// The OS click thresholds changed.
    ClickSettings {
        /// The new thresholds; none restores the registry's default.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        settings: Option<ClickSettings>,
    },
    /// A precision touchpad's viewport reported a status or a content transform.
    Viewport {
        /// What it reported.
        viewport: Viewport,
        /// Where and when.
        #[cfg_attr(feature = "serde", serde(flatten))]
        frame: Frame,
    },
    /// The touchpad's open manipulation ended as cancelled.
    ViewportAbandoned {
        /// Where and when.
        #[cfg_attr(feature = "serde", serde(flatten))]
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
        #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "is_zero"))]
        surface: u64,
        /// When, on the event timeline.
        time_ns: u64,
        /// Why.
        reason: CancelReason,
    },
    /// The contacts with one label ended (a capture loss).
    CancelContact {
        /// The surface.
        #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "is_zero"))]
        surface: u64,
        /// The label.
        contact: u64,
        /// When, on the event timeline.
        time_ns: u64,
        /// Why.
        reason: CancelReason,
    },
    /// The host reported a system notice (an `app` recording).
    Notice {
        /// The notice.
        notice: Notice,
    },
    /// The host asked about a phase of the session ending (an `app` recording).
    SessionEnd {
        /// The phase.
        phase: SessionEndPhase,
    },
    /// The user reopened the application (an `app` recording).
    Reopen,
    /// The window's input method acted (a `window` recording).
    Ime {
        /// What it did.
        ime: Ime,
        /// Its text was replaced before writing (ADR-0215 §5): replay refuses the row.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "std::ops::Not::not")
        )]
        redacted: bool,
    },
    /// The window was told which text store its input method serves, or that it serves none
    /// (a `window` recording).
    StoreFocus {
        /// The store; absent when none.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "Option::is_none")
        )]
        focus: Option<StoreFocus>,
    },
    /// The window's input method was granted a lock on a text store and read or edited it
    /// through it (a `window` recording).
    StoreLock {
        /// The lock and its calls.
        lock: StoreLock,
        /// Its text was replaced before writing (ADR-0215 §5): replay refuses the row.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "std::ops::Not::not")
        )]
        redacted: bool,
    },
    /// The window system asked to close the window, and its handler answered (a `window`
    /// recording).
    CloseRequest {
        /// What the handler answered.
        answer: CloseAnswer,
        /// The request was raised while a method of the handler ran, or off the window's
        /// thread: the window system was told to keep the window at once, and the handler was
        /// asked after the method returned.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "std::ops::Not::not")
        )]
        queued: bool,
    },
    /// The writer stopped here: no row follows, and the recording is whole up to it (any
    /// stream).
    Cut {
        /// Why.
        reason: CutReason,
    },
    /// The window closed: its handler heard `closed` (a `window` recording).
    Closed {
        /// Why.
        cause: CloseCause,
    },
    /// The application read the system preferences (a `platform` recording).
    Preferences {
        /// What the read returned.
        read: PreferenceRead,
    },
    /// The application read the displays (a `platform` recording).
    Displays {
        /// What the read returned.
        read: DisplayRead,
        /// Each display's name and persistent key were replaced before writing (ADR-0215 §5):
        /// a key by a pseudonym numbered within the recording, so equal keys stay equal.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "std::ops::Not::not")
        )]
        redacted: bool,
    },
    /// A window published a new snapshot (a `window_state` recording).
    State {
        /// The snapshot.
        state: WindowState,
        /// Its title was replaced before writing (ADR-0215 §5): replay refuses the row.
        #[cfg_attr(
            feature = "serde",
            serde(default, skip_serializing_if = "std::ops::Not::not")
        )]
        redacted: bool,
    },
}

#[cfg(feature = "serde")]
const fn is_zero(value: &u64) -> bool {
    *value == 0
}

#[cfg(feature = "serde")]
const fn is_one(value: &f64) -> bool {
    *value == 1.0
}

#[cfg(feature = "serde")]
const fn one() -> f64 {
    1.0
}
