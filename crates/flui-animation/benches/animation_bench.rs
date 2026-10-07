//! Criterion benchmarks for flui-animation hot paths.
//!
//! These measure the paths that run every frame: tween interpolation, curve
//! evaluation, spring stepping, the controller tick and its listener fan-out,
//! and the cost of starting a run. Run with `cargo bench -p flui-animation`;
//! the numbers in `docs/PERFORMANCE.md` should be sourced from here, not
//! estimated.
//!
//! Every controller benchmark measures a controller that is genuinely mid-run:
//! a time-based run whose duration is far longer than any benchmark can
//! advance, or a simulation that cannot settle in that span. A completed run
//! makes `tick_at` an early return, which prices a lock and a branch rather
//! than a frame.

// Target-level lint relaxations — crate-level allows don't reach this
// target. `unwrap` in bench code: a panic IS the failure report
// (docs/PANIC-POLICY.md).
#![expect(clippy::unwrap_used)]
// Benchmark harness functions are internal measurement scaffolding, not a
// public API surface, so they are exempt from the crate's missing-docs lint.

use std::hint::black_box;
use std::sync::Arc;
use std::time::Duration;

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};

use flui_animation::smoothing::{SmoothDamp, exp_decay_half_life};
use flui_animation::{
    Animatable, AnimatedValue, Animation, AnimationController, ColorTween, Curve, CurvedAnimation,
    Curves, FloatTween, FrictionSimulation, Simulation, SpringDescription, SpringSimulation, Tween,
};
use flui_foundation::Listenable;
use flui_foundation::geometry::Offset;
use flui_painting::styling::Color;

/// One frame at 60 Hz, in seconds.
const FRAME: f64 = 1.0 / 60.0;

/// A run no benchmark can finish: criterion advances at most a few hundred
/// million frames (a few million seconds at 60 Hz), while this lasts ~136
/// years.
const NEVER_ENDING: Duration = Duration::from_secs(u32::MAX as u64);

fn tween_transform(c: &mut Criterion) {
    let mut group = c.benchmark_group("tween_transform");

    let f = FloatTween::new(0.0, 100.0);
    group.bench_function("f64", |b| {
        b.iter(|| black_box(f.transform(black_box(0.37))));
    });

    // Premultiplied Oklab: prices the two colour-space conversions (powf/cbrt per
    // channel) each interior frame pays.
    let col = ColorTween::new(Color::rgba(0, 0, 255, 255), Color::rgba(255, 255, 0, 128));
    group.bench_function("color", |b| {
        b.iter(|| black_box(col.transform(black_box(0.37))));
    });

    let off = Tween::new(Offset::new(0.0, 0.0), Offset::new(100.0, 200.0));
    group.bench_function("offset", |b| {
        b.iter(|| black_box(off.transform(black_box(0.37))));
    });

    group.finish();
}

fn curve_eval(c: &mut Criterion) {
    let mut group = c.benchmark_group("curve_eval");

    group.bench_function("linear", |b| {
        b.iter(|| black_box(Curves::Linear.transform(black_box(0.37))));
    });
    group.bench_function("ease_in_out", |b| {
        b.iter(|| black_box(Curves::EaseInOut.transform(black_box(0.37))));
    });
    group.bench_function("elastic_out", |b| {
        b.iter(|| black_box(Curves::ElasticOut.transform(black_box(0.37))));
    });
    // Two cubic segments + rescale arithmetic: the M3 emphasized default.
    group.bench_function("three_point_cubic_emphasized", |b| {
        b.iter(|| black_box(Curves::EaseInOutCubicEmphasized.transform(black_box(0.37))));
    });

    group.finish();
}

fn smoothing_step(c: &mut Criterion) {
    let mut group = c.benchmark_group("smoothing");

    group.bench_function("exp_decay_half_life", |b| {
        b.iter(|| {
            black_box(exp_decay_half_life(
                black_box(10.0),
                black_box(100.0),
                black_box(0.25),
                black_box(1.0 / 120.0),
            ))
        });
    });

    // The target flips every step, so the damper is always chasing a target
    // it is far from — a fixed target converges within a few hundred
    // iterations and leaves the benchmark pricing the at-rest regime.
    let mut damp = SmoothDamp::new(0.2);
    let mut pos = 0.0_f64;
    let mut target = 100.0_f64;
    group.bench_function("smooth_damp_step", |b| {
        b.iter(|| {
            target = 100.0 - target;
            pos = damp.step(black_box(pos), black_box(target), black_box(1.0 / 120.0));
            black_box(pos)
        });
    });

    group.finish();
}

