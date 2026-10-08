//! VelocityTracker benchmarks
//!
//! Hot path: `VelocityTracker::estimate` is called by `DragGestureRecognizer`
//! on every `pointerup` / `pointercancel` to seed the fling animation. The
//! algorithm walks a 20-slot circular buffer and runs a least-squares
//! quadratic fit on the surviving samples; cost is O(N) where N ≤ 20.
//!
//! Performance targets (per `docs/testing.md` and the constitution's "60 fps
//! / 16 ms frame" budget):
//! - `estimate` on a full 20-sample buffer: < 5 µs (about 0.03% of a 16 ms
//!   frame; this is a target, not a measured frame-time guarantee).
//! - `add_position` push: < 100 ns (one slot write; must not allocate).
//!
//! Follows the workspace benchmark template at
//! `rust-studio/.../templates/benchmark-report.md` (Setup / Workload /
//! Results / Profile Notes / Interpretation / Decision).
//!
//! Run with `cargo bench -p flui-interaction --bench velocity_tracker_bench`.

// Bench harness, not public API; `criterion_group!` generates the
// undocumentable entry fn.

use std::hint::black_box;
use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};
use flui_foundation::geometry::Offset;
use flui_interaction::PointerKind;
use flui_interaction::processing::{OneEuroFilter2D, VelocityEstimator, VelocityTracker};

/// Build a deterministic linear swipe: `samples` positions equally spaced
/// over `duration_ms`, with `dx` advancing `slope_px_per_s` per second.
fn linear_swipe(
    samples: usize,
    duration_ms: u64,
    slope_px_per_s: f64,
) -> Vec<(Instant, Offset<f64>)> {
    let start = Instant::now();
    let dt = Duration::from_millis(duration_ms / samples as u64);
    (0..samples)
        .map(|i| {
            let t = start + dt * i as u32;
            let x = slope_px_per_s * (i as f64 * dt.as_secs_f64());
            (t, Offset::new(x, 0.0))
        })
        .collect()
}

/// Benchmark `VelocityTracker::estimate` on a full 20-sample buffer.
///
/// The tracker is filled in the (untimed) `iter_batched` setup so the timed
/// region is ONLY `estimate()` — the cost the bench name claims. (Per-move
/// fill cost is measured separately by `bench_add_position`.)
fn bench_estimate_lsq(c: &mut Criterion) {
    let samples = black_box(linear_swipe(20, 100, 1000.0));
    // Query on the samples' own clock, right after the newest one, so a
    // slow batch setup can never trip the stop gate and time a no-op.
    let query = samples.last().expect("non-empty swipe").0;
    c.bench_function("VelocityTracker::estimate (LSQ, 20 samples)", |b| {
        b.iter_batched(
            || {
                let mut tracker = VelocityTracker::with_kind(PointerKind::Touch);
                for (t, p) in &samples {
                    tracker.add_position(*t, *p);
                }
                tracker
            },
            |mut tracker| black_box(tracker.estimate_at(query)),
            criterion::BatchSize::SmallInput,
        );
    });
}

/// Benchmark the 3-sample case — a quick flick that did not accumulate 100 ms
/// of history. The fill happens in (untimed) setup so only `estimate()` is
/// measured.
fn bench_estimate_short(c: &mut Criterion) {
    let samples = black_box(linear_swipe(3, 30, 500.0));
    // Query on the samples' own clock, right after the newest one, so a
    // slow batch setup can never trip the stop gate and time a no-op.
    let query = samples.last().expect("non-empty swipe").0;
    c.bench_function("VelocityTracker::estimate (LSQ, 3 samples)", |b| {
        b.iter_batched(
            || {
                let mut tracker = VelocityTracker::with_kind(PointerKind::Touch);
                for (t, p) in &samples {
                    tracker.add_position(*t, *p);
                }
                tracker
            },
            |mut tracker| black_box(tracker.estimate_at(query)),
            criterion::BatchSize::SmallInput,
        );
    });
}

