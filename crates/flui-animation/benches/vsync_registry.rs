//! Criterion benchmarks for [`Vsync`]'s registry resolution cost.
//!
//! These measure `tick_all`'s per-controller lookup cost in isolation — not
//! the animation math `tick_at` itself, which `animation_bench.rs`'s
//! `controller_tick` group already prices — across a growing resident
//! population, both stopped and running, plus the cost of tearing a whole
//! subtree's registrations down at once. Run with
//! `cargo bench -p flui-animation --bench vsync_registry`; the tables in
//! `docs/PERFORMANCE.md` are sourced from here, not estimated.

// Target-level lint relaxations — crate-level allows don't reach this
// target. `unwrap` in bench code: a panic IS the failure report
// (docs/PANIC-POLICY.md).
#![expect(clippy::unwrap_used)]

use std::hint::black_box;
use std::time::Duration;

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};

use flui_animation::{AnimationController, Vsync};

/// A registry of `count` stopped, never-started controllers: the walk's
/// lookup cost with no animation work at all, the shape the tables in
/// `docs/PERFORMANCE.md` compare against.
fn stopped_vsync_registry(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("stopped_vsync_registry");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(500));
    group.measurement_time(Duration::from_secs(1));
    for count in [100, 1_000, 5_000, 10_000] {
        let vsync = Vsync::new();
        let mut owners = Vec::with_capacity(count);
        for _ in 0..count {
            owners
                .push(AnimationController::builder(Duration::from_secs(1)).build_on(Some(&vsync)));
        }
        black_box(&owners);
        assert_eq!(vsync.len(), count);
        vsync.tick_all(
            &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.0)),
        );
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |bench, _| {
            bench.iter(|| {
                vsync.tick_all(
                    &flui_animation::MotionClock::new()
                        .frame(std::time::Duration::from_secs_f64(black_box(1.0))),
                );
            });
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
        let mut owners = Vec::with_capacity(count);
        for _ in 0..count {
            let owner =
                AnimationController::builder(Duration::from_secs(3600)).build_on(Some(&vsync));
            owner.controller().forward().unwrap();
            owners.push(owner);
        }
        black_box(&owners);
        assert_eq!(vsync.len(), count);
        vsync.tick_all(
            &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.0)),
        ); // anchor every run before the timed loop
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |bench, _| {
            bench.iter(|| {
                vsync.tick_all(
                    &flui_animation::MotionClock::new()
                        .frame(std::time::Duration::from_secs_f64(black_box(1.0))),
                );
            });
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
    let mut owners = Vec::with_capacity(COUNT as usize);
    for i in 0..COUNT {
        let owner = AnimationController::builder(Duration::from_secs(3600)).build_on(Some(&vsync));
        if i < RUNNING {
            owner.controller().forward().unwrap();
        }
        owners.push(owner);
    }
    black_box(&owners);
    assert_eq!(vsync.len(), COUNT as usize);
    vsync.tick_all(
        &flui_animation::MotionClock::new().frame(std::time::Duration::from_secs_f64(0.0)),
    );
    group.bench_function(COUNT.to_string(), |bench| {
        bench.iter(|| {
            vsync.tick_all(
                &flui_animation::MotionClock::new()
                    .frame(std::time::Duration::from_secs_f64(black_box(1.0))),
            );
        });
    });
    group.finish();
}

/// Teardown through the production owner: each dispose withdraws its registry
/// seat and retires its controller. Setup is untimed. Extra observer clones
/// keep controller allocation alive until the returned batch is dropped after
/// timing; owner retirement itself remains part of the measured operation.
/// LargeInput bounds the number of full registries constructed simultaneously.
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
                    let mut retained = Vec::with_capacity(count);
                    let registrations: Vec<_> = (0..count)
                        .map(|_| {
                            let owner = AnimationController::builder(Duration::from_secs(1))
                                .build_on(Some(&vsync));
                            retained.push(owner.controller().clone());
                            owner
                        })
                        .collect();
                    (vsync, registrations, retained)
                },
                |(vsync, registrations, retained)| {
                    for mut owner in registrations {
                        black_box(&mut owner).dispose();
                    }
                    // Moved out, not dropped here: see the doc comment above.
                    (vsync, retained)
                },
                BatchSize::LargeInput,
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
