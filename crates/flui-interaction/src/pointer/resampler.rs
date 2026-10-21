//! Event resampler for smooth pointer event processing
//!
//! The resampler buffers one pointer's events and releases them on a
//! caller-paced sampling clock:
//!
//! - every buffered event whose time is at or before the sample time is
//!   emitted, in arrival order;
//! - when the next buffered move lies after the sample time, one extra move
//!   is synthesized at the position interpolated between the last emitted
//!   point and that move, at the sample time;
//! - sequence boundaries (`Down`, `Up`, `Cancel`, `Enter`, `Leave`) are never
//!   dropped or reordered. At the 100-event pressure threshold, adjacent moves
//!   are coalesced; otherwise the oldest non-boundary event is discarded.
//!   Boundaries can grow the queue past that threshold. This is not a hard
//!   memory bound: standalone callers must sample or stop regularly, including
//!   when feeding repeated contacts without moves.
//!   Prepared output is separate delivery debt: repeated observer failures can
//!   grow it beyond the source threshold, including through measured moves.
//!   A healthy driver must consume that debt, or explicit `clear` cancel it.
//!
//! This is particularly beneficial for:
//! - Devices with low-frequency sensors
//! - Mismatched input/display refresh rates (e.g., 120Hz input, 90Hz display)
//! - High-precision stylus input
//!
//! # Time
//!
//! Each event is placed on the sampling clock by its own time, not by when it
//! reached the resampler. [`PointerEventResampler::add_event_at`] takes that
//! time from the caller. [`PointerEventResampler::add_event`] reads the
//! event's owned `EventTime` and maps it onto [`Instant`] through the
//! smallest delivery latency seen so far, so a burst of events delivered
//! together keeps its original spacing; zero is a valid coarse timestamp.
//! An event without a supported time is placed at arrival. Times are made monotonic in
//! queue order, and emitted events carry non-decreasing `time` values.
//!
//! # Example
//!
//! ```rust
//! use std::time::{Duration, Instant};
//!
//! use flui_interaction::ids::PointerId;
//! use flui_interaction::pointer::PointerEventResampler;
//!
//! let resampler = PointerEventResampler::new(PointerId::new(core::num::NonZeroU64::MIN));
//! // The 60 Hz frame tick. The resampler enforces a 1 ms minimum
//! // interval between samples, so back-to-back ticks coalesce.
//! let now = Instant::now();
//! let next = now + Duration::from_millis(16);
//! resampler.sample(now, next, |_event| {
//!     // dispatch to recognisers here
//! });
//! let _has_pending = resampler.has_pending_events();
//! ```

use std::{collections::VecDeque, sync::Arc};

use web_time::{Duration, Instant};

use flui_foundation::geometry::{Offset, Point};
use flui_platform::EventTime;
use parking_lot::Mutex;
use smallvec::SmallVec;

use crate::{
    events::{PointerEvent, PointerEventExt, PointerMove, PointerPosition, PointerSample},
    ids::PointerId,
};

/// Recommended lookback from the presentation/frame time to the resample
/// target: `sample_time = frame_time - DEFAULT_RESAMPLE_LOOKBACK`.
///
/// 38 ms is two 60 Hz frames of worst-case input latency (33.3 ms) plus a
/// 4.7 ms input-driver margin. On 120 Hz input/display pipelines a ~22 ms lookback is
/// appropriate instead — pass your own offset to [`PointerEventResampler::sample`].
pub const DEFAULT_RESAMPLE_LOOKBACK: Duration = Duration::from_millis(38);

/// Number of buffered events past which the queue makes room for each new
/// one: adjacent moves are folded together, else the oldest event that is not
/// a sequence boundary is dropped. Boundaries (`Down`, `Up`, `Cancel`,
/// `Enter`, `Leave`) are never dropped, so the queue exceeds this only by the
/// boundaries a caller adds without ever sampling.
const MAX_BUFFERED_EVENTS: usize = 100;

/// Most `coalesced` samples one move keeps when the queue folds older moves
/// into it; the oldest beyond this are dropped.
const MAX_COALESCED_HISTORY: usize = MAX_BUFFERED_EVENTS;