/// Benchmark repeated `estimate()` queries on an unchanged buffer — the
/// pattern on the drag-end path, where a single frame asks for
/// the velocity and the estimate (and any future code may re-query). With the
/// estimate memoized, only the first call per unchanged buffer runs the O(N)
/// QR solve; the rest are cache hits. The fill happens in (untimed) setup, so
/// the timed region is four back-to-back `estimate()` calls with no
/// intervening `add_position`.
fn bench_estimate_repeated(c: &mut Criterion) {
    let samples = black_box(linear_swipe(20, 100, 1000.0));
    // Query on the samples' own clock, right after the newest one, so a
    // slow batch setup can never trip the stop gate and time a no-op.
    let query = samples.last().expect("non-empty swipe").0;
    c.bench_function("VelocityTracker::estimate (LSQ, 4 repeated queries)", |b| {
        b.iter_batched(
            || {
                let mut tracker = VelocityTracker::with_kind(PointerKind::Touch);
                for (t, p) in &samples {
                    tracker.add_position(*t, *p);
                }
                tracker
            },
            |mut tracker| {
                // Four queries against the same buffer: 1 solve + 3 cache hits.
                black_box(tracker.estimate_at(query));
                black_box(tracker.estimate_at(query));
                black_box(tracker.estimate_at(query));
                black_box(tracker.estimate_at(query))
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

/// `add_position` push cost. Called once per pointer-move event on the
/// drag hot path. Target: < 100 ns per push, zero allocations.
fn bench_add_position(c: &mut Criterion) {
    let samples = black_box(linear_swipe(20, 100, 1000.0));
    c.bench_function("VelocityTracker::add_position (push)", |b| {
        b.iter(|| {
            let mut tracker = VelocityTracker::with_kind(PointerKind::Touch);
            for (t, p) in &samples {
                tracker.add_position(*t, *p);
            }
            black_box(tracker.sample_count())
        });
    });
}

/// iOS-flavour tracker (weighted 2-point velocity). Same input workload
/// as the LSQ case so the two benches are directly comparable. The iOS
/// flavour should be faster than the LSQ fit (3 multiplies vs 20-sample
/// matrix solve) but reports lower-quality velocity for non-linear
/// motion.
fn bench_ios_estimate(c: &mut Criterion) {
    let samples = black_box(linear_swipe(20, 100, 1000.0));
    let query = samples.last().expect("non-empty swipe").0;
    c.bench_function("VelocityTracker::estimate Ios (20 samples)", |b| {
        b.iter_batched(
            || {
                let mut tracker =
                    VelocityTracker::with_estimator(PointerKind::Touch, VelocityEstimator::Ios);
                for (t, p) in &samples {
                    tracker.add_position(*t, *p);
                }
                tracker
            },
            |mut tracker| black_box(tracker.estimate_at(query)),
            criterion::BatchSize::SmallInput,
        );
    });
}

/// Benchmark the AOSP impulse strategy on the same full 20-sample buffer as
/// the LSQ bench, so the two strategies price against each other directly.
fn bench_estimate_impulse(c: &mut Criterion) {
    let samples = black_box(linear_swipe(20, 100, 1000.0));
    let query = samples.last().expect("non-empty swipe").0;
    c.bench_function("VelocityTracker::estimate Impulse (20 samples)", |b| {
        b.iter_batched(
            || {
                let mut tracker =
                    VelocityTracker::with_estimator(PointerKind::Touch, VelocityEstimator::Impulse);
                for (t, p) in &samples {
                    tracker.add_position(*t, *p);
                }
                tracker
            },
            |mut tracker| black_box(tracker.estimate_at(query)),
            criterion::BatchSize::SmallInput,
        );
    });
}

/// Benchmark one One-Euro filter step — the per-pointer-move cost when the
/// filter sits in the input path (must be well under the 100 ns slot-write
/// budget class, it is pure arithmetic).
fn bench_one_euro_step(c: &mut Criterion) {
    let start = Instant::now();
    let mut filter = OneEuroFilter2D::default();
    let mut i = 0u32;
    c.bench_function("OneEuroFilter2D::filter (per move)", |b| {
        b.iter(|| {
            i = i.wrapping_add(1);
            let t = start + Duration::from_millis(8) * i;
            let p = Offset::new(i as f64 * 0.5, 50.0);
            black_box(filter.filter(black_box(t), black_box(p)))
        });
    });
}

criterion_group!(
    velocity_benches,
    bench_estimate_lsq,
    bench_estimate_short,
    bench_estimate_repeated,
    bench_add_position,
    bench_ios_estimate,
    bench_estimate_impulse,
    bench_one_euro_step,
);
criterion_main!(velocity_benches);
