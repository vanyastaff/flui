//! Geometry benchmarks for Point, Rect, Size operations
//!
//! Performance targets:
//! - Point::distance: <10ns
//! - Rect::intersect: <20ns
//! - Rect::union: <20ns

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use flui_types::geometry::{Point, Rect, Size, Vec2, px};

fn point_distance_benchmark(c: &mut Criterion) {
    let p1 = Point::new(px(10.0), px(20.0));
    let p2 = Point::new(px(50.0), px(80.0));

    c.bench_function("Point::distance", |b| {
        b.iter(|| {
            black_box(p1.distance(black_box(p2)))
        })
    });
}

fn point_addition_benchmark(c: &mut Criterion) {
    let p = Point::new(px(10.0), px(20.0));
    let vec = Vec2::new(px(5.0), px(10.0));

    c.bench_function("Point + Vec2", |b| {
        b.iter(|| {
            black_box(p + vec)
        })
    });
}

fn rect_intersect_benchmark(c: &mut Criterion) {
    let rect1 = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));
    let rect2 = Rect::from_xywh(px(50.0), px(50.0), px(100.0), px(100.0));

    c.bench_function("Rect::intersect", |b| {
        b.iter(|| {
            black_box(black_box(&rect1).intersect(black_box(&rect2)))
        })
    });
}

fn rect_union_benchmark(c: &mut Criterion) {
    let rect1 = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));
    let rect2 = Rect::from_xywh(px(50.0), px(50.0), px(100.0), px(100.0));

    c.bench_function("Rect::union", |b| {
        b.iter(|| {
            black_box(black_box(&rect1).union(black_box(&rect2)))
        })
    });
}

fn rect_contains_benchmark(c: &mut Criterion) {
    let rect = Rect::from_xywh(px(0.0), px(0.0), px(100.0), px(100.0));
    let point = Point::new(px(50.0), px(50.0));

    c.bench_function("Rect::contains", |b| {
        b.iter(|| {
            black_box(black_box(&rect).contains(black_box(point)))
        })
    });
}

fn rect_inflate_benchmark(c: &mut Criterion) {
    let rect = Rect::from_xywh(px(10.0), px(10.0), px(80.0), px(80.0));

    c.bench_function("Rect::inflate", |b| {
        b.iter(|| {
            black_box(black_box(&rect).inflate(black_box(px(10.0)), black_box(px(10.0))))
        })
    });
}

fn size_area_benchmark(c: &mut Criterion) {
    let size = Size::new(px(100.0), px(200.0));

    c.bench_function("Size::area", |b| {
        b.iter(|| {
            black_box(black_box(&size).area())
        })
    });
}

fn rect_construction_benchmark(c: &mut Criterion) {
    c.bench_function("Rect::from_xywh", |b| {
        b.iter(|| {
            black_box(Rect::from_xywh(
                black_box(px(10.0)),
                black_box(px(20.0)),
                black_box(px(100.0)),
                black_box(px(200.0))
            ))
        })
    });
}

criterion_group!(
    geometry_benches,
    point_distance_benchmark,
    point_addition_benchmark,
    rect_intersect_benchmark,
    rect_union_benchmark,
    rect_contains_benchmark,
    rect_inflate_benchmark,
    size_area_benchmark,
    rect_construction_benchmark
);

criterion_main!(geometry_benches);
