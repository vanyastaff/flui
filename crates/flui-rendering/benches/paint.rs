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
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use flui_foundation::RenderId;
use flui_interaction::InteractionLane;
use flui_objects::{
    RenderClipPath, RenderClipRRect, RenderOpacity, RenderRotatedBox, RenderTransform,
};
use flui_rendering::hit_testing::PathClipTarget;
use flui_rendering::pipeline::{PaintPhase, PipelineOwner};
use flui_rendering::testing::update_render_object;
use flui_types::geometry::px;
use flui_types::painting::Path;
use flui_types::styling::{BorderRadius, BorderRadiusExt};
use flui_types::{Matrix4, Point, Rect, Size};

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

/// What a property change costs on the update arm versus the repaint arm —
/// the comparison shared by the opacity-alpha, transform-matrix, and
/// rotated-box-turn benches below.
///
/// Both arms mutate the SAME tree (from `build`) by the SAME property (via
/// `mutate`) through the same seam a widget rebuild uses; the only difference
/// is that `repaint` additionally marks the effect node needing paint, which
/// makes the frame take the old path. Nothing else is dirtied in either arm,
/// so the two are directly comparable — an earlier version of this benchmark
/// dirtied a sibling in one arm only, which quietly folded a whole extra
/// boundary's repaint and recapture into one side of the comparison.
///
/// Read the two `layered` groups together, not separately:
///
/// - **inline** — the leaves merge into one `PictureLayer`, so the retained
///   capture is a handful of nodes at any `subtree` and the update arm is
///   flat. This is the best case, and the one a background or a text run hits.
/// - **layered** — every leaf is its own repaint boundary, so the capture's
///   layer count grows with `subtree` and the update arm grows with it too: a
///   graft clones every captured node. The win narrows to the ratio of
///   "clone N layers" against "re-record N layers plus repaint their content".
///
/// Quoting only the inline number would overstate the general case.
fn bench_effect_change(
    c: &mut Criterion,
    group: &str,
    build: impl Fn(bool, usize) -> (PipelineOwner<PaintPhase>, RenderId),
    mutate: impl Fn(&mut PipelineOwner<PaintPhase>, RenderId),
) {
    for (layered, name) in [(false, "inline"), (true, "layered")] {
        let mut bench_group = c.benchmark_group(format!("{group}/{name}"));
        for &subtree in &[1_usize, 10, 100, 1_000] {
            bench_group.bench_with_input(
                BenchmarkId::new("update", subtree),
                &subtree,
                |b, &subtree| {
                    b.iter_batched(
                        || {
                            let (mut owner, id) = build(layered, subtree);
                            mutate(&mut owner, id);
                            owner
                        },
                        |mut owner| {
                            owner
                                .run_paint()
                                .expect("run_paint must succeed after the effect change");
                            black_box(owner)
                        },
                        criterion::BatchSize::SmallInput,
                    );
                },
            );
            bench_group.bench_with_input(
                BenchmarkId::new("repaint", subtree),
                &subtree,
                |b, &subtree| {
                    b.iter_batched(
                        || {
                            let (mut owner, id) = build(layered, subtree);
                            mutate(&mut owner, id);
                            // Force the old path: an explicit paint mark wins
                            // over the layer-update mark the setter reported.
                            owner.mark_needs_paint(id);
                            owner
                        },
                        |mut owner| {
                            owner
                                .run_paint()
                                .expect("run_paint must succeed after the effect change");
                            black_box(owner)
                        },
                        criterion::BatchSize::SmallInput,
                    );
                },
            );
        }
        bench_group.finish();
    }
}

// ============================================================================
// run_paint — an alpha change: update arm vs. repaint arm
// ============================================================================

/// What an alpha change costs on the update arm versus the repaint arm — see
/// [`bench_effect_change`] for what the two arms and the `inline`/`layered`
/// split measure.
fn bench_opacity_alpha_change(c: &mut Criterion) {
    bench_effect_change(
        c,
        "paint/opacity_alpha_change",
        |layered, subtree| helpers::build_effect_tree(layered, subtree, RenderOpacity::new(0.5)),
        |owner, id| update_render_object::<RenderOpacity, _>(owner, id, |o| o.set_opacity(0.25)),
    );
}

// ============================================================================
// run_paint — a transform matrix change: update arm vs. repaint arm
// ============================================================================

/// What a matrix change costs on the update arm versus the repaint arm — see
/// [`bench_effect_change`]. The seeded matrix is a SCALE, never a
/// translation: a translation owns no `TransformLayer` at all (painted as a
/// plain offset — see `RenderTransform::paint_effects`), which would make
/// every `update` iteration a structural (`PAINT`) change instead of the
/// `COMPOSITED_LAYER_UPDATE` this benchmark exists to measure.
fn bench_transform_matrix_change(c: &mut Criterion) {
    bench_effect_change(
        c,
        "paint/transform_matrix_change",
        |layered, subtree| {
            helpers::build_effect_tree(
                layered,
                subtree,
                RenderTransform::new(Matrix4::scaling(2.0, 2.0, 1.0)),
            )
        },
        |owner, id| {
            update_render_object::<RenderTransform, _>(owner, id, |t| {
                t.set_transform(Matrix4::scaling(3.0, 3.0, 1.0))
            });
        },
    );
}

