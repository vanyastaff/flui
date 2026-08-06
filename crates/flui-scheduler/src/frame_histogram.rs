//! Fixed-bucket latency histograms over pulled [`FrameSnapshot`]s — issue
//! #556's frame-pacing observability, closing the plan's "p50/p90/p99 for
//! frame interval, produce-to-present, and input-to-present" deliverable.
//!
//! # Why fixed buckets, not a percentile library
//!
//! A histogram here is deliberately NOT an exact-order-statistic structure
//! (no sort of raw samples, no reservoir, no `Vec`): [`LatencyHistogram::
//! record`] is an index-and-increment into a fixed `[u32; N]` array, the
//! same shape `crate::frame_telemetry::FrameHistory` (crate-private)'s own fixed ring
//! already uses — allocation-free by construction (the whole type is
//! `Copy`, so it cannot own a heap allocation in safe Rust), not merely by
//! convention. [`LatencyHistogram::p50`]/[`LatencyHistogram::p90`]/[`LatencyHistogram::p99`] are therefore
//! APPROXIMATE (bucket-boundary resolution, nearest-rank estimation — see
//! [`LatencyHistogram::percentile`]'s own doc), which this module states
//! rather than implies: good enough for "is this frame roughly on-cadence
//! or badly janked", not for reproducing the exact value a full-sample
//! statistics library would compute.
//!
//! # Built over the pull side, not a fourth ring
//!
//! The three builder functions below consume an already-pulled
//! `&[FrameSnapshot]` (from [`crate::frame_clock::FrameClock::frames_since`],
//! itself the only allocating step in this whole path) rather than
//! accumulating their own persistent state on `FrameClock` — the clock's
//! existing fixed-capacity ring is the one source of truth for frame
//! history; a histogram is a VIEW computed fresh over it on demand, never a
//! second copy that could drift from the ring's own retention/eviction
//! policy.
//!
//! # Scope: the metric, not its exposure
//!
//! This module answers "what were recent frame intervals / produce-to-
//! present spans / input-to-present latencies", as fixed-bucket
//! percentiles a caller can log, assert on, or render. It deliberately does
//! NOT wire a `tracing` event or an on-screen overlay line — that
//! exposure-path work (a structured `tracing::event!` emission point, and
//! an overlay-text consumer of these percentiles) is real, separate design
//! work with its own call-site and formatting decisions, not a natural
//! extension of a pure histogram type, and is left for a follow-up rather
//! than rushed into this slice.

use web_time::Duration;

use crate::frame_telemetry::FrameSnapshot;

/// Upper bound, in microseconds, of each finite bucket — roughly
/// power-of-two spaced from sub-frame (500µs) to a full second, dense
/// around 60Hz's ~16.6ms cadence (the 8ms/16ms/33ms buckets) so "one frame
/// late" and "two frames late" land in different buckets rather than the
/// same one. Everything larger than the last bound falls into one more
/// (implicit) overflow bucket — see [`LatencyHistogram::percentile`] for
/// how that bucket reports its own approximation.
const BUCKET_UPPER_BOUNDS_US: [u64; 12] = [
    500, 1_000, 2_000, 4_000, 8_000, 16_000, 33_000, 50_000, 100_000, 250_000, 500_000, 1_000_000,
];

const BUCKET_COUNT: usize = BUCKET_UPPER_BOUNDS_US.len() + 1;

/// A fixed-capacity, allocation-free latency histogram.
///
/// `Copy`: every field is a fixed-size array of `u32` plus a `u32` count —
/// this type cannot own a heap allocation, in safe Rust, by construction.
#[derive(Debug, Clone, Copy)]
pub struct LatencyHistogram {
    buckets: [u32; BUCKET_COUNT],
    total: u32,
}

