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
//! [`FORMAT_VERSION`], apart from [`PROTOCOL_VERSION`](crate::version::PROTOCOL_VERSION).
//!
//! With the `serde` feature its types read and write the recording; without it they are plain
//! data, which the input recording's preference rows share.

mod delivery;
mod display;
mod pointer;
mod preferences;
mod text_store;
mod window;

pub use delivery::{Ime, ImeEvent, MemoryPressureLevel, Notice, SessionEndPhase};
pub use display::{
    Absence, Color, Display, DisplayColor, DisplayRead, DynamicRange, Gamut, Headroom,
};
pub use pointer::{
    Action, Angles, CancelReason, Frame, Gesture, GestureComponent, GestureDelta, GesturePhase,
    HoldPhase, Modifier, PointerKind, Reading, Scroll, ScrollInversion, ScrollPhase, ScrollUnit,
    Swipe, Viewport, ViewportStatus, Wheel, WheelAxis,
};
pub use preferences::{
    Failure, FailureKind, Gestures, Motion, MouseGeometry, PreferenceRead, Preferences, Regional,
    ScrollBarVisibility, ScrollTrackClick, TextWeight, TouchGeometry, WheelStep,
};
pub use text_store::{
    Autofill, Capitalization, Correction, EnterAction, InputPurpose, MAX_STORE_STEPS,
    MAX_STORE_TEXT_BYTES, StoreComposition, StoreDocument, StoreError, StoreFocus, StoreLock,
    StoreLockKind, StoreStatus, StoreStep,
};
pub use window::{
    Appearance, Availability, Capabilities, Capability, CloseAnswer, CloseCause, CursorGrab,
    Execution, InsetCurve, InsetMotion, KeyboardInset, SizeConstraints, SystemControls,
    WindowFrame, WindowGeometry, WindowLevel, WindowMetrics, WindowMode, WindowPosition,
    WindowState,
};

/// The native recording format's major version.
pub const FORMAT_VERSION: u32 = 4;

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
