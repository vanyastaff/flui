//! Where a window was, kept in the host storage and restored on the next run
//! (ADR-0237 decision 6).
//!
//! [`recall`] reads the stored [`WindowPlacement`] before a window opens and
//! [`restore_into`] fits it to the displays of this run, so the first reveal is
//! already where the window belongs. A [`PlacementRecorder`] then follows the
//! window's published state and writes the placement back:
//!
//! - at most once every [`WRITE_INTERVAL`] while it changes: a change inside the
//!   interval is held, and [`tick`](PlacementRecorder::tick) writes it when the
//!   interval has passed ([`next_due`](PlacementRecorder::next_due) tells the
//!   owner when to wake for that);
//! - at once when the window closes or the session ends
//!   ([`begin_flush`](PlacementRecorder::begin_flush), [`settle`]).
//!
//! A write counts as stored only when storage answered `Ok`: until then the
//! recorder still owes it, and a refused write is attempted again at the next
//! due time with the newest placement.
//!
//! The recorder reads no clock and no display list of its own: the caller hands
//! both in, so a test drives it with a time it chooses.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll, Wake, Waker};
use std::thread::{self, Thread};
use std::time::{Duration, Instant};

use flui_foundation::geometry::Size;
use flui_platform::WindowOptions;
use flui_platform::{
    DisplaySnapshot, Storage, StorageFuture, StorageName, StoredVersion, WindowCapabilities,
    WindowGeometry, WindowMode, WindowPlacement, WindowState, WriteMode,
};

/// The shortest time between two writes of a window that keeps changing.
pub(crate) const WRITE_INTERVAL: Duration = Duration::from_secs(2);

/// How long a closing window or an ending session waits, in all, for the final
/// writes to land before giving up on them.
pub(crate) const FLUSH_BUDGET: Duration = Duration::from_secs(4);

/// A placement is a few hundred bytes; a stored value past this is not one.
const READ_LIMIT: u64 = 4096;

