//! Where a window was, kept in the application's storage and restored on the
//! next run (ADR-0237 decision 6).
//!
//! A [`PlacementRecall`] reads the stored [`WindowPlacement`] before a window
//! opens, without blocking: the owner turns on, and the window waits for the
//! answer at most [`RECALL_BOUND`], then opens from its configuration.
//! [`restore_into`] fits what was read to the displays of this run, so the first
//! reveal is already where the window belongs. A [`PlacementRecorder`] then
//! follows the window's published state and writes the placement back:
//!
//! - at most once every [`WRITE_INTERVAL`] while it changes: a change inside the
//!   interval is held, and [`tick`](PlacementRecorder::tick) writes it when the
//!   interval has passed ([`next_due`](PlacementRecorder::next_due) tells the
//!   owner when to wake for that);
//! - at once when the window closes or the session ends
//!   ([`begin_flush`](PlacementRecorder::begin_flush), then
//!   [`continue_flush`](PlacementRecorder::continue_flush) at owner turns, or
//!   [`settle`] when nothing comes after).
//!
//! A write counts as stored only when storage answered `Ok`: until then the
//! recorder still owes it, and a refused write is attempted again at the next
//! due time with the newest placement.
//!
//! Nothing here reads a clock or a display list of its own: the caller hands
//! both in, so a test drives it with a time it chooses.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};
use std::time::{Duration, Instant};

use flui_foundation::geometry::Size;
use flui_foundation::panic::{FirstPanic, drop_contained};
use flui_platform::WindowOptions;
use flui_platform::{
    DisplaySnapshot, StorageError, StorageFuture, StorageName, Stored, StoredVersion,
    WindowCapabilities, WindowGeometry, WindowInteraction, WindowMode, WindowPlacement,
    WindowState, WriteMode,
};
use flui_view::persist::StorageHandle;

/// The shortest time between two writes of a window that keeps changing.
pub(crate) const WRITE_INTERVAL: Duration = Duration::from_secs(2);

/// How long a window that remembers its placement waits for storage to answer
/// the read before it opens from its configuration.
///
/// A local file answers in well under a millisecond and an encrypted or
/// sandboxed store in a few; past this the application's first window would
/// visibly lag its launch, which costs more than a window that opens where its
/// configuration says. Measured on the host's placement clock, from the read's
/// start.
pub(crate) const RECALL_BOUND: Duration = Duration::from_millis(250);

/// A placement is a few hundred bytes; a stored value past this is not one.
const READ_LIMIT: u64 = 4096;

/// What a window's stored placement turned out to be.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Recalled {
    /// The placement stored, decoded and checked by
    /// [`WindowPlacement::from_bytes`].
    Placement(WindowPlacement),
    /// Storage holds no placement under the name: a first run.
    Absent,
    /// Storage holds bytes that are no placement this build reads: another
    /// version of the format, bytes cut short, a field out of range, or a
    /// value past the read limit.
    Unreadable,
    /// Storage refused the read.
    Refused(StorageError),
    /// Storage did not answer within [`RECALL_BOUND`].
    TimedOut,
    /// The storage's read panicked.
    Panicked,
}

impl Recalled {
    /// What storage answered for a read of `name`.
    fn of_stored(answer: Result<Stored, StorageError>, name: &StorageName) -> Self {
        match answer {
            Ok(Stored { bytes: None, .. }) => Self::Absent,
            Ok(Stored {
                bytes: Some(bytes), ..
            }) => Self::of_bytes(&bytes, name),
            Err(StorageError::TooLarge { len, limit }) => {
                tracing::warn!(
                    storage = name.as_str(),
                    len,
                    limit,
                    "the stored window placement is too large to be one"
                );
                Self::Unreadable
            }
            Err(error) => {
                tracing::warn!(%error, storage = name.as_str(), "the stored window placement cannot be read");
                Self::Refused(error)
            }
        }
    }

    /// `bytes` read as a placement of `name`.
    pub(crate) fn of_bytes(bytes: &[u8], name: &StorageName) -> Self {
        match WindowPlacement::from_bytes(bytes) {
            Ok(placement) => Self::Placement(placement),
            Err(error) => {
                tracing::warn!(%error, storage = name.as_str(), "the stored window placement is not usable");
                Self::Unreadable
            }
        }
    }

