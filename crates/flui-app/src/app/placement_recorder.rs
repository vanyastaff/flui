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
    WindowCapabilities, WindowGeometry, WindowMode, WindowPlacement, WindowState, WriteMode,
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
    pub(crate) fn baseline(&self) -> Baseline {
        match self {
            Self::Placement(placement) => Baseline::Holds(placement.to_bytes()),
            // Nothing worth keeping: the window's first placement replaces it.
            Self::Absent | Self::Unreadable => Baseline::Nothing,
            // Storage may hold a placement this run never saw: the window's
            // first placement does not overwrite it, a change the user makes
            // does.
            Self::Refused(_) | Self::TimedOut | Self::Panicked => Baseline::Unknown,
        }
    }
}

/// What a recorder starts from: what storage holds under its name, as far as
/// the run knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Baseline {
    /// Storage holds these bytes: a placement equal to them is not written.
    Holds(Vec<u8>),
    /// Storage holds nothing worth keeping: the window's first placement is
    /// written at once.
    Nothing,
    /// The run could not read what storage holds: the window's first placement
    /// is taken as what storage holds, and only a change to it is written.
    Unknown,
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
    frame_inset: impl Fn(f64) -> Size<f64>,
) {
    // The frame is measured at the scale of the display the window lands on,
    // which `resolve` decides; ask for the scale of the display it will pick by
    // resolving once without a frame, then again with the frame at that scale.
    let Some(first) = placement.resolve(displays, capabilities, Size::new(0.0, 0.0)) else {
        return;
    };
    let scale = displays
        .get(first.display)
        .map_or(1.0, |display| display.scale_factor);
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

/// Follows one window's published state and writes its placement to storage.
pub(crate) struct PlacementRecorder {
    storage: StorageHandle,
    name: StorageName,
    /// Wakes the owner when storage answers a write in flight, from whatever
    /// thread storage answers on.
    answer: Waker,
    /// Set by [`Self::answer`]'s waker: storage has an answer to take.
    answered: Arc<AtomicBool>,
    record: RefCell<WriteRecord>,
    /// The window's last published state, so a change of the displays alone
    /// is measured again ([`Self::redisplay`]).
    last_state: RefCell<Option<WindowState>>,
    /// The window-state subscription that feeds [`Self::observe`], held for as
    /// long as the recorder lives.
    subscription: RefCell<Option<flui_view::WindowSubscription>>,
}

/// What the recorder has captured, what storage has taken, and what is in
/// flight between the two.
#[derive(Default)]
struct WriteRecord {
    /// The placement last captured; `None` until the window reports one.
    latest: Option<Vec<u8>>,
    /// The bytes storage last answered `Ok` for, or that it already held.
    written: Option<Vec<u8>>,
    /// Whether the first placement captured is taken as what storage holds
    /// instead of being written ([`Baseline::Unknown`]).
    adopt_first: bool,
    /// A write handed to storage whose answer is not in yet.
    in_flight: Option<InFlight>,
    /// When a write was last handed to storage.
    last_attempt: Option<Instant>,
    /// Whether storage refused a write since the last flush began.
    refused: bool,
    /// Whether the window was maximized before it was last minimized, so a
    /// minimized window comes back maximized.
    maximized_before: bool,
}

struct InFlight {
    bytes: Vec<u8>,
    answer: StorageFuture<StoredVersion>,
}

impl WriteRecord {
    /// Whether the newest placement is not the one storage holds.
    fn owes_write(&self) -> bool {
        self.latest.is_some() && self.latest != self.written
    }

    fn due(&self, now: Instant) -> bool {
        self.last_attempt
            .is_none_or(|at| now.saturating_duration_since(at) >= WRITE_INTERVAL)
    }

    /// Take in `bytes` as the newest placement.
    fn capture(&mut self, bytes: Vec<u8>) {
        if std::mem::take(&mut self.adopt_first) {
            self.written = Some(bytes.clone());
        }
        self.latest = Some(bytes);
    }

    /// Take storage's answer to the write in flight, if it has one. An answer
    /// that panics is a refused write.
    fn poll_in_flight(&mut self, name: &StorageName, ctx: &mut Context<'_>) {
        let Some(in_flight) = &mut self.in_flight else {
            return;
        };
        let mut first = FirstPanic::new();
        let answer = first.contain(|| in_flight.answer.as_mut().poll(ctx));
        report_panic(first, name, "writing");
        match answer {
            Some(Poll::Pending) => {}
            Some(Poll::Ready(Ok(_))) => {
                self.written = self.in_flight.take().map(|sent| sent.bytes);
            }
            Some(Poll::Ready(Err(error))) => {
                // Not stored: the newest placement is still owed and is tried
                // again at the next due time.
                tracing::warn!(%error, storage = name.as_str(), "the window placement was not stored");
                self.refuse();
            }
            None => self.refuse(),
        }
    }

    fn refuse(&mut self) {
        self.refused = true;
        if let Some(in_flight) = self.in_flight.take() {
            drop_contained(in_flight.answer);
        }
    }

    /// Hand the newest placement to storage, unless one write is already out.
    /// A `publish` that panics is a refused write.
    fn start_write(
        &mut self,
        storage: &StorageHandle,
        name: &StorageName,
        now: Instant,
        ctx: &mut Context<'_>,
    ) {
        if self.in_flight.is_some() || !self.owes_write() {
            return;
        }
        let Some(bytes) = self.latest.clone() else {
            return;
        };
        self.last_attempt = Some(now);
        let mut first = FirstPanic::new();
        let answer = first.contain(|| storage.publish(name, bytes.clone(), WriteMode::Replace));
        report_panic(first, name, "writing");
        let Some(answer) = answer else {
            self.refuse();
            return;
        };
        self.in_flight = Some(InFlight { bytes, answer });
        self.poll_in_flight(name, ctx);
    }
}

impl PlacementRecorder {
    /// A recorder writing to `storage` under `name`, which holds what
    /// `baseline` says.
    ///
    /// `wake_owner` runs, on any thread, when storage answers a write in
    /// flight: the owner then takes the answer at its next turn instead of
    /// looking for it on a timer.
    pub(crate) fn new(
        storage: StorageHandle,
        name: StorageName,
        baseline: Baseline,
        wake_owner: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        let answered = Arc::new(AtomicBool::new(false));
        let (written, adopt_first) = match baseline {
            Baseline::Holds(bytes) => (Some(bytes), false),
            Baseline::Nothing => (None, false),
            Baseline::Unknown => (None, true),
        };
        Self {
            storage,
            name,
            answer: Waker::from(Arc::new(AnswerWake {
                answered: Arc::clone(&answered),
                wake_owner,
            })),
            answered,
            record: RefCell::new(WriteRecord {
                written,
                adopt_first,
                ..WriteRecord::default()
            }),
            last_state: RefCell::new(None),
            subscription: RefCell::new(None),
        }
    }

    /// The name the placement is written under.
    pub(crate) fn name(&self) -> StorageName {
        self.name
    }

    /// The newest placement the window reported, as stored; `None` until it
    /// reports one. The freshest placement the run knows: a window reopened
    /// under the same name opens from it rather than from a read that may
    /// race this recorder's last write.
    pub(crate) fn latest(&self) -> Option<Vec<u8>> {
        self.record.borrow().latest.clone()
    }

    /// Keep `subscription` alive as long as the recorder.
    pub(crate) fn hold(&self, subscription: flui_view::WindowSubscription) {
        *self.subscription.borrow_mut() = Some(subscription);
    }

    /// Stop following the window: its subscription ends, the placement it
    /// last reported stays owed until it is stored.
    pub(crate) fn release_window(&self) {
        let subscription = self.subscription.borrow_mut().take();
        drop_contained(subscription);
    }

    /// Take in the window's newest state at `now`, writing it when
    /// [`WRITE_INTERVAL`] has passed since the last write was handed to storage.
    pub(crate) fn observe(&self, state: &WindowState, displays: &DisplaySnapshot, now: Instant) {
        *self.last_state.borrow_mut() = Some(state.clone());
        self.measure(state, displays, now);
    }

    /// The displays changed while the window did not: measure its last state
    /// against `displays`, so a placement kept relative to a display's usable
    /// area follows a moved taskbar or a rearranged desktop. Nothing before the
    /// window reported a state.
    pub(crate) fn redisplay(&self, displays: &DisplaySnapshot, now: Instant) {
        let state = self.last_state.borrow().clone();
        if let Some(state) = state {
            self.measure(&state, displays, now);
        }
    }

    fn measure(&self, state: &WindowState, displays: &DisplaySnapshot, now: Instant) {
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
        record.capture(placement.to_bytes());
        self.poll_answer(&mut record);
        if record.due(now) {
            record.start_write(
                &self.storage,
                &self.name,
                now,
                &mut Context::from_waker(&self.answer),
            );
        }
    }

    /// When the owner should wake to [`tick`](Self::tick) this recorder, seen
    /// from `now`: when a held change may be written, or now once storage has
    /// answered a write in flight. A write still out has no deadline: storage's
    /// answer wakes the owner itself. `None` when storage holds the newest
    /// placement.
    pub(crate) fn next_due(&self, now: Instant) -> Option<Instant> {
        let record = self.record.borrow();
        if record.in_flight.is_some() {
            return self.answered.load(Ordering::Acquire).then_some(now);
        }
        if !record.owes_write() {
            return None;
        }
        record.last_attempt.map(|at| at + WRITE_INTERVAL)
    }

    /// Write a held change if its interval is over at `now`, and take storage's
    /// answer to a write in flight. Call it when the owner wakes at
    /// [`next_due`](Self::next_due).
    pub(crate) fn tick(&self, now: Instant) {
        let mut record = self.record.borrow_mut();
        self.poll_answer(&mut record);
        if record.due(now) {
            record.start_write(
                &self.storage,
                &self.name,
                now,
                &mut Context::from_waker(&self.answer),
            );
        }
    }

    /// Take storage's answer to the write in flight, registering the owner's
    /// wake with the answer if it is not in yet.
    fn poll_answer(&self, record: &mut WriteRecord) {
        // Cleared first: an answer arriving after this poll sets it again.
        self.answered.store(false, Ordering::Release);
        record.poll_in_flight(&self.name, &mut Context::from_waker(&self.answer));
    }

    /// Hand the newest placement to storage now, whatever the interval says.
    /// [`continue_flush`](Self::continue_flush) and
    /// [`poll_flush`](Self::poll_flush) say when storage has answered.
    pub(crate) fn begin_flush(&self, now: Instant) {
        let mut record = self.record.borrow_mut();
        record.refused = false;
        self.poll_answer(&mut record);
        record.start_write(
            &self.storage,
            &self.name,
            now,
            &mut Context::from_waker(&self.answer),
        );
    }

    /// [`poll_flush`](Self::poll_flush) at an owner turn, without waiting:
    /// storage's answer wakes the owner, which calls this again.
    pub(crate) fn continue_flush(&self, now: Instant) -> FlushState {
        // Cleared first: an answer arriving after this poll sets it again.
        self.answered.store(false, Ordering::Release);
        let answer = self.answer.clone();
        self.poll_flush(&mut Context::from_waker(&answer), now)
    }

    /// Where the flush is, taking storage's answers with `ctx`'s waker.
    ///
    /// A write that was already in flight when the flush began may answer for
    /// an older placement than the newest: then the newest is handed to storage
    /// in turn, so the flush is over only when storage holds what the window
    /// last reported. A write storage refuses ends the flush with that
    /// placement unstored: a closing window has no later time to retry at.
    pub(crate) fn poll_flush(&self, ctx: &mut Context<'_>, now: Instant) -> FlushState {
        let mut record = self.record.borrow_mut();
        record.poll_in_flight(&self.name, ctx);
        if record.in_flight.is_none() && record.owes_write() && !record.refused {
            record.start_write(&self.storage, &self.name, now, ctx);
            // Register `ctx`'s waker with the answer the start did not.
            record.poll_in_flight(&self.name, ctx);
        }
        match (&record.in_flight, record.owes_write()) {
            (None, false) => FlushState::Stored,
            (None, true) if record.refused => FlushState::Refused,
            // A write owed and not refused was just started above.
            (Some(_), _) | (None, true) => FlushState::Waiting,
        }
    }
}

/// Where a recorder's flush is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlushState {
    /// A write is out and storage has not answered it yet.
    Waiting,
    /// Storage holds the window's newest placement.
    Stored,
    /// Storage refused the newest placement; it is lost.
    Refused,
}

