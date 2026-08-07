//! Per-presentation frame telemetry: a fixed-capacity ring of
//! [`FrameSnapshot`]s carrying input→present attribution (issue #556).
//!
//! This extends [`FrameClock`](crate::frame_clock::FrameClock)'s existing
//! "frame timestamps" responsibility (see that type's own module doc's
//! ownership table) rather than adding a fourth owner: the clock already
//! knows the physical instant a produce decision was made, so it is the
//! natural place to also remember what that produce actually did and which
//! routed inputs it carried.
//!
//! # Zero allocation on the frame-production path
//!
//! `FrameHistory` (crate-private) stores its ring as an inline `[Option<FrameSnapshot>; N]`
//! array — allocated once, as part of the owning `FrameClock`'s own storage,
//! never per frame. [`InputEpochs`] is the same shape: a fixed-size inline
//! array plus a length, never a `Vec`. Recording a frame
//! ([`FrameClock::record_frame`](crate::frame_clock::FrameClock::record_frame))
//! and stamping an input epoch
//! ([`FrameClock::stamp_input_epoch`](crate::frame_clock::FrameClock::stamp_input_epoch))
//! are therefore both index-and-assign operations with no allocation of
//! their own — proven by a dedicated counting-allocator integration test
//! (`crates/flui-scheduler/tests/frame_telemetry_allocation.rs`), the same
//! pattern `flui-engine`'s raster-backpressure accounting path used to prove
//! the same claim about itself. Only the PULL side
//! ([`FrameClock::frames_since`](crate::frame_clock::FrameClock::frames_since))
//! allocates — a `Vec` built on the *consumer's* call, never on the frame
//! path.

use flui_foundation::PresentationId;
use web_time::{Duration, Instant};

use crate::frame::FrameId;

/// How many routed input events one produced frame can carry attribution
/// for, before `InputEpochs::push` (crate-private) starts overwriting its own oldest
/// entries.
///
/// This bounds the RAW dispatch stream, not a coalesced one:
/// [`crate::frame_clock::FrameClock::stamp_input_epoch`] is called from
/// `UiRealm::handle_input_addressed`, at the TOP of that method, before
/// `GestureBinding::handle_pointer_event`/`flush_pending_moves` run —
/// coalescing of high-frequency pointer moves happens downstream, once per
/// frame, in `UiRealm::render_frame_entered`. So between two produces this
/// buffer can genuinely see more than [`MAX_COALESCED_INPUT_EPOCHS`] *raw*
/// events (a 1 kHz mouse against a 60 Hz produce cadence is on the order of
/// 16 raw moves per frame — routine during any drag, not an edge case).
/// `InputEpochs::push` handles that by overwriting the OLDEST retained
/// epoch rather than rejecting the newest one: the arrival closest to the
/// produce that will actually carry its effect is the one a jank consumer
/// most needs attribution for (it is what the frame's layout/hit-test saw),
/// while the earliest arrivals in a long coalescing window are the least
/// representative of what the frame ended up doing.
/// [`InputEpochs::overflowed`] still reports the condition rather than
/// hiding it.
pub const MAX_COALESCED_INPUT_EPOCHS: usize = 8;

/// How many recent frames [`FrameClock::record_frame`](crate::frame_clock::FrameClock::record_frame)
/// retains before the oldest entry is overwritten. ~2 seconds of history at
/// 60 Hz — enough for a jank-diagnosis consumer to pull a recent window
/// without this ring's own footprint growing unbounded.
pub const FRAME_HISTORY_CAPACITY: usize = 120;

/// Identity for one routed input event, minted by
/// [`FrameClock::stamp_input_epoch`](crate::frame_clock::FrameClock::stamp_input_epoch)
/// at dispatch time.
///
/// Scoped to the presentation whose clock minted it — two different
/// presentations' `InputEpochId(0)` are not the same event. A consumer that
/// needs process-wide uniqueness pairs this with the [`FrameSnapshot`] it
/// was pulled alongside, whose own [`FrameSnapshot::presentation`] field
/// names that presentation explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InputEpochId(u64);

impl InputEpochId {
    /// The raw counter value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl core::fmt::Display for InputEpochId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.0, f)
    }
}

