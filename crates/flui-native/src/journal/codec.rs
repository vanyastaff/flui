//! The native recording codec (ADR-0215 §4).
//!
//! A native recording is the journal of one backend's [`ContactRegistry`]:
//! a header with the backend, its clock anchors (epoch, uptime and the
//! performance counter's count and frequency) and the registry's setup, then
//! one row per registry call — a reading, a coalesced batch, a device
//! hot-plug, a capture loss. Rows hold plain [`NativeReading`]s, so a Win32 or
//! Apple recording replays through the shared decoder on any host.
//!
//! The schema is `flui-protocol`'s [`native_recording`](wire), which a reader
//! such as the driver reads without this crate; this codec maps it onto the
//! registry's own types. The framing is the `HostInput` recording's: JSON
//! Lines, one header object and one object per row, a major version in the
//! header, unknown fields skipped and an unknown variant a typed error, never
//! a skipped row.

mod scroll;

use scroll::{scroll_from_wire, scroll_to_wire};

use std::io::{BufRead, Write};
use std::num::{NonZeroU8, NonZeroU64};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_foundation::geometry::{Bounds, DevicePixelRatio, EdgeInsets, Point, Size, WindowMetrics};
use flui_platform::EventTime;
use flui_platform::keyboard::Modifiers;
use flui_platform::pointer::{PenTool, PointerButton, PointerButtons, PointerEvent, PointerKind};
use flui_platform::text_store::TextStore;
use flui_platform::{
    AppTitleBar, Availability, Brightness, CapabilityAbsence, CapabilityParts, CloseDecision,
    ColorGamut, ControlsOffset, CursorGrab, DisplayColor, DisplayId, DisplayInfo, DisplayParts,
    GestureParts, Headroom, HourCycle, ImeEvent, InsetCurve, InsetMotion, InvalidPreference,
    KeyboardInset, Locale, MeasurementSystem, MemoryPressureLevel, MotionPreference,
    NativeMouseGeometry, NativeTouchGeometry, NumberingSystem, PlatformInput, PreferenceParts,
    RefreshPeriod, RegionOverride, RegionalParts, SessionEndPhase, SizeConstraints, SystemControls,
    SystemNotice, SystemPreferences, TextWeightPreference, Weekday, WheelParts, WheelStep,
    WindowCapability, WindowExecutionState, WindowExtent, WindowFrame, WindowGeometry,
    WindowInsets, WindowInteraction, WindowLevel, WindowMode, WindowState, WindowStateParts,
};
use flui_protocol::native_recording as wire;
use serde::Serialize;

use crate::input::assembly::{
    ClickSettings, ContactRegistry, ContactScroll, ContentTransform, CounterAnchor, DecodeError,
    GestureComponent, GestureDelta, GesturePhase, GestureReading, MoveBatch, NativeCancel,
    NativeClock, NativeContact, NativeDevice, NativeReading, NativeSurface, NativeTime, PenAngles,
    QpcTicks, RawPoint, RawSensors, ReadingAction, ReadingTime, TouchpadFrame, ViewportReading,
    ViewportStatus, WheelAxis, WheelReading, decode,
};
use crate::input::windows::direct_manipulation::manipulation::TouchpadAssembler;

pub use wire::{Backend, CloseAnswer, CloseCause, CutReason, FORMAT_VERSION, FailureKind, Stream};

use super::order::{CutBudget, RowOrder, read_bounded_line};
use super::store::{self, RecordedStore, StoreDivergence, StoreFocus, StoreLock};
use crate::error::PlatformError;

/// Why a native recording could not be read or written.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CodecError {
    /// Reading or writing the stream failed.
    #[error("native recording I/O failed: {0}")]
    Io(#[from] std::io::Error),
    /// Line `line` is longer than [`wire::MAX_LINE_BYTES`]: refused before it
    /// is parsed.
    #[error("line {line} of the native recording is longer than {limit} bytes")]
    LineTooLong {
        /// The 1-based line, the header being line 1.
        line: u64,
        /// The bound.
        limit: usize,
    },
    /// Line `line` takes the recording past the [`wire::MAX_RECORDING_BYTES`]
    /// it may hold: refused before it is parsed.
    #[error("line {line} takes the native recording past {limit} bytes")]
    TooLarge {
        /// The 1-based line.
        line: u64,
        /// The bound.
        limit: usize,
    },
    /// Line `line` is past the [`wire::MAX_ROWS`] rows a recording holds.
    #[error("line {line} is past the {limit} rows a native recording holds")]
    TooManyRows {
        /// The 1-based line.
        line: u64,
        /// The bound.
        limit: usize,
    },
    /// Line `line` is not a well-formed object.
    #[error("line {line} of the native recording is malformed: {source}")]
    Json {
        /// The 1-based line, the header being line 1.
        line: u64,
        /// What the parser refused.
        source: MalformedJson,
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
    /// the reading vocabulary grew and the schema has to follow.
    #[error("line {line} holds a {what} variant this native format does not mirror")]
    Unmirrored {
        /// The 1-based line the value was to be written to.
        line: u64,
        /// The type that grew.
        what: &'static str,
    },
}

/// What the JSON parser or writer refused on a line of a native recording.
/// Its message and [`source`](std::error::Error::source) are the parser's; the
/// parser's own type stays out of this crate's API.
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct MalformedJson(serde_json::Error);

/// What a recording journals and, for a registry's, the clock and setup it
/// started with.
#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    /// The backend.
    pub backend: Backend,
    /// What the recording journals.
    pub stream: Stream,
    /// The epoch instant; a registry's alone, zero for another stream.
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
    /// The recording joined a live stream late: a row may name a text store
    /// whose focus or document it never saw, which its replay refuses.
    pub joined_late: bool,
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
            stream: Stream::Registry,
            epoch,
            uptime,
            counter,
            reserved_contact: None,
            first_pointer: None,
            first_device: None,
            click_settings: None,
            provenance: None,
            joined_late: false,
        }
    }

    /// A header for `backend`'s `stream` other than a registry's, which has
    /// no clock of its own.
    pub fn for_stream(backend: Backend, stream: Stream) -> Self {
        Self {
            stream,
            ..Self::new(backend, EventTime::from_nanos(0), Duration::ZERO, None)
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
#[expect(
    clippy::exhaustive_enums,
    reason = "replay reproduces each row or refuses it, never skipping one"
)]
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
    /// An open contact scrolled: a wheel step or a trackpad pan from where
    /// its cursor rests, with its phase already resolved.
    Scroll(ContactScroll),
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
    /// The host reported a system notice (an `app` stream).
    Notice(SystemNotice),
    /// The host asked about a phase of the session ending (an `app` stream).
    SessionEnd(SessionEndPhase),
    /// The user reopened the application (an `app` stream).
    Reopen,
    /// The window's input method delivered an event (a `window` stream).
    Ime {
        /// The event.
        event: ImeEvent,
        /// Its text was replaced before writing (ADR-0215 §5): replay
        /// refuses the row.
        redacted: bool,
    },
    /// The application read the displays (a `platform` stream).
    Displays {
        /// What the read returned.
        read: DisplayRead,
        /// Each display's name and persistent key were replaced before
        /// writing (ADR-0215 §5): a key by a pseudonym numbered within the
        /// recording, so equal keys stay equal and the row still replays.
        redacted: bool,
    },
    /// The application read the system preferences (a `platform` stream).
    Preferences(PreferenceRead),
    /// A window published a new snapshot (a `window_state` stream).
    State {
        /// The snapshot.
        state: RecordedWindowState,
        /// Its title was replaced before writing (ADR-0215 §5): replay
        /// refuses the row.
        redacted: bool,
    },
    /// The window was told which text store its input method serves, or
    /// that it serves none (a `window` stream).
    StoreFocus(Option<StoreFocus>),
    /// The window's input method held a lock on a text store and made these
    /// calls in it (a `window` stream).
    StoreLock {
        /// The lock.
        lock: StoreLock,
        /// Its text was replaced before writing (ADR-0215 §5): replay
        /// refuses the row.
        redacted: bool,
    },
    /// The window system asked to close the window, and its handler
    /// answered (a `window` stream).
    CloseRequest {
        /// What the handler answered.
        answer: CloseAnswer,
        /// The request waited behind a running method of the handler, or came
        /// from another thread: the window system was told to keep the window.
        queued: bool,
    },
    /// The writer stopped the recording here; no row follows (any stream).
    Cut(CutReason),
    /// The window closed (a `window` stream).
    Closed(CloseCause),
}

/// What one read of the system preferences returned.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum PreferenceRead {
    /// The preferences the system reported.
    Observed(SystemPreferences),
    /// The read failed; a failure is not an observation.
    Failed(ReadFailure),
}

/// What one read of the displays returned.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum DisplayRead {
    /// The displays the system reported, at the platform's snapshot revision.
    Observed {
        /// The snapshot's revision, as the platform counted it.
        revision: u64,
        /// The displays, in the snapshot's order.
        displays: Vec<RecordedDisplay>,
    },
    /// The read failed.
    Failed(ReadFailure),
}

/// One display of a read: its fields, and the label its id is written as.
///
/// The label is the number the recording platform's registry issued; it names
/// the display within the recording only, so a replay issues an id of its own
/// for each label ([`DisplayParts::into_info`]).
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedDisplay {
    /// The display within this recording.
    pub label: u64,
    /// Its fields.
    pub parts: DisplayParts,
}

impl RecordedDisplay {
    /// The display `info`, labelled with its id's number.
    pub fn of(info: DisplayInfo) -> Self {
        let (id, parts) = info.into_parts();
        Self {
            label: id.get(),
            parts,
        }
    }
}

/// One snapshot a window published: its revision, its facts, and the label of
/// the display it is on.
///
/// The label is the number the recording platform's registry issued the
/// display, as a `platform` recording of the same session writes it; a replay
/// issues an id of its own for each label, so the facts are handed out with
/// the display the replay chose ([`Self::into_parts`]).
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedWindowState {
    revision: u64,
    display: Option<u64>,
    /// The facts, with no display: `display` names it.
    /// Boxed: a snapshot outweighs every other row several times over.
    parts: Box<WindowStateParts>,
}

impl RecordedWindowState {
    /// The snapshot `state`, its display labelled with its id's number.
    pub fn of(state: WindowState) -> Self {
        let revision = state.revision.get();
        let mut parts = state.into_parts();
        let display = parts.display.take().map(DisplayId::get);
        Self {
            revision,
            display,
            parts: Box::new(parts),
        }
    }

    /// The revision the window published it at.
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// The label of the display the window was on, if the platform could tell.
    pub const fn display(&self) -> Option<u64> {
        self.display
    }

    /// The window's title.
    pub fn title(&self) -> Option<&str> {
        self.parts.title.as_deref()
    }

    /// The facts, on `display`: the id the replay issued for
    /// [`Self::display`]'s label.
    pub fn into_parts(self, display: Option<DisplayId>) -> WindowStateParts {
        WindowStateParts {
            display,
            ..*self.parts
        }
    }

    /// The snapshot with its title replaced by `title`.
    #[cfg(all(debug_assertions, feature = "native-capture"))]
    pub(crate) fn retitled(mut self, title: impl FnOnce(String) -> String) -> Self {
        self.parts.title = self.parts.title.map(title);
        self
    }
}

/// Why a read of the system failed: which [`PlatformError`] it returned, and
/// its message when it has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadFailure {
    /// Which error.
    pub kind: FailureKind,
    /// What failed, in the host's words.
    pub message: Option<String>,
}

