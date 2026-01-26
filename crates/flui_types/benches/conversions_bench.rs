//! Unit conversion benchmarks
//!
//! Performance targets: Conversions should be zero-cost or near-zero-cost

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use flui_types::geometry::{px, device_px, Pixels};

fn pixels_to_device_pixels_benchmark(c: &mut Criterion) {
    let logical = px(100.0);
    let scale = 2.0;

    c.bench_function("Pixels::to_device_pixels", |b| {
        b.iter(|| {
            black_box(black_box(logical).to_device_pixels(black_box(scale)))
        })
    });
}

fn device_pixels_to_pixels_benchmark(c: &mut Criterion) {
    let device = device_px(200);
    let scale = 2.0;

    c.bench_function("DevicePixels::to_pixels", |b| {
        b.iter(|| {
            black_box(black_box(device).to_pixels(black_box(scale)))
        })
    });
}

fn pixels_scale_benchmark(c: &mut Criterion) {
    let value = px(100.0);
    let factor = 1.5;

    c.bench_function("Pixels::scale", |b| {
        b.iter(|| {
            black_box(black_box(value).scale(black_box(factor)))
        })
    });
}

fn pixels_arithmetic_benchmark(c: &mut Criterion) {
    let a = px(100.0);
    let b = px(50.0);

    c.bench_function("Pixels addition", |b| {
        b.iter(|| {
            black_box(black_box(a) + black_box(b))
        })
    });
}

fn pixels_comparison_benchmark(c: &mut Criterion) {
    let a = px(100.0);
    let b = px(50.0);

    c.bench_function("Pixels comparison", |b| {
        b.iter(|| {
            black_box(black_box(a) > black_box(b))
        })
    });
}

fn pixels_min_max_benchmark(c: &mut Criterion) {
    let a = px(100.0);
    let b = px(50.0);

    c.bench_function("Pixels::max", |b| {
        b.iter(|| {
            black_box(black_box(a).max(black_box(b)))
        })
    });
}

criterion_group!(
    conversion_benches,
    pixels_to_device_pixels_benchmark,
    device_pixels_to_pixels_benchmark,
    pixels_scale_benchmark,
    pixels_arithmetic_benchmark,
    pixels_comparison_benchmark,
    pixels_min_max_benchmark
);

criterion_main!(conversion_benches);
