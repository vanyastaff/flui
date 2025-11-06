use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use flui_types::physics::{SpringDescription, SpringSimulation};
use flui_types::{Color, Matrix4, Point, Rect, Size};

fn bench_point_arithmetic(c: &mut Criterion) {
    let mut group = c.benchmark_group("point_arithmetic");

    group.bench_function("point_add", |b| {
        let p1 = Point::new(1.5, 2.5);
        let p2 = Point::new(3.5, 4.5);
        b.iter(|| black_box(p1 + p2));
    });

    group.bench_function("point_mul", |b| {
        let p = Point::new(1.5, 2.5);
        b.iter(|| black_box(p * 2.0));
    });

    group.bench_function("point_distance", |b| {
        let p1 = Point::new(0.0, 0.0);
        let p2 = Point::new(3.0, 4.0);
        b.iter(|| black_box(p1.distance_to(p2)));
    });

    group.finish();
}

fn bench_color_blending(c: &mut Criterion) {
    let mut group = c.benchmark_group("color_operations");

    group.bench_function("color_lerp", |b| {
        let c1 = Color::RED;
        let c2 = Color::BLUE;
        b.iter(|| black_box(Color::lerp(c1, c2, 0.5)));
    });

    group.bench_function("color_blend_over", |b| {
        let fg = Color::rgba(255, 0, 0, 128);
        let bg = Color::WHITE;
        b.iter(|| black_box(fg.blend_over(bg)));
    });

    group.bench_function("color_blend_batch", |b| {
        let colors = vec![
            Color::rgba(255, 0, 0, 128),
            Color::rgba(0, 255, 0, 128),
            Color::rgba(0, 0, 255, 128),
            Color::rgba(255, 255, 0, 128),
        ];
        let bg = Color::WHITE;
        b.iter(|| black_box(Color::blend_over_batch(&colors, bg)));
    });

    group.finish();
}

fn bench_matrix_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("matrix_operations");

    group.bench_function("matrix_multiply", |b| {
        let m1 = Matrix4::translation(10.0, 20.0, 0.0);
        let m2 = Matrix4::scaling(2.0, 2.0, 1.0);
        b.iter(|| black_box(m1 * m2));
    });

    group.bench_function("matrix_transform_point", |b| {
        let m = Matrix4::translation(10.0, 20.0, 0.0) * Matrix4::scaling(2.0, 2.0, 1.0);
        b.iter(|| black_box(m.transform_point(5.0, 5.0)));
    });

    group.bench_function("matrix_transform_points_batch", |b| {
        let m = Matrix4::translation(10.0, 20.0, 0.0);
        let points = vec![
            Point::new(0.0, 0.0),
            Point::new(1.0, 1.0),
            Point::new(2.0, 2.0),
            Point::new(3.0, 3.0),
            Point::new(4.0, 4.0),
            Point::new(5.0, 5.0),
            Point::new(6.0, 6.0),
            Point::new(7.0, 7.0),
        ];
        b.iter(|| black_box(m.transform_points(&points)));
    });

    group.bench_function("matrix_inverse", |b| {
        let m = Matrix4::translation(10.0, 20.0, 0.0) * Matrix4::scaling(2.0, 2.0, 1.0);
        b.iter(|| black_box(m.try_inverse()));
    });

    group.finish();
}

fn bench_rect_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("rect_operations");

    group.bench_function("rect_intersects", |b| {
        let r1 = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let r2 = Rect::from_xywh(50.0, 50.0, 100.0, 100.0);
        b.iter(|| black_box(r1.intersects(&r2)));
    });

    group.bench_function("rect_intersection", |b| {
        let r1 = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let r2 = Rect::from_xywh(50.0, 50.0, 100.0, 100.0);
        b.iter(|| black_box(r1.intersection(&r2)));
    });

    group.bench_function("rect_union", |b| {
        let r1 = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let r2 = Rect::from_xywh(50.0, 50.0, 100.0, 100.0);
        b.iter(|| black_box(r1.union(&r2)));
    });

    group.bench_function("rect_contains_point", |b| {
        let r = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let p = Point::new(50.0, 50.0);
        b.iter(|| black_box(r.contains(p)));
    });

    group.finish();
}

fn bench_physics_simulation(c: &mut Criterion) {
    let mut group = c.benchmark_group("physics_simulation");

    group.bench_function("spring_position", |b| {
        let spring = SpringDescription::new(1.0, 100.0, 10.0);
        let sim = SpringSimulation::new(spring, 0.0, 100.0, 0.0);
        b.iter(|| black_box(sim.position(0.1)));
    });

    group.bench_function("spring_velocity", |b| {
        let spring = SpringDescription::new(1.0, 100.0, 10.0);
        let sim = SpringSimulation::new(spring, 0.0, 100.0, 0.0);
        b.iter(|| black_box(sim.velocity(0.1)));
    });

    group.bench_function("spring_is_done", |b| {
        let spring = SpringDescription::new(1.0, 100.0, 10.0);
        let sim = SpringSimulation::new(spring, 0.0, 100.0, 0.0);
        b.iter(|| black_box(sim.is_done(0.1)));
    });

    group.finish();
}

fn bench_type_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("type_sizes");

    // Verify that types are small and Copy
    group.bench_function("verify_type_sizes", |b| {
        b.iter(|| {
            assert_eq!(std::mem::size_of::<Point>(), 8);
            assert_eq!(std::mem::size_of::<Size>(), 8);
            assert_eq!(std::mem::size_of::<Color>(), 4);
            assert_eq!(std::mem::size_of::<Rect>(), 16);
            assert_eq!(std::mem::size_of::<Matrix4>(), 64);

            // Verify Copy trait
            let p = Point::new(1.0, 2.0);
            let _p2 = p; // Copy
            let _p3 = p; // Still valid because of Copy

            let c = Color::RED;
            let _c2 = c;
            let _c3 = c;

            black_box(true)
        });
    });

    group.finish();
}

fn bench_const_evaluation(c: &mut Criterion) {
    let mut group = c.benchmark_group("const_evaluation");

    // These should be evaluated at compile time
    const IDENTITY: Matrix4 = Matrix4::identity();
    const RED: Color = Color::RED;
    const ZERO_POINT: Point = Point::ZERO;
    const ZERO_RECT: Rect = Rect::ZERO;

    group.bench_function("const_values", |b| {
        b.iter(|| {
            black_box(IDENTITY);
            black_box(RED);
            black_box(ZERO_POINT);
            black_box(ZERO_RECT);
        });
    });

    group.finish();
}

fn bench_batch_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_operations");

    // Compare single vs batch operations
    for size in [10, 50, 100, 500].iter() {
        let points: Vec<Point> = (0..*size).map(|i| Point::new(i as f32, i as f32)).collect();
        let m = Matrix4::translation(10.0, 20.0, 0.0);

        group.bench_with_input(BenchmarkId::new("transform_single", size), size, |b, _| {
            b.iter(|| {
                let mut result = Vec::with_capacity(points.len());
                for p in &points {
                    let (x, y) = m.transform_point(p.x, p.y);
                    result.push(Point::new(x, y));
                }
                black_box(result)
            });
        });

        group.bench_with_input(BenchmarkId::new("transform_batch", size), size, |b, _| {
            b.iter(|| black_box(m.transform_points(&points)));
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_point_arithmetic,
    bench_color_blending,
    bench_matrix_operations,
    bench_rect_operations,
    bench_physics_simulation,
    bench_type_sizes,
    bench_const_evaluation,
    bench_batch_operations
);

criterion_main!(benches);
