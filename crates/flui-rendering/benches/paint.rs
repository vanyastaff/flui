//! U1b — paint and compositing pipeline benchmarks.
//!
//! Measures `PipelineOwner::run_paint` and `run_compositing` over flat and
//! deep tree shapes. Setup runs layout (and layout+compositing for paint
//! benches) so each timed iteration measures only the phase under test.
//!
//! Run with:
//!   cargo bench -p flui-rendering --bench paint

mod helpers;

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

// ============================================================================
// run_compositing — flat tree, N nodes
// ============================================================================

fn bench_flat_run_compositing(c: &mut Criterion) {
    let mut group = c.benchmark_group("paint/compositing_flat");
    for &n in &[10_usize, 100, 1_000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || helpers::build_flat_compositing_ready(n),
                |mut owner| {
                    owner
                        .run_compositing()
                        .expect("run_compositing must succeed on a freshly laid-out flat tree");
                    black_box(owner)
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

// ============================================================================
// run_paint — flat tree, N nodes
// ============================================================================

fn bench_flat_run_paint(c: &mut Criterion) {
    let mut group = c.benchmark_group("paint/paint_flat");
    for &n in &[10_usize, 100, 1_000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || helpers::build_flat_paint_ready(n),
                |mut owner| {
                    owner
                        .run_paint()
                        .expect("run_paint must succeed on a freshly composited flat tree");
                    black_box(owner)
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

// ============================================================================
// run_paint — deep chain
// ============================================================================

fn bench_deep_run_paint(c: &mut Criterion) {
    let mut group = c.benchmark_group("paint/paint_deep");
    for &depth in &[10_usize, 100, 1_000] {
        group.bench_with_input(BenchmarkId::from_parameter(depth), &depth, |b, &depth| {
            b.iter_batched(
                || helpers::build_deep_paint_ready(depth),
                |mut owner| {
                    owner
                        .run_paint()
                        .expect("run_paint must succeed on a freshly composited deep chain");
                    black_box(owner)
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_flat_run_compositing,
    bench_flat_run_paint,
    bench_deep_run_paint,
);
criterion_main!(benches);