/// One routed input event's identity and arrival instant, stamped at
/// dispatch time and coalesced into whichever frame actually carries its
/// effect.
///
/// `#[non_exhaustive]`: a future field (event kind, device id) is additive.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct InputEpoch {
    /// This event's identity, scoped to the presentation that stamped it.
    pub id: InputEpochId,
    /// When this event was routed to its target presentation — read from
    /// the SAME clock ([`FrameClock::now`](crate::frame_clock::FrameClock::now))
    /// every other timestamp on this presentation's frames comes from, so a
    /// [`ClockSource::Manual`](crate::frame_clock::ClockSource::Manual) test
    /// gets a deterministic, replayable latency computation — never a
    /// wall-clock read racing against a scripted timeline.
    pub arrival: Instant,
}

/// A bounded, allocation-free collection of input epochs coalesced into one
/// produced frame.
///
/// `Copy`: every field is either `Copy` itself or a fixed-size array of
/// `Copy` elements — this type never allocates, by construction, not just by
/// convention.
#[derive(Debug, Clone, Copy)]
pub struct InputEpochs {
    epochs: [Option<InputEpoch>; MAX_COALESCED_INPUT_EPOCHS],
    len: u8,
    /// Ring write cursor: the slot the NEXT [`Self::push`] writes to. While
    /// `len < MAX_COALESCED_INPUT_EPOCHS` this also marks one-past the last
    /// occupied slot; once full, it marks the OLDEST occupied slot — the one
    /// about to be overwritten — which is also where [`Self::iter`] starts
    /// reading from.
    next: u8,
    overflowed: bool,
}

impl InputEpochs {
    const EMPTY: Self = Self {
        epochs: [None; MAX_COALESCED_INPUT_EPOCHS],
        len: 0,
        next: 0,
        overflowed: false,
    };

    /// Add `epoch`. Beyond [`MAX_COALESCED_INPUT_EPOCHS`], this overwrites
    /// the OLDEST retained epoch instead of rejecting the new one — see
    /// [`MAX_COALESCED_INPUT_EPOCHS`]'s own doc for why keeping the newest
    /// arrivals is the correct discard policy here, not just a convenient
    /// one. [`Self::overflowed`] still latches `true` so a consumer can
    /// tell this frame's coalescing window exceeded capacity at all.
    fn push(&mut self, epoch: InputEpoch) {
        let index = self.next as usize;
        if (self.len as usize) < MAX_COALESCED_INPUT_EPOCHS {
            self.len += 1;
        } else {
            self.overflowed = true;
        }
        self.epochs[index] = Some(epoch);
        self.next = ((index + 1) % MAX_COALESCED_INPUT_EPOCHS) as u8;
    }

    /// Every coalesced epoch, oldest arrival first. Once
    /// [`MAX_COALESCED_INPUT_EPOCHS`] has been exceeded and the ring has
    /// wrapped, "oldest" means the oldest SURVIVING epoch — see
    /// `Self::push`'s discard policy.
    #[must_use = "this returns the iterator rather than consuming it"]
    pub fn iter(&self) -> impl Iterator<Item = &InputEpoch> {
        let len = self.len as usize;
        let start = if len < MAX_COALESCED_INPUT_EPOCHS {
            0
        } else {
            self.next as usize
        };
        let epochs = &self.epochs;
        (0..len).filter_map(move |i| epochs[(start + i) % MAX_COALESCED_INPUT_EPOCHS].as_ref())
    }

    /// How many epochs are coalesced here.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// Whether no epochs are coalesced here.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Whether at least one epoch was dropped for exceeding
    /// [`MAX_COALESCED_INPUT_EPOCHS`] before this frame was recorded.
    #[must_use]
    pub fn overflowed(&self) -> bool {
        self.overflowed
    }
}

impl Default for InputEpochs {
    fn default() -> Self {
        Self::EMPTY
    }
}

/// Where a produced frame ended up.
///
/// `#[non_exhaustive]`: additive — a future outcome (e.g. a distinguished
/// "withheld by first-frame deferral" case, once a caller wants to record
/// those too) does not break an existing match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PresentOutcome {
    /// The frame reached the screen.
    Presented,
    /// The submit failed (surface/device error) after the segment ran.
    Errored,
}

