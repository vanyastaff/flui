//! Engine benchmarks (placeholder for future benchmarks)

use criterion::{criterion_group, criterion_main, Criterion};

fn vertex_benchmarks(c: &mut Criterion) {
    c.bench_function("rect_instance_create", |b| {
        b.iter(|| {
            flui_engine::vertex::RectInstance::rect(
                [10.0, 20.0, 100.0, 50.0],
                [1.0, 0.0, 0.0, 1.0],
            )
        });
    });
}

criterion_group!(benches, vertex_benchmarks);
criterion_main!(benches);
