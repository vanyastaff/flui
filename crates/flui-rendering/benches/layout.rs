//! U1a — layout pipeline benchmark.
//!
//! Measures `PipelineOwner::run_layout` and `layout_dirty_root` over three
//! canonical tree shapes. Each benchmark builds a fresh tree in `setup` (not
//! counted) then times only the work that happens on every frame.
//!
//! Run with:
//!   cargo bench -p flui-rendering --bench layout

mod helpers;

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use flui_rendering::constraints::BoxConstraints;
use flui_types::{Size, geometry::px};

// ============================================================================
// Flat: 1 RenderFlex root + N leaves
// ============================================================================

fn bench_flat_run_layout(c: &mut Criterion) {
    let mut group = c.benchmark_group("layout/flat");
    for &n in &[10_usize, 100, 1_000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || helpers::build_flat(n),
                |mut owner| {
                    owner
                        .run_layout()
                        .expect("run_layout must succeed on a valid flat tree");
                    // Prevent the optimizer from discarding the owner.
                    black_box(owner)
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

// ============================================================================
// Deep: 1 000-node single-child chain
// ============================================================================

fn bench_deep_run_layout(c: &mut Criterion) {
    let mut group = c.benchmark_group("layout/deep");
    for &depth in &[10_usize, 100, 1_000] {
        group.bench_with_input(BenchmarkId::from_parameter(depth), &depth, |b, &depth| {
            b.iter_batched(
                || helpers::build_deep(depth),
                |mut owner| {
                    owner
                        .run_layout()
                        .expect("run_layout must succeed on a valid deep chain");
                    black_box(owner)
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

// ============================================================================
// Wide: RenderFlex + N children (distinct name for intent clarity)
// ============================================================================

fn bench_wide_run_layout(c: &mut Criterion) {
    let mut group = c.benchmark_group("layout/wide");
    for &n in &[10_usize, 100, 1_000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || helpers::build_wide(n),
                |mut owner| {
                    owner
                        .run_layout()
                        .expect("run_layout must succeed on a valid wide tree");
                    black_box(owner)
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

// ============================================================================
// layout_dirty_root in isolation (measures the single-root layout entry point)
// ============================================================================

fn bench_layout_dirty_root(c: &mut Criterion) {
    let mut group = c.benchmark_group("layout/dirty_root");
    for &n in &[10_usize, 100, 1_000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            let constraints = BoxConstraints::tight(Size::new(px(200.0), px(200.0)));
            b.iter_batched(
                || helpers::build_flat(n),
                |mut owner| {
                    let root_id = owner.root_id().expect(
                        "root must be set: build_flat always sets root_id before returning",
                    );
                    let size = owner
                        .layout_dirty_root(root_id, black_box(constraints))
                        .expect("layout_dirty_root must succeed on a freshly built valid tree");
                    black_box(size)
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_flat_run_layout,
    bench_deep_run_layout,
    bench_wide_run_layout,
    bench_layout_dirty_root,
);
criterion_main!(benches);
