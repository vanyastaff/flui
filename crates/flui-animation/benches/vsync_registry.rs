//! Criterion benchmarks for [`Vsync`]'s registry resolution cost.
//!
//! These measure a presentation clock and `tick_all` across a growing resident
//! population, both stopped and running. Active frames include controller
//! sampling and delivery; `animation_bench.rs` also isolates one controller.
//! They also measure the cost of tearing a whole
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

use flui_animation::{Animation, AnimationController, MotionClock, Vsync};

const FRAME: Duration = Duration::from_nanos(16_666_667);
// The same 136-year run used by the single-controller benchmarks. Criterion
// cannot finish it by advancing one virtual frame per measured iteration.
const NEVER_ENDING: Duration = Duration::from_secs(u32::MAX as u64);

fn registry_frame(vsync: &Vsync, clock: &mut MotionClock, raw: &mut Duration) {
    *raw = raw
        .checked_add(FRAME)
        .expect("benchmark timeline exhausted");
    vsync.tick_all(&clock.frame(black_box(*raw)));
}

fn assert_live_progress(
    vsync: &Vsync,
    clock: &mut MotionClock,
    raw: &mut Duration,
    controller: &AnimationController,
) {
    registry_frame(vsync, clock, raw);
    let before = controller.value();
    registry_frame(vsync, clock, raw);
    assert!(
        controller.value() > before,
        "a registry benchmark must sample new animation time on every iteration"
    );
}

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
        let mut clock = MotionClock::new();
        let mut raw = Duration::ZERO;
        vsync.tick_all(&clock.frame(raw));
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |bench, _| {
            bench.iter(|| {
                registry_frame(&vsync, &mut clock, &mut raw);
            });
        });
    }
    group.finish();
}

/// Every controller running a long forward leg that never completes during
/// the sample window, sampled at a new 60 Hz timestamp on every iteration.
/// Admission and progress checks happen outside the measured loop.
fn running_vsync_registry(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("running_vsync_registry");
    group.sample_size(10);
    group.warm_up_time(Duration::from_millis(500));
    group.measurement_time(Duration::from_secs(1));
    for count in [100, 1_000, 5_000, 10_000] {
        let vsync = Vsync::new();
        let mut owners = Vec::with_capacity(count);
        for _ in 0..count {
            let owner = AnimationController::builder(NEVER_ENDING).build_on(Some(&vsync));
            owner.controller().forward().unwrap();
            owners.push(owner);
        }
        black_box(&owners);
        assert_eq!(vsync.len(), count);
        let mut clock = MotionClock::new();
        let mut raw = Duration::ZERO;
        vsync.tick_all(&clock.frame(raw)); // anchor every run before the timed loop
        assert_live_progress(&vsync, &mut clock, &mut raw, owners[0].controller());
        let before = owners[0].controller().value();
        let before_raw = raw;
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |bench, _| {
            bench.iter(|| {
                registry_frame(&vsync, &mut clock, &mut raw);
            });
        });
        assert!(owners.iter().all(|owner| owner.controller().is_animating()));
        if raw > before_raw {
            assert!(
                owners[0].controller().value() > before,
                "the measured loop must advance its active controllers"
            );
        }
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
        let owner = AnimationController::builder(NEVER_ENDING).build_on(Some(&vsync));
        if i < RUNNING {
            owner.controller().forward().unwrap();
        }
        owners.push(owner);
    }
    black_box(&owners);
    assert_eq!(vsync.len(), COUNT as usize);
    let mut clock = MotionClock::new();
    let mut raw = Duration::ZERO;
    vsync.tick_all(&clock.frame(raw));
    assert_live_progress(&vsync, &mut clock, &mut raw, owners[0].controller());
    let before = owners[0].controller().value();
    let before_raw = raw;
    group.bench_function(COUNT.to_string(), |bench| {
        bench.iter(|| {
            registry_frame(&vsync, &mut clock, &mut raw);
        });
    });
    assert_eq!(
        owners
            .iter()
            .filter(|owner| owner.controller().is_animating())
            .count(),
        RUNNING as usize
    );
    if raw > before_raw {
        assert!(
            owners[0].controller().value() > before,
            "the measured loop must advance its active controllers"
        );
    }
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