    /// The placement to open the window at, when one was read.
    pub(crate) fn placement(&self) -> Option<&WindowPlacement> {
        match self {
            Self::Placement(placement) => Some(placement),
            Self::Absent
            | Self::Unreadable
            | Self::Refused(_)
            | Self::TimedOut
            | Self::Panicked => None,
        }
    }

    /// What this says storage holds, for the recorder that follows the window.
    pub(crate) fn stored(&self) -> StoredPlacement {
        match self {
            Self::Placement(placement) => StoredPlacement::Holds(placement.to_bytes()),
            // Nothing worth keeping: the window's first placement replaces it.
            Self::Absent | Self::Unreadable => StoredPlacement::Nothing,
            // Storage may hold a placement this run never saw.
            Self::Refused(_) | Self::TimedOut | Self::Panicked => StoredPlacement::Unknown {
                user_changed: false,
            },
        }
    }
}

/// What storage holds under a recorder's name, as far as the run knows: what
/// decides whether a placement the window reports is owed to storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StoredPlacement {
    /// Storage holds these bytes: a placement equal to them is not written.
    Holds(Vec<u8>),
    /// Storage holds nothing worth keeping: the window's placement is owed.
    Nothing,
    /// The run could not read what storage holds, so it may hold a placement
    /// the user kept. Nothing the host or the application did to the window
    /// is owed: only a change the host states the user made
    /// ([`ChangeOrigin::User`]), after which `user_changed` is `true` and the
    /// window's placement is owed.
    Unknown {
        /// Whether the user changed the window since.
        user_changed: bool,
    },
}

impl StoredPlacement {
    /// Whether `latest`, the window's newest placement, is owed to storage.
    fn owes(&self, latest: &[u8]) -> bool {
        match self {
            Self::Holds(held) => held != latest,
            Self::Nothing => true,
            Self::Unknown { user_changed } => *user_changed,
        }
    }
}

/// Who changed a window between two of its reports. A change is the user's
/// only when the report that carries it says so
/// ([`WindowInteraction::is_user`]: placing or resizing it); nothing is
/// inferred from what did not
/// change, nor from the report before.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChangeOrigin {
    /// The report that carries the change says the user is placing the
    /// window.
    User,
    /// Anything else: where the window opened, a display or scale change, a
    /// window manager's or compositor's adjustment, an application's command,
    /// and every change on a host that cannot tell.
    Other,
}

impl ChangeOrigin {
    /// Who changed a window whose new report says `now`, with `changed`
    /// whether that report changed its placement.
    fn of(now: WindowInteraction, changed: bool) -> Self {
        if changed && now.is_user() {
            Self::User
        } else {
            Self::Other
        }
    }
}

/// A read of one window's stored placement, from its start before the window
/// opens until it settles.
///
/// The read belongs to the window that started it: the window takes the
/// outcome when it opens, and an answer that comes later has nothing left to
/// arrive at, since the read is dropped with the recall. So a late answer is
/// never applied to a window, least of all to one the user has moved since.
pub(crate) struct PlacementRecall {
    name: StorageName,
    state: RecallState,
}

enum RecallState {
    Reading {
        read: StorageFuture<Stored>,
        /// When the window stops waiting, on the caller's clock.
        deadline: Instant,
        /// Wakes the owner when storage answers, from whatever thread it
        /// answers on.
        wake_owner: Waker,
    },
    Settled(Recalled),
}

impl PlacementRecall {
    /// Start reading `name` from `storage` at `now`; `wake_owner` wakes the
    /// owner when storage answers.
    ///
    /// Asks storage once, here, and polls the read once: a store that answers
    /// at once settles the recall before this returns. The storage's `read` and
    /// its future are the application's code when it overrides the storage:
    /// a panic in either settles the recall as [`Recalled::Panicked`].
    pub(crate) fn start(
        storage: &StorageHandle,
        name: StorageName,
        now: Instant,
        wake_owner: Waker,
    ) -> Self {
        let mut first = FirstPanic::new();
        let state = match first.contain(|| storage.read(&name, READ_LIMIT)) {
            Some(read) => RecallState::Reading {
                read,
                deadline: now + RECALL_BOUND,
                wake_owner,
            },
            None => RecallState::Settled(Recalled::Panicked),
        };
        report_panic(first, &name, "starting the read");
        let mut recall = Self { name, state };
        recall.poll(now);
        recall
    }