impl ReadFailure {
    /// The failure `error` describes. An error with no kind of its own is
    /// [`FailureKind::Other`], with its text as the message.
    pub fn of(error: &PlatformError) -> Self {
        let (kind, message) = match error {
            PlatformError::PreferencesDeferred => (FailureKind::Deferred, None),
            PlatformError::Preferences { message } => {
                (FailureKind::Preferences, Some(message.clone()))
            }
            PlatformError::Displays { message } => (FailureKind::Displays, Some(message.clone())),
            PlatformError::Unsupported(unsupported) => {
                (FailureKind::Unsupported, Some(unsupported.to_string()))
            }
            PlatformError::EventLoop { message } => (FailureKind::EventLoop, Some(message.clone())),
            other => (FailureKind::Other, Some(other.to_string())),
        };
        Self { kind, message }
    }
}

impl Header {
    /// This header as the wire holds it, naming no device: a stream's devices are known once
    /// its rows are, so whoever writes the stream fills them in.
    ///
    /// # Errors
    ///
    /// [`CodecError::Unmirrored`] for a value the format does not mirror.
    pub fn to_wire(&self) -> Result<wire::Header, CodecError> {
        header_to_wire(self, Vec::new()).map_err(|error| error.at(1))
    }
}

impl Row {
    /// This row as the wire holds it.
    ///
    /// # Errors
    ///
    /// [`CodecError::Unmirrored`] for a value the format does not mirror.
    pub fn to_wire(&self) -> Result<wire::Row, CodecError> {
        row_to_wire(self).map_err(|error| error.at(0))
    }

    /// This row as the wire holds it, a text store lock's text moved rather
    /// than copied.
    #[cfg(all(debug_assertions, feature = "native-capture"))]
    pub(super) fn into_wire(self) -> Result<wire::Row, CodecError> {
        match self {
            Self::StoreLock { lock, redacted } => Ok(wire::Row::StoreLock {
                lock: store::lock_to_wire(lock),
                redacted,
            }),
            row => row.to_wire(),
        }
    }

    /// The row's name in the schema, as a replay error names it.
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Reading(_) => "reading",
            Self::Batch { .. } => "batch",
            Self::ClickSettings(_) => "click_settings",
            Self::Viewport { .. } => "viewport",
            Self::ViewportAbandoned { .. } => "viewport_abandoned",
            Self::Wheel(_) => "wheel",
            Self::Scroll(_) => "scroll",
            Self::AddDevice { .. } => "add_device",
            Self::RemoveDevice { .. } => "remove_device",
            Self::CancelSurface { .. } => "cancel_surface",
            Self::CancelContact { .. } => "cancel_contact",
            Self::Notice(_) => "notice",
            Self::SessionEnd(_) => "session_end",
            Self::Reopen => "reopen",
            Self::Ime { .. } => "ime",
            Self::Preferences(_) => "preferences",
            Self::Displays { .. } => "displays",
            Self::State { .. } => "state",
            Self::StoreFocus(_) => "store_focus",
            Self::StoreLock { .. } => "store_lock",
            Self::CloseRequest { .. } => "close_request",
            Self::Closed(_) => "closed",
            Self::Cut(_) => "cut",
        }
    }
}

/// Why a recording could not be replayed: a row the target does not take,
/// one whose text was redacted or that holds only part of what happened, or
/// one the target reproduced otherwise than recorded. Replay stops there,
/// never skipping a row.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ReplayError {
    /// Line `line` is a row of a kind this replay does not reproduce.
    #[error("line {line} is a `{row}` row, which this replay does not reproduce")]
    Unsupported {
        /// The 1-based line, the header being line 1.
        line: u64,
        /// The row's name in the schema.
        row: &'static str,
    },
    /// Line `line` lost its text to redaction: replaying other text would
    /// claim to reproduce what the recording does not hold.
    #[error("line {line} is a redacted `{row}` row, which cannot be replayed")]
    Redacted {
        /// The 1-based line.
        line: u64,
        /// The row's name in the schema.
        row: &'static str,
    },
    /// Line `line` holds only part of what happened (a text store lock past
    /// the journal's bounds, a handler that panicked while it answered).
    #[error("line {line} is an incomplete `{row}` row, which cannot be replayed")]
    Incomplete {
        /// The 1-based line.
        line: u64,
        /// The row's name in the schema.
        row: &'static str,
    },
    /// Line `line` is a text store lock the recording cannot set its store up
    /// for: the recording began after the store was focused, or after the lock
    /// that found the store's document (a live stream joined late).
    #[error("line {line} is a `{row}` row on a store the recording joined after")]
    JoinedLate {
        /// The 1-based line.
        line: u64,
        /// The row's name in the schema.
        row: &'static str,
    },
    /// The writer cut the recording at line `line`: what it holds is not all
    /// the source journaled (`NativeRecording::replay`, which returns only
    /// input, says so this way).
    #[error("the native recording was cut at line {line} ({reason:?})")]
    Cut {
        /// The 1-based line of the `cut` row.
        line: u64,
        /// Why the writer cut it.
        reason: CutReason,
    },
    /// Line `line` was reproduced otherwise than recorded.
    #[error("line {line} (`{row}`) replayed otherwise than recorded: {divergence}")]
    Diverged {
        /// The 1-based line.
        line: u64,
        /// The row's name in the schema.
        row: &'static str,
        /// How.
        divergence: Divergence,
    },
}

/// How a replayed row departed from its recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Divergence {
    /// The replaying window's handler decided a close request otherwise.
    #[error("the close request was answered {replayed:?}, recorded {recorded:?}")]
    CloseDecision {
        /// What the recorded handler decided.
        recorded: CloseDecision,
        /// What the replaying handler decided.
        replayed: CloseDecision,
    },
    /// A text store lock replayed to another answer.
    #[error(transparent)]
    Store(#[from] StoreDivergence),
}

/// A target's refusal of a kind of row it does not reproduce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unsupported;

/// What a recording replays into. A registry's rows reach it as the input
/// the registry assembles; each other row as itself. A target takes the rows
/// it reproduces and refuses the rest, which is the default.
pub trait ReplayTarget {
    /// Input a registry's rows assembled.
    fn input(&mut self, input: PlatformInput) -> Result<(), Unsupported> {
        let _ = input;
        Err(Unsupported)
    }

    /// A system notice.
    fn notice(&mut self, notice: SystemNotice) -> Result<(), Unsupported> {
        let _ = notice;
        Err(Unsupported)
    }

    /// A phase of the session ending.
    fn session_end(&mut self, phase: SessionEndPhase) -> Result<(), Unsupported> {
        let _ = phase;
        Err(Unsupported)
    }

    /// A reopen.
    fn reopen(&mut self) -> Result<(), Unsupported> {
        Err(Unsupported)
    }

    /// An input method's event.
    fn ime(&mut self, event: &ImeEvent) -> Result<(), Unsupported> {
        let _ = event;
        Err(Unsupported)
    }

    /// What a read of the system preferences returned: the target answers
    /// the application's next read with it.
    fn preferences(&mut self, read: &PreferenceRead) -> Result<(), Unsupported> {
        let _ = read;
        Err(Unsupported)
    }

    /// What a read of the displays returned: the target answers the
    /// application's next read with it.
    fn displays(&mut self, read: &DisplayRead) -> Result<(), Unsupported> {
        let _ = read;
        Err(Unsupported)
    }

    /// A snapshot a window published: the target publishes it for its
    /// window, through the window's successor rules.
    fn window_state(&mut self, state: &RecordedWindowState) -> Result<(), Unsupported> {
        let _ = state;
        Err(Unsupported)
    }

    /// The window was told to serve `focus`'s store, or none: the target
    /// makes the store it replays that store's locks on, starting with the
    /// recorded status. A store it was told about earlier stays: a lock
    /// deferred past a focus change still runs on it.
    fn store_focus(&mut self, focus: Option<&StoreFocus>) -> Result<(), Unsupported> {
        let _ = focus;
        Err(Unsupported)
    }

    /// The store the target replays `store`'s locks on, which an earlier
    /// `store_focus` row named; `None` refuses the row.
    fn text_store(&mut self, store: RecordedStore) -> Option<Rc<dyn TextStore>> {
        let _ = store;
        None
    }

    /// The window system asks to close the window: the target asks its
    /// window's handler and returns what was decided.
    fn close_request(&mut self) -> Result<CloseDecision, Unsupported> {
        Err(Unsupported)
    }

    /// The window closed: the target closes its window unless it already
    /// has (after a close request its handler granted), without asking its
    /// handler.
    fn closed(&mut self, cause: CloseCause) -> Result<(), Unsupported> {
        let _ = cause;
        Err(Unsupported)
    }
}

/// The target [`NativeRecording::replay`] collects a registry's input into.
struct Collect(Vec<PlatformInput>);