// ============================================================================
// run_paint — a rotated-box quarter-turn change: update arm vs. repaint arm
// ============================================================================

/// What a parity-preserving quarter-turn change costs on the update arm
/// versus the repaint arm — see [`bench_effect_change`]. The turn moves
/// 1 → 3 — same parity, so the setter reports
/// `COMPOSITED_LAYER_UPDATE | SEMANTICS`, never `LAYOUT`.
fn bench_rotated_box_turn_change(c: &mut Criterion) {
    bench_effect_change(
        c,
        "paint/rotated_box_turn_change",
        |layered, subtree| helpers::build_effect_tree(layered, subtree, RenderRotatedBox::new(1)),
        |owner, id| {
            update_render_object::<RenderRotatedBox, _>(owner, id, |r| r.set_quarter_turns(3));
        },
    );
}

// ============================================================================
// run_paint — a clip-rrect border-radius change: update arm vs. repaint arm
// ============================================================================

/// What a border-radius change costs on the update arm versus the repaint
/// arm — see [`bench_effect_change`]. `RenderClipRRect`'s border radius is a
/// plain `Option<BorderRadius>` field (`crates/flui-objects/src/proxy/clip.rs`),
/// so this is a straight copy of the opacity/transform/rotated-box shape: no
/// owner-lane resolution is involved, and the existing driver applies with no
/// changes. Compare against [`bench_clip_path_token_change`], the one clip
/// property that IS owner-lane resolved.
fn bench_clip_rrect_radius_change(c: &mut Criterion) {
    bench_effect_change(
        c,
        "paint/clip_rrect_radius_change",
        |layered, subtree| {
            helpers::build_effect_tree(
                layered,
                subtree,
                RenderClipRRect::anti_alias().with_border_radius(BorderRadius::circular(px(8.0))),
            )
        },
        |owner, id| {
            update_render_object::<RenderClipRRect, _>(owner, id, |r| {
                r.set_border_radius(Some(BorderRadius::circular(px(2.0))))
            });
        },
    );
}

// ============================================================================
// run_paint — a clip-path token change: update arm vs. repaint arm
// (owner-lane resolved, unlike every other producer this file measures)
// ============================================================================

/// Builds a data-only path around `size`; the token registered under it
/// stands in for a real widget-supplied path factory.
fn default_rect_clip(size: Size) -> Path {
    let mut path = Path::new();
    path.add_rect(Rect::from_origin_size(Point::ZERO, size));
    path
}

/// A fresh [`InteractionLane`] plus two path-clip targets registered on it:
/// `target_a` seeds the built tree (mirrors the rrect bench's initial
/// radius) and `target_b` is what `mutate` swaps in. `target_b`'s clipper
/// increments the returned counter on every resolve — the oracle that the
/// timed `run_paint` genuinely reached [`resolve_path_clip`](
/// flui_rendering::traits::resolve_path_clip) rather than degrading to the
/// whole-box default.
fn new_path_clip_pair() -> (
    InteractionLane,
    PathClipTarget,
    PathClipTarget,
    Arc<AtomicUsize>,
) {
    let lane = InteractionLane::try_new().expect("interaction lane for the bench");
    let handle = lane.dispatch_handle();
    let clip_calls = Arc::new(AtomicUsize::new(0));
    let (target_a, target_b) = lane.enter(|| {
        let target_a = handle
            .register_path_clipper(default_rect_clip)
            .expect("register target_a");
        let calls = Arc::clone(&clip_calls);
        let target_b = handle
            .register_path_clipper(move |size| {
                calls.fetch_add(1, Ordering::Relaxed);
                default_rect_clip(size)
            })
            .expect("register target_b");
        (target_a, target_b)
    });
    (lane, target_a, target_b, clip_calls)
}

