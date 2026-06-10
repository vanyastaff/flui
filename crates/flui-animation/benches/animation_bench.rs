//! Criterion benchmarks for flui-animation hot paths.
//!
//! These measure the paths that run every frame: tween interpolation, curve
//! evaluation, spring stepping, and the controller tick. Run with
//! `cargo bench -p flui-animation`; the numbers in `docs/PERFORMANCE.md` should
//! be sourced from here, not estimated.

// Benchmark harness functions are internal measurement scaffolding, not a
// public API surface, so they are exempt from the crate's missing-docs lint.
#![allow(missing_docs)]

use std::hint::black_box;
use std::sync::Arc;
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};

use flui_animation::smoothing::{SmoothDamp, exp_decay_half_life};
use flui_animation::{
    Animatable, AnimatedValue, Animation, AnimationController, ColorTween, Curve, CurvedAnimation,
    Curves, FloatTween, OklabColorTween, Simulation, SpringDescription, SpringSimulation, Tween,
};
use flui_scheduler::Scheduler;
use flui_types::geometry::{Offset, px};
use flui_types::styling::Color;

fn tween_transform(c: &mut Criterion) {
    let mut group = c.benchmark_group("tween_transform");

    let f = FloatTween::new(0.0, 100.0);
    group.bench_function("f32", |b| {
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

    let off = Tween::new(
        Offset::new(px(0.0), px(0.0)),
        Offset::new(px(100.0), px(200.0)),
    );
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

    let mut damp = SmoothDamp::new(0.2);
    let mut pos = 0.0_f32;
    group.bench_function("smooth_damp_step", |b| {
        b.iter(|| {
            pos = damp.step(black_box(pos), black_box(100.0), black_box(1.0 / 120.0));
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
            let t = black_box(0.1_f32);
            black_box((sim.x(t), sim.dx(t)))
        });
    });

    // Per-component color spring: advance one frame and read the value.
    let mut value = AnimatedValue::new(Color::rgba(0, 0, 0, 255), SpringDescription::smooth());
    value.animate_to(Color::rgba(255, 128, 0, 255));
    group.bench_function("animated_value_color_frame", |b| {
        b.iter(|| {
            value.advance(black_box(1.0 / 60.0));
            black_box(value.value())
        });
    });

    group.finish();
}

fn controller_tick(c: &mut Criterion) {
    let mut group = c.benchmark_group("controller");

    let scheduler = Arc::new(Scheduler::new());
    let controller = AnimationController::new(Duration::from_millis(300), scheduler);
    controller.forward().unwrap();
    let mut t = 0.0_f64;
    group.bench_function("tick_at", |b| {
        b.iter(|| {
            t += 1.0 / 60.0;
            controller.tick_at(black_box(t));
        });
    });

    // Reading a curved combinator's value goes through one Arc<dyn> hop.
    let parent: Arc<dyn Animation<f32>> = Arc::new(controller.clone());
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
    smoothing_step,
    spring_step,
    controller_tick
);
criterion_main!(benches);