/// Minimum time between samples to prevent excessive resampling
const MIN_SAMPLE_INTERVAL: Duration = Duration::from_millis(1); // 1ms

/// Where a buffered event sits on the sampling clock.
#[derive(Debug, Clone, Copy)]
enum Stamp {
    /// Given by the caller ([`PointerEventResampler::add_event_at`]).
    Explicit(Instant),
    /// The event's own nanosecond time, mapped through the event-clock base
    /// when the event is sampled.
    EventTime(u64),
    /// The event carried no time; placed at its arrival.
    Arrival(Instant),
}

/// Buffered pointer event with its place on the sampling clock.
#[derive(Debug, Clone)]
struct BufferedEvent {
    event: PointerEvent,
    stamp: Stamp,
}

/// The last emitted contact point: where interpolation starts from.
#[derive(Debug, Clone, Copy)]
struct Anchor {
    position: Offset<f64>,
    timestamp: Instant,
    nanos: u64,
    /// Sensor values from the most recently delivered reading, never the future bracket.
    sample: PointerSample,
}

/// Pointer event resampler for smooth motion
///
/// Maintains a queue of pointer events and generates resampled events
/// at caller-determined frequencies for smoother gesture recognition.
///
/// # Thread Safety
///
/// A cheap shared handle (`Arc<Mutex<_>>`): clones address the same
/// queue, so the owner can sample while user callbacks re-enter it. No
/// lock is held while a callback runs.
/// Prepared events are consumed once across handles. Concurrent drivers run
/// their callbacks on their own threads without a callback-completion ordering
/// guarantee; ordered UI observation requires synchronous owner driving.
#[derive(Debug, Clone)]
pub struct PointerEventResampler {
    inner: Arc<Mutex<ResamplerInner>>,
}

#[derive(Debug)]
struct ResamplerInner {
    /// Pointer ID this resampler tracks
    pointer_id: PointerId,
    /// Queue of buffered events, in arrival order.
    event_queue: VecDeque<BufferedEvent>,
    /// Prepared output remains deliverable until consumed or explicitly cleared.
    pending_output: VecDeque<OutputBatch>,
    /// Admission reserves reusable storage; live frozen prefixes prevent reuse.
    output_pool: Vec<OutputBatch>,
    /// Whether the pointer is currently down
    is_down: bool,
    /// Whether the pointer is being tracked
    is_tracked: bool,
    /// The last emitted contact point; `None` after a terminal event.
    anchor: Option<Anchor>,
    /// Last sample time
    last_sample_time: Option<Instant>,
    /// Sampling-clock time of the last dequeued event; later stamps are
    /// raised to it so queue order and time order agree.
    last_stamp: Option<Instant>,
    /// The latest `time` value emitted; later emitted times are raised to it.
    last_emitted_nanos: u64,
    /// The [`Instant`] at which the event clock read zero, estimated from
    /// the smallest delivery latency seen (`arrival - event time`).
    event_clock_base: Option<Instant>,
}

type OutputBatch = Arc<Mutex<VecDeque<PointerEvent>>>;

/// One frozen output prefix. Nested drivers can consume its shared debt, but
/// newly prepared batches never join this prefix after its admission.
pub(crate) struct PreparedEvents {
    batches: SmallVec<[OutputBatch; 4]>,
    owner: Arc<Mutex<ResamplerInner>>,
}

impl Drop for PreparedEvents {
    fn drop(&mut self) {
        self.batches.clear();
        self.owner.lock().reclaim_output();
    }
}

impl PreparedEvents {
    /// Commit each event before handing it to an observer. A panicking observer
    /// consumes that event only; the unvisited tail stays in the shared queue.
    /// Dropping this lazy iterator early preserves all events it did not yield.
    pub(crate) fn deliveries(&self) -> impl Iterator<Item = PointerEvent> + '_ {
        self.batches
            .iter()
            .flat_map(|batch| std::iter::from_fn(move || batch.lock().pop_front()))
    }
}

/// The event's own time in nanoseconds, if it carries a usable one.
fn event_nanos(event: &PointerEvent) -> Option<u64> {
    crate::events::event_time(event).map(EventTime::as_nanos)
}

