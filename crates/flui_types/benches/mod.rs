//! Benchmark harness for flui-types performance validation
//!
//! These benchmarks verify the performance contracts defined in
//! specs/001-flui-types/contracts/README.md:
//! - Point distance: <10ns
//! - Rectangle intersection: <20ns
//! - Color blending: <20ns
//!
//! Run with: cargo bench

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use flui_types::geometry::{px, Pixels, Point, Rect, Size};
use flui_types::styling::Color;

// ============================================================================
// Geometry Benchmarks
// ============================================================================

fn bench_point_distance(c: &mut Criterion) {
    let p1 = Point::new(px(100.0), px(200.0));
    let p2 = Point::new(px(300.0), px(400.0));

    c.bench_function("point_distance_to", |b| {
        b.iter(|| black_box(p1).distance_to(black_box(p2)))
    });
}

fn bench_rect_intersect(c: &mut Criterion) {
    let rect1 = Rect::from_ltwh(px(10.0), px(10.0), px(100.0), px(100.0));
    let rect2 = Rect::from_ltwh(px(50.0), px(50.0), px(100.0), px(100.0));

    c.bench_function("rect_intersect", |b| {
        b.iter(|| black_box(rect1).intersect(black_box(rect2)))
    });
}

fn bench_rect_union(c: &mut Criterion) {
    let rect1 = Rect::from_ltwh(px(10.0), px(10.0), px(100.0), px(100.0));
    let rect2 = Rect::from_ltwh(px(50.0), px(50.0), px(100.0), px(100.0));

    c.bench_function("rect_union", |b| {
        b.iter(|| black_box(rect1).union(black_box(rect2)))
    });
}

fn bench_rect_contains(c: &mut Criterion) {
    let rect = Rect::from_ltwh(px(0.0), px(0.0), px(100.0), px(100.0));
    let point = Point::new(px(50.0), px(50.0));

    c.bench_function("rect_contains", |b| {
        b.iter(|| black_box(rect).contains(black_box(point)))
    });
}

// ============================================================================
// Color Benchmarks
// ============================================================================

fn bench_color_mix(c: &mut Criterion) {
    let color1 = Color::from_rgba(255, 0, 0, 255);
    let color2 = Color::from_rgba(0, 0, 255, 255);

    c.bench_function("color_mix", |b| {
        b.iter(|| black_box(color1).mix(black_box(color2), black_box(0.5)))
    });
}

fn bench_color_blend_over(c: &mut Criterion) {
    let foreground = Color::from_rgba(255, 0, 0, 128);
    let background = Color::from_rgba(0, 0, 255, 255);

    c.bench_function("color_blend_over", |b| {
        b.iter(|| black_box(foreground).blend_over(black_box(background)))
    });
}

fn bench_color_lighten(c: &mut Criterion) {
    let color = Color::from_rgba(128, 64, 32, 255);

    c.bench_function("color_lighten", |b| {
        b.iter(|| black_box(color).lighten(black_box(0.2)))
    });
}

fn bench_color_from_hex(c: &mut Criterion) {
    c.bench_function("color_from_hex", |b| {
        b.iter(|| Color::from_hex(black_box("#FF5733")))
    });
}

// ============================================================================
// Unit Conversion Benchmarks
// ============================================================================

fn bench_pixels_to_device_pixels(c: &mut Criterion) {
    let pixels = px(100.0);
    let scale_factor = 2.0;

    c.bench_function("pixels_to_device_pixels", |b| {
        b.iter(|| black_box(pixels).to_device_pixels(black_box(scale_factor)))
    });
}

fn bench_point_to_device_pixels(c: &mut Criterion) {
    let point = Point::new(px(100.0), px(200.0));
    let scale_factor = 2.0;

    c.bench_function("point_to_device_pixels", |b| {
        b.iter(|| {
            Point::new(
                black_box(point).x.to_device_pixels(black_box(scale_factor)),
                black_box(point).y.to_device_pixels(black_box(scale_factor)),
            )
        })
    });
}

// ============================================================================
// Benchmark Groups
// ============================================================================

criterion_group!(
    geometry_benches,
    bench_point_distance,
    bench_rect_intersect,
    bench_rect_union,
    bench_rect_contains
);

criterion_group!(
    color_benches,
    bench_color_mix,
    bench_color_blend_over,
    bench_color_lighten,
    bench_color_from_hex
);

criterion_group!(
    conversion_benches,
    bench_pixels_to_device_pixels,
    bench_point_to_device_pixels
);

criterion_main!(geometry_benches, color_benches, conversion_benches);