/// One presentation's produced-frame record: physical timing plus every
/// routed input this frame carried, the exported unit
/// [`FrameClock::frames_since`](crate::frame_clock::FrameClock::frames_since)
/// hands back as an owned value.
///
/// `#[non_exhaustive]`: a future field (e.g. a distinguished layout/paint
/// sub-span) is additive. `Copy` + plain data (`Send`): every field is a
/// primitive, an `Instant`, or [`InputEpochs`] — no interior mutability, no
/// shared ownership, so a pulled `Vec<FrameSnapshot>` is trivially safe to
/// hand to a different thread (a devtools exporter, for instance).
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct FrameSnapshot {
    /// Which presentation produced this frame. A realm can host more than
    /// one presentation, each with its own `FrameClock` and therefore its
    /// own `frame_id` sequence — two different presentations' `frame_id`
    /// values collide (both start counting from 1), so a consumer that
    /// pulls snapshots across presentations (or exports them into one trace
    /// file, as `flui-devtools`' `Timeline::record_frame_snapshots` does)
    /// needs this to tell them apart. [`InputEpochId`]'s own doc makes the
    /// same point about pairing an epoch id with its presentation.
    pub presentation: PresentationId,
    /// This presentation's own produce-count-derived frame identity (see
    /// [`FrameClock::record_frame`](crate::frame_clock::FrameClock::record_frame)'s
    /// doc for why it is derived from `produced_count()` rather than a
    /// second, redundant counter).
    pub frame_id: FrameId,
    /// The physical instant [`FrameClock::poll`](crate::frame_clock::FrameClock::poll)
    /// used to decide this frame should produce.
    pub clock_timestamp: Instant,
    /// When this presentation's build+layout+paint segment started.
    pub segment_start: Instant,
    /// When it finished.
    pub segment_end: Instant,
    /// When the resulting scene was handed to the raster backend.
    pub submit_at: Instant,
    /// What became of the submit.
    pub present_outcome: PresentOutcome,
    /// Every routed input event coalesced into this frame.
    pub input_epochs: InputEpochs,
}

impl FrameSnapshot {
    /// Wall-clock span this frame's build+layout+paint segment occupied.
    #[must_use]
    pub fn segment_span(&self) -> Duration {
        self.segment_end
            .saturating_duration_since(self.segment_start)
    }

    /// (submit − arrival) latency for every coalesced input, paired with
    /// that input's own id. An input that arrived earlier has a strictly
    /// larger latency than one that arrived later, for the same submit —
    /// this is what lets a consumer attribute jank to a SPECIFIC input
    /// rather than only see a distribution.
    #[must_use = "this returns the iterator rather than consuming it"]
    pub fn latencies(&self) -> impl Iterator<Item = (InputEpochId, Duration)> + '_ {
        self.input_epochs.iter().map(move |epoch| {
            (
                epoch.id,
                self.submit_at.saturating_duration_since(epoch.arrival),
            )
        })
    }
}

/// The fixed-capacity ring [`FrameSnapshot`]s land in. Not `pub`: reached
/// only through [`FrameClock`](crate::frame_clock::FrameClock)'s own
/// `record_frame`/`frames_since` methods, which own the write cursor and the
/// pending-input-epoch buffer that feeds it.
#[derive(Debug)]
pub(crate) struct FrameHistory {
    // A boxed slice, not `[Option<FrameSnapshot>; FRAME_HISTORY_CAPACITY]`:
    // `FrameSnapshot`'s own size (dominated by `InputEpochs`'s inline
    // array) makes a bare stack-sized array literal exceed clippy's
    // `large_stack_arrays` threshold. `vec![None; N].into_boxed_slice()`
    // builds it on the heap directly (one allocation, at construction —
    // never on the frame-production path `record` runs on) rather than
    // materializing the whole array as a stack temporary first.
    slots: Box<[Option<FrameSnapshot>]>,
    next: usize,
}

impl Default for FrameHistory {
    fn default() -> Self {
        Self {
            slots: vec![None; FRAME_HISTORY_CAPACITY].into_boxed_slice(),
            next: 0,
        }
    }
}

impl FrameHistory {
    /// Overwrite the oldest slot with `snapshot` — an index-and-assign, no
    /// allocation, no shift of the other slots. Takes `&FrameSnapshot`
    /// (copying the ~hundred-byte value once, into the slot) rather than by
    /// value, per clippy's `large_types_passed_by_value`.
    pub(crate) fn record(&mut self, snapshot: &FrameSnapshot) {
        let capacity = self.slots.len();
        self.slots[self.next] = Some(*snapshot);
        self.next = (self.next + 1) % capacity;
    }