fn measured_move(movement: &PointerMove, sample: PointerSample) -> PointerMove {
    PointerMove::new(movement.pointer, movement.buttons, sample)
        .with_modifiers(movement.modifiers)
        .with_coalesced(movement.coalesced().to_vec())
        .with_predicted(movement.predicted().to_vec())
}

/// Raise the event's own `time` to at least `floor`; returns the time it
/// now carries (or `floor` when it has none).
fn raise_time(event: &mut PointerEvent, floor: u64) -> u64 {
    let time = match event {
        PointerEvent::Down(button) => &mut button.sample.time,
        PointerEvent::Up(button) => &mut button.sample.time,
        PointerEvent::Move(update) => {
            if update.current().time.as_nanos() >= floor {
                return update.current().time.as_nanos();
            }
            let mut sample = *update.current();
            sample.time = EventTime::from_nanos(sample.time.as_nanos().max(floor));
            *update = measured_move(update, sample);
            return sample.time.as_nanos();
        }
        _ => return floor,
    };
    *time = EventTime::from_nanos(time.as_nanos().max(floor));
    time.as_nanos()
}

/// An event that opens, closes or re-scopes a pointer sequence; the queue
/// never drops one, so its consumer always sees the sequence's shape.
fn is_sequence_boundary(event: &PointerEvent) -> bool {
    matches!(
        event,
        PointerEvent::Down(_)
            | PointerEvent::Up(_)
            | PointerEvent::Cancel(_)
            | PointerEvent::Enter(_)
            | PointerEvent::Leave(_)
    )
}

impl ResamplerInner {
    fn cancel_output(&mut self) {
        for batch in self.pending_output.drain(..) {
            batch.lock().clear();
        }
    }

    fn reclaim_output(&mut self) {
        self.pending_output.retain(|batch| !batch.lock().is_empty());
        let idle = self
            .output_pool
            .iter()
            .enumerate()
            .filter(|(_, batch)| Arc::strong_count(batch) == 1)
            .max_by_key(|(_, batch)| batch.lock().capacity())
            .map(|(index, _)| index);
        let mut index = 0;
        self.output_pool.retain(|batch| {
            let keep = Some(index) == idle || Arc::strong_count(batch) != 1;
            index += 1;
            keep
        });
    }

    fn reserve_output(&mut self, capacity: usize) -> usize {
        self.reclaim_output();
        // The pool's sole Arc cannot acquire another owner without this inner
        // lock. In particular, an empty but still frozen prefix is not reusable.
        let index = self
            .output_pool
            .iter()
            .position(|batch| Arc::strong_count(batch) == 1)
            .unwrap_or_else(|| {
                self.output_pool.push(Arc::new(Mutex::new(VecDeque::new())));
                self.output_pool.len() - 1
            });
        self.output_pool[index].lock().reserve(capacity);
        self.pending_output.reserve(1);
        index
    }

    fn prepared_events(
        &mut self,
        emitted: SmallVec<[PointerEvent; 4]>,
        owner: &Arc<Mutex<Self>>,
    ) -> PreparedEvents {
        self.reclaim_output();
        if !emitted.is_empty() {
            let index = self.reserve_output(emitted.len());
            let batch = &self.output_pool[index];
            batch.lock().extend(emitted);
            self.pending_output.push_back(Arc::clone(batch));
        }
        PreparedEvents {
            batches: self.pending_output.iter().cloned().collect(),
            owner: Arc::clone(owner),
        }
    }

    /// Queue the event and return the diagnostic owed after unlocking.
    fn enqueue(&mut self, event: PointerEvent, stamp: Stamp) -> Option<PointerId> {
        match &event {
            PointerEvent::Down(..) => {
                self.is_down = true;
                self.is_tracked = true;
            }
            PointerEvent::Up(..) | PointerEvent::Cancel(..) => {
                self.is_down = false;
            }
            PointerEvent::Leave(..) => {
                self.is_tracked = false;
            }
            _ => {}
        }

        let overflow = !is_sequence_boundary(&event)
            && self.event_queue.len() >= MAX_BUFFERED_EVENTS
            && !self.coalesce_one_move()
            && !self.drop_oldest_droppable();
        self.event_queue.push_back(BufferedEvent { event, stamp });
        self.reserve_output(self.event_queue.len().saturating_add(1));
        overflow.then_some(self.pointer_id)
    }

