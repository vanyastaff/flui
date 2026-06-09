//! VelocityTracker benchmarks
//!
//! Hot path: `VelocityTracker::estimate` is called by `DragGestureRecognizer`
//! on every `pointerup` / `pointercancel` to seed the fling animation. The
//! algorithm walks a 20-slot circular buffer and runs a least-squares
//! quadratic fit on the surviving samples; cost is O(N) where N ≤ 20.
//!
//! Performance targets (per `docs/testing.md` and the constitution's "60 fps
//! / 16 ms frame" budget):
//! - `estimate` on a full 20-sample buffer: < 5 µs (12.5% of one frame is
//!   already a lot; 5 µs is comfortable headroom for the rest of drag-end).
//! - `add_position` push: < 100 ns (one slot write; must not allocate).
//!
//! Follows the workspace benchmark template at
//! `rust-studio/.../templates/benchmark-report.md` (Setup / Workload /
//! Results / Profile Notes / Interpretation / Decision).
//!
//! Run with `cargo bench -p flui-interaction --bench velocity_tracker_bench`.

use std::hint::black_box;
use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};
use flui_interaction::processing::{
    IosFlingVelocityTracker, VelocityEstimationStrategy, VelocityTracker,
};
use flui_types::geometry::{Offset, Pixels};
use flui_types::gestures::PointerDeviceKind;

/// Build a deterministic linear swipe: `samples` positions equally spaced
/// over `duration_ms`, with `dx` advancing `slope_px_per_s` per second.
fn linear_swipe(
    samples: usize,
    duration_ms: u64,
    slope_px_per_s: f32,
) -> Vec<(Instant, Offset<Pixels>)> {
    let start = Instant::now();
    let dt = Duration::from_millis(duration_ms / samples as u64);
    (0..samples)
        .map(|i| {
            let t = start + dt * i as u32;
            let x = slope_px_per_s * (i as f32 * dt.as_secs_f32());
            (t, Offset::new(Pixels(x), Pixels(0.0)))
        })
        .collect()
}

/// Benchmark `VelocityTracker::estimate` on a full 20-sample buffer.
///
/// One iteration feeds 20 samples then calls `estimate()`. This is the
/// realistic drag-end cost: the recogniser's `on_end` callback computes
/// the fling velocity from the last ~100 ms of motion.
fn bench_estimate_lsq(c: &mut Criterion) {
    let samples = black_box(linear_swipe(20, 100, 1000.0));
    c.bench_function("VelocityTracker::estimate (LSQ, 20 samples)", |b| {
        b.iter(|| {
            let mut tracker = VelocityTracker::with_kind(PointerDeviceKind::Touch);
            for (t, p) in &samples {
                tracker.add_position(*t, *p);
            }
            black_box(tracker.estimate())
        });
    });
}

/// Benchmark the empty / 3-sample case — drag-end after a quick flick that
/// did not accumulate 100 ms of history. `estimate()` must short-circuit
/// early via `MIN_SAMPLE_SIZE` and report zero.
fn bench_estimate_short(c: &mut Criterion) {
    let samples = black_box(linear_swipe(3, 30, 500.0));
    c.bench_function("VelocityTracker::estimate (LSQ, 3 samples)", |b| {
        b.iter(|| {
            let mut tracker = VelocityTracker::with_kind(PointerDeviceKind::Touch);
            for (t, p) in &samples {
                tracker.add_position(*t, *p);
            }
            black_box(tracker.estimate())
        });
    });
}

/// `add_position` push cost. Called once per pointer-move event on the
/// drag hot path. Target: < 100 ns per push, zero allocations.
fn bench_add_position(c: &mut Criterion) {
    let samples = black_box(linear_swipe(20, 100, 1000.0));
    c.bench_function("VelocityTracker::add_position (push)", |b| {
        b.iter(|| {
            let mut tracker = VelocityTracker::with_kind(PointerDeviceKind::Touch);
            for (t, p) in &samples {
                tracker.add_position(*t, *p);
            }
            black_box(tracker.sample_count())
        });
    });
}

/// `with_strategy` constructor is the source-compat entry point used by
/// callers that picked `VelocityEstimationStrategy` in the legacy API.
/// All three variants route to the L2S tracker (the L2S algorithm is
/// strictly better), so the cost should be allocation-free and ~identical
/// across variants.
fn bench_with_strategy(c: &mut Criterion) {
    let mut group = c.benchmark_group("VelocityTracker::with_strategy");
    for strategy in [
        VelocityEstimationStrategy::LeastSquaresPolynomial,
        VelocityEstimationStrategy::LinearRegression,
        VelocityEstimationStrategy::TwoSample,
    ] {
        let label = format!("{strategy:?}");
        group.bench_function(label, |b| {
            b.iter(|| black_box(VelocityTracker::with_strategy(black_box(strategy))));
        });
    }
    group.finish();
}

/// iOS-flavour tracker (weighted 2-point velocity). Same input workload
/// as the LSQ case so the two benches are directly comparable. The iOS
/// flavour should be faster than the LSQ fit (3 multiplies vs 20-sample
/// matrix solve) but reports lower-quality velocity for non-linear
/// motion.
fn bench_ios_estimate(c: &mut Criterion) {
    let samples = black_box(linear_swipe(20, 100, 1000.0));
    c.bench_function("IosFlingVelocityTracker::estimate (20 samples)", |b| {
        b.iter(|| {
            let mut tracker = IosFlingVelocityTracker::with_kind(PointerDeviceKind::Touch);
            for (t, p) in &samples {
                tracker.add_position(*t, *p);
            }
            black_box(tracker.estimate())
        });
    });
}

criterion_group!(
    velocity_benches,
    bench_estimate_lsq,
    bench_estimate_short,
    bench_add_position,
    bench_with_strategy,
    bench_ios_estimate,
);
criterion_main!(velocity_benches);