fn spring_step(c: &mut Criterion) {
    let mut group = c.benchmark_group("spring");

    let sim = SpringSimulation::new(
        SpringDescription::with_response_and_damping(0.3, 0.8),
        0.0,
        100.0,
        0.0,
    );
    group.bench_function("simulation_x_dx", |b| {
        b.iter(|| {
            let t = black_box(0.1_f64);
            black_box((sim.x(t), sim.dx(t)))
        });
    });

    // Per-component color spring: the first frame after a retarget. Each
    // iteration starts from a freshly retargeted value, so the spring is
    // always in motion — advancing one shared value forever would price a
    // settled spring at an ever-growing elapsed time.
    group.bench_function("animated_value_color_frame", |b| {
        b.iter_batched(
            || {
                let mut value =
                    AnimatedValue::new(Color::rgba(0, 0, 0, 255), SpringDescription::smooth());
                value.animate_to(Color::rgba(255, 128, 0, 255));
                value
            },
            |mut value| {
                value.advance(black_box(FRAME));
                black_box(value.value());
                value
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

/// Register `value_listeners` no-op value listeners and `status_listeners`
/// no-op status listeners on `controller`.
fn add_listeners(
    controller: &AnimationController,
    value_listeners: usize,
    status_listeners: usize,
) {
    for _ in 0..value_listeners {
        controller.add_listener(Arc::new(|| {
            black_box(());
        }));
    }
    for _ in 0..status_listeners {
        controller.add_status_listener(Arc::new(|status| {
            black_box(status);
        }));
    }
}

/// Advance `controller` one 60 Hz frame per iteration, starting from `t = 0`.
fn bench_live_tick(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    variant: &str,
    controller: &AnimationController,
) {
    let mut t = 0.0_f64;
    group.bench_function(BenchmarkId::new("tick_at", variant), |b| {
        b.iter(|| {
            t += FRAME;
            controller.tick_at(black_box(t));
        });
    });
    assert!(
        controller.status().is_running(),
        "the {variant} run must still be live after the benchmark, or it priced a finished run"
    );
}

fn controller_tick(c: &mut Criterion) {
    let mut group = c.benchmark_group("controller");

    // Linear time-based run, no listeners.
    let linear = AnimationController::without_ticker(NEVER_ENDING);
    linear.forward().unwrap();
    bench_live_tick(&mut group, "linear", &linear);
    linear.dispose();

    // The same run eased through a cubic Bézier.
    let eased = AnimationController::without_ticker(NEVER_ENDING);
    eased
        .animate_to_curved(1.0, None, Arc::new(Curves::EaseInOut))
        .unwrap();
    bench_live_tick(&mut group, "ease_in_out", &eased);
    eased.dispose();

    // Value fan-out: every tick notifies the value listeners; the status
    // listener is registered but no tick changes the status.
    for value_listeners in [1, 4] {
        let controller = AnimationController::without_ticker(NEVER_ENDING);
        add_listeners(&controller, value_listeners, 1);
        controller.forward().unwrap();
        bench_live_tick(
            &mut group,
            &format!("{value_listeners}_value_1_status_listeners"),
            &controller,
        );
        controller.dispose();
    }

    // Simulation runs: a near-unit drag that takes ~2e10 s to slow below the
    // default velocity tolerance, and an undamped spring that never settles.
    let friction = AnimationController::unbounded_without_ticker(NEVER_ENDING);
    friction
        .animate_with(FrictionSimulation::new(1.0 - 1e-9, 0.0, 1000.0))
        .unwrap();
    bench_live_tick(&mut group, "simulation_friction", &friction);
    friction.dispose();

    let spring = AnimationController::unbounded_without_ticker(NEVER_ENDING);
    spring
        .animate_with(SpringSimulation::new(
            SpringDescription::new(1.0, 100.0, 0.0),
            0.0,
            1000.0,
            0.0,
        ))
        .unwrap();
    bench_live_tick(&mut group, "simulation_spring", &spring);
    spring.dispose();

    // Reading a curved combinator's value goes through one Arc<dyn> hop and
    // the curve; the parent sits mid-run so the curve is actually evaluated.
    let parent_controller = AnimationController::without_ticker(Duration::from_secs(1));
    parent_controller.forward().unwrap();
    parent_controller.tick_at(0.37);
    let parent: Arc<dyn Animation<f64>> = Arc::new(parent_controller.clone());
    let curved = CurvedAnimation::new(parent, Curves::EaseInOut);
    group.bench_function("curved_value", |b| {
        b.iter(|| black_box(curved.value()));
    });
    assert!(parent_controller.status().is_running());
    drop(curved);
    parent_controller.dispose();

    group.finish();
}

fn controller_status(c: &mut Criterion) {
    let mut group = c.benchmark_group("controller");

    // One iteration is two status transitions, each fanned out to every
    // listener: `forward_from(0)` (Completed -> Forward) and a tick past the
    // end (Forward -> Completed).
    for status_listeners in [1, 4, 8] {
        let controller = AnimationController::without_ticker(Duration::from_secs(1));
        add_listeners(&controller, 0, status_listeners);
        group.bench_function(BenchmarkId::new("status_fan_out", status_listeners), |b| {
            b.iter(|| {
                black_box(controller.forward_from(Some(0.0)).unwrap());
                controller.tick_at(black_box(2.0));
            });
        });
        controller.dispose();
    }

    // Starting a run from rest on a fresh controller with a detached ticker
    // (the `is_animating`-reporting widget shape). Construction and the
    // returned future's drop sit outside the timed region.
    group.bench_function("forward", |b| {
        b.iter_batched(
            || AnimationController::with_detached_ticker(NEVER_ENDING),
            |controller| {
                let run = controller.forward().unwrap();
                (controller, run)
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

criterion_group!(
    benches,
    tween_transform,
    curve_eval,
    smoothing_step,
    spring_step,
    controller_tick,
    controller_status
);
criterion_main!(benches);