impl ReplayTarget for Collect {
    fn input(&mut self, input: PlatformInput) -> Result<(), Unsupported> {
        self.0.push(input);
        Ok(())
    }
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
    /// Read a whole recording. A recording is untrusted input: a line longer
    /// than [`wire::MAX_LINE_BYTES`] is refused before it is parsed, a row
    /// past [`wire::MAX_ROWS`] or a line that takes the recording past
    /// [`wire::MAX_RECORDING_BYTES`] is refused, and each row's values are checked
    /// against their ranges before the row is built.
    ///
    /// # Errors
    ///
    /// [`CodecError`] for the first line that cannot be read.
    pub fn read(mut input: impl BufRead) -> Result<Self, CodecError> {
        let mut buffer = String::new();
        let mut total = read_bounded_line(&mut input, &mut buffer, 1)?;
        let value: serde_json::Value =
            serde_json::from_str(&buffer).map_err(|source| CodecError::Json {
                line: 1,
                source: MalformedJson(source),
            })?;
        let found = value
            .get(wire::VERSION_KEY)
            .and_then(serde_json::Value::as_u64)
            .and_then(|version| u32::try_from(version).ok())
            .unwrap_or(0);
        if found != FORMAT_VERSION {
            return Err(CodecError::Version {
                found,
                supported: FORMAT_VERSION,
            });
        }
        let dto: wire::Header =
            serde_json::from_value(value).map_err(|source| json_error(1, source))?;
        let declared = dto.devices.clone();
        let header = header_from_wire(dto).map_err(|error| error.at(1))?;
        let mut rows = Vec::new();
        let mut order = RowOrder::new(header.joined_late);
        let mut line: u64 = 1;
        loop {
            line += 1;
            let read = read_bounded_line(&mut input, &mut buffer, line)?;
            if read == 0 {
                break;
            }
            total = total.saturating_add(read);
            if total > wire::MAX_RECORDING_BYTES {
                return Err(CodecError::TooLarge {
                    line,
                    limit: wire::MAX_RECORDING_BYTES,
                });
            }
            if usize::try_from(line - 1).map_or(true, |rows| rows > wire::MAX_ROWS) {
                return Err(CodecError::TooManyRows {
                    line,
                    limit: wire::MAX_ROWS,
                });
            }
            if buffer.trim().is_empty() {
                continue;
            }
            let dto: wire::Row =
                serde_json::from_str(&buffer).map_err(|source| json_error(line, source))?;
            let row = row_from_wire(dto).map_err(|error| error.at(line))?;
            order
                .admit(&row)
                .map_err(|what| CodecError::Invalid { line, what })?;
            for (device, kind) in row_devices(&row) {
                let kind = kind_to_wire(kind).map_err(|error| error.at(line))?;
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

    /// Write the recording, declaring in the header every device its written
    /// rows read with the kinds they read it as. A recording past
    /// [`wire::MAX_ROWS`] rows or [`wire::MAX_RECORDING_BYTES`] bytes is
    /// written up to the last row that fits with room for a `cut` row after
    /// it, which ends it: the writer never writes what its reader refuses, and
    /// one that fits whole is written whole.
    ///
    /// # Errors
    ///
    /// [`CodecError::Unmirrored`] for a value this format does not mirror,
    /// and [`CodecError::Io`] when the sink fails.
    pub fn write(&self, mut out: impl Write) -> Result<(), CodecError> {
        // The header that declares every row's devices is the longest it can
        // be: what fits after it fits after the one that declares fewer.
        let longest = encode_line(1, &self.header_declaring(&self.rows)?)?.len();
        let mut budget = CutBudget::after_header(longest);
        // Each row is encoded only once the rows before it fit, so nothing
        // past the cut is encoded, and no value there can fail the write.
        let mut lines: Vec<Vec<u8>> = Vec::new();
        let mut cut = None;
        for (index, row) in self.rows.iter().enumerate() {
            let number = index as u64 + 2;
            let wire = row_to_wire(row).map_err(|error| error.at(number))?;
            let line = encode_line(number, &wire)?;
            let follows = index + 1 < self.rows.len();
            if let Err(reason) = budget.admit(line.len(), follows) {
                cut = Some(reason);
                break;
            }
            lines.push(line);
            if matches!(row, Row::Cut(_)) {
                // The recorder cut it already: nothing after it is written.
                break;
            }
        }
        let kept = lines.len();
        let header = self.header_declaring(&self.rows[..kept])?;
        out.write_all(&encode_line(1, &header)?)?;
        for line in &lines[..kept] {
            out.write_all(line)?;
        }
        if let Some(reason) = cut {
            let number = kept as u64 + 2;
            out.write_all(&encode_line(number, &wire::Row::Cut { reason })?)?;
        }
        out.flush()?;
        Ok(())
    }

    /// The header as the wire holds it, declaring the devices `rows` read.
    fn header_declaring(&self, rows: &[Row]) -> Result<wire::Header, CodecError> {
        let mut devices: Vec<wire::Device> = Vec::new();
        for (index, row) in rows.iter().enumerate() {
            let line = index as u64 + 2;
            for (device, kind) in row_devices(row) {
                let kind = kind_to_wire(kind).map_err(|error| error.at(line))?;
                match devices.iter_mut().find(|entry| entry.device == device.0) {
                    Some(entry) if entry.kinds.contains(&kind) => {}
                    Some(entry) => entry.kinds.push(kind),
                    None => devices.push(wire::Device {
                        device: device.0,
                        kinds: vec![kind],
                    }),
                }
            }
        }
        header_to_wire(&self.header, devices).map_err(|error| error.at(1))
    }

    /// The input the backend's registry assembles from these rows, as the
    /// backends deliver it: a reading that fails delivers its cancel.
    ///
    /// # Errors
    ///
    /// [`ReplayError::Unsupported`] for the first row that is not a
    /// registry's, and [`ReplayError::Cut`] for a recording its writer cut:
    /// what it holds is not all the registry assembled (`replay_into` replays
    /// it up to the cut and says so).
    pub fn replay(&self) -> Result<Vec<PlatformInput>, ReplayError> {
        let mut collect = Collect(Vec::new());
        match self.replay_into(&mut collect)? {
            Replayed::Whole => Ok(collect.0),
            Replayed::Cut { line, reason } => Err(ReplayError::Cut { line, reason }),
        }
    }

    /// Replay every row into `target`, in order: a registry's rows as the
    /// input the registry assembles from them, each other row as itself.
    /// Returns whether the recording was whole, or cut by its writer, and
    /// where: every row before the cut was replayed.
    ///
    /// # Errors
    ///
    /// [`ReplayError`] for the first row `target` refuses or whose text was
    /// redacted; the rows after it are not replayed.
    pub fn replay_into(&self, target: &mut dyn ReplayTarget) -> Result<Replayed, ReplayError> {
        let mut registry = self.header.registry();
        let mut batch = MoveBatch::new();
        let mut touchpad = TouchpadAssembler::new(NativeSurface(0));
        let pointer = |event| PlatformInput::Pointer(event);
        // Each store the replay saw focused, and whether a lock gave it a
        // document yet.
        let mut stores = std::collections::BTreeMap::<RecordedStore, bool>::new();
        for (index, row) in self.rows.iter().enumerate() {
            let line = index as u64 + 2;
            let unsupported = |Unsupported| ReplayError::Unsupported {
                line,
                row: row.name(),
            };
            let mut out: Vec<PlatformInput> = Vec::new();
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
                Row::Scroll(reading) => out.extend(registry.scroll(reading)),
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
                Row::Notice(notice) => target.notice(*notice).map_err(unsupported)?,
                Row::SessionEnd(phase) => target.session_end(*phase).map_err(unsupported)?,
                Row::Reopen => target.reopen().map_err(unsupported)?,
                Row::Ime { redacted: true, .. }
                | Row::State { redacted: true, .. }
                | Row::StoreLock { redacted: true, .. } => {
                    return Err(ReplayError::Redacted {
                        line,
                        row: row.name(),
                    });
                }
                Row::StoreLock { lock, .. } if lock.protected => {
                    return Err(ReplayError::Redacted {
                        line,
                        row: row.name(),
                    });
                }
                Row::StoreLock { lock, .. } if lock.incomplete => {
                    return Err(ReplayError::Incomplete {
                        line,
                        row: row.name(),
                    });
                }
                Row::CloseRequest {
                    answer: CloseAnswer::Panicked,
                    ..
                } => {
                    return Err(ReplayError::Incomplete {
                        line,
                        row: row.name(),
                    });
                }
                Row::Ime { event, .. } => target.ime(event).map_err(unsupported)?,
                Row::Preferences(read) => target.preferences(read).map_err(unsupported)?,
                Row::Displays { read, .. } => target.displays(read).map_err(unsupported)?,
                Row::State { state, .. } => target.window_state(state).map_err(unsupported)?,
                Row::StoreFocus(focus) => {
                    if let Some(focus) = focus {
                        stores.insert(focus.store, false);
                    }
                    target.store_focus(focus.as_ref()).map_err(unsupported)?;
                }
                Row::StoreLock { lock, .. } => {
                    // A recording that began after the store was focused, or
                    // after the lock that found its document, cannot set a
                    // store to what the input method edited.
                    let documented =
                        stores.get_mut(&lock.store).ok_or(ReplayError::JoinedLate {
                            line,
                            row: row.name(),
                        })?;
                    if lock.opened.is_none() && !*documented {
                        return Err(ReplayError::JoinedLate {
                            line,
                            row: row.name(),
                        });
                    }
                    *documented = true;
                    let store = target
                        .text_store(lock.store)
                        .ok_or(ReplayError::Unsupported {
                            line,
                            row: row.name(),
                        })?;
                    lock.replay_on(&*store)
                        .map_err(|divergence| ReplayError::Diverged {
                            line,
                            row: row.name(),
                            divergence: divergence.into(),
                        })?;
                }
                // The handler was not asked: an earlier request was granted,
                // or the window had let go of its handler. Nothing is asked on
                // replay either; the `closed` row closes the window.
                Row::CloseRequest {
                    answer: CloseAnswer::Closing | CloseAnswer::NoHandler,
                    ..
                } => {}
                // The writer stopped here: everything before replayed, and the
                // caller learns the recording is not whole.
                Row::Cut(reason) => {
                    return Ok(Replayed::Cut {
                        line,
                        reason: *reason,
                    });
                }
                Row::CloseRequest { answer, .. } => {
                    let replayed = target.close_request().map_err(unsupported)?;
                    let recorded = match answer {
                        CloseAnswer::KeepOpen => CloseDecision::KeepOpen,
                        _ => CloseDecision::Close,
                    };
                    if replayed != recorded {
                        return Err(ReplayError::Diverged {
                            line,
                            row: row.name(),
                            divergence: Divergence::CloseDecision { recorded, replayed },
                        });
                    }
                }
                Row::Closed(cause) => target.closed(*cause).map_err(unsupported)?,
            }
            for input in out {
                target.input(input).map_err(unsupported)?;
            }
        }
        Ok(Replayed::Whole)
    }
}

/// How much of a recording a replay reproduced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "a cut recording was replayed only up to its cut"]
#[expect(
    clippy::exhaustive_enums,
    reason = "a recording is whole or its writer cut it"
)]
pub enum Replayed {
    /// Every row, the recording being whole.
    Whole,
    /// Every row before line `line`, where the writer cut the recording.
    Cut {
        /// The 1-based line of the `cut` row.
        line: u64,
        /// Why the writer cut it.
        reason: CutReason,
    },
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
        | Row::Scroll(_)
        | Row::CancelSurface { .. }
        | Row::CancelContact { .. }
        | Row::Notice(_)
        | Row::SessionEnd(_)
        | Row::Reopen
        | Row::Ime { .. }
        | Row::Preferences(_)
        | Row::Displays { .. }
        | Row::State { .. }
        | Row::StoreFocus(_)
        | Row::StoreLock { .. }
        | Row::CloseRequest { .. }
        | Row::Closed(_)
        | Row::Cut(_) => Vec::new(),
    };
    readings
        .into_iter()
        .filter_map(|reading| Some((reading.device?, reading.kind)))
        .collect()
}