    /// Every retained snapshot with `frame_id` strictly greater than
    /// `since` (or every retained snapshot, if `since` is `None`), oldest
    /// first. Allocates a `Vec` — this is the consumer-side pull, never the
    /// frame-production path.
    pub(crate) fn since(&self, since: Option<FrameId>) -> Vec<FrameSnapshot> {
        let mut collected: Vec<FrameSnapshot> = self
            .slots
            .iter()
            .filter_map(Option::as_ref)
            .copied()
            .filter(|snapshot| match since {
                Some(since_id) => snapshot.frame_id > since_id,
                None => true,
            })
            .collect();
        collected.sort_by_key(|snapshot| snapshot.frame_id);
        collected
    }
}

/// Mints [`InputEpochId`]s and buffers pending [`InputEpoch`]s until the
/// next [`FrameClock::record_frame`](crate::frame_clock::FrameClock::record_frame)
/// drains them — the presentation-side coalescing half of input→present
/// attribution. Not `pub`: reached only through `FrameClock`.
#[derive(Debug, Default)]
pub(crate) struct PendingInputEpochs {
    next_id: u64,
    pending: InputEpochs,
}

impl PendingInputEpochs {
    pub(crate) fn stamp(&mut self, arrival: Instant) -> InputEpochId {
        let id = InputEpochId(self.next_id);
        self.next_id += 1;
        self.pending.push(InputEpoch { id, arrival });
        id
    }

    /// Take every pending epoch, leaving the buffer empty — called exactly
    /// when a frame that will actually be recorded is about to be built, so
    /// an epoch that arrives while a segment ran but was NOT submitted this
    /// pump (deferred, no damage) stays pending for whichever later pump
    /// does submit, rather than being silently attributed to a frame that
    /// never reached the screen.
    pub(crate) fn drain(&mut self) -> InputEpochs {
        core::mem::replace(&mut self.pending, InputEpochs::EMPTY)
    }