    /// A recall that settled before it started: what the run already knows.
    pub(crate) fn settled(name: StorageName, recalled: Recalled) -> Self {
        Self {
            name,
            state: RecallState::Settled(recalled),
        }
    }

    /// Take storage's answer if it has one, or give up on it once `now` is at
    /// the deadline. `None` while the window still waits.
    pub(crate) fn poll(&mut self, now: Instant) -> Option<&Recalled> {
        if let RecallState::Reading {
            read,
            deadline,
            wake_owner,
        } = &mut self.state
        {
            let mut first = FirstPanic::new();
            let answer = first.contain(|| read.as_mut().poll(&mut Context::from_waker(wake_owner)));
            let settled = match answer {
                None => Some(Recalled::Panicked),
                Some(Poll::Ready(answer)) => Some(Recalled::of_stored(answer, &self.name)),
                Some(Poll::Pending) if now >= *deadline => {
                    tracing::warn!(
                        storage = self.name.as_str(),
                        bound_ms = RECALL_BOUND.as_millis(),
                        "storage did not answer the window placement read in time; \
                         the window opens from its configuration"
                    );
                    Some(Recalled::TimedOut)
                }
                Some(Poll::Pending) => None,
            };
            report_panic(first, &self.name, "reading");
            if let Some(settled) = settled {
                // The read's own drop is the storage's code too.
                let state = std::mem::replace(&mut self.state, RecallState::Settled(settled));
                drop_contained(state);
            }
        }
        self.outcome()
    }

    /// What the read settled as; `None` while it waits.
    pub(crate) fn outcome(&self) -> Option<&Recalled> {
        match &self.state {
            RecallState::Settled(recalled) => Some(recalled),
            RecallState::Reading { .. } => None,
        }
    }

    /// When the window stops waiting for the read; `None` once it settled.
    pub(crate) fn deadline(&self) -> Option<Instant> {
        match &self.state {
            RecallState::Reading { deadline, .. } => Some(*deadline),
            RecallState::Settled(_) => None,
        }
    }

    /// The name the placement is kept under.
    pub(crate) fn name(&self) -> StorageName {
        self.name
    }
}

impl Drop for PlacementRecall {
    fn drop(&mut self) {
        // A read still out is the storage's future: its drop is the storage's
        // code, contained like its poll.
        if matches!(self.state, RecallState::Reading { .. }) {
            let state =
                std::mem::replace(&mut self.state, RecallState::Settled(Recalled::TimedOut));
            drop_contained(state);
        }
    }
}

/// A panic out of the application's storage costs the window its placement
/// and nothing more: it is reported here and its payload discarded.
fn report_panic(first: FirstPanic, name: &StorageName, while_doing: &'static str) {
    if let Some(payload) = first.into_payload() {
        tracing::error!(
            storage = name.as_str(),
            panic = flui_foundation::panic::payload_text(payload.as_ref()),
            "the storage panicked {while_doing} the window placement; \
             the window opens from its configuration"
        );
        flui_foundation::panic::discard_payload(payload);
    }
}

/// Open `options` where `placement` was, fitted to `displays`.
///
/// `capabilities` and `frame` are what the platform says of its windows before
/// one exists (`Platform::window_capabilities`, `Platform::window_frame_inset`
/// at the target display's scale): a platform that cannot place a window at open
/// keeps size and mode and leaves the position to the system, and the fit is of
/// the outer size, frame included. The options' position, size, mode and
/// display come from the placement; everything else stays the configuration's.
/// Does nothing when no display can hold the placement.
pub(crate) fn restore_into(
    options: &mut WindowOptions,
    placement: &WindowPlacement,
    displays: &DisplaySnapshot,
    capabilities: &WindowCapabilities,
    frame_inset: impl Fn(flui_foundation::geometry::DevicePixelRatio) -> Size<f64>,
) {
    // The frame is measured at the scale of the display the window lands on,
    // which `resolve` decides; ask for the scale of the display it will pick by
    // resolving once without a frame, then again with the frame at that scale.
    let Some(first) = placement.resolve(displays, capabilities, Size::new(0.0, 0.0)) else {
        return;
    };
    let scale = displays.get(first.display).map_or(
        flui_foundation::geometry::DevicePixelRatio::ONE,
        |display| display.scale_factor,
    );
    let frame = if options.frame.at_open(capabilities).has_system_title_bar() {
        frame_inset(scale)
    } else {
        Size::new(0.0, 0.0)
    };
    let Some(resolved) = placement.resolve(displays, capabilities, frame) else {
        return;
    };
    options.size = resolved.size;
    options.display = Some(resolved.display);
    options.position = resolved.offset_in_display;
    options.mode = resolved.mode;
    // A window that opens away from normal returns to the bounds it was
    // remembered at: outer, so the content size plus the frame. A frame
    // measured as no extent gives no such bounds: the window keeps the ones
    // its configuration gave, and the refusal is reported.
    if resolved.mode == WindowMode::Normal {
        options.normal_geometry = None;
        return;
    }
    match WindowGeometry::try_new(
        resolved.offset_in_display,
        Size::new(
            resolved.size.width + frame.width,
            resolved.size.height + frame.height,
        ),
    ) {
        Ok(normal) => options.normal_geometry = Some(normal),
        Err(refused) => tracing::warn!(
            %refused,
            "the remembered bounds a window returns to describe no window; it keeps its configured ones"
        ),
    }
}