impl LatencyHistogram {
    /// An empty histogram — every [`Self::percentile`] query on it returns
    /// `None` (there is nothing to report a percentile of).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            buckets: [0; BUCKET_COUNT],
            total: 0,
        }
    }

    /// Record one observed `duration` — an index-and-increment into the
    /// fixed bucket array, no allocation.
    pub fn record(&mut self, duration: Duration) {
        let micros = u64::try_from(duration.as_micros()).unwrap_or(u64::MAX);
        let index = BUCKET_UPPER_BOUNDS_US
            .iter()
            .position(|&bound| micros <= bound)
            .unwrap_or(BUCKET_UPPER_BOUNDS_US.len());
        self.buckets[index] = self.buckets[index].saturating_add(1);
        self.total = self.total.saturating_add(1);
    }

    /// How many observations this histogram has recorded.
    #[must_use]
    pub fn count(&self) -> u32 {
        self.total
    }

    /// Whether no observations have been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// The bucket-boundary approximation of the `p`-th percentile (`p`
    /// clamped to `0.0..=1.0`), or `None` if nothing has been recorded.
    ///
    /// Walks buckets in ascending order, accumulating counts, and returns
    /// the upper bound of the first bucket whose cumulative count reaches
    /// `ceil(p * count())` — standard nearest-rank percentile estimation,
    /// at BUCKET resolution rather than exact-value resolution: the
    /// returned `Duration` is one of `BUCKET_UPPER_BOUNDS_US`'s (crate-private) values,
    /// never the exact observed duration. The overflow bucket (everything
    /// past the largest finite bound) reports the largest finite bound
    /// itself rather than a fabricated upper bound that does not exist —
    /// callers already read this the way every fixed-bucket histogram's top
    /// bucket is read: "at least this long".
    #[must_use]
    pub fn percentile(&self, p: f64) -> Option<Duration> {
        if self.total == 0 {
            return None;
        }
        let p = p.clamp(0.0, 1.0);
        let target = (p * f64::from(self.total)).ceil().max(1.0);
        let mut cumulative = 0u32;
        for (index, &bucket_count) in self.buckets.iter().enumerate() {
            cumulative = cumulative.saturating_add(bucket_count);
            if f64::from(cumulative) >= target {
                let bound_index = index.min(BUCKET_UPPER_BOUNDS_US.len() - 1);
                return Some(Duration::from_micros(BUCKET_UPPER_BOUNDS_US[bound_index]));
            }
        }
        // Unreachable when `total > 0`: the buckets' counts sum to `total`,
        // so the loop above always finds a bucket reaching `target` (at
        // most `total` itself). No `expect` on this pure query path --
        // `None` is a safe, if impossible, fallback rather than a panic.
        None
    }

    /// The 50th-percentile bucket-boundary approximation.
    #[must_use]
    pub fn p50(&self) -> Option<Duration> {
        self.percentile(0.50)
    }

    /// The 90th-percentile bucket-boundary approximation.
    #[must_use]
    pub fn p90(&self) -> Option<Duration> {
        self.percentile(0.90)
    }

    /// The 99th-percentile bucket-boundary approximation.
    #[must_use]
    pub fn p99(&self) -> Option<Duration> {
        self.percentile(0.99)
    }
}

impl Default for LatencyHistogram {
    fn default() -> Self {
        Self::new()
    }
}

/// Build a [`LatencyHistogram`] of frame-to-frame intervals — consecutive
/// [`FrameSnapshot::clock_timestamp`] deltas — from a `frames_since`-pulled
/// slice. `snapshots` need not already be sorted by `frame_id`: this sorts
/// a local index list rather than assuming the caller already has (even
/// though `FrameHistory::since`'s own contract already returns them oldest
/// first).
#[must_use]
pub fn frame_interval_histogram(snapshots: &[FrameSnapshot]) -> LatencyHistogram {
    let mut ordered: Vec<&FrameSnapshot> = snapshots.iter().collect();
    ordered.sort_by_key(|snapshot| snapshot.frame_id);
    let mut histogram = LatencyHistogram::new();
    for pair in ordered.windows(2) {
        let interval = pair[1]
            .clock_timestamp
            .saturating_duration_since(pair[0].clock_timestamp);
        histogram.record(interval);
    }
    histogram
}

/// Build a [`LatencyHistogram`] of produce-to-present spans —
/// [`FrameSnapshot::submit_at`] minus [`FrameSnapshot::clock_timestamp`],
/// i.e. from the instant `poll` decided to produce to the instant the
/// resulting scene was handed to the raster backend — from a
/// `frames_since`-pulled slice.
#[must_use]
pub fn produce_to_present_histogram(snapshots: &[FrameSnapshot]) -> LatencyHistogram {
    let mut histogram = LatencyHistogram::new();
    for snapshot in snapshots {
        let span = snapshot
            .submit_at
            .saturating_duration_since(snapshot.clock_timestamp);
        histogram.record(span);
    }
    histogram
}

