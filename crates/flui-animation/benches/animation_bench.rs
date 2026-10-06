//! Criterion benchmarks for flui-animation hot paths.
//!
//! These measure the paths that run every frame: tween interpolation, curve
//! evaluation, spring stepping, and the controller tick. Run with
//! `cargo bench -p flui-animation`; the numbers in `docs/PERFORMANCE.md` should
//! be sourced from here, not estimated.

// Target-level lint relaxations — crate-level allows don't reach this
// target. `unwrap` in test/example code: a panic IS the failure report
// (docs/PANIC-POLICY.md); style items here are ship-wave debt.
#![expect(clippy::unwrap_used)]
// Benchmark harness functions are internal measurement scaffolding, not a
// public API surface, so they are exempt from the crate's missing-docs lint.

use std::hint::black_box;
use std::sync::Arc;
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};

use flui_animation::{
    Animatable, AnimatedValue, Animation, AnimationController, ColorTween, Curve, CurvedAnimation,
    Curves, FloatTween, OklabColorTween, Simulation, SpringDescription, SpringSimulation,
    Tolerance, Tween,
};
use flui_foundation::geometry::Offset;
use flui_painting::styling::Color;
use flui_scheduler::UpdateScheduler;

fn tween_transform(c: &mut Criterion) {
    let mut group = c.benchmark_group("tween_transform");

    let f = FloatTween::new(0.0, 100.0);
    group.bench_function("f64", |b| {
        b.iter(|| black_box(f.transform(black_box(0.37))));
    });

    let col = ColorTween::new(Color::rgba(0, 0, 0, 255), Color::rgba(255, 128, 0, 255));
    group.bench_function("color", |b| {
        b.iter(|| black_box(col.transform(black_box(0.37))));
    });

    // Perceptual color path: prices the two Oklab conversions (powf/cbrt per
    // channel) against the componentwise sRGB lerp above.
    let oklab = OklabColorTween::new(Color::rgba(0, 0, 255, 255), Color::rgba(255, 255, 0, 255));
    group.bench_function("color_oklab", |b| {
        b.iter(|| black_box(oklab.transform(black_box(0.37))));
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

fn spring_step(c: &mut Criterion) {
    let mut group = c.benchmark_group("spring");

    let spring =
        SpringDescription::with_response_and_damping(Duration::from_millis(300), 0.8).unwrap();
    let sim = SpringSimulation::try_new(spring, 0.0, 100.0, 0.0, Tolerance::DEFAULT).unwrap();
    group.bench_function("simulation_x_dx", |b| {
        b.iter(|| {
            let t = black_box(0.1_f64);
            black_box((sim.x(t), sim.dx(t)))
        });
    });

    // Construction includes the rest-time search.
    group.bench_function("simulation_new", |b| {
        b.iter(|| {
            black_box(
                SpringSimulation::try_new(
                    black_box(spring),
                    black_box(0.0),
                    black_box(100.0),
                    black_box(250.0),
                    Tolerance::DEFAULT,
                )
                .unwrap(),
            )
        });
    });

    // Per-component color spring: retarget, then one frame advance and read.
    let smooth =
        SpringDescription::with_duration_and_bounce(Duration::from_millis(500), 0.0).unwrap();
    let mut value = AnimatedValue::new(Color::rgba(0, 0, 0, 255), smooth).unwrap();
    group.bench_function("animated_value_color_frame", |b| {
        b.iter(|| {
            value.animate_to(Color::rgba(255, 128, 0, 255)).unwrap();
            value.advance(black_box(Duration::from_nanos(16_666_667)));
            black_box(value.value())
        });
    });

    group.finish();
}

fn controller_tick(c: &mut Criterion) {
    let mut group = c.benchmark_group("controller");

    let scheduler = UpdateScheduler::new();
    let controller = AnimationController::new(Duration::from_millis(300), &scheduler);
    controller.forward().unwrap();
    let mut t = 0.0_f64;
    group.bench_function("tick_at", |b| {
        b.iter(|| {
            t += 1.0 / 60.0;
            controller.tick_at(black_box(t));
        });
    });

    // Reading a curved combinator's value goes through one Arc<dyn> hop.
    let parent: Arc<dyn Animation<f64>> = Arc::new(controller.clone());
    let curved = CurvedAnimation::new(parent, Curves::EaseInOut);
    group.bench_function("curved_value", |b| {
        b.iter(|| black_box(curved.value()));
    });

    controller.dispose();
    group.finish();
}

criterion_group!(
    benches,
    tween_transform,
    curve_eval,
    spring_step,
    controller_tick
);
criterion_main!(benches);