/// A conversion failure, before its line is known.
#[derive(Debug)]
pub(super) enum DtoError {
    /// The schema has a variant this codec does not read.
    UnknownSpelling(String),
    Invalid(String),
    /// This crate has a variant the schema does not mirror.
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

pub(super) type DtoResult<T> = Result<T, DtoError>;

pub(super) fn invalid(what: impl Into<String>) -> DtoError {
    DtoError::Invalid(what.into())
}

/// A schema variant this codec does not read, by its spelling.
pub(super) fn unread(tag: &str) -> DtoError {
    DtoError::UnknownSpelling(tag.to_owned())
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
    CodecError::Json {
        line,
        source: MalformedJson(source),
    }
}

/// `value` as line `line` of a recording, its newline included.
fn encode_line(line: u64, value: &impl Serialize) -> Result<Vec<u8>, CodecError> {
    let mut bytes = serde_json::to_vec(value).map_err(|source| CodecError::Json {
        line,
        source: MalformedJson(source),
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

const fn nanos(time: EventTime) -> u64 {
    time.as_nanos()
}

fn duration_nanos(duration: Duration) -> DtoResult<u64> {
    u64::try_from(duration.as_nanos()).map_err(|_| invalid("duration past u64 nanoseconds"))
}

fn click_settings_to_wire(settings: ClickSettings) -> DtoResult<wire::ClickSettings> {
    Ok(wire::ClickSettings {
        interval_ns: duration_nanos(settings.interval)?,
        width: settings.area.0,
        height: settings.area.1,
    })
}

fn click_settings_from_wire(settings: wire::ClickSettings) -> ClickSettings {
    ClickSettings {
        interval: Duration::from_nanos(settings.interval_ns),
        area: (settings.width, settings.height),
    }
}

fn header_to_wire(header: &Header, devices: Vec<wire::Device>) -> DtoResult<wire::Header> {
    Ok(wire::Header {
        flui_native_recording: FORMAT_VERSION,
        backend: header.backend,
        stream: header.stream,
        epoch_ns: (header.stream == Stream::Registry).then(|| nanos(header.epoch)),
        uptime_ns: (header.stream == Stream::Registry)
            .then(|| duration_nanos(header.uptime))
            .transpose()?,
        counter: header.counter.map(|anchor| wire::Counter {
            count: anchor.count.0,
            frequency: anchor.frequency.get(),
        }),
        reserved_contact: header.reserved_contact.map(|contact| contact.0),
        first_pointer: header.first_pointer.map(NonZeroU64::get),
        first_device: header.first_device.map(NonZeroU64::get),
        click_settings: header
            .click_settings
            .map(click_settings_to_wire)
            .transpose()?,
        provenance: header.provenance.clone(),
        joined_late: header.joined_late,
        devices,
    })
}

fn header_from_wire(header: wire::Header) -> DtoResult<Header> {
    let nonzero =
        |value: u64, what: &str| NonZeroU64::new(value).ok_or_else(|| invalid(format!("{what} 0")));
    let counter = header
        .counter
        .map(|counter| {
            Ok::<_, DtoError>(CounterAnchor {
                count: QpcTicks(counter.count),
                frequency: nonzero(counter.frequency, "counter frequency")?,
            })
        })
        .transpose()?;
    Ok(Header {
        reserved_contact: header.reserved_contact.map(NativeContact),
        first_pointer: header
            .first_pointer
            .map(|first| nonzero(first, "first pointer"))
            .transpose()?,
        first_device: header
            .first_device
            .map(|first| nonzero(first, "first device"))
            .transpose()?,
        click_settings: header.click_settings.map(click_settings_from_wire),
        provenance: header.provenance,
        joined_late: header.joined_late,
        ..match (header.stream, header.epoch_ns, header.uptime_ns) {
            (Stream::Registry, Some(epoch), Some(uptime)) => Header::new(
                header.backend,
                EventTime::from_nanos(epoch),
                Duration::from_nanos(uptime),
                counter,
            ),
            (Stream::Registry, _, _) => {
                return Err(invalid(
                    "a registry's recording without its epoch and uptime",
                ));
            }
            (Stream::App | Stream::Window | Stream::Platform | Stream::WindowState, None, None)
                if counter.is_none() =>
            {
                Header::for_stream(header.backend, header.stream)
            }
            (Stream::App | Stream::Window | Stream::Platform | Stream::WindowState, _, _) => {
                return Err(invalid("a clock on a recording that is not a registry's"));
            }
            (other, _, _) => return Err(unread(other.name())),
        }
    })
}

fn kind_to_wire(kind: PointerKind) -> DtoResult<wire::PointerKind> {
    Ok(match kind {
        PointerKind::Mouse => wire::PointerKind::Mouse,
        PointerKind::Touch => wire::PointerKind::Touch,
        PointerKind::Pen {
            tool: PenTool::Tip, ..
        } => wire::PointerKind::Pen,
        PointerKind::Pen {
            tool: PenTool::Eraser,
            ..
        } => wire::PointerKind::Eraser,
        PointerKind::Pen { .. } => return Err(DtoError::Unmirrored("PenTool")),
        PointerKind::Trackpad => wire::PointerKind::Trackpad,
        PointerKind::Virtual => wire::PointerKind::Virtual,
        PointerKind::Unknown => wire::PointerKind::Unknown,
        _ => return Err(DtoError::Unmirrored("PointerKind")),
    })
}

fn kind_from_wire(kind: wire::PointerKind) -> DtoResult<PointerKind> {
    Ok(match kind {
        wire::PointerKind::Mouse => PointerKind::Mouse,
        wire::PointerKind::Touch => PointerKind::Touch,
        wire::PointerKind::Pen => PointerKind::Pen { tool: PenTool::Tip },
        wire::PointerKind::Eraser => PointerKind::Pen {
            tool: PenTool::Eraser,
        },
        wire::PointerKind::Trackpad => PointerKind::Trackpad,
        wire::PointerKind::Virtual => PointerKind::Virtual,
        wire::PointerKind::Unknown => PointerKind::Unknown,
        other => return Err(unread(other.name())),
    })
}

fn cancel_to_wire(reason: NativeCancel) -> wire::CancelReason {
    match reason {
        NativeCancel::Platform => wire::CancelReason::Platform,
        NativeCancel::CaptureLost => wire::CancelReason::CaptureLost,
        NativeCancel::FocusLost => wire::CancelReason::FocusLost,
        NativeCancel::DeviceRemoved => wire::CancelReason::DeviceRemoved,
        NativeCancel::InvalidInput => wire::CancelReason::InvalidInput,
    }
}

fn cancel_from_wire(reason: wire::CancelReason) -> DtoResult<NativeCancel> {
    Ok(match reason {
        wire::CancelReason::Platform => NativeCancel::Platform,
        wire::CancelReason::CaptureLost => NativeCancel::CaptureLost,
        wire::CancelReason::FocusLost => NativeCancel::FocusLost,
        wire::CancelReason::DeviceRemoved => NativeCancel::DeviceRemoved,
        wire::CancelReason::InvalidInput => NativeCancel::InvalidInput,
        other => return Err(unread(other.name())),
    })
}

/// Each modifier with the schema's name for it.
const MODIFIERS: [(Modifiers, wire::Modifier); 12] = [
    (Modifiers::ALT, wire::Modifier::Alt),
    (Modifiers::ALT_GRAPH, wire::Modifier::AltGraph),
    (Modifiers::CAPS_LOCK, wire::Modifier::CapsLock),
    (Modifiers::CONTROL, wire::Modifier::Control),
    (Modifiers::FN, wire::Modifier::Fn),
    (Modifiers::FN_LOCK, wire::Modifier::FnLock),
    (Modifiers::META, wire::Modifier::Meta),
    (Modifiers::NUM_LOCK, wire::Modifier::NumLock),
    (Modifiers::SCROLL_LOCK, wire::Modifier::ScrollLock),
    (Modifiers::SHIFT, wire::Modifier::Shift),
    (Modifiers::SYMBOL, wire::Modifier::Symbol),
    (Modifiers::SYMBOL_LOCK, wire::Modifier::SymbolLock),
];

fn modifiers_to_wire(modifiers: Modifiers) -> DtoResult<Vec<wire::Modifier>> {
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
        .map(|(_, name)| *name)
        .collect())
}

fn modifiers_from_wire(names: &[wire::Modifier]) -> DtoResult<Modifiers> {
    names.iter().try_fold(Modifiers::NONE, |held, name| {
        MODIFIERS
            .iter()
            .find(|(_, spelling)| spelling == name)
            .map(|(modifier, _)| held | *modifier)
            .ok_or_else(|| unread(name.name()))
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

fn buttons_to_wire(buttons: PointerButtons) -> Vec<u8> {
    buttons.iter().map(|button| button.number().get()).collect()
}

fn time_to_wire(time: NativeTime) -> wire::Time {
    match time {
        NativeTime::TickMillis { tick, observed_ms } => {
            wire::Time::TickMillis { tick, observed_ms }
        }
        NativeTime::UptimeSeconds(seconds) => wire::Time::UptimeSeconds(seconds),
        NativeTime::Epoch(time) => wire::Time::Epoch(nanos(time)),
    }
}

fn time_from_wire(time: wire::Time) -> DtoResult<NativeTime> {
    Ok(match time {
        wire::Time::TickMillis { tick, observed_ms } => {
            NativeTime::TickMillis { tick, observed_ms }
        }
        wire::Time::UptimeSeconds(seconds) => NativeTime::UptimeSeconds(seconds),
        wire::Time::Epoch(nanos) => NativeTime::Epoch(EventTime::from_nanos(nanos)),
        _ => return Err(unread("Time")),
    })
}

fn gesture_to_wire(gesture: GestureReading) -> wire::Gesture {
    wire::Gesture {
        component: match gesture.component {
            GestureComponent::Scale => wire::GestureComponent::Scale,
            GestureComponent::Rotation => wire::GestureComponent::Rotation,
            GestureComponent::Session => wire::GestureComponent::Session,
        },
        phase: match gesture.phase {
            GesturePhase::Began => wire::GesturePhase::Began,
            GesturePhase::Changed => wire::GesturePhase::Changed,
            GesturePhase::Ended => wire::GesturePhase::Ended,
            GesturePhase::Cancelled => wire::GesturePhase::Cancelled,
        },
        delta: match gesture.delta {
            GestureDelta::Scale(factor) => wire::GestureDelta::Scale(factor),
            GestureDelta::Rotation(radians) => wire::GestureDelta::Rotation(radians),
            GestureDelta::None => wire::GestureDelta::None,
        },
    }
}

fn gesture_from_wire(gesture: wire::Gesture) -> DtoResult<GestureReading> {
    Ok(GestureReading {
        component: match gesture.component {
            wire::GestureComponent::Scale => GestureComponent::Scale,
            wire::GestureComponent::Rotation => GestureComponent::Rotation,
            wire::GestureComponent::Session => GestureComponent::Session,
            other => return Err(unread(other.name())),
        },
        phase: match gesture.phase {
            wire::GesturePhase::Began => GesturePhase::Began,
            wire::GesturePhase::Changed => GesturePhase::Changed,
            wire::GesturePhase::Ended => GesturePhase::Ended,
            wire::GesturePhase::Cancelled => GesturePhase::Cancelled,
            other => return Err(unread(other.name())),
        },
        delta: match gesture.delta {
            wire::GestureDelta::Scale(factor) => GestureDelta::Scale(factor),
            wire::GestureDelta::Rotation(radians) => GestureDelta::Rotation(radians),
            wire::GestureDelta::None => GestureDelta::None,
            _ => return Err(unread("GestureDelta")),
        },
    })
}

fn viewport_to_wire(reading: ViewportReading) -> wire::Viewport {
    match reading {
        ViewportReading::Status(status) => wire::Viewport::Status(match status {
            ViewportStatus::Building => wire::ViewportStatus::Building,
            ViewportStatus::Enabled => wire::ViewportStatus::Enabled,
            ViewportStatus::Disabled => wire::ViewportStatus::Disabled,
            ViewportStatus::Running => wire::ViewportStatus::Running,
            ViewportStatus::Inertia => wire::ViewportStatus::Inertia,
            ViewportStatus::Ready => wire::ViewportStatus::Ready,
            ViewportStatus::Suspended => wire::ViewportStatus::Suspended,
        }),
        ViewportReading::Content(transform) => {
            wire::Viewport::Content([transform.scale(), transform.x(), transform.y()])
        }
    }
}

fn viewport_from_wire(viewport: wire::Viewport) -> DtoResult<ViewportReading> {
    Ok(match viewport {
        wire::Viewport::Status(status) => ViewportReading::Status(match status {
            wire::ViewportStatus::Building => ViewportStatus::Building,
            wire::ViewportStatus::Enabled => ViewportStatus::Enabled,
            wire::ViewportStatus::Disabled => ViewportStatus::Disabled,
            wire::ViewportStatus::Running => ViewportStatus::Running,
            wire::ViewportStatus::Inertia => ViewportStatus::Inertia,
            wire::ViewportStatus::Ready => ViewportStatus::Ready,
            wire::ViewportStatus::Suspended => ViewportStatus::Suspended,
            other => return Err(unread(other.name())),
        }),
        wire::Viewport::Content([scale, x, y]) => ViewportReading::Content(
            ContentTransform::try_new(scale, x, y)
                .ok_or_else(|| invalid("a content transform that is not finite"))?,
        ),
        _ => return Err(unread("Viewport")),
    })
}

fn action_to_wire(action: ReadingAction) -> wire::Action {
    match action {
        ReadingAction::Enter => wire::Action::Enter,
        ReadingAction::Leave => wire::Action::Leave,
        ReadingAction::Move => wire::Action::Move,
        ReadingAction::Press(button) => wire::Action::Press(button.number().get()),
        ReadingAction::Release(button) => wire::Action::Release(button.number().get()),
        ReadingAction::Cancel(reason) => wire::Action::Cancel(cancel_to_wire(reason)),
        ReadingAction::Gesture(gesture) => wire::Action::Gesture(gesture_to_wire(gesture)),
    }
}

fn action_from_wire(action: wire::Action) -> DtoResult<ReadingAction> {
    Ok(match action {
        wire::Action::Enter => ReadingAction::Enter,
        wire::Action::Leave => ReadingAction::Leave,
        wire::Action::Move => ReadingAction::Move,
        wire::Action::Press(number) => ReadingAction::Press(button(number)?),
        wire::Action::Release(number) => ReadingAction::Release(button(number)?),
        wire::Action::Cancel(reason) => ReadingAction::Cancel(cancel_from_wire(reason)?),
        wire::Action::Gesture(gesture) => ReadingAction::Gesture(gesture_from_wire(gesture)?),
        _ => return Err(unread("Action")),
    })
}

fn angles_to_wire(angles: PenAngles) -> wire::Angles {
    match angles {
        PenAngles::TiltDegrees { x, y } => wire::Angles::TiltDegrees { x, y },
        PenAngles::Spherical { altitude, azimuth } => wire::Angles::Spherical { altitude, azimuth },
    }
}

fn angles_from_wire(angles: wire::Angles) -> DtoResult<PenAngles> {
    Ok(match angles {
        wire::Angles::TiltDegrees { x, y } => PenAngles::TiltDegrees { x, y },
        wire::Angles::Spherical { altitude, azimuth } => PenAngles::Spherical { altitude, azimuth },
        _ => return Err(unread("Angles")),
    })
}

fn frame_to_wire(frame: &TouchpadFrame) -> DtoResult<wire::Frame> {
    Ok(wire::Frame {
        time: time_to_wire(frame.time.coarse),
        counter: frame.time.counter.map(|count| count.0),
        position: [frame.position.0, frame.position.1],
        scale: frame.scale,
        modifiers: modifiers_to_wire(frame.modifiers)?,
    })
}

fn frame_from_wire(frame: wire::Frame) -> DtoResult<TouchpadFrame> {
    Ok(TouchpadFrame {
        time: ReadingTime {
            coarse: time_from_wire(frame.time)?,
            counter: frame.counter.map(QpcTicks),
        },
        position: (frame.position[0], frame.position[1]),
        scale: frame.scale,
        modifiers: modifiers_from_wire(&frame.modifiers)?,
    })
}

fn wheel_to_wire(reading: &WheelReading) -> DtoResult<wire::Wheel> {
    Ok(wire::Wheel {
        axis: match reading.axis {
            WheelAxis::Vertical => wire::WheelAxis::Vertical,
            WheelAxis::Horizontal => wire::WheelAxis::Horizontal,
        },
        distance: reading.distance,
        position: [reading.position.0, reading.position.1],
        scale: reading.scale,
        time_ns: nanos(reading.time),
        modifiers: modifiers_to_wire(reading.modifiers)?,
    })
}

fn wheel_from_wire(wheel: wire::Wheel) -> DtoResult<WheelReading> {
    Ok(WheelReading {
        axis: match wheel.axis {
            wire::WheelAxis::Vertical => WheelAxis::Vertical,
            wire::WheelAxis::Horizontal => WheelAxis::Horizontal,
            other => return Err(unread(other.name())),
        },
        distance: wheel.distance,
        position: (wheel.position[0], wheel.position[1]),
        scale: wheel.scale,
        time: EventTime::from_nanos(wheel.time_ns),
        modifiers: modifiers_from_wire(&wheel.modifiers)?,
    })
}

fn reading_to_wire(reading: &NativeReading) -> DtoResult<wire::Reading> {
    let sensors = reading.sensors;
    Ok(wire::Reading {
        surface: reading.surface.0,
        device: reading.device.map(|device| device.0),
        contact: reading.contact.0,
        kind: kind_to_wire(reading.kind)?,
        primary: reading.primary,
        action: action_to_wire(reading.action),
        buttons: reading.buttons.map(buttons_to_wire),
        modifiers: modifiers_to_wire(reading.modifiers)?,
        click_count: reading.click_count.map(NonZeroU8::get),
        time: time_to_wire(reading.time.coarse),
        counter: reading.time.counter.map(|count| count.0),
        position: reading.position.map(|point| [point.x, point.y]),
        scale: reading.scale,
        pressure: sensors.pressure,
        tangential_pressure: sensors.tangential_pressure,
        angles: sensors.angles.map(angles_to_wire),
        twist_degrees: sensors.twist_degrees,
        contact_size: sensors.contact_size.map(|(width, height)| [width, height]),
    })
}

fn reading_from_wire(dto: wire::Reading) -> DtoResult<NativeReading> {
    let time = ReadingTime {
        coarse: time_from_wire(dto.time)?,
        counter: dto.counter.map(QpcTicks),
    };
    let mut reading = NativeReading::new(
        NativeContact(dto.contact),
        kind_from_wire(dto.kind)?,
        action_from_wire(dto.action)?,
        time,
    )
    .on(NativeSurface(dto.surface))
    .with_modifiers(modifiers_from_wire(&dto.modifiers)?)
    .with_sensors(RawSensors {
        pressure: dto.pressure,
        tangential_pressure: dto.tangential_pressure,
        angles: dto.angles.map(angles_from_wire).transpose()?,
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

fn row_to_wire(row: &Row) -> DtoResult<wire::Row> {
    let readings = |readings: &[NativeReading]| {
        readings
            .iter()
            .map(reading_to_wire)
            .collect::<DtoResult<Vec<_>>>()
    };
    Ok(match row {
        Row::Reading(reading) => wire::Row::Reading {
            reading: reading_to_wire(reading)?,
        },
        Row::Batch {
            coalesced,
            predicted,
            current,
        } => wire::Row::Batch {
            coalesced: readings(coalesced)?,
            predicted: readings(predicted)?,
            current: reading_to_wire(current)?,
        },
        Row::ClickSettings(settings) => wire::Row::ClickSettings {
            settings: settings.map(click_settings_to_wire).transpose()?,
        },
        Row::Viewport { reading, frame } => wire::Row::Viewport {
            viewport: viewport_to_wire(*reading),
            frame: frame_to_wire(frame)?,
        },
        Row::ViewportAbandoned { frame } => wire::Row::ViewportAbandoned {
            frame: frame_to_wire(frame)?,
        },
        Row::Wheel(reading) => wire::Row::Wheel {
            wheel: wheel_to_wire(reading)?,
        },
        Row::Scroll(reading) => wire::Row::Scroll {
            scroll: scroll_to_wire(reading)?,
        },
        Row::AddDevice { device, kind, time } => wire::Row::AddDevice {
            device: device.0,
            kind: kind_to_wire(*kind)?,
            time_ns: nanos(*time),
        },
        Row::RemoveDevice { device, time } => wire::Row::RemoveDevice {
            device: device.0,
            time_ns: nanos(*time),
        },
        Row::CancelSurface {
            surface,
            time,
            reason,
        } => wire::Row::CancelSurface {
            surface: surface.0,
            time_ns: nanos(*time),
            reason: cancel_to_wire(*reason),
        },
        Row::CancelContact {
            surface,
            contact,
            time,
            reason,
        } => wire::Row::CancelContact {
            surface: surface.0,
            contact: contact.0,
            time_ns: nanos(*time),
            reason: cancel_to_wire(*reason),
        },
        Row::Notice(notice) => wire::Row::Notice {
            notice: notice_to_wire(*notice)?,
        },
        Row::SessionEnd(phase) => wire::Row::SessionEnd {
            phase: phase_to_wire(*phase)?,
        },
        Row::Reopen => wire::Row::Reopen,
        Row::Ime { event, redacted } => wire::Row::Ime {
            ime: wire::Ime::Event(ime_to_wire(event)?),
            redacted: *redacted,
        },
        Row::Preferences(read) => wire::Row::Preferences {
            read: read_to_wire(read)?,
        },
        Row::Displays { read, redacted } => wire::Row::Displays {
            read: display_read_to_wire(read)?,
            redacted: *redacted,
        },
        Row::State { state, redacted } => wire::Row::State {
            state: window_state_to_wire(state)?,
            redacted: *redacted,
        },
        Row::StoreFocus(focus) => wire::Row::StoreFocus {
            focus: focus.as_ref().map(store::focus_to_wire).transpose()?,
        },
        Row::StoreLock { lock, redacted } => wire::Row::StoreLock {
            lock: store::lock_to_wire(lock.clone()),
            redacted: *redacted,
        },
        Row::CloseRequest { answer, queued } => wire::Row::CloseRequest {
            answer: *answer,
            queued: *queued,
        },
        Row::Closed(cause) => wire::Row::Closed { cause: *cause },
        Row::Cut(reason) => wire::Row::Cut { reason: *reason },
    })
}

fn notice_to_wire(notice: SystemNotice) -> DtoResult<wire::Notice> {
    Ok(match notice {
        SystemNotice::Sleep => wire::Notice::Sleep,
        SystemNotice::Wake => wire::Notice::Wake,
        SystemNotice::SessionLocked => wire::Notice::SessionLocked,
        SystemNotice::SessionUnlocked => wire::Notice::SessionUnlocked,
        SystemNotice::MemoryPressure(level) => wire::Notice::MemoryPressure(match level {
            MemoryPressureLevel::Moderate => wire::MemoryPressureLevel::Moderate,
            MemoryPressureLevel::Critical => wire::MemoryPressureLevel::Critical,
            _ => return Err(DtoError::Unmirrored("MemoryPressureLevel")),
        }),
        _ => return Err(DtoError::Unmirrored("SystemNotice")),
    })
}

fn notice_from_wire(notice: wire::Notice) -> DtoResult<SystemNotice> {
    Ok(match notice {
        wire::Notice::Sleep => SystemNotice::Sleep,
        wire::Notice::Wake => SystemNotice::Wake,
        wire::Notice::SessionLocked => SystemNotice::SessionLocked,
        wire::Notice::SessionUnlocked => SystemNotice::SessionUnlocked,
        wire::Notice::MemoryPressure(level) => SystemNotice::MemoryPressure(match level {
            wire::MemoryPressureLevel::Moderate => MemoryPressureLevel::Moderate,
            wire::MemoryPressureLevel::Critical => MemoryPressureLevel::Critical,
            other => return Err(unread(other.name())),
        }),
        _ => return Err(unread("Notice")),
    })
}

fn phase_to_wire(phase: SessionEndPhase) -> DtoResult<wire::SessionEndPhase> {
    Ok(match phase {
        SessionEndPhase::Query => wire::SessionEndPhase::Query,
        SessionEndPhase::Cancelled => wire::SessionEndPhase::Cancelled,
        SessionEndPhase::Ending => wire::SessionEndPhase::Ending,
        _ => return Err(DtoError::Unmirrored("SessionEndPhase")),
    })
}

fn phase_from_wire(phase: wire::SessionEndPhase) -> DtoResult<SessionEndPhase> {
    Ok(match phase {
        wire::SessionEndPhase::Query => SessionEndPhase::Query,
        wire::SessionEndPhase::Cancelled => SessionEndPhase::Cancelled,
        wire::SessionEndPhase::Ending => SessionEndPhase::Ending,
        other => return Err(unread(other.name())),
    })
}

fn ime_to_wire(event: &ImeEvent) -> DtoResult<wire::ImeEvent> {
    Ok(match event {
        ImeEvent::Enabled => wire::ImeEvent::Enabled,
        ImeEvent::Preedit { text, cursor } => wire::ImeEvent::Preedit {
            text: text.clone(),
            cursor: cursor.map(|(start, end)| [start, end]),
        },
        ImeEvent::Commit(text) => wire::ImeEvent::Commit(text.clone()),
        ImeEvent::Disabled => wire::ImeEvent::Disabled,
        _ => return Err(DtoError::Unmirrored("ImeEvent")),
    })
}

fn ime_from_wire(ime: wire::Ime) -> DtoResult<ImeEvent> {
    let wire::Ime::Event(event) = ime else {
        return Err(unread("Ime"));
    };
    Ok(match event {
        wire::ImeEvent::Enabled => ImeEvent::Enabled,
        wire::ImeEvent::Preedit { text, cursor } => ImeEvent::Preedit {
            text,
            cursor: cursor.map(|[start, end]| (start, end)),
        },
        wire::ImeEvent::Commit(text) => ImeEvent::Commit(text),
        wire::ImeEvent::Disabled => ImeEvent::Disabled,
        _ => return Err(unread("ImeEvent")),
    })
}

fn row_from_wire(row: wire::Row) -> DtoResult<Row> {
    let readings = |readings: Vec<wire::Reading>| {
        readings
            .into_iter()
            .map(reading_from_wire)
            .collect::<DtoResult<Vec<_>>>()
    };
    Ok(match row {
        wire::Row::Reading { reading } => Row::Reading(reading_from_wire(reading)?),
        wire::Row::Batch {
            coalesced,
            predicted,
            current,
        } => Row::Batch {
            coalesced: readings(coalesced)?,
            predicted: readings(predicted)?,
            current: reading_from_wire(current)?,
        },
        wire::Row::ClickSettings { settings } => {
            Row::ClickSettings(settings.map(click_settings_from_wire))
        }
        wire::Row::Viewport { viewport, frame } => Row::Viewport {
            reading: viewport_from_wire(viewport)?,
            frame: frame_from_wire(frame)?,
        },
        wire::Row::ViewportAbandoned { frame } => Row::ViewportAbandoned {
            frame: frame_from_wire(frame)?,
        },
        wire::Row::Wheel { wheel } => Row::Wheel(wheel_from_wire(wheel)?),
        wire::Row::Scroll { scroll } => Row::Scroll(scroll_from_wire(scroll)?),
        wire::Row::AddDevice {
            device,
            kind,
            time_ns,
        } => Row::AddDevice {
            device: NativeDevice(device),
            kind: kind_from_wire(kind)?,
            time: EventTime::from_nanos(time_ns),
        },
        wire::Row::RemoveDevice { device, time_ns } => Row::RemoveDevice {
            device: NativeDevice(device),
            time: EventTime::from_nanos(time_ns),
        },
        wire::Row::CancelSurface {
            surface,
            time_ns,
            reason,
        } => Row::CancelSurface {
            surface: NativeSurface(surface),
            time: EventTime::from_nanos(time_ns),
            reason: cancel_from_wire(reason)?,
        },
        wire::Row::CancelContact {
            surface,
            contact,
            time_ns,
            reason,
        } => Row::CancelContact {
            surface: NativeSurface(surface),
            contact: NativeContact(contact),
            time: EventTime::from_nanos(time_ns),
            reason: cancel_from_wire(reason)?,
        },
        wire::Row::Notice { notice } => Row::Notice(notice_from_wire(notice)?),
        wire::Row::SessionEnd { phase } => Row::SessionEnd(phase_from_wire(phase)?),
        wire::Row::Reopen => Row::Reopen,
        wire::Row::Ime { ime, redacted } => Row::Ime {
            event: ime_from_wire(ime)?,
            redacted,
        },
        wire::Row::Preferences { read } => Row::Preferences(read_from_wire(read)?),
        wire::Row::Displays { read, redacted } => Row::Displays {
            read: display_read_from_wire(read)?,
            redacted,
        },
        wire::Row::State { state, redacted } => Row::State {
            state: window_state_from_wire(state)?,
            redacted,
        },
        wire::Row::StoreFocus { focus } => {
            Row::StoreFocus(focus.map(store::focus_from_wire).transpose()?)
        }
        wire::Row::StoreLock { lock, redacted } => Row::StoreLock {
            lock: store::lock_from_wire(lock)?,
            redacted,
        },
        wire::Row::CloseRequest { answer, queued } => Row::CloseRequest { answer, queued },
        wire::Row::Closed { cause } => Row::Closed(cause),
        wire::Row::Cut { reason } => Row::Cut(reason),
        _ => return Err(unread("Row")),
    })
}

fn read_to_wire(read: &PreferenceRead) -> DtoResult<wire::PreferenceRead> {
    Ok(match read {
        PreferenceRead::Observed(values) => {
            wire::PreferenceRead::Observed(preferences_to_wire(values.clone())?)
        }
        PreferenceRead::Failed(failure) => wire::PreferenceRead::Failed(failure_to_wire(failure)),
    })
}

fn read_from_wire(read: wire::PreferenceRead) -> DtoResult<PreferenceRead> {
    Ok(match read {
        wire::PreferenceRead::Observed(values) => {
            PreferenceRead::Observed(preferences_from_wire(values)?)
        }
        wire::PreferenceRead::Failed(wire::Failure { kind, message }) => {
            PreferenceRead::Failed(ReadFailure { kind, message })
        }
        _ => return Err(unread("PreferenceRead")),
    })
}

/// `values` in the schema, through [`PreferenceParts`]: a field added there
/// fails to compile here until it is mirrored.
fn preferences_to_wire(values: SystemPreferences) -> DtoResult<wire::Preferences> {
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
                    _ => return Err(DtoError::Unmirrored("MotionPreference")),
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
                    _ => return Err(DtoError::Unmirrored("TextWeightPreference")),
                })
            })
            .transpose()?,
        locales: locales.map(|locales| locales.iter().map(Locale::to_language_tag).collect()),
        regional: regional_to_wire(regional),
        gestures: wire::Gestures {
            double_click_interval_ns: double_click_interval.map(duration_nanos).transpose()?,
            double_tap_interval_ns: double_tap_interval.map(duration_nanos).transpose()?,
            long_press_timeout_ns: long_press_timeout.map(duration_nanos).transpose()?,
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
                    _ => return Err(DtoError::Unmirrored("WheelStep")),
                })
            })
            .transpose()?,
        wheel_horizontal_characters: horizontal_characters,
    })
}

