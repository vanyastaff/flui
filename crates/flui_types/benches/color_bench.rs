//! Color benchmarks for blending, mixing, and conversions
//!
//! Performance targets:
//! - Color::lerp (mix): <20ns
//! - Color::blend_over: <20ns

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use flui_types::styling::Color;

fn color_lerp_benchmark(c: &mut Criterion) {
    let c1 = Color::rgb(255, 0, 0);
    let c2 = Color::rgb(0, 0, 255);

    c.bench_function("Color::lerp", |b| {
        b.iter(|| {
            black_box(Color::lerp(black_box(c1), black_box(c2), black_box(0.5)))
        })
    });
}

fn color_blend_over_benchmark(c: &mut Criterion) {
    let foreground = Color::rgba(255, 0, 0, 128);
    let background = Color::rgb(0, 0, 255);

    c.bench_function("Color::blend_over", |b| {
        b.iter(|| {
            black_box(black_box(&foreground).blend_over(black_box(background)))
        })
    });
}

fn color_lighten_benchmark(c: &mut Criterion) {
    let color = Color::rgb(100, 150, 200);

    c.bench_function("Color::lighten", |b| {
        b.iter(|| {
            black_box(black_box(&color).lighten(black_box(0.2)))
        })
    });
}

fn color_darken_benchmark(c: &mut Criterion) {
    let color = Color::rgb(100, 150, 200);

    c.bench_function("Color::darken", |b| {
        b.iter(|| {
            black_box(black_box(&color).darken(black_box(0.2)))
        })
    });
}

fn color_with_alpha_benchmark(c: &mut Criterion) {
    let color = Color::rgb(100, 150, 200);

    c.bench_function("Color::with_alpha", |b| {
        b.iter(|| {
            black_box(black_box(&color).with_alpha(black_box(128)))
        })
    });
}

fn color_from_hex_benchmark(c: &mut Criterion) {
    c.bench_function("Color::from_hex", |b| {
        b.iter(|| {
            black_box(Color::from_hex(black_box("#FF5733")))
        })
    });
}

fn color_to_hex_benchmark(c: &mut Criterion) {
    let color = Color::rgb(255, 87, 51);

    c.bench_function("Color::to_hex", |b| {
        b.iter(|| {
            black_box(black_box(&color).to_hex())
        })
    });
}

fn color_premultiply_benchmark(c: &mut Criterion) {
    let color = Color::rgba(255, 128, 64, 128);

    c.bench_function("Color::premultiply", |b| {
        b.iter(|| {
            black_box(black_box(&color).premultiply())
        })
    });
}

criterion_group!(
    color_benches,
    color_lerp_benchmark,
    color_blend_over_benchmark,
    color_lighten_benchmark,
    color_darken_benchmark,
    color_with_alpha_benchmark,
    color_from_hex_benchmark,
    color_to_hex_benchmark,
    color_premultiply_benchmark
);

criterion_main!(color_benches);
