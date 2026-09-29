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

use crate::frame_telemetry::{FrameSnapshot, PresentOutcome};

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
/// i.e. from the instant `poll` decided to produce to the instant the raster
/// backend returned from consuming the scene (present-inclusive on the
/// production backend — see [`FrameSnapshot::submit_at`]) — from a
/// `frames_since`-pulled slice.
///
/// **Only [`PresentOutcome::Presented`] snapshots are folded in.** An
/// `Errored` submit never reached the screen, so its span is not a
/// present latency; including it would report a number for a frame the user
/// never saw.
#[must_use]
pub fn produce_to_present_histogram(snapshots: &[FrameSnapshot]) -> LatencyHistogram {
    let mut histogram = LatencyHistogram::new();
    for snapshot in presented(snapshots) {
        let span = snapshot
            .submit_at
            .saturating_duration_since(snapshot.clock_timestamp);
        histogram.record(span);
    }
    histogram
}

/// The presented subset of `snapshots`.
///
/// Both present-latency histograms filter through this. A failed submit is
/// excluded for two independent reasons: nothing reached the screen, and a
/// *retryable* failure deliberately retains its input epochs so the retry can
/// attribute them (see `EpochDisposition::Retain`) — so folding the failed
/// attempt in would also count those same inputs a second time when the retry
/// succeeds.
fn presented(snapshots: &[FrameSnapshot]) -> impl Iterator<Item = &FrameSnapshot> {
    snapshots
        .iter()
        .filter(|snapshot| snapshot.present_outcome == PresentOutcome::Presented)
}

/// Build a [`LatencyHistogram`] of input-to-present latencies — every
/// coalesced input epoch's own `(submit_at - arrival)` value (see
/// [`FrameSnapshot::latencies`]), across every snapshot in `snapshots`,
/// folded into one histogram rather than kept per-frame.
///
/// **This tail is truncated whenever a snapshot's epoch ring overflowed,
/// and it is truncated at the end that matters.** The ring evicts its
/// OLDEST epochs, which are the ones with the largest `submit_at - arrival`
/// — so under the overflow condition that is routine during a drag (a
/// high-rate pointer stamps far more raw events per frame than the ring
/// holds), the high percentiles of this histogram are biased LOW, dropping
/// exactly the worst samples a p99 exists to surface. The condition is
/// detectable per snapshot: check [`crate::InputEpochs::overflowed`] before
/// treating this histogram's tail as complete. The frame-interval and
/// produce-to-present histograms are unaffected — they take one value per
/// frame and no ring bounds them.
///
/// **Only [`PresentOutcome::Presented`] snapshots are folded in**, for the
/// double-count reason as much as the never-presented one: a retryable failure
/// retains its input epochs so the retry can attribute them, so counting the
/// failed attempt would record those inputs once here and again when the retry
/// presents.
#[must_use]
pub fn input_to_present_histogram(snapshots: &[FrameSnapshot]) -> LatencyHistogram {
    let mut histogram = LatencyHistogram::new();
    for snapshot in presented(snapshots) {
        for (_, latency) in snapshot.latencies() {
            histogram.record(latency);
        }
    }
    histogram
}

#[cfg(test)]
mod tests {

    use super::*;

    // `LatencyHistogram` is `Copy`, so it cannot own a heap allocation in
    // safe Rust -- the compile-time half of this module's "allocation-free"
    // claim, checked mechanically rather than only asserted in prose.
    static_assertions::assert_impl_all!(LatencyHistogram: Copy);
}