/// Build a [`LatencyHistogram`] of input-to-present latencies — every
/// coalesced input epoch's own `(submit_at - arrival)` value (see
/// [`FrameSnapshot::latencies`]), across every snapshot in `snapshots`,
/// folded into one histogram rather than kept per-frame.
#[must_use]
pub fn input_to_present_histogram(snapshots: &[FrameSnapshot]) -> LatencyHistogram {
    let mut histogram = LatencyHistogram::new();
    for snapshot in snapshots {
        for (_, latency) in snapshot.latencies() {
            histogram.record(latency);
        }
    }
    histogram
}

#[cfg(test)]
mod tests {
    use web_time::Instant;

    use super::*;
    use crate::frame::FrameId;
    use crate::frame_telemetry::{InputEpochs, PresentOutcome};
    use flui_foundation::PresentationId;

    // `LatencyHistogram` is `Copy`, so it cannot own a heap allocation in
    // safe Rust -- the compile-time half of this module's "allocation-free"
    // claim, checked mechanically rather than only asserted in prose.
    static_assertions::assert_impl_all!(LatencyHistogram: Copy);

    #[test]
    fn empty_histogram_reports_no_percentiles() {
        let histogram = LatencyHistogram::new();
        assert!(histogram.is_empty());
        assert_eq!(histogram.count(), 0);
        assert_eq!(histogram.p50(), None);
        assert_eq!(histogram.p90(), None);
        assert_eq!(histogram.p99(), None);
    }

    /// Ten known durations, deliberately spanning several buckets --
    /// pins EXACT bucket values for p50/p90/p99, not just "some value".
    #[test]
    fn percentiles_land_on_the_expected_bucket_boundaries_for_a_known_distribution() {
        let mut histogram = LatencyHistogram::new();
        // 9 observations at 900us -- each falls in the bucket bounded by
        // 1000us (900 > 500, the first bound, so it skips to the second).
        for _ in 0..9 {
            histogram.record(Duration::from_micros(900));
        }
        // 1 observation at 900ms -- falls in the bucket bounded by
        // 1_000_000us (900_000 <= 1_000_000, the largest finite bound).
        histogram.record(Duration::from_millis(900));

        assert_eq!(histogram.count(), 10);
        // 9 of 10 observations are 900us -> bucket bound 1000us. p50 (ceil(0.5*10)=5th
        // observation) and p90 (ceil(0.9*10)=9th observation) both land in
        // that same bucket.
        assert_eq!(histogram.p50(), Some(Duration::from_millis(1)));
        assert_eq!(histogram.p90(), Some(Duration::from_millis(1)));
        // p99 (ceil(0.99*10)=10th observation) reaches the 900ms sample's
        // own bucket (bound 1_000_000us).
        assert_eq!(histogram.p99(), Some(Duration::from_secs(1)));
    }

    #[test]
    fn observations_past_the_largest_bound_land_in_the_overflow_bucket_and_report_its_floor() {
        let mut histogram = LatencyHistogram::new();
        histogram.record(Duration::from_secs(5)); // far past the 1s largest finite bound
        assert_eq!(histogram.count(), 1);
        // The overflow bucket reports the largest finite bound as its own
        // "at least this long" floor, not a fabricated exact value.
        assert_eq!(histogram.p50(), Some(Duration::from_secs(1)));
        assert_eq!(histogram.p99(), Some(Duration::from_secs(1)));
    }

    fn presentation_id() -> PresentationId {
        PresentationId::new(1)
    }

    fn snapshot(
        frame_index: usize,
        clock_timestamp: Instant,
        submit_at: Instant,
        input_epochs: InputEpochs,
    ) -> FrameSnapshot {
        FrameSnapshot {
            presentation: presentation_id(),
            frame_id: FrameId::zip(frame_index),
            clock_timestamp,
            segment_start: clock_timestamp,
            segment_end: clock_timestamp,
            submit_at,
            present_outcome: PresentOutcome::Presented,
            input_epochs,
        }
    }