/// How many times a refused write of one placement is attempted before the
/// recorder gives up on it until the window's placement changes again.
pub(crate) const WRITE_ATTEMPTS: u32 = 3;

/// Follows one window's published state and writes its placement to storage.
///
/// Storage is the application's code when it provides its own: its
/// `publish`, its future's `poll` and that future's drop run with none of the
/// recorder's state borrowed, so storage may call back into the recorder from
/// any of them. A write taken out to be started or polled leaves a mark in
/// the state ([`Write::Starting`], [`Write::Polling`]), so a call that comes
/// back in meanwhile finds the write busy and starts no second one; the call
/// that took it out finishes it. A panic in any of them is a refused write.
pub(crate) struct PlacementRecorder {
    storage: StorageHandle,
    name: StorageName,
    /// Wakes the owner when storage answers a write in flight, from whatever
    /// thread storage answers on.
    answer: Waker,
    /// Set by [`Self::answer`]'s waker: storage has an answer to take.
    answered: Arc<AtomicBool>,
    record: RefCell<WriteRecord>,
    /// The window-state subscription that feeds [`Self::observe`], held for as
    /// long as the recorder follows the window.
    subscription: RefCell<Option<flui_view::WindowSubscription>>,
}

/// What the recorder has captured, what storage holds, and the write
/// between the two.
struct WriteRecord {
    /// The placement last captured; `None` until the window reports one.
    latest: Option<Vec<u8>>,
    /// What storage holds, as far as the recorder knows.
    stored: StoredPlacement,
    write: Write,
    /// When a write was last handed to storage.
    last_attempt: Option<Instant>,
    /// Refused attempts at writing [`Self::latest`] since it last changed.
    failures: u32,
    /// The placement the recorder gave up writing after
    /// [`WRITE_ATTEMPTS`]: not attempted again until the window's placement
    /// changes, or a flush asks for it.
    given_up: Option<Vec<u8>>,
    /// Whether storage refused a write since the last flush began.
    refused_in_flush: bool,
    /// Whether the window was maximized before it was last minimized, so a
    /// minimized window comes back maximized.
    maximized_before: bool,
    /// The window's last report and what it was measured against, so the
    /// next one can be attributed ([`ChangeOrigin`]) and a change of the
    /// displays alone measured again ([`PlacementRecorder::redisplay`]).
    seen: Option<Seen>,
}

/// One report of the window, as the recorder measured it.
struct Seen {
    state: WindowState,
    bytes: Vec<u8>,
}

/// The write between the recorder and storage.
enum Write {
    /// No write is out.
    Idle,
    /// A write is being handed to storage: its `publish` runs.
    Starting,
    /// Storage took a write and has not answered it.
    Out {
        bytes: Vec<u8>,
        answer: StorageFuture<StoredVersion>,
    },
    /// Storage's answer is being polled.
    Polling,
}

impl Write {
    fn is_idle(&self) -> bool {
        matches!(self, Self::Idle)
    }
}

impl WriteRecord {
    fn new(stored: StoredPlacement) -> Self {
        Self {
            latest: None,
            stored,
            write: Write::Idle,
            last_attempt: None,
            failures: 0,
            given_up: None,
            refused_in_flush: false,
            maximized_before: false,
            seen: None,
        }
    }