/// The [`bench_effect_change`] comparison for a producer whose changed
/// property is an owner-lane [`PathClipTarget`] rather than plain data.
///
/// `resolve_path_clip` reads the currently active lane through a thread-local
/// (`resolve_path_clip_target`, `crates/flui-interaction/src/routing/interaction_lane.rs`),
/// not a parameter `run_paint` is handed — so the timed `run_paint` call
/// itself has to execute inside [`InteractionLane::enter`], not just the
/// setup that registers the target. `bench_effect_change`'s driver calls
/// `run_paint` with no lane at all, so this is a SECOND driver rather than a
/// change to that one: `bench_effect_change` and its three existing callers
/// (opacity/transform/rotated-box) are untouched — same closures, same
/// generated benchmark ids, same codegen. `bench_clip_rrect_radius_change`
/// above confirms the alternative (threading an optional `run` hook through
/// `bench_effect_change` itself) was not needed either: only this one
/// producer needs a lane.
///
/// Each `(shape, arm, subtree)` benchmark id gets its own lane and its own
/// [`new_path_clip_pair`], so a degrade at one size cannot hide behind a
/// working size elsewhere. Every `b.iter_batched` iteration rebuilds the tree
/// from scratch (`build` re-seeds `target_a`, `mutate` re-applies `target_b`),
/// so the code path is identical run to run — there is no "resolves on some
/// iterations, degrades on others" — and the counter is read once, AFTER
/// `b.iter_batched` returns, never inside the timed closure. A zero count
/// there means every iteration degraded silently, which must fail loud.
///
/// What the counter does NOT distinguish, and why that is fine here: both
/// arms resolve `target_b` exactly once per iteration — the update arm once
/// while rebuilding the patched layer, the repaint arm once while repainting
/// the node — so the resolver's own cost is the same constant added to both
/// arms and cancels out of the ratio. The ratio the two arms show is
/// therefore expected to compress toward 1× versus the plain-data rrect/
/// opacity/transform/rotated-box benches, and can never cross it: the update
/// arm can never be MORE expensive than repaint when repaint pays every cost
/// the update arm pays, plus the full subtree repaint.
fn bench_effect_change_in_lane(
    c: &mut Criterion,
    group: &str,
    build: impl Fn(bool, usize, PathClipTarget) -> (PipelineOwner<PaintPhase>, RenderId),
    mutate: impl Fn(&mut PipelineOwner<PaintPhase>, RenderId, PathClipTarget),
) {
    for (layered, name) in [(false, "inline"), (true, "layered")] {
        let full_group = format!("{group}/{name}");
        let mut bench_group = c.benchmark_group(full_group.clone());
        for &subtree in &[1_usize, 10, 100, 1_000] {
            bench_group.bench_with_input(
                BenchmarkId::new("update", subtree),
                &subtree,
                |b, &subtree| {
                    let (lane, target_a, target_b, clip_calls) = new_path_clip_pair();
                    b.iter_batched(
                        || {
                            lane.enter(|| {
                                let (mut owner, id) = build(layered, subtree, target_a);
                                mutate(&mut owner, id, target_b);
                                owner
                            })
                        },
                        |mut owner| {
                            lane.enter(|| {
                                owner
                                    .run_paint()
                                    .expect("run_paint must succeed after the effect change");
                            });
                            black_box(owner)
                        },
                        criterion::BatchSize::SmallInput,
                    );
                    assert!(
                        clip_calls.load(Ordering::Relaxed) > 0,
                        "{full_group}/update/{subtree}: the registered path clipper never \
                         resolved — resolve_path_clip degraded to the whole-box default \
                         (no active InteractionLane around run_paint), so this benchmark \
                         measured nothing"
                    );
                },
            );
            bench_group.bench_with_input(
                BenchmarkId::new("repaint", subtree),
                &subtree,
                |b, &subtree| {
                    let (lane, target_a, target_b, clip_calls) = new_path_clip_pair();
                    b.iter_batched(
                        || {
                            lane.enter(|| {
                                let (mut owner, id) = build(layered, subtree, target_a);
                                mutate(&mut owner, id, target_b);
                                // Force the old path: an explicit paint mark
                                // wins over the layer-update mark the setter
                                // reported.
                                owner.mark_needs_paint(id);
                                owner
                            })
                        },
                        |mut owner| {
                            lane.enter(|| {
                                owner
                                    .run_paint()
                                    .expect("run_paint must succeed after the effect change");
                            });
                            black_box(owner)
                        },
                        criterion::BatchSize::SmallInput,
                    );
                    assert!(
                        clip_calls.load(Ordering::Relaxed) > 0,
                        "{full_group}/repaint/{subtree}: the registered path clipper never \
                         resolved — resolve_path_clip degraded to the whole-box default \
                         (no active InteractionLane around run_paint), so this benchmark \
                         measured nothing"
                    );
                },
            );
        }
        bench_group.finish();
    }
}

/// What a clip-path token change costs on the update arm versus the repaint
/// arm — see [`bench_effect_change_in_lane`] for why this producer needs its
/// own driver. `target_a` seeds `RenderClipPath` at construction; `mutate`
/// replaces it with `target_b`, reporting `COMPOSITED_LAYER_UPDATE |
/// SEMANTICS` exactly like the rrect radius setter above (`RenderClip::
/// set_path_clip_target`, `crates/flui-objects/src/proxy/clip.rs`).
fn bench_clip_path_token_change(c: &mut Criterion) {
    bench_effect_change_in_lane(
        c,
        "paint/clip_path_token_change",
        |layered, subtree, target_a| {
            let mut clip = RenderClipPath::anti_alias();
            let _ = clip.set_path_clip_target(Some(target_a));
            helpers::build_effect_tree(layered, subtree, clip)
        },
        |owner, id, target_b| {
            update_render_object::<RenderClipPath, _>(owner, id, |r| {
                r.set_path_clip_target(Some(target_b))
            });
        },
    );
}

criterion_group!(
    benches,
    bench_flat_run_compositing,
    bench_flat_run_paint,
    bench_deep_run_paint,
    bench_single_dirty_boundary,
    bench_eight_dirty_boundaries,
    bench_opacity_alpha_change,
    bench_transform_matrix_change,
    bench_rotated_box_turn_change,
    bench_clip_rrect_radius_change,
    bench_clip_path_token_change,
);
criterion_main!(benches);