fn pixel_ratio(value: f64) -> DtoResult<DevicePixelRatio> {
    DevicePixelRatio::try_new(value).ok_or_else(|| invalid(format!("pixel ratio {value}")))
}

/// `parts` in the schema, each format as its Unicode value; none when no
/// format was observed.
fn regional_to_wire(parts: RegionalParts) -> Option<Box<wire::Regional>> {
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
fn regional_from_wire(wire: Option<Box<wire::Regional>>) -> DtoResult<RegionalParts> {
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
            .map(|value| {
                NumberingSystem::try_new(&value).map_err(|error| invalid(error.to_string()))
            })
            .transpose()?,
        region: region
            .map(|value| {
                RegionOverride::try_new(&value).map_err(|error| invalid(error.to_string()))
            })
            .transpose()?,
    })
}
/// The preferences `wire` describes, validated as the `with_*` methods
/// validate them.
fn preferences_from_wire(wire: wire::Preferences) -> DtoResult<SystemPreferences> {
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
    let rejected = |error: InvalidPreference| invalid(error.to_string());
    PreferenceParts {
        text_scale,
        motion: motion
            .map(|motion| {
                Ok(match motion {
                    wire::Motion::NoPreference => MotionPreference::NoPreference,
                    wire::Motion::Reduce => MotionPreference::Reduce,
                    wire::Motion::Scaled(scale) => {
                        MotionPreference::from_duration_scale(scale).map_err(rejected)?
                    }
                    _ => return Err(unread("Motion")),
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
                    _ => return Err(unread("TextWeight")),
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
                    .collect::<DtoResult<Arc<[Locale]>>>()
            })
            .transpose()?,
        regional: regional_from_wire(regional)?,
        gestures: GestureParts {
            double_click_interval: gestures.double_click_interval_ns.map(Duration::from_nanos),
            double_tap_interval: gestures.double_tap_interval_ns.map(Duration::from_nanos),
            long_press_timeout: gestures.long_press_timeout_ns.map(Duration::from_nanos),
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
                    .map_err(rejected)
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
                    .map_err(rejected)
                })
                .transpose()?,
        },
        wheel: WheelParts {
            vertical: wheel_vertical
                .map(|step| {
                    Ok(match step {
                        wire::WheelStep::Lines(lines) => WheelStep::Lines(lines),
                        wire::WheelStep::Page => WheelStep::Page,
                        _ => return Err(unread("WheelStep")),
                    })
                })
                .transpose()?,
            horizontal_characters: wheel_horizontal_characters,
        },
    }
    .try_into_preferences()
    .map_err(rejected)
}