/// The earliest [`next_due`](PlacementRecorder::next_due) of `recorders`.
pub(crate) fn earliest_due<'a>(
    now: Instant,
    recorders: impl IntoIterator<Item = &'a Rc<PlacementRecorder>>,
) -> Option<Instant> {
    recorders
        .into_iter()
        .filter_map(|recorder| recorder.next_due(now))
        .min()
}

/// Write every recorder's newest placement now and wait for storage to answer
/// all of them, for at most `budget` in all: one deadline for the lot, so an
/// ending session spends its few seconds once and not once per window.
///
/// Blocks the calling thread: only the end of the loop or of the session,
/// with nothing after it to wait at, calls it. Returns the names whose newest
/// placement storage did not take, refused or not answered by the deadline,
/// each reported here.
pub(crate) fn settle(
    recorders: &[Rc<PlacementRecorder>],
    now: Instant,
    budget: Duration,
) -> Vec<StorageName> {
    for recorder in recorders {
        recorder.begin_flush(now);
    }
    let deadline = Instant::now() + budget;
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut ctx = Context::from_waker(&waker);
    loop {
        // Poll every recorder, not just until one is pending: each registers
        // the waker with its own answer.
        let mut waiting = false;
        let mut lost = Vec::new();
        for recorder in recorders {
            match recorder.poll_flush(&mut ctx, now) {
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