    fn timestamp(&self, stamp: Stamp) -> Instant {
        let raw = match stamp {
            Stamp::Explicit(at) | Stamp::Arrival(at) => at,
            Stamp::EventTime(nanos) => self
                .event_clock_base
                .and_then(|base| base.checked_add(Duration::from_nanos(nanos)))
                .expect("BUG: an EventTime stamp is only created after the base is set"),
        };
        match self.last_stamp {
            Some(floor) => raw.max(floor),
            None => raw,
        }
    }

    /// Drop the oldest event that is not a sequence boundary. Returns `false`
    /// when every queued event is one.
    fn drop_oldest_droppable(&mut self) -> bool {
        let Some(index) = self
            .event_queue
            .iter()
            .position(|buffered| !is_sequence_boundary(&buffered.event))
        else {
            return false;
        };
        self.event_queue.remove(index);
        true
    }

    /// Make room for one more move: fold the oldest move that has a newer
    /// move right behind it into that newer move's `coalesced` history.
    /// Returns `false` when no two moves are adjacent.
    fn coalesce_one_move(&mut self) -> bool {
        let Some(index) = (0..self.event_queue.len().saturating_sub(1)).find(|&i| {
            matches!(self.event_queue[i].event, PointerEvent::Move(_))
                && matches!(self.event_queue[i + 1].event, PointerEvent::Move(_))
        }) else {
            return false;
        };
        let [older, newer] = self
            .event_queue
            .make_contiguous()
            .get_disjoint_mut([index, index + 1])
            .expect("BUG: adjacent queue entries exist");
        if let (PointerEvent::Move(older), PointerEvent::Move(newer)) =
            (&mut older.event, &mut newer.event)
        {
            if newer.try_coalesce_from(older).is_err() {
                return false;
            }
            // Keep the newest samples only, so a queue that is never sampled
            // cannot grow without bound through the history either.
            newer.retain_latest_coalesced(MAX_COALESCED_HISTORY);
        }
        self.event_queue.remove(index);
        true
    }

    fn emit(
        &mut self,
        mut event: PointerEvent,
        at: Instant,
        out: &mut SmallVec<[PointerEvent; 4]>,
    ) {
        self.last_stamp = Some(at);
        let nanos = raise_time(&mut event, self.last_emitted_nanos);
        self.last_emitted_nanos = nanos;
        match &event {
            PointerEvent::Down(..) | PointerEvent::Move(..) => {
                let sample = match &event {
                    PointerEvent::Down(button) => button.sample,
                    PointerEvent::Move(movement) => *movement.current(),
                    _ => unreachable!("BUG: contact sample matched above"),
                };
                self.anchor = Some(Anchor {
                    position: event
                        .position()
                        .expect("BUG: measured contact carries position"),
                    timestamp: at,
                    nanos,
                    sample,
                });
            }
            PointerEvent::Up(..) | PointerEvent::Cancel(..) | PointerEvent::Leave(..) => {
                self.anchor = None;
            }
            _ => {}
        }
        out.push(event);
    }
}

/// Diagnostics run subscribers, which may inspect or enqueue on this resampler.
fn report_boundary_overflow(pointer_id: Option<PointerId>) {
    if let Some(pointer_id) = pointer_id {
        tracing::debug!(
            ?pointer_id,
            "resampler queue full of sequence boundaries; queueing past the cap"
        );
    }
}