    /// Whether the newest placement is owed to storage outside a flush: not
    /// stored, and not given up on.
    fn owes_write(&self) -> bool {
        self.latest
            .as_deref()
            .is_some_and(|latest| self.given_up.as_deref() != Some(latest))
            && self.owes_flush()
    }

    /// Whether the newest placement is owed to storage at a flush, which
    /// tries a given-up placement once more.
    fn owes_flush(&self) -> bool {
        self.latest
            .as_deref()
            .is_some_and(|latest| self.stored.owes(latest))
    }

    /// The wait after the last attempt before the next: [`WRITE_INTERVAL`],
    /// doubled for each refused attempt.
    fn interval(&self) -> Duration {
        WRITE_INTERVAL.saturating_mul(1 << self.failures.min(WRITE_ATTEMPTS))
    }

    fn due(&self, now: Instant) -> bool {
        self.last_attempt
            .is_none_or(|at| now.saturating_duration_since(at) >= self.interval())
    }

    /// Take in `bytes` as the newest placement, changed by `origin`.
    fn capture(&mut self, bytes: Vec<u8>, origin: ChangeOrigin) {
        if self.latest.as_deref() == Some(bytes.as_slice()) {
            return;
        }
        if origin == ChangeOrigin::User
            && let StoredPlacement::Unknown { user_changed } = &mut self.stored
        {
            *user_changed = true;
        }
        self.failures = 0;
        self.given_up = None;
        self.latest = Some(bytes);
    }

    /// A write was refused, or panicked. `true` when this refusal is the
    /// last attempt at this placement, which the caller reports once.
    fn refuse(&mut self) -> bool {
        self.refused_in_flush = true;
        self.failures = self.failures.saturating_add(1);
        if self.failures >= WRITE_ATTEMPTS && self.given_up.is_none() {
            self.given_up.clone_from(&self.latest);
            return true;
        }
        false
    }

    /// Who changed the window from its last report to `state`, measured as
    /// `bytes`.
    fn origin_of(&self, state: &WindowState, bytes: &[u8]) -> ChangeOrigin {
        let Some(seen) = &self.seen else {
            // The first report is where the window opened.
            return ChangeOrigin::Other;
        };
        ChangeOrigin::of(state.interaction, seen.bytes.as_slice() != bytes)
    }
}

/// What came of polling storage's answer to a write.
enum Answer {
    Pending(StorageFuture<StoredVersion>),
    Stored,
    Refused(StorageError),
    Panicked,
}

impl PlacementRecorder {
    /// A recorder writing to `storage` under `name`, which holds what
    /// `stored` says.
    ///
    /// `wake_owner` runs, on any thread, when storage answers a write in
    /// flight: the owner then takes the answer at its next turn instead of
    /// looking for it on a timer.
    pub(crate) fn new(
        storage: StorageHandle,
        name: StorageName,
        stored: StoredPlacement,
        wake_owner: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let answered = Arc::new(AtomicBool::new(false));
        Self {
            storage,
            name,
            answer: Waker::from(Arc::new(AnswerWake {
                answered: Arc::clone(&answered),
                wake_owner,
            })),
            answered,
            record: RefCell::new(WriteRecord::new(stored)),
            subscription: RefCell::new(None),
        }
    }

    /// The name the placement is written under.
    pub(crate) fn name(&self) -> StorageName {
        self.name
    }

    /// Give a window reopened under the same name everything this recorder
    /// holds for the name: the window's newest placement, which the new
    /// window opens from (no read racing this recorder's last write could give
    /// it), what storage holds as far as this recorder knows, and the write it
    /// has out. One recorder writes a name at a time: this one owes nothing and
    /// writes nothing from here on, and the successor takes storage's answer
    /// to the write in flight before it starts one of its own.
    ///
    /// [`HandOver::Busy`] while storage's code runs this recorder's write (the
    /// call came back in from it), and [`HandOver::Nothing`] before the window
    /// reported a placement.
    pub(crate) fn hand_over(&self) -> HandOver {
        let mut record = self.record.borrow_mut();
        if matches!(record.write, Write::Starting | Write::Polling) {
            return HandOver::Busy;
        }
        let Some(latest) = record.latest.take() else {
            return HandOver::Nothing;
        };
        let write = std::mem::replace(&mut record.write, Write::Idle);
        let stored = std::mem::replace(&mut record.stored, StoredPlacement::Nothing);
        record.given_up = None;
        HandOver::Taken(Succession {
            latest,
            stored,
            write,
        })
    }