/// The placement stored under `name`, if there is one this build can read.
///
/// Blocks the calling thread on the storage read, once, before the first
/// window opens. A missing value is the normal first run; a value that cannot
/// be read or decoded is reported and the window opens from its configuration.
pub(crate) fn recall(storage: &dyn Storage, name: &StorageName) -> Option<WindowPlacement> {
    let stored = match pollster::block_on(storage.read(name, READ_LIMIT)) {
        Ok(stored) => stored,
        Err(error) => {
            tracing::warn!(%error, storage = name.as_str(), "the stored window placement cannot be read");
            return None;
        }
    };
    match WindowPlacement::from_bytes(&stored.bytes?) {
        Ok(placement) => Some(placement),
        Err(error) => {
            tracing::warn!(%error, storage = name.as_str(), "the stored window placement is not usable");
            None
        }
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
    // remembered at: outer, so the content size plus the frame.
    options.normal_geometry = (resolved.mode != WindowMode::Normal).then(|| {
        WindowGeometry::new(
            resolved.offset_in_display,
            Size::new(
                resolved.size.width + frame.width,
                resolved.size.height + frame.height,
            ),
        )
    });
}

/// Follows one window's published state and writes its placement to storage.
pub(crate) struct PlacementRecorder {
    storage: Arc<dyn Storage>,
    name: StorageName,
    /// Wakes the owner when storage answers a write in flight, from whatever
    /// thread storage answers on.
    answer: Waker,
    /// Set by [`Self::answer`]'s waker: storage has an answer to take.
    answered: Arc<AtomicBool>,
    record: RefCell<WriteRecord>,
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

    /// Take storage's answer to the write in flight, if it has one.
    fn poll_in_flight(&mut self, cx: &mut Context<'_>) {
        let Some(in_flight) = &mut self.in_flight else {
            return;
        };
        match in_flight.answer.as_mut().poll(cx) {
            Poll::Pending => {}
            Poll::Ready(Ok(_)) => {
                self.written = self.in_flight.take().map(|sent| sent.bytes);
            }
            Poll::Ready(Err(error)) => {
                // Not stored: the newest placement is still owed and is tried
                // again at the next due time.
                tracing::warn!(%error, "the window placement was not stored");
                self.refused = true;
                self.in_flight = None;
            }
        }
    }

    /// Hand the newest placement to storage, unless one write is already out.
    fn start_write(
        &mut self,
        storage: &dyn Storage,
        name: &StorageName,
        now: Instant,
        cx: &mut Context<'_>,
    ) {
        if self.in_flight.is_some() || !self.owes_write() {
            return;
        }
        let Some(bytes) = self.latest.clone() else {
            return;
        };
        self.last_attempt = Some(now);
        let answer = storage.publish(name, bytes.clone(), WriteMode::Replace);
        self.in_flight = Some(InFlight { bytes, answer });
        self.poll_in_flight(cx);
    }
}

impl PlacementRecorder {
    /// A recorder writing to `storage` under `name`, which already holds
    /// `recalled` when the window opened from it.
    ///
    /// `wake_owner` runs, on any thread, when storage answers a write in
    /// flight: the owner then takes the answer at its next turn instead of
    /// looking for it on a timer.
    pub(crate) fn new(
        storage: Arc<dyn Storage>,
        name: StorageName,
        recalled: Option<&WindowPlacement>,
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
            record: RefCell::new(WriteRecord {
                written: recalled.map(WindowPlacement::to_bytes),
                ..WriteRecord::default()
            }),
            subscription: RefCell::new(None),
        }
    }

    /// Keep `subscription` alive as long as the recorder.
    pub(crate) fn hold(&self, subscription: flui_view::WindowSubscription) {
        *self.subscription.borrow_mut() = Some(subscription);
    }

    /// Take in the window's newest state at `now`, writing it when
    /// [`WRITE_INTERVAL`] has passed since the last write was handed to storage.
    pub(crate) fn observe(&self, state: &WindowState, displays: &DisplaySnapshot, now: Instant) {
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
        record.latest = Some(placement.to_bytes());
        self.poll_answer(&mut record);
        if record.due(now) {
            record.start_write(
                &*self.storage,
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
                &*self.storage,
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
        record.poll_in_flight(&mut Context::from_waker(&self.answer));
    }

    /// Hand the newest placement to storage now, whatever the interval says.
    /// [`poll_flush`](Self::poll_flush) says when storage has answered.
    pub(crate) fn begin_flush(&self, now: Instant) {
        let mut record = self.record.borrow_mut();
        record.refused = false;
        self.poll_answer(&mut record);
        record.start_write(
            &*self.storage,
            &self.name,
            now,
            &mut Context::from_waker(&self.answer),
        );
    }

    /// Whether the newest placement is stored, taking storage's answers with
    /// `cx`'s waker.
    ///
    /// A write that was already in flight when the flush began may answer for
    /// an older placement than the newest: then the newest is handed to storage
    /// in turn, so the flush is over only when storage holds what the window
    /// last reported. A write storage refuses ends the flush with that
    /// placement unstored: a closing window has no later time to retry at.
    pub(crate) fn poll_flush(&self, cx: &mut Context<'_>, now: Instant) -> bool {
        let mut record = self.record.borrow_mut();
        record.poll_in_flight(cx);
        if record.in_flight.is_none() && record.owes_write() && !record.refused {
            record.start_write(&*self.storage, &self.name, now, cx);
            // Register `cx`'s waker with the answer the start did not.
            record.poll_in_flight(cx);
        }
        record.in_flight.is_none() && (!record.owes_write() || record.refused)
    }
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
pub(crate) fn settle(recorders: &[Rc<PlacementRecorder>], now: Instant, budget: Duration) {
    for recorder in recorders {
        recorder.begin_flush(now);
    }
    let deadline = Instant::now() + budget;
    let waker = Waker::from(Arc::new(Unpark(thread::current())));
    let mut cx = Context::from_waker(&waker);
    loop {
        // Poll every recorder, not just until one is pending: each registers
        // the waker with its own answer.
        let mut settled = true;
        for recorder in recorders {
            settled &= recorder.poll_flush(&mut cx, now);
        }
        if settled {
            return;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            tracing::warn!("window placements were not stored in time");
            return;
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