fn failure_to_wire(failure: &ReadFailure) -> wire::Failure {
    wire::Failure {
        kind: failure.kind,
        message: failure.message.clone(),
    }
}

fn display_read_to_wire(read: &DisplayRead) -> DtoResult<wire::DisplayRead> {
    Ok(match read {
        DisplayRead::Observed { revision, displays } => wire::DisplayRead::Observed {
            revision: *revision,
            displays: displays
                .iter()
                .map(display_to_wire)
                .collect::<DtoResult<_>>()?,
        },
        DisplayRead::Failed(failure) => wire::DisplayRead::Failed(failure_to_wire(failure)),
    })
}

fn display_read_from_wire(read: wire::DisplayRead) -> DtoResult<DisplayRead> {
    Ok(match read {
        wire::DisplayRead::Observed { revision, displays } => DisplayRead::Observed {
            revision,
            displays: displays
                .into_iter()
                .map(display_from_wire)
                .collect::<DtoResult<_>>()?,
        },
        wire::DisplayRead::Failed(wire::Failure { kind, message }) => {
            DisplayRead::Failed(ReadFailure { kind, message })
        }
        _ => return Err(unread("DisplayRead")),
    })
}

const fn bounds_to_wire(bounds: Bounds<i32>) -> [i32; 4] {
    [
        bounds.origin.x,
        bounds.origin.y,
        bounds.size.width,
        bounds.size.height,
    ]
}

const fn bounds_from_wire([x, y, width, height]: [i32; 4]) -> Bounds<i32> {
    Bounds::new(Point::new(x, y), Size::new(width, height))
}

/// Every absence a capability can carry has a wire name: the match is over
/// the constants, which name each value the type holds, so a lasting reason
/// added to [`CapabilityAbsence`] fails to compile here until it is mirrored.
const fn absence_to_wire(absence: CapabilityAbsence) -> wire::Absence {
    match absence {
        CapabilityAbsence::NOT_ON_THIS_PLATFORM => wire::Absence::NotOnThisPlatform,
        CapabilityAbsence::NOT_ON_THIS_DEVICE => wire::Absence::NotOnThisDevice,
        CapabilityAbsence::NOT_YET_ON_THIS_HOST => wire::Absence::NotYetOnThisHost,
        CapabilityAbsence::WITHDRAWN => wire::Absence::Withdrawn,
    }
}

fn absence_from_wire(absence: wire::Absence) -> DtoResult<CapabilityAbsence> {
    Ok(match absence {
        wire::Absence::NotOnThisPlatform => CapabilityAbsence::NOT_ON_THIS_PLATFORM,
        wire::Absence::NotOnThisDevice => CapabilityAbsence::NOT_ON_THIS_DEVICE,
        wire::Absence::NotYetOnThisHost => CapabilityAbsence::NOT_YET_ON_THIS_HOST,
        wire::Absence::Withdrawn => CapabilityAbsence::WITHDRAWN,
        other => return Err(unread(other.name())),
    })
}