    /// Read every pending epoch WITHOUT draining the buffer — for a submit
    /// attempt that failed on a path its own caller has already armed a
    /// retry for (e.g. `EngineError::SurfaceLost`). Draining here would
    /// leave the eventual retry's own real [`Self::drain`] with nothing to
    /// attribute to the inputs that arrived before the failed attempt, so
    /// the frame that eventually reaches the screen would carry no
    /// attribution for them at all. `InputEpochs` is `Copy`, so this is a
    /// plain read, not a second live handle into the buffer.
    pub(crate) fn peek(&self) -> InputEpochs {
        self.pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn epoch(id: u64, arrival: Instant) -> InputEpoch {
        InputEpoch {
            id: InputEpochId(id),
            arrival,
        }
    }

    fn snapshot_at(index: usize, now: Instant) -> FrameSnapshot {
        FrameSnapshot {
            presentation: PresentationId::new(1),
            frame_id: FrameId::zip(index),
            clock_timestamp: now,
            segment_start: now,
            segment_end: now,
            submit_at: now,
            present_outcome: PresentOutcome::Presented,
            input_epochs: InputEpochs::EMPTY,
        }
    }

    #[test]
    fn input_epochs_default_is_empty() {
        let epochs = InputEpochs::default();
        assert!(epochs.is_empty());
        assert_eq!(epochs.len(), 0);
        assert!(!epochs.overflowed());
        assert_eq!(epochs.iter().count(), 0);
    }

    #[test]
    fn input_epochs_preserves_push_order() {
        let now = Instant::now();
        let mut epochs = InputEpochs::EMPTY;
        epochs.push(epoch(0, now));
        epochs.push(epoch(1, now + Duration::from_millis(1)));
        let ids: Vec<u64> = epochs.iter().map(|e| e.id.get()).collect();
        assert_eq!(ids, vec![0, 1]);
    }

    #[test]
    fn input_epochs_beyond_capacity_sets_overflowed_and_keeps_the_newest_ones() {
        let now = Instant::now();
        let mut epochs = InputEpochs::EMPTY;
        let total = MAX_COALESCED_INPUT_EPOCHS as u64 + 3;
        for i in 0..total {
            epochs.push(epoch(i, now + Duration::from_millis(i)));
        }
        assert!(epochs.overflowed());
        assert_eq!(epochs.len(), MAX_COALESCED_INPUT_EPOCHS);
        // The kept epochs are the LAST MAX_COALESCED_INPUT_EPOCHS pushed
        // (the newest arrivals) — the ones closest to whatever produce
        // ends up carrying them — not the oldest, longest-stale ones.
        let ids: Vec<u64> = epochs.iter().map(|e| e.id.get()).collect();
        let expected_first_kept = total - MAX_COALESCED_INPUT_EPOCHS as u64;
        assert_eq!(
            ids,
            (expected_first_kept..total).collect::<Vec<_>>(),
            "oldest-surviving-first order must still hold after wrapping"
        );
    }

    #[test]
    fn frame_history_since_none_returns_every_retained_snapshot_oldest_first() {
        let mut history = FrameHistory::default();
        let now = Instant::now();
        for i in 1..=5usize {
            history.record(&snapshot_at(i, now));
        }
        let all = history.since(None);
        let ids: Vec<u64> = all.iter().map(|s| s.frame_id.get() as u64).collect();
        assert_eq!(ids, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn frame_history_since_some_excludes_at_and_before() {
        let mut history = FrameHistory::default();
        let now = Instant::now();
        for i in 1..=5usize {
            history.record(&snapshot_at(i, now));
        }
        let recent = history.since(Some(FrameId::zip(3)));
        let ids: Vec<u64> = recent.iter().map(|s| s.frame_id.get() as u64).collect();
        assert_eq!(ids, vec![4, 5]);
    }

    #[test]
    fn frame_history_wraps_and_overwrites_the_oldest_slot() {
        let mut history = FrameHistory::default();
        let now = Instant::now();
        for i in 1..=(FRAME_HISTORY_CAPACITY + 2) {
            history.record(&snapshot_at(i, now));
        }
        let all = history.since(None);
        assert_eq!(all.len(), FRAME_HISTORY_CAPACITY, "ring stays bounded");
        // The two oldest (frame_id 1 and 2) were overwritten.
        assert_eq!(all.first().unwrap().frame_id, FrameId::zip(3));
        assert_eq!(
            all.last().unwrap().frame_id,
            FrameId::zip(FRAME_HISTORY_CAPACITY + 2)
        );
    }

    #[test]
    fn pending_input_epochs_stamp_mints_increasing_ids_and_drain_empties_the_buffer() {
        let mut pending = PendingInputEpochs::default();
        let now = Instant::now();
        let a = pending.stamp(now);
        let b = pending.stamp(now + Duration::from_millis(5));
        assert_ne!(a, b);

        let drained = pending.drain();
        assert_eq!(drained.len(), 2);
        assert!(
            pending.drain().is_empty(),
            "a second drain finds nothing left"
        );
    }

    /// `peek` must report the same epochs `drain` would, but leave them
    /// pending for a later real `drain` — the property a failed submit
    /// that will be retried depends on to not lose input attribution.
    #[test]
    fn peek_reports_pending_epochs_without_draining_them() {
        let mut pending = PendingInputEpochs::default();
        let now = Instant::now();
        let a = pending.stamp(now);
        let b = pending.stamp(now + Duration::from_millis(5));

        let peeked = pending.peek();
        assert_eq!(peeked.len(), 2, "peek must see every pending epoch");
        let ids: Vec<u64> = peeked.iter().map(|e| e.id.get()).collect();
        assert_eq!(ids, vec![a.get(), b.get()]);

        // Unlike `drain`, a second `peek` sees the SAME epochs again.
        assert_eq!(
            pending.peek().len(),
            2,
            "peek must not have consumed anything"
        );

        let drained = pending.drain();
        assert_eq!(
            drained.len(),
            2,
            "the epochs peek reported must still be there for a real drain"
        );
    }

    #[test]
    fn frame_snapshot_latencies_are_larger_for_older_arrivals() {
        let now = Instant::now();
        let mut epochs = InputEpochs::EMPTY;
        let older_arrival = now;
        let newer_arrival = now + Duration::from_millis(10);
        epochs.push(epoch(0, older_arrival));
        epochs.push(epoch(1, newer_arrival));
        let submit_at = now + Duration::from_millis(20);

        let snapshot = FrameSnapshot {
            presentation: PresentationId::new(1),
            frame_id: FrameId::zip(1),
            clock_timestamp: now,
            segment_start: now,
            segment_end: now,
            submit_at,
            present_outcome: PresentOutcome::Presented,
            input_epochs: epochs,
        };

        let latencies: Vec<(InputEpochId, Duration)> = snapshot.latencies().collect();
        assert_eq!(latencies.len(), 2);
        assert!(
            latencies[0].1 > latencies[1].1,
            "the older arrival (id 0) must have the larger latency: {latencies:?}"
        );
        assert_eq!(latencies[0].1, Duration::from_millis(20));
        assert_eq!(latencies[1].1, Duration::from_millis(10));
    }
}
