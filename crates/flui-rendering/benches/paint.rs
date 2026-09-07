//! U1b — paint and compositing pipeline benchmarks.
//!
//! Measures `PipelineOwner::run_paint` and `run_compositing` over flat and
//! deep tree shapes. Setup runs layout (and layout+compositing for paint
//! benches) so each timed iteration measures only the phase under test.
//!
//! Run with:
//!   cargo bench -p flui-rendering --bench paint

// Bench harness, not public API; `criterion_group!` generates the
// undocumentable entry fn.

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

// ============================================================================
// run_paint — one dirty boundary in a tree of N
// ============================================================================

/// The frame retention exists for: N repaint boundaries, ONE of them dirty.
///
/// `run_paint` builds a dirty-node set and threads it through the whole walk,
/// but `paint_subtree` never reads it — the descent repaints everything and a
/// fresh `LayerTree` is produced each pass (documented on `run_paint`). This
/// benchmark is the measurement that says what that costs: compare it against
/// `paint/paint_flat` at the same N, where every node is genuinely dirty.
///
/// Two numbers close together mean the clean boundaries are being repainted
/// for nothing. It is deliberately measured BEFORE any retention work, so the
/// claim that retention helps has a baseline to be checked against rather than
/// an assertion.
///
/// The setup takes the warm-up pass's layer tree — see the helper. Leaving it
/// would put an O(N) drop of the previous tree inside the timed region, which
/// a real frame does not pay; it was worth ~15% at N = 1000.
fn bench_single_dirty_boundary(c: &mut Criterion) {
    let mut group = c.benchmark_group("paint/one_dirty_boundary");
    for &n in &[10_usize, 100, 1_000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || {
                    let (mut owner, leaf_ids) = helpers::build_boundary_tree_painted_once(n);
                    // Dirty exactly one leaf; `mark_needs_paint` walks to its
                    // enclosing boundary, so the queue holds one boundary.
                    owner.mark_needs_paint(leaf_ids[0]);
                    owner
                },
                |mut owner| {
                    owner
                        .run_paint()
                        .expect("run_paint must succeed with one dirty boundary");
                    black_box(owner)
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

/// The control for `one_dirty_boundary`: the SAME tree, eight dirty
/// boundaries instead of one.
///
/// Comparing one-dirty against `paint/paint_flat` would prove nothing — that
/// tree has half the nodes and no `OffsetLayer` per child. Comparing it
/// against itself with eight times the dirty work is the measurement that
/// isolates the dirty set's effect, and two matching numbers say it has none.
fn bench_eight_dirty_boundaries(c: &mut Criterion) {
    let mut group = c.benchmark_group("paint/eight_dirty_boundaries");
    for &n in &[10_usize, 100, 1_000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || {
                    let (mut owner, leaf_ids) = helpers::build_boundary_tree_painted_once(n);
                    for &id in &leaf_ids {
                        owner.mark_needs_paint(id);
                    }
                    owner
                },
                |mut owner| {
                    owner
                        .run_paint()
                        .expect("run_paint must succeed with eight dirty boundaries");
                    black_box(owner)
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

/// The prize an update-only commit (issue #536) competes for, measured
/// without building one.
///
/// `opacity_tick/repaint` is what an alpha change costs TODAY: `RenderOpacity`
/// is not a repaint boundary, so the mark walks up to the enclosing
/// `RenderRepaintBoundary` and the whole subtree under it repaints.
/// `opacity_tick/graft` is the same tree on a frame where that boundary is
/// clean and its capture is replayed instead — the floor any update-only
/// design could reach, since patching one layer on top of a graft is O(1).
///
/// The ratio between them is the entire available win, as a function of how
/// much content sits under the opacity. Two numbers close together mean
/// neither design in #536 is worth building; a widening gap says how much is
/// on the table and at what subtree size it starts to matter.
///
/// Both arms dirty exactly ONE leaf and run one `run_paint`, so the only
/// difference between them is WHICH branch is dirty — the sibling exists so
/// the clean-boundary arm still has paint work and cannot take `run_paint`'s
/// nothing-is-dirty early return.
fn bench_opacity_tick(c: &mut Criterion) {
    for &subtree in &[1_usize, 10, 100, 1_000] {
        let mut group = c.benchmark_group("paint/opacity_tick");
        group.bench_with_input(
            BenchmarkId::new("repaint", subtree),
            &subtree,
            |b, &subtree| {
                b.iter_batched(
                    || {
                        let (mut owner, opacity_leaf, _sibling) =
                            helpers::build_opacity_tree_painted_once(subtree);
                        owner.mark_needs_paint(opacity_leaf);
                        owner
                    },
                    |mut owner| {
                        owner
                            .run_paint()
                            .expect("run_paint must succeed with the opacity subtree dirty");
                        black_box(owner)
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
        group.bench_with_input(
            BenchmarkId::new("graft", subtree),
            &subtree,
            |b, &subtree| {
                b.iter_batched(
                    || {
                        let (mut owner, _opacity_leaf, sibling) =
                            helpers::build_opacity_tree_painted_once(subtree);
                        owner.mark_needs_paint(sibling);
                        owner
                    },
                    |mut owner| {
                        owner
                            .run_paint()
                            .expect("run_paint must succeed with only the sibling dirty");
                        black_box(owner)
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
        group.finish();
    }
}

criterion_group!(
    benches,
    bench_flat_run_compositing,
    bench_flat_run_paint,
    bench_deep_run_paint,
    bench_single_dirty_boundary,
    bench_eight_dirty_boundaries,
    bench_opacity_tick,
);
criterion_main!(benches);