    /// Three produced frames at a known, exact 16ms cadence -> the frame-
    /// interval histogram's p50 must land in the bucket that 16ms interval
    /// falls into (the 16ms bound itself), not some other bucket -- kills
    /// "the histogram is built but reads the wrong field" and "off-by-one
    /// in the sort/window pairing".
    #[test]
    fn frame_interval_histogram_pins_a_known_60hz_like_cadence() {
        let base = Instant::now();
        let snapshots = vec![
            snapshot(1, base, base, InputEpochs::default()),
            snapshot(
                2,
                base + Duration::from_millis(16),
                base + Duration::from_millis(16),
                InputEpochs::default(),
            ),
            snapshot(
                3,
                base + Duration::from_millis(32),
                base + Duration::from_millis(32),
                InputEpochs::default(),
            ),
        ];
        let histogram = frame_interval_histogram(&snapshots);
        assert_eq!(histogram.count(), 2, "3 frames -> 2 consecutive intervals");
        assert_eq!(histogram.p50(), Some(Duration::from_millis(16)));
    }

    /// Unsorted input (deliberately out of `frame_id` order) must still
    /// produce the same histogram as the sorted input -- proves the
    /// function sorts rather than assuming caller order.
    #[test]
    fn frame_interval_histogram_sorts_out_of_order_input() {
        let base = Instant::now();
        let in_order = vec![
            snapshot(1, base, base, InputEpochs::default()),
            snapshot(
                2,
                base + Duration::from_millis(16),
                base + Duration::from_millis(16),
                InputEpochs::default(),
            ),
        ];
        let mut shuffled = in_order.clone();
        shuffled.reverse();

        let expected = frame_interval_histogram(&in_order).p50();
        let actual = frame_interval_histogram(&shuffled).p50();
        assert_eq!(actual, expected);
    }

    /// A known produce-to-present span (segment/clock instant to submit
    /// instant) pins the exact bucket, not merely "a nonzero histogram".
    #[test]
    fn produce_to_present_histogram_pins_a_known_span() {
        let base = Instant::now();
        let snapshots = vec![snapshot(
            1,
            base,
            base + Duration::from_micros(4_500),
            InputEpochs::default(),
        )];
        let histogram = produce_to_present_histogram(&snapshots);
        assert_eq!(histogram.count(), 1);
        assert_eq!(histogram.p50(), Some(Duration::from_millis(8)));
    }

    /// Two coalesced inputs in one real, `FrameClock`-produced frame (driven
    /// through the clock's own public API -- `InputEpochs`' fields are
    /// private, so this crate has no way to hand-build one otherwise, the
    /// same constraint `flui-devtools`' own frame-telemetry tests document),
    /// at known arrival instants on a deterministic `ManualClock` -- the
    /// input-to-present histogram must see BOTH latencies (one histogram
    /// across every coalesced input, not one per frame only), each in its
    /// own correct bucket -- kills "only the first/last coalesced input is
    /// folded in" and "the wrong field is fed into the histogram".
    #[test]
    fn input_to_present_histogram_folds_in_every_coalesced_input_across_all_snapshots() {
        use crate::frame_clock::{ClockSource, DemandKind, FrameClock, PollDecision};
        use flui_foundation::{ManualClock, MonotonicClock};

        let manual = ManualClock::new();
        let clock = FrameClock::with_source(ClockSource::Manual(manual.clone()));

        clock.stamp_input_epoch(manual.now()); // arrival at t=0
        manual.advance(Duration::from_millis(10));
        clock.stamp_input_epoch(manual.now()); // arrival at t=10ms

        manual.advance(Duration::from_millis(10)); // now t=20ms
        clock.mark_demand(DemandKind::Dirty);
        let now = manual.now();
        assert_eq!(clock.poll(now), PollDecision::Produce);
        let submit_at = manual.now(); // t=20ms
        let snapshot = clock.record_frame(
            presentation_id(),
            now,
            now,
            now,
            submit_at,
            PresentOutcome::Presented,
        );

        let histogram = input_to_present_histogram(std::slice::from_ref(&snapshot));
        assert_eq!(
            histogram.count(),
            2,
            "both coalesced inputs must be folded in"
        );
        // First input's latency: submit(20ms) - arrival(0ms) = 20ms -> bucket bound 33_000us.
        // Second input's latency: submit(20ms) - arrival(10ms) = 10ms -> bucket bound 16_000us.
        assert_eq!(histogram.p50(), Some(Duration::from_millis(16)));
        assert_eq!(histogram.p99(), Some(Duration::from_millis(33)));
    }
}