    /// A recorder that takes over `succession` from a closed window's
    /// recorder for the same name.
    pub(crate) fn succeeding(
        storage: StorageHandle,
        name: StorageName,
        succession: Succession,
        wake_owner: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let Succession { stored, write, .. } = succession;
        let recorder = Self::new(storage, name, stored, wake_owner);
        recorder.record.borrow_mut().write = write;
        recorder
    }

    /// Keep `subscription` alive as long as the recorder follows the window.
    pub(crate) fn hold(&self, subscription: flui_view::WindowSubscription) {
        let replaced = self.subscription.borrow_mut().replace(subscription);
        drop_contained(replaced);
    }

    /// Stop following the window: its subscription ends, the placement it
    /// last reported stays owed until it is stored.
    pub(crate) fn release_window(&self) {
        let subscription = self.subscription.borrow_mut().take();
        drop_contained(subscription);
    }

    /// Take in the window's newest state at `now`, measured against
    /// `displays`, and write it when it is owed and [`WRITE_INTERVAL`] has
    /// passed since the last write.
    pub(crate) fn observe(&self, state: &WindowState, displays: &DisplaySnapshot, now: Instant) {
        self.measure(state, displays, false);
        self.pump(now, false);
    }

    /// The displays changed while the window did not: measure its last state
    /// against `displays`, so a placement kept relative to a display's usable
    /// area follows a moved taskbar or a rearranged desktop. Such a change is
    /// never the user's. Nothing before the window reported a state.
    pub(crate) fn redisplay(&self, displays: &DisplaySnapshot, now: Instant) {
        let last = self
            .record
            .borrow()
            .seen
            .as_ref()
            .map(|seen| seen.state.clone());
        if let Some(state) = last {
            self.measure(&state, displays, true);
            self.pump(now, false);
        }
    }

    /// Record the window's `state`; `redisplay` marks a measure no change of
    /// the window caused.
    fn measure(&self, state: &WindowState, displays: &DisplaySnapshot, redisplay: bool) {
        let mut record = self.record.borrow_mut();
        match state.mode {
            WindowMode::Maximized => record.maximized_before = true,
            WindowMode::Normal | WindowMode::Fullscreen => record.maximized_before = false,
            WindowMode::Minimized | _ => {}
        }
        let Some(placement) = WindowPlacement::from_state(state, displays, record.maximized_before)
        else {
            return;
        };
        let bytes = placement.to_bytes();
        let origin = if redisplay {
            ChangeOrigin::Other
        } else {
            record.origin_of(state, &bytes)
        };
        record.seen = Some(Seen {
            state: state.clone(),
            bytes: bytes.clone(),
        });
        record.capture(bytes, origin);
    }

    /// When the owner should wake to [`tick`](Self::tick) this recorder: when
    /// a held change may be written. A write in flight has no deadline:
    /// storage's answer wakes the owner itself ([`AnswerWake`]), and the turn
    /// it brings takes the answer. `None` when nothing is owed, or the
    /// recorder gave up on the placement it owes.
    pub(crate) fn next_due(&self) -> Option<Instant> {
        let record = self.record.borrow();
        if !record.write.is_idle() {
            return None;
        }
        if !record.owes_write() {
            return None;
        }
        record.last_attempt.map(|at| at + record.interval())
    }

    /// Take storage's answer to a write in flight, and write a held change
    /// whose interval is over at `now`. Call it when the owner wakes at
    /// [`next_due`](Self::next_due).
    pub(crate) fn tick(&self, now: Instant) {
        self.pump(now, false);
    }

    /// Hand the newest placement to storage now, whatever the interval says,
    /// a placement given up on included.
    /// [`continue_flush`](Self::continue_flush) and
    /// [`poll_flush`](Self::poll_flush) say when storage has answered.
    pub(crate) fn begin_flush(&self, now: Instant) {
        self.record.borrow_mut().refused_in_flush = false;
        self.pump(now, true);
    }