/// `display` in the schema, through [`DisplayParts`]: a field added there
/// fails to compile here until it is mirrored, and a variant added to its
/// colour is refused as unmirrored until it is.
fn display_to_wire(display: &RecordedDisplay) -> DtoResult<wire::Display> {
    let DisplayParts {
        name,
        bounds,
        usable_bounds,
        scale_factor,
        refresh_period,
        is_primary,
        persistent_key,
        color,
    } = display.parts.clone();
    let color = match color {
        Ok(color) => {
            // The schema writes a standard range with the headroom it has by
            // its variant, SDR white.
            let (gamut, dynamic_range, headroom) = match color {
                DisplayColor::Standard(gamut) => {
                    (gamut, wire::DynamicRange::Standard, Ok(Headroom::SDR))
                }
                DisplayColor::High(gamut, headroom) => (gamut, wire::DynamicRange::High, headroom),
                _ => return Err(DtoError::Unmirrored("DisplayColor")),
            };
            wire::DisplayColor::Known(wire::Color {
                gamut: match gamut {
                    ColorGamut::Srgb => wire::Gamut::Srgb,
                    ColorGamut::DisplayP3 => wire::Gamut::DisplayP3,
                    ColorGamut::Rec2020 => wire::Gamut::Rec2020,
                    _ => return Err(DtoError::Unmirrored("ColorGamut")),
                },
                dynamic_range,
                headroom: match headroom {
                    Ok(headroom) => wire::Headroom::Multiple(headroom.get()),
                    Err(absence) => wire::Headroom::Absent(absence_to_wire(absence)),
                },
            })
        }
        Err(absence) => wire::DisplayColor::Absent(absence_to_wire(absence)),
    };
    Ok(wire::Display {
        label: display.label,
        name,
        bounds: bounds_to_wire(bounds),
        usable_bounds: bounds_to_wire(usable_bounds),
        scale_factor: scale_factor.get(),
        refresh_period_ns: refresh_period
            .map(|period| duration_nanos(period.get()))
            .transpose()?,
        primary: is_primary,
        persistent_key,
        color,
    })
}

/// The display `wire` describes, validated as its types validate it.
fn display_from_wire(wire: wire::Display) -> DtoResult<RecordedDisplay> {
    let wire::Display {
        label,
        name,
        bounds,
        usable_bounds,
        scale_factor,
        refresh_period_ns,
        primary,
        persistent_key,
        color,
    } = wire;
    let color = match color {
        wire::DisplayColor::Known(wire::Color {
            gamut,
            dynamic_range,
            headroom,
        }) => {
            let gamut = match gamut {
                wire::Gamut::Srgb => ColorGamut::Srgb,
                wire::Gamut::DisplayP3 => ColorGamut::DisplayP3,
                wire::Gamut::Rec2020 => ColorGamut::Rec2020,
                other => return Err(unread(other.name())),
            };
            let headroom = match headroom {
                wire::Headroom::Multiple(multiple) => Ok(Headroom::try_new(multiple)
                    .map_err(|error| invalid(format!("headroom {multiple}: {error}")))?),
                wire::Headroom::Absent(absence) => Err(absence_from_wire(absence)?),
                _ => return Err(unread("Headroom")),
            };
            Ok(match dynamic_range {
                // The one headroom a standard range has; any other, or none,
                // is a row no display wrote.
                wire::DynamicRange::Standard if headroom == Ok(Headroom::SDR) => {
                    DisplayColor::Standard(gamut)
                }
                wire::DynamicRange::Standard => {
                    return Err(invalid(
                        "a standard-range display with a headroom other than SDR white",
                    ));
                }
                wire::DynamicRange::High => DisplayColor::High(gamut, headroom),
                other => return Err(unread(other.name())),
            })
        }
        wire::DisplayColor::Absent(absence) => Err(absence_from_wire(absence)?),
        _ => return Err(unread("DisplayColor")),
    };
    let refresh_period = refresh_period_ns
        .map(|nanos| {
            RefreshPeriod::try_from(Duration::from_nanos(nanos))
                .map_err(|error| invalid(format!("refresh period {nanos} ns: {error}")))
        })
        .transpose()?;
    let scale_factor = DevicePixelRatio::try_new(scale_factor)
        .ok_or_else(|| invalid(format!("scale factor {scale_factor}")))?;
    Ok(RecordedDisplay {
        label,
        parts: DisplayParts {
            name,
            bounds: bounds_from_wire(bounds),
            usable_bounds: bounds_from_wire(usable_bounds),
            scale_factor,
            refresh_period,
            is_primary: primary,
            persistent_key,
            color,
        },
    })
}

const fn size_to_wire(size: Size<f64>) -> [f64; 2] {
    [size.width, size.height]
}

/// The extent `[width, height]` describes, refused when it is not one a
/// snapshot holds.
fn extent_from_wire([width, height]: [f64; 2], what: &str) -> DtoResult<WindowExtent> {
    WindowExtent::try_new(Size::new(width, height))
        .map_err(|refused| invalid(format!("{what}: {refused}")))
}

fn geometry_to_wire(geometry: WindowGeometry) -> wire::WindowGeometry {
    wire::WindowGeometry {
        position: geometry.position().map(|position| [position.x, position.y]),
        size: size_to_wire(geometry.size()),
    }
}

fn geometry_from_wire(geometry: wire::WindowGeometry, what: &str) -> DtoResult<WindowGeometry> {
    let [width, height] = geometry.size;
    WindowGeometry::try_new(
        geometry.position.map(|[x, y]| Point::new(x, y)),
        Size::new(width, height),
    )
    .map_err(|refused| invalid(format!("{what}: {refused}")))
}

const fn availability_to_wire(availability: Availability) -> wire::Availability {
    match availability {
        Availability::Native => wire::Availability::Native,
        Availability::Emulated => wire::Availability::Emulated,
        Availability::Unavailable => wire::Availability::Unavailable,
    }
}

fn availability_from_wire(availability: wire::Availability) -> DtoResult<Availability> {
    Ok(match availability {
        wire::Availability::Native => Availability::Native,
        wire::Availability::Emulated => Availability::Emulated,
        wire::Availability::Unavailable => Availability::Unavailable,
        other => return Err(unread(other.name())),
    })
}

const fn capability_to_wire(capability: WindowCapability) -> wire::Capability {
    match capability {
        WindowCapability::Native => wire::Capability::Native,
        WindowCapability::Emulated => wire::Capability::Emulated,
        WindowCapability::Absent(absence) => {
            wire::Capability::Unavailable(absence_to_wire(absence))
        }
    }
}

fn capability_from_wire(capability: wire::Capability) -> DtoResult<WindowCapability> {
    Ok(match capability {
        wire::Capability::Native => WindowCapability::Native,
        wire::Capability::Emulated => WindowCapability::Emulated,
        wire::Capability::Unavailable(absence) => {
            WindowCapability::Absent(absence_from_wire(absence)?)
        }
        _ => return Err(unread("Capability")),
    })
}

/// `capabilities` in the schema, through [`CapabilityParts`]: a capability
/// added there fails to compile here until it is mirrored.
fn capabilities_to_wire(capabilities: CapabilityParts) -> wire::Capabilities {
    let CapabilityParts {
        sets_title,
        sets_position,
        sets_size,
        sets_constraints,
        sets_resizable,
        sets_frame,
        app_title_bar,
        minimizes,
        maximizes,
        enters_fullscreen,
        hides,
        shows,
        activates,
        closes,
        material,
        confines_cursor,
        locks_cursor,
        keeps_on_top,
        keeps_on_bottom,
        sets_resize_increments,
        sets_aspect_ratio,
        reports_presentation,
        reports_keyboard_inset,
        reports_system_gestures,
        observes_position,
        observes_minimized,
        places_at_open,
        frame_rate_hint,
        cursor_image,
    } = capabilities;
    wire::Capabilities {
        sets_title: capability_to_wire(sets_title),
        sets_position: capability_to_wire(sets_position),
        sets_size: capability_to_wire(sets_size),
        sets_constraints: capability_to_wire(sets_constraints),
        sets_resizable: capability_to_wire(sets_resizable),
        sets_frame: capability_to_wire(sets_frame),
        app_title_bar: capability_to_wire(app_title_bar),
        minimizes: capability_to_wire(minimizes),
        maximizes: capability_to_wire(maximizes),
        enters_fullscreen: capability_to_wire(enters_fullscreen),
        hides: capability_to_wire(hides),
        shows: capability_to_wire(shows),
        activates: capability_to_wire(activates),
        closes: capability_to_wire(closes),
        material: capability_to_wire(material),
        confines_cursor: capability_to_wire(confines_cursor),
        locks_cursor: capability_to_wire(locks_cursor),
        keeps_on_top: capability_to_wire(keeps_on_top),
        keeps_on_bottom: capability_to_wire(keeps_on_bottom),
        sets_resize_increments: capability_to_wire(sets_resize_increments),
        sets_aspect_ratio: capability_to_wire(sets_aspect_ratio),
        reports_presentation: capability_to_wire(reports_presentation),
        reports_keyboard_inset: capability_to_wire(reports_keyboard_inset),
        reports_system_gestures: capability_to_wire(reports_system_gestures),
        observes_position: availability_to_wire(observes_position),
        observes_minimized: availability_to_wire(observes_minimized),
        places_at_open: availability_to_wire(places_at_open),
        frame_rate_hint: availability_to_wire(frame_rate_hint),
        cursor_image: availability_to_wire(cursor_image),
    }
}

fn capabilities_from_wire(capabilities: wire::Capabilities) -> DtoResult<CapabilityParts> {
    let wire::Capabilities {
        sets_title,
        sets_position,
        sets_size,
        sets_constraints,
        sets_resizable,
        sets_frame,
        app_title_bar,
        minimizes,
        maximizes,
        enters_fullscreen,
        hides,
        shows,
        activates,
        closes,
        material,
        confines_cursor,
        locks_cursor,
        keeps_on_top,
        keeps_on_bottom,
        sets_resize_increments,
        sets_aspect_ratio,
        reports_presentation,
        reports_keyboard_inset,
        reports_system_gestures,
        observes_position,
        observes_minimized,
        places_at_open,
        frame_rate_hint,
        cursor_image,
    } = capabilities;
    Ok(CapabilityParts {
        sets_title: capability_from_wire(sets_title)?,
        sets_position: capability_from_wire(sets_position)?,
        sets_size: capability_from_wire(sets_size)?,
        sets_constraints: capability_from_wire(sets_constraints)?,
        sets_resizable: capability_from_wire(sets_resizable)?,
        sets_frame: capability_from_wire(sets_frame)?,
        app_title_bar: capability_from_wire(app_title_bar)?,
        minimizes: capability_from_wire(minimizes)?,
        maximizes: capability_from_wire(maximizes)?,
        enters_fullscreen: capability_from_wire(enters_fullscreen)?,
        hides: capability_from_wire(hides)?,
        shows: capability_from_wire(shows)?,
        activates: capability_from_wire(activates)?,
        closes: capability_from_wire(closes)?,
        material: capability_from_wire(material)?,
        confines_cursor: capability_from_wire(confines_cursor)?,
        locks_cursor: capability_from_wire(locks_cursor)?,
        keeps_on_top: capability_from_wire(keeps_on_top)?,
        keeps_on_bottom: capability_from_wire(keeps_on_bottom)?,
        sets_resize_increments: capability_from_wire(sets_resize_increments)?,
        sets_aspect_ratio: capability_from_wire(sets_aspect_ratio)?,
        reports_presentation: capability_from_wire(reports_presentation)?,
        reports_keyboard_inset: capability_from_wire(reports_keyboard_inset)?,
        reports_system_gestures: capability_from_wire(reports_system_gestures)?,
        observes_position: availability_from_wire(observes_position)?,
        observes_minimized: availability_from_wire(observes_minimized)?,
        places_at_open: availability_from_wire(places_at_open)?,
        frame_rate_hint: availability_from_wire(frame_rate_hint)?,
        cursor_image: availability_from_wire(cursor_image)?,
    })
}

