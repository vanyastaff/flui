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
use flui_interaction::processing::{IosFlingVelocityTracker, VelocityTracker};
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
/// The tracker is filled in the (untimed) `iter_batched` setup so the timed
/// region is ONLY `estimate()` — the cost the bench name claims. (Per-move
/// fill cost is measured separately by `bench_add_position`.)
fn bench_estimate_lsq(c: &mut Criterion) {
    let samples = black_box(linear_swipe(20, 100, 1000.0));
    c.bench_function("VelocityTracker::estimate (LSQ, 20 samples)", |b| {
        b.iter_batched(
            || {
                let mut tracker = VelocityTracker::with_kind(PointerDeviceKind::Touch);
                for (t, p) in &samples {
                    tracker.add_position(*t, *p);
                }
                tracker
            },
            |tracker| black_box(tracker.estimate()),
            criterion::BatchSize::SmallInput,
        );
    });
}

/// Benchmark the 3-sample case — a quick flick that did not accumulate 100 ms
/// of history. The fill happens in (untimed) setup so only `estimate()` is
/// measured.
fn bench_estimate_short(c: &mut Criterion) {
    let samples = black_box(linear_swipe(3, 30, 500.0));
    c.bench_function("VelocityTracker::estimate (LSQ, 3 samples)", |b| {
        b.iter_batched(
            || {
                let mut tracker = VelocityTracker::with_kind(PointerDeviceKind::Touch);
                for (t, p) in &samples {
                    tracker.add_position(*t, *p);
                }
                tracker
            },
            |tracker| black_box(tracker.estimate()),
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
            let mut tracker = VelocityTracker::with_kind(PointerDeviceKind::Touch);
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
    c.bench_function("IosFlingVelocityTracker::estimate (20 samples)", |b| {
        b.iter_batched(
            || {
                let mut tracker = IosFlingVelocityTracker::with_kind(PointerDeviceKind::Touch);
                for (t, p) in &samples {
                    tracker.add_position(*t, *p);
                }
                tracker
            },
            |tracker| black_box(tracker.estimate()),
            criterion::BatchSize::SmallInput,
        );
    });
}

criterion_group!(
    velocity_benches,
    bench_estimate_lsq,
    bench_estimate_short,
    bench_add_position,
    bench_ios_estimate,
);
criterion_main!(velocity_benches);