    /// [`poll_flush`](Self::poll_flush) at an owner turn, without waiting:
    /// storage's answer wakes the owner, which calls this again.
    pub(crate) fn continue_flush(&self, now: Instant) -> FlushState {
        let answer = self.answer.clone();
        self.poll_flush(&answer, now)
    }

    /// Where the flush is, taking storage's answers with `waker`.
    ///
    /// A write that was already in flight when the flush began may answer for
    /// an older placement than the newest: then the newest is handed to storage
    /// in turn, so the flush is over only when storage holds what the window
    /// last reported. A write storage refuses ends the flush with that
    /// placement unstored: a closing window has no later time to retry at.
    pub(crate) fn poll_flush(&self, waker: &Waker, now: Instant) -> FlushState {
        self.answered.store(false, Ordering::Release);
        self.take_answer(waker);
        let start = {
            let record = self.record.borrow();
            record.write.is_idle() && record.owes_flush() && !record.refused_in_flush
        };
        if start {
            self.start_write(waker, now);
        }
        let record = self.record.borrow();
        match (record.write.is_idle(), record.owes_flush()) {
            (true, false) => FlushState::Stored,
            (true, true) if record.refused_in_flush => FlushState::Refused,
            // A write is out, or one owed and not refused was just started.
            (false, _) | (true, true) => FlushState::Waiting,
        }
    }

    /// Take storage's answer, then start the owed write when it is due at
    /// `now` (always when `flush`).
    fn pump(&self, now: Instant, flush: bool) {
        // Cleared first: an answer arriving after this poll sets it again.
        self.answered.store(false, Ordering::Release);
        let answer = self.answer.clone();
        self.take_answer(&answer);
        let start = {
            let record = self.record.borrow();
            record.write.is_idle()
                && if flush {
                    record.owes_flush()
                } else {
                    record.owes_write() && record.due(now)
                }
        };
        if start {
            self.start_write(&answer, now);
        }
    }

    /// Hand the newest placement to storage, then poll its answer once with
    /// `waker`. Storage's `publish` runs with nothing borrowed.
    fn start_write(&self, waker: &Waker, now: Instant) {
        let bytes = {
            let mut record = self.record.borrow_mut();
            if !record.write.is_idle() {
                return;
            }
            let Some(bytes) = record.latest.clone() else {
                return;
            };
            record.write = Write::Starting;
            record.last_attempt = Some(now);
            bytes
        };
        let mut first = FirstPanic::new();
        let published = first.contain(|| {
            self.storage
                .publish(&self.name, bytes.clone(), WriteMode::Replace)
        });
        report_panic(first, &self.name, "writing");
        let gave_up = {
            let mut record = self.record.borrow_mut();
            if let Some(answer) = published {
                record.write = Write::Out { bytes, answer };
                false
            } else {
                record.write = Write::Idle;
                record.refuse()
            }
        };
        self.report_given_up(gave_up);
        self.take_answer(waker);
    }

    /// Take storage's answer to the write in flight, if it has one,
    /// registering `waker` with it otherwise. The poll and the drop of a
    /// settled answer run with nothing borrowed.
    fn take_answer(&self, waker: &Waker) {
        let (bytes, mut answer) = {
            let mut record = self.record.borrow_mut();
            match std::mem::replace(&mut record.write, Write::Polling) {
                Write::Out { bytes, answer } => (bytes, answer),
                other @ (Write::Idle | Write::Starting | Write::Polling) => {
                    record.write = other;
                    return;
                }
            }
        };
        let mut first = FirstPanic::new();
        let polled = first.contain(|| answer.as_mut().poll(&mut Context::from_waker(waker)));
        report_panic(first, &self.name, "writing");
        let outcome = match polled {
            Some(Poll::Pending) => Answer::Pending(answer),
            Some(Poll::Ready(Ok(_))) => {
                drop_contained(answer);
                Answer::Stored
            }
            Some(Poll::Ready(Err(error))) => {
                drop_contained(answer);
                Answer::Refused(error)
            }
            None => {
                drop_contained(answer);
                Answer::Panicked
            }
        };
        let (refused, gave_up) = {
            let mut record = self.record.borrow_mut();
            match outcome {
                Answer::Pending(answer) => {
                    record.write = Write::Out { bytes, answer };
                    (None, false)
                }
                Answer::Stored => {
                    record.write = Write::Idle;
                    record.failures = 0;
                    record.stored = StoredPlacement::Holds(bytes);
                    (None, false)
                }
                Answer::Refused(error) => {
                    record.write = Write::Idle;
                    (Some(error), record.refuse())
                }
                Answer::Panicked => {
                    record.write = Write::Idle;
                    (None, record.refuse())
                }
            }
        };
        if let Some(error) = refused {
            tracing::warn!(%error, storage = self.name.as_str(), "the window placement was not stored");
        }
        self.report_given_up(gave_up);
    }

