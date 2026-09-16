//! Criterion benchmarks for [`Vsync`]'s registry resolution cost (issue #1060).
//!
//! These measure `tick_all`'s per-controller lookup cost in isolation — not
//! the animation math `tick_at` itself, which `animation_bench.rs`'s
//! `controller_tick` group already prices — across a growing resident
//! population, both stopped and running, plus the cost of tearing a whole
//! subtree's registrations down at once. Run with
//! `cargo bench -p flui-animation --bench vsync_registry`; the tables in
//! `docs/PERFORMANCE.md` are sourced from here, not estimated.

// Target-level lint relaxations — crate-level allows don't reach this
// target. `unwrap` in test/example code: a panic IS the failure report
// (docs/PANIC-POLICY.md); style items here are ship-wave debt.
#![expect(clippy::unwrap_used)]

use std::hint::black_box;
use std::time::Duration;

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};

use flui_animation::{AnimationController, Vsync};

/// A registry of `count` stopped, never-started controllers — issue #1060's
/// own baseline shape, kept verbatim so the before/after tables line up with
/// the numbers already recorded against `main`.
fn stopped_vsync_registry(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("stopped_vsync_registry");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(500));
    group.measurement_time(Duration::from_secs(1));
    for count in [100, 1_000, 5_000, 10_000] {
        let vsync = Vsync::new();
        for _ in 0..count {
            vsync.register(AnimationController::with_detached_ticker(
                Duration::from_secs(1),
            ));
        }
        vsync.tick_all(0.0);
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |bench, _| {
            bench.iter(|| vsync.tick_all(black_box(1.0)));
        });
    }
    group.finish();
}

/// Every controller running a long forward leg that never completes during
/// the sample window: the walk cost when the per-controller bookkeeping
/// (generation check, anchor, running-status read) is on top of the lookup,
/// not skipped by it.
fn running_vsync_registry(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("running_vsync_registry");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(500));
    group.measurement_time(Duration::from_secs(1));
    for count in [100, 1_000] {
        let vsync = Vsync::new();
        for _ in 0..count {
            let controller = AnimationController::with_detached_ticker(Duration::from_secs(3600));
            controller.forward().unwrap();
            vsync.register(controller);
        }
        vsync.tick_all(0.0); // anchor every run before the timed loop
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |bench, _| {
            bench.iter(|| vsync.tick_all(black_box(1.0)));
        });
    }
    group.finish();
}

/// 10% running, 90% stopped at N = 1,000 — the realistic implicit-animation
/// mix (a scroll fling and a handful of open transitions among a much larger
/// resident population of already-settled controllers).
fn mixed_vsync_registry(criterion: &mut Criterion) {
    const COUNT: u32 = 1_000;
    const RUNNING: u32 = COUNT / 10;

    let mut group = criterion.benchmark_group("mixed_vsync_registry");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(500));
    group.measurement_time(Duration::from_secs(1));

    let vsync = Vsync::new();
    for i in 0..COUNT {
        let controller = AnimationController::with_detached_ticker(Duration::from_secs(3600));
        if i < RUNNING {
            controller.forward().unwrap();
        }
        vsync.register(controller);
    }
    vsync.tick_all(0.0);
    group.bench_function(COUNT.to_string(), |bench| {
        bench.iter(|| vsync.tick_all(black_box(1.0)));
    });
    group.finish();
}

/// Teardown of a subtree: `count` sequential `unregister` calls against a
/// freshly filled registry. `iter_batched` rebuilds the registry per
/// iteration (untimed setup) so the timed closure is only the removals —
/// this is the second quadratic the indexed registry removes (`retain` over
/// a linear store, once per removal, made the whole teardown O(N^2)).
fn unregister_all(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("unregister_all");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(500));
    group.measurement_time(Duration::from_secs(1));
    for count in [1_000, 10_000] {
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |bench, _| {
            bench.iter_batched(
                || {
                    let vsync = Vsync::new();
                    let registrations: Vec<_> = (0..count)
                        .map(|_| {
                            vsync.register(AnimationController::with_detached_ticker(
                                Duration::from_secs(1),
                            ))
                        })
                        .collect();
                    (vsync, registrations)
                },
                |(vsync, registrations)| {
                    for registration in registrations {
                        vsync.unregister(black_box(registration));
                    }
                },
                BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    stopped_vsync_registry,
    running_vsync_registry,
    mixed_vsync_registry,
    unregister_all
);
criterion_main!(benches);