impl PointerEventResampler {
    /// Creates a new resampler for the given pointer ID
    pub fn new(pointer_id: PointerId) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ResamplerInner {
                pointer_id,
                event_queue: VecDeque::new(),
                pending_output: VecDeque::new(),
                output_pool: vec![Arc::new(Mutex::new(VecDeque::new()))],
                is_down: false,
                is_tracked: false,
                anchor: None,
                last_sample_time: None,
                last_stamp: None,
                last_emitted_nanos: 0,
                event_clock_base: None,
            })),
        }
    }

    /// Marks the pointer as down and tracked WITHOUT queueing an event.
    ///
    /// The binding dispatches the `Down` event directly (synchronously, to
    /// close the arena and cache the hit test), so the resampler must not also
    /// re-emit it on the next `sample()`. This establishes the tracking state
    /// reported by [`Self::is_tracked`]. Sampling always advances accepted
    /// source events, including hover and a queued Leave; no separate tracking
    /// call is required when feeding the standalone queue.
    pub fn start_tracking(&self) {
        let mut inner = self.inner.lock();
        inner.is_down = true;
        inner.is_tracked = true;
    }

    /// Adds a pointer event to the resampling queue, placed on the sampling
    /// clock by the event's own time (see the module's "Time" section).
    ///
    /// Events are buffered and will be processed during the next `sample()`
    /// call. Sequence boundaries (`Down`, `Up`, `Cancel`, `Enter`, `Leave`)
    /// are never dropped; a full queue coalesces adjacent moves or discards
    /// its oldest non-boundary event.
    pub fn add_event(&self, event: PointerEvent) {
        self.add_event_with_arrival(event, Instant::now());
    }

    /// The binding owns the arrival clock; keep mapping and enqueue atomic.
    pub(crate) fn add_event_with_arrival(&self, event: PointerEvent, arrival: Instant) {
        // Derive the stamp, install the clock base and enqueue under one lock:
        // a `stop` from another handle in between would clear the base an
        // `EventTime` stamp relies on.
        let mut inner = self.inner.lock();
        let stamp = match event_nanos(&event)
            .and_then(|nanos| Some((nanos, arrival.checked_sub(Duration::from_nanos(nanos))?)))
        {
            Some((nanos, candidate)) => {
                let base = inner
                    .event_clock_base
                    .map_or(candidate, |base| base.min(candidate));
                inner.event_clock_base = Some(base);
                Stamp::EventTime(nanos)
            }
            None => Stamp::Arrival(arrival),
        };
        let overflow = inner.enqueue(event, stamp);
        drop(inner);
        report_boundary_overflow(overflow);
    }

    /// Adds a pointer event that happened at `timestamp` on the sampling
    /// clock (the clock whose times are passed to [`Self::sample`]).
    ///
    /// The deterministic form of [`Self::add_event`], for callers that
    /// already map event times onto the sampling clock (replay, virtual
    /// clocks). A timestamp earlier than one already queued is raised to it,
    /// so arrival order is kept.
    pub fn add_event_at(&self, event: PointerEvent, timestamp: Instant) {
        let overflow = self.inner.lock().enqueue(event, Stamp::Explicit(timestamp));
        report_boundary_overflow(overflow);
    }

    /// Samples events at the specified time and invokes callback with resampled
    /// events
    ///
    /// # Arguments
    ///
    /// * `sample_time` - Current sample time (typically current frame time)
    /// * `next_sample_time` - Next expected sample time; must be after
    ///   `sample_time` (a non-advancing window is ignored)
    /// * `callback` - Function to call with each resampled event
    ///
    /// # Resampling Strategy
    ///
    /// - Every queued event at or before `sample_time` is emitted, in order.
    /// - If the next queued event is a move after `sample_time`, one move is
    ///   synthesized at the position interpolated between the last emitted
    ///   point and that move, at the fraction
    ///   `(sample_time - last) / (next - last)`; it is skipped when the
    ///   position did not change.
    ///   Sensors retain the last emitted reading (including `None`); pressure,
    ///   orientation and contact size are not copied from the future bracket
    ///   or interpolated. Synthetic moves contain no measured/predicted history.
    /// - A sample time earlier than the previous one, or less than 1 ms after
    ///   it, prepares no new output. Previously prepared delivery debt remains
    ///   consumable even when sampling does not advance or tracking was reset.
    ///
    /// The callback runs after the resampler's state is committed, with no
    /// lock held. A panic consumes the current packet and leaves the unvisited
    /// tail for the next healthy driver. Nested driving consumes older output
    /// before its newly prepared prefix; adding alone waits for another drive.
    /// Explicit [`Self::clear`] cancels remaining source and output debt.
    pub fn sample<F>(&self, sample_time: Instant, next_sample_time: Instant, mut callback: F)
    where
        F: FnMut(PointerEvent),
    {
        if next_sample_time <= sample_time {
            tracing::warn!("ignoring a non-advancing pointer sampling window");
            return;
        }

        let emitted = self.prepare_sample(sample_time);
        for event in emitted.deliveries() {
            callback(event);
        }
    }

    fn prepare_sample(&self, sample_time: Instant) -> PreparedEvents {
        {
            let mut inner = self.inner.lock();

            // Admission state does not revoke accepted source delivery.
            if inner.event_queue.is_empty() {
                return inner.prepared_events(SmallVec::new(), &self.inner);
            }

            // Enforce minimum sample interval
            if let Some(last_time) = inner.last_sample_time {
                let Some(elapsed) = sample_time.checked_duration_since(last_time) else {
                    let pointer_id = inner.pointer_id;
                    let emitted = inner.prepared_events(SmallVec::new(), &self.inner);
                    drop(inner);
                    tracing::warn!(?pointer_id, "ignoring a regressed pointer sample time");
                    return emitted;
                };
                if elapsed < MIN_SAMPLE_INTERVAL {
                    return inner.prepared_events(SmallVec::new(), &self.inner);
                }
            }

            inner.last_sample_time = Some(sample_time);
            let mut emitted = SmallVec::<[PointerEvent; 4]>::new();

            // Process all events up to sample_time
            while let Some(front) = inner.event_queue.front() {
                let at = inner.timestamp(front.stamp);
                if at > sample_time {
                    break; // Future event, wait for next sample
                }
                let buffered = inner
                    .event_queue
                    .pop_front()
                    .expect("BUG: event_queue front returned Some in the loop guard");
                inner.emit(buffered.event, at, &mut emitted);
            }

            // The next move is in the future: emit the position the pointer
            // had at the sample time, on the segment from the last emitted
            // point to that move.
            if let Some(anchor) = inner.anchor
                && let Some(next) = inner.event_queue.front()
                && let PointerEvent::Move(next_move) = &next.event
            {
                let next_at = inner.timestamp(next.stamp);
                // anchor.timestamp <= sample_time < next_at, so the span is
                // positive and the fraction lies in [0, 1).
                let span = next_at.duration_since(anchor.timestamp).as_secs_f64();
                let elapsed = sample_time.duration_since(anchor.timestamp).as_secs_f64();
                let fraction = (elapsed / span).clamp(0.0, 1.0);
                let next_pos = next.event.position().expect("BUG: Move carries position");
                let lerp = |from: f64, to: f64| from + (to - from) * fraction;
                let position = Offset::new(
                    lerp(anchor.position.dx, next_pos.dx),
                    lerp(anchor.position.dy, next_pos.dy),
                );
                if position != anchor.position && position.dx.is_finite() && position.dy.is_finite()
                {
                    let next_nanos = next_move.current().time.as_nanos();
                    let nanos = if next_nanos > anchor.nanos {
                        // Exact in f64 for any realistic span; rounding down
                        // keeps the stamp at or before the next event.
                        anchor.nanos + ((next_nanos - anchor.nanos) as f64 * fraction) as u64
                    } else {
                        anchor.nanos
                    };
                    let mut sample = anchor.sample;
                    sample.position =
                        PointerPosition::try_new(Point::new(position.dx, position.dy))
                            .expect("BUG: finite interpolated position checked above");
                    sample.time = EventTime::from_nanos(nanos);
                    let interpolated = PointerEvent::Move(
                        PointerMove::new(next_move.pointer, next_move.buttons, sample)
                            .with_modifiers(next_move.modifiers),
                    );
                    inner.emit(interpolated, sample_time, &mut emitted);
                }
            }

            inner.prepared_events(emitted, &self.inner)
        }
    }

    /// Deliver measured packets through a synchronous mid-contact boundary.
    ///
    /// A button edge must follow earlier hardware movement without ending the
    /// contact or resetting its interpolation anchor. New packets admitted by
    /// callbacks remain queued for the next boundary or sampling window.
    pub(crate) fn flush_through(
        &self,
        boundary: EventTime,
        mut callback: impl FnMut(PointerEvent),
    ) {
        let emitted = {
            let mut inner = self.inner.lock();
            let mut emitted = SmallVec::new();
            while inner.event_queue.front().is_some_and(|buffered| {
                event_nanos(&buffered.event).is_some_and(|time| time <= boundary.as_nanos())
            }) {
                let buffered = inner
                    .event_queue
                    .pop_front()
                    .expect("BUG: the queued prefix was checked before removal");
                let at = inner.timestamp(buffered.stamp);
                inner.emit(buffered.event, at, &mut emitted);
            }
            inner.prepared_events(emitted, &self.inner)
        };
        for event in emitted.deliveries() {
            callback(event);
        }
    }

    /// Commit the accepted measured prefix without advancing frame sampling or
    /// ending the contact. The binding freezes all prefixes before dispatch.
    pub(crate) fn prepare_pending_events(&self) -> PreparedEvents {
        let mut inner = self.inner.lock();
        let mut emitted = SmallVec::new();
        while let Some(buffered) = inner.event_queue.pop_front() {
            let at = inner.timestamp(buffered.stamp);
            inner.emit(buffered.event, at, &mut emitted);
        }
        inner.prepared_events(emitted, &self.inner)
    }

    /// Stops resampling and flushes all remaining events
    ///
    /// Invokes the callback with every buffered event, in order, and resets
    /// the resampler for the next sequence. Prepared output remains pending if
    /// the callback panics; a subsequent sample or stop can deliver its tail.
    pub fn stop<F>(&self, mut callback: F)
    where
        F: FnMut(PointerEvent),
    {
        let emitted = {
            let mut inner = self.inner.lock();
            let mut emitted = SmallVec::new();
            while let Some(buffered) = inner.event_queue.pop_front() {
                let at = inner.timestamp(buffered.stamp);
                inner.emit(buffered.event, at, &mut emitted);
            }

            // Reset state before callbacks can re-enter this resampler.
            inner.is_tracked = false;
            inner.is_down = false;
            inner.anchor = None;
            inner.last_sample_time = None;
            inner.last_stamp = None;
            inner.last_emitted_nanos = 0;
            inner.event_clock_base = None;
            inner.prepared_events(emitted, &self.inner)
        };

        for event in emitted.deliveries() {
            callback(event);
        }
    }

    /// Whether the latest admitted boundary leaves the pointer down.
    ///
    /// This describes admission, not which events callbacks have observed.
    pub fn is_down(&self) -> bool {
        self.inner.lock().is_down
    }

    /// Whether the latest admitted Down/start-tracking has not been followed by Leave/stop.
    ///
    /// This admission query does not gate delivery of accepted events. A queued
    /// Leave can make it false while its earlier events still await sampling.
    pub fn is_tracked(&self) -> bool {
        self.inner.lock().is_tracked
    }

    /// Whether source events or previously prepared output remain deliverable.
    pub fn has_pending_events(&self) -> bool {
        self.pending_event_count() != 0
    }

    /// Number of source events and prepared output packets waiting for delivery.
    #[inline]
    #[must_use]
    pub fn pending_event_count(&self) -> usize {
        let inner = self.inner.lock();
        inner.event_queue.len()
            + inner
                .pending_output
                .iter()
                .map(|batch| batch.lock().len())
                .sum::<usize>()
    }

    /// Returns the pointer ID this resampler tracks
    pub fn pointer_id(&self) -> PointerId {
        self.inner.lock().pointer_id
    }

    /// Withdraw already-prepared contact output after its owning sequence is
    /// detached. Source events remain available for terminal flushing.
    pub(crate) fn cancel_prepared_events(&self) {
        self.inner.lock().cancel_output();
    }

    /// Cancel all source events and not-yet-consumed output, including a frozen
    /// prefix currently being driven. A packet already passed to a callback
    /// remains consumed. Events admitted after clear belong to a later prefix.
    pub fn clear(&self) {
        let mut inner = self.inner.lock();
        inner.event_queue.clear();
        inner.cancel_output();
        inner.anchor = None;
    }
}