    /// Say once, when the recorder gives up on a placement, that it will not
    /// try again until the window's placement changes.
    fn report_given_up(&self, gave_up: bool) {
        if gave_up {
            tracing::warn!(
                storage = self.name.as_str(),
                attempts = WRITE_ATTEMPTS,
                "storage keeps refusing the window placement; it is tried again when the \
                 window's placement changes, or when the window closes"
            );
        }
    }
}

/// What a closing recorder gives the recorder of a window reopened under its
/// name ([`PlacementRecorder::hand_over`]).
pub(crate) enum HandOver {
    /// The name's placement, its stored state and its write, moved over.
    Taken(Succession),
    /// The closed window never reported a placement: nothing to take.
    Nothing,
    /// Storage's code is running this recorder's write: nothing is moved,
    /// and the reopened window is not remembered, so two writers never race.
    Busy,
}

/// The name's placement, what storage holds and the write in flight, as one
/// recorder hands them to the next.
pub(crate) struct Succession {
    /// The closed window's newest placement, which the new window opens from.
    pub(crate) latest: Vec<u8>,
    stored: StoredPlacement,
    write: Write,
}

/// Where a recorder's flush is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlushState {
    /// A write is out and storage has not answered it yet.
    Waiting,
    /// Storage holds the window's newest placement, or nothing is owed.
    Stored,
    /// Storage refused the newest placement; it is lost.
    Refused,
}

/// The earliest [`next_due`](PlacementRecorder::next_due) of `recorders`.
pub(crate) fn earliest_due<'a>(
    recorders: impl IntoIterator<Item = &'a Rc<PlacementRecorder>>,
) -> Option<Instant> {
    recorders
        .into_iter()
        .filter_map(|recorder| recorder.next_due())
        .min()
}

/// Write every recorder's newest placement now and wait for storage to answer
/// all of them, for at most `budget` in all: one deadline for the lot, so an
/// ending session spends its time once and not once per window.
///
/// Blocks the calling thread: only the end of the loop or of the session,
/// with nothing after it to wait at, calls it. `between` runs before each
/// look at the answers: the host runs there what providers posted back to the
/// owner, since a provider may answer through the owner's queue, and the
/// owner thread is unparked when they post. Returns the names whose newest
/// placement storage did not take, refused or not answered by the deadline,
/// each reported here.
pub(crate) fn settle(
    recorders: &[Rc<PlacementRecorder>],
    now: Instant,
    budget: Duration,
    mut between: impl FnMut(),
) -> Vec<StorageName> {
    for recorder in recorders {
        recorder.begin_flush(now);
    }
    let deadline = Instant::now() + budget;
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    loop {
        between();
        // Poll every recorder, not just until one is pending: each registers
        // the waker with its own answer.
        let mut waiting = false;
        let mut lost = Vec::new();
        for recorder in recorders {
            match recorder.poll_flush(&waker, now) {
                FlushState::Stored => {}
                FlushState::Waiting => {
                    waiting = true;
                    lost.push(recorder.name());
                }
                FlushState::Refused => lost.push(recorder.name()),
            }
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if !waiting || left.is_zero() {
            if !lost.is_empty() {
                let names: Vec<&str> = lost.iter().map(StorageName::as_str).collect();
                tracing::warn!(
                    storage = ?names,
                    "window placements were not stored; those windows open from their \
                     last stored placement next time"
                );
            }
            return lost;
        }
        thread::park_timeout(left);
    }
}

/// The waker handed to a write in flight: notes that storage answered and
/// wakes the owner to take the answer.
struct AnswerWake {
    answered: Arc<AtomicBool>,
    wake_owner: Arc<dyn Fn() + Send + Sync>,
}

impl Wake for AnswerWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.answered.store(true, Ordering::Release);
        (self.wake_owner)();
    }
}

struct Unpark(Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

#[cfg(test)]
#[path = "placement_recorder_tests.rs"]
mod tests;