/// `frame` in the schema.
fn window_frame_to_wire(frame: WindowFrame) -> DtoResult<wire::WindowFrame> {
    Ok(match frame {
        WindowFrame::System => wire::WindowFrame::System,
        WindowFrame::AppTitleBar(bar) => wire::WindowFrame::AppTitleBar {
            system_controls: match bar.system_controls() {
                SystemControls::Shown => wire::SystemControls::Shown,
                SystemControls::Offset(offset) => {
                    wire::SystemControls::Offset([offset.along(), offset.down()])
                }
                SystemControls::Hidden => wire::SystemControls::Hidden,
                _ => return Err(DtoError::Unmirrored("SystemControls")),
            },
        },
        WindowFrame::None => wire::WindowFrame::None,
        _ => return Err(DtoError::Unmirrored("WindowFrame")),
    })
}

/// The frame `wire` describes, with its offset validated as its type validates it.
fn window_frame_from_wire(wire: wire::WindowFrame) -> DtoResult<WindowFrame> {
    Ok(match wire {
        wire::WindowFrame::System => WindowFrame::System,
        wire::WindowFrame::AppTitleBar { system_controls } => WindowFrame::AppTitleBar(
            AppTitleBar::new().with_system_controls(match system_controls {
                wire::SystemControls::Shown => SystemControls::Shown,
                wire::SystemControls::Offset([along, down]) => {
                    SystemControls::Offset(ControlsOffset::try_new(along, down).map_err(
                        |error| invalid(format!("controls offset {along}, {down}: {error}")),
                    )?)
                }
                wire::SystemControls::Hidden => SystemControls::Hidden,
            }),
        ),
        wire::WindowFrame::None => WindowFrame::None,
    })
}

/// `state` in the schema, through [`WindowStateParts`]: a fact the snapshot
/// gains fails to compile here until it is mirrored.
fn window_state_to_wire(state: &RecordedWindowState) -> DtoResult<wire::WindowState> {
    let WindowStateParts {
        outer,
        content_size,
        mode,
        normal_geometry,
        visible,
        occluded,
        focus,
        title,
        constraints: SizeConstraints { min, max },
        resizable,
        frame,
        display: _,
        metrics,
        brightness,
        insets,
        execution,
        hovered,
        interaction,
        cursor_grab,
        level,
        capabilities,
    } = (*state.parts).clone();
    Ok(wire::WindowState {
        revision: state.revision,
        outer: geometry_to_wire(outer),
        content_size: size_to_wire(content_size.get()),
        mode: match mode {
            WindowMode::Normal => wire::WindowMode::Normal,
            WindowMode::Minimized => wire::WindowMode::Minimized,
            WindowMode::Maximized => wire::WindowMode::Maximized,
            WindowMode::Fullscreen => wire::WindowMode::Fullscreen,
            _ => return Err(DtoError::Unmirrored("WindowMode")),
        },
        normal_geometry: normal_geometry.map(geometry_to_wire),
        visible,
        occluded,
        focused: focus.is_focused(),
        title,
        constraints: wire::SizeConstraints {
            min: min.map(|min| size_to_wire(min.get())),
            max: max.map(|max| size_to_wire(max.get())),
        },
        resizable,
        frame: window_frame_to_wire(frame)?,
        display: state.display,
        metrics: metrics.map(|metrics| {
            let physical = metrics.physical();
            wire::WindowMetrics {
                physical: [physical.width, physical.height],
                scale: metrics.scale().get(),
            }
        }),
        appearance: match brightness {
            Brightness::Light => wire::Appearance::Light,
            Brightness::Dark => wire::Appearance::Dark,
        },
        safe_area: edges_to_wire(insets.safe_area()),
        keyboard: keyboard_to_wire(insets.keyboard())?,
        system_gestures: edges_to_wire(insets.system_gestures()),
        execution: match execution {
            WindowExecutionState::Running => wire::Execution::Running,
            WindowExecutionState::Suspended => wire::Execution::Suspended,
            WindowExecutionState::Detached => wire::Execution::Detached,
            _ => return Err(DtoError::Unmirrored("WindowExecutionState")),
        },
        hovered,
        active: focus.is_active(),
        user_placing: match interaction {
            WindowInteraction::UserPlacing | WindowInteraction::UserResizing => Some(true),
            WindowInteraction::None => Some(false),
            WindowInteraction::Unknown => None,
            _ => return Err(DtoError::Unmirrored("WindowInteraction")),
        },
        user_resizing: interaction == WindowInteraction::UserResizing,
        cursor_grab: match cursor_grab {
            CursorGrab::None => None,
            CursorGrab::Confined => Some(wire::CursorGrab::Confined),
            CursorGrab::Locked => Some(wire::CursorGrab::Locked),
        },
        level: match level {
            WindowLevel::Normal => None,
            WindowLevel::AlwaysOnTop => Some(wire::WindowLevel::AlwaysOnTop),
            WindowLevel::AlwaysOnBottom => Some(wire::WindowLevel::AlwaysOnBottom),
        },
        capabilities: capabilities_to_wire(capabilities),
    })
}

/// Edges as the schema writes them: top, right, bottom, left.
fn edges_to_wire(edges: EdgeInsets) -> [f64; 4] {
    [edges.top, edges.right, edges.bottom, edges.left]
}

fn edges_from_wire([top, right, bottom, left]: [f64; 4]) -> EdgeInsets {
    EdgeInsets::new(top, right, bottom, left)
}

/// `keyboard` in the schema; `None` for a hidden keyboard at rest.
fn keyboard_to_wire(keyboard: KeyboardInset) -> DtoResult<Option<wire::KeyboardInset>> {
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
                    _ => return Err(DtoError::Unmirrored("InsetCurve")),
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
fn keyboard_from_wire(wire: wire::KeyboardInset) -> DtoResult<KeyboardInset> {
    let target = edges_from_wire(wire.target);
    let refused = |error: flui_platform::InvalidInsets| invalid(format!("keyboard inset: {error}"));
    let Some(motion) = wire.motion else {
        return KeyboardInset::try_at_rest(target).map_err(refused);
    };
    let curve = match motion.curve {
        wire::InsetCurve::Linear => InsetCurve::Linear,
        wire::InsetCurve::CubicBezier { x1, y1, x2, y2 } => {
            InsetCurve::CubicBezier { x1, y1, x2, y2 }
        }
        wire::InsetCurve::SystemKeyboard => InsetCurve::SystemKeyboard,
        _ => return Err(unread("InsetCurve")),
    };
    let motion = InsetMotion::try_new(
        edges_from_wire(motion.from),
        EventTime::from_nanos(motion.start_ns),
        Duration::from_nanos(motion.duration_ns),
        curve,
    )
    .map_err(refused)?;
    KeyboardInset::try_moving(target, motion).map_err(refused)
}

/// The snapshot `wire` describes, validated as its types validate it.
fn window_state_from_wire(wire: wire::WindowState) -> DtoResult<RecordedWindowState> {
    let wire::WindowState {
        revision,
        outer,
        content_size,
        mode,
        normal_geometry,
        visible,
        occluded,
        focused,
        title,
        constraints,
        resizable,
        frame,
        display,
        metrics,
        appearance,
        safe_area,
        keyboard,
        system_gestures,
        execution,
        hovered,
        active,
        user_placing,
        user_resizing,
        cursor_grab,
        level,
        capabilities,
    } = wire;
    let metrics = metrics
        .map(|wire::WindowMetrics { physical, scale }| {
            let [width, height] = physical;
            WindowMetrics::try_new(Size::new(width, height), scale)
                .map_err(|error| invalid(format!("metrics {width}x{height} at {scale}: {error}")))
        })
        .transpose()?;
    let insets = WindowInsets::try_new(
        edges_from_wire(safe_area),
        keyboard
            .map(keyboard_from_wire)
            .transpose()?
            .unwrap_or_default(),
        edges_from_wire(system_gestures),
    )
    .map_err(|error| invalid(format!("insets {safe_area:?} {system_gestures:?}: {error}")))?;
    let parts = WindowStateParts {
        outer: geometry_from_wire(outer, "outer")?,
        content_size: extent_from_wire(content_size, "content size")?,
        mode: match mode {
            wire::WindowMode::Normal => WindowMode::Normal,
            wire::WindowMode::Minimized => WindowMode::Minimized,
            wire::WindowMode::Maximized => WindowMode::Maximized,
            wire::WindowMode::Fullscreen => WindowMode::Fullscreen,
            other => return Err(unread(other.name())),
        },
        normal_geometry: normal_geometry
            .map(|geometry| geometry_from_wire(geometry, "normal geometry"))
            .transpose()?,
        visible,
        occluded,
        // A hidden window stands nowhere, as `WindowState::successor` publishes it.
        focus: if visible {
            flui_platform::WindowFocus::reported(focused, active)
        } else {
            flui_platform::WindowFocus::Inactive
        },
        title,
        constraints: SizeConstraints::new(
            constraints
                .min
                .map(|min| extent_from_wire(min, "minimum size"))
                .transpose()?,
            constraints
                .max
                .map(|max| extent_from_wire(max, "maximum size"))
                .transpose()?,
        ),
        resizable,
        frame: window_frame_from_wire(frame)?,
        display: None,
        metrics,
        // The schema's vibrant values, which no host writes, are their theme:
        // vibrancy is a material, not a theme.
        brightness: match appearance {
            wire::Appearance::Light | wire::Appearance::VibrantLight => Brightness::Light,
            wire::Appearance::Dark | wire::Appearance::VibrantDark => Brightness::Dark,
            other => return Err(unread(other.name())),
        },
        insets,
        execution: match execution {
            wire::Execution::Running => WindowExecutionState::Running,
            wire::Execution::Suspended => WindowExecutionState::Suspended,
            wire::Execution::Detached => WindowExecutionState::Detached,
            other => return Err(unread(other.name())),
        },
        hovered,
        interaction: match (user_placing, user_resizing) {
            (_, true) => WindowInteraction::UserResizing,
            (Some(true), false) => WindowInteraction::UserPlacing,
            (Some(false), false) => WindowInteraction::None,
            (None, false) => WindowInteraction::Unknown,
        },
        cursor_grab: match cursor_grab {
            None => CursorGrab::None,
            Some(wire::CursorGrab::Confined) => CursorGrab::Confined,
            Some(wire::CursorGrab::Locked) => CursorGrab::Locked,
            Some(other) => return Err(unread(other.name())),
        },
        level: match level {
            None => WindowLevel::Normal,
            Some(wire::WindowLevel::AlwaysOnTop) => WindowLevel::AlwaysOnTop,
            Some(wire::WindowLevel::AlwaysOnBottom) => WindowLevel::AlwaysOnBottom,
            Some(other) => return Err(unread(other.name())),
        },
        capabilities: capabilities_from_wire(capabilities)?,
    };
    Ok(RecordedWindowState {
        revision,
        display,
        parts: Box::new(parts),
    })
}
