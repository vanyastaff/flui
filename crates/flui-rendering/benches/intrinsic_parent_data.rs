//! Borrowed intrinsic path parent-data slice benchmark (ADR-0010 D3).
//!
//! Every `box_intrinsic_query_borrowed` call for a non-leaf node eagerly
//! builds three heap collections:
//!   1. `child_ids: Vec<RenderId>` — N child IDs.
//!   2. `child_parent_data_owned: Vec<Option<Box<dyn ParentData>>>` — N
//!      `dyn_clone::clone_box` heap allocations.
//!   3. `child_parent_data_refs: Vec<Option<&dyn ParentData>>` — N fat-pointer
//!      refs that borrow from (2).
//!
//! These collections are rebuilt on EVERY intrinsic call, even when the results
//! are later memoized.  On a 10-child flex row this is ~13 heap allocations per
//! `child_max_intrinsic_width` call, fired inside `perform_layout` on the hot
//! animation path.
//!
//! ## Driver tree
//!
//! ```
//! IntrinsicQueryingDriver (Arity=Single)
//!   └─ RenderFlex::row() (N flex-bearing children)
//!        └─ N × RenderColoredBox (leaf, FlexParentData::inflexible())
//! ```
//!
//! The driver's `perform_layout` calls `ctx.child_max_intrinsic_width(0, ∞)`
//! before laying out the child.  That query routes through
//! `box_intrinsic_query_borrowed` on the `RenderFlex`, which calls
//! `build_intrinsic_child_parent_data` on its N children and builds all three
//! collections.  The `FlexParentData::inflexible()` seed on each leaf is what
//! `build_intrinsic_child_parent_data` clones, faithfully reproducing the full
//! allocation cost even without an element tree.
//!
//! N is parameterized over {1, 8, 64}.  Each criterion iteration receives a
//! freshly-built tree (setup is excluded from measurement), so the intrinsic
//! cache is empty and the full slice-build fires on every sample.
//!
//! Run:
//! ```sh
//! env -u CARGO_TARGET_DIR cargo bench -p flui-rendering --bench intrinsic_parent_data
//! ```

// Bench harness, not public API; `criterion_group!` generates the
// undocumentable entry fn.
#![allow(missing_docs)]

mod helpers;

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use flui_foundation::Diagnosticable;
use flui_objects::{RenderColoredBox, RenderFlex};
use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxHitTestContext, BoxLayoutContext},
    hit_testing::HitTestBehavior,
    parent_data::{BoxParentData, FlexParentData},
    pipeline::{Layout, PipelineOwner},
    testing::{box_node, tree},
    traits::RenderBox,
};
use flui_tree::Single;
use flui_types::{Size, geometry::px};

// ============================================================================
// Driver widget
// ============================================================================

/// A render box whose `perform_layout` queries child 0's max-intrinsic-width
/// BEFORE laying it out.
///
/// Querying a child's intrinsic from inside `perform_layout` routes through
/// `box_intrinsic_query_borrowed` (the borrowed-arena path wired by
/// `layout_dirty_root`).  That function calls `build_intrinsic_child_parent_data`
/// on the queried node's children, triggering the three-Vec allocation we are
/// measuring.  Placing a multi-child `RenderFlex` at index 0 ensures the slice
/// is N elements wide.
#[derive(Debug, Default)]
struct IntrinsicQueryingDriver;

impl Diagnosticable for IntrinsicQueryingDriver {}

impl RenderBox for IntrinsicQueryingDriver {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        // Triggers box_intrinsic_query_borrowed on child 0 (the RenderFlex),
        // which calls build_intrinsic_child_parent_data for its N children.
        // black_box prevents the compiler from eliding the query or sinking it
        // past the layout call.
        let _max_width = black_box(ctx.child_max_intrinsic_width(0, black_box(f32::INFINITY)));
        let constraints = *ctx.constraints();
        ctx.layout_child(0, constraints);
        constraints.smallest()
    }

    fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        false
    }

    fn hit_test_behavior(&self) -> HitTestBehavior {
        HitTestBehavior::Opaque
    }
}

// ============================================================================
// Tree builder
// ============================================================================

/// Tight 500 × 100 constraints used by this bench (wide row, shallow height).
fn bench_constraints() -> BoxConstraints {
    BoxConstraints::tight(Size::new(px(500.0), px(100.0)))
}

/// Build: `IntrinsicQueryingDriver` -> `RenderFlex::row()` -> `child_count`
/// `RenderColoredBox` leaves, each seeded with `FlexParentData::inflexible()`.
///
/// The seeds are what `build_intrinsic_child_parent_data` clones
/// (`ParentDataSeed::to_box`) on every intrinsic call, faithfully reproducing
/// the N-alloc cost without requiring a full element tree.
fn build_intrinsic_bench_tree(child_count: usize) -> PipelineOwner<Layout> {
    let flex_children = (0..child_count).map(|_| {
        box_node(RenderColoredBox::red(10.0, 10.0))
            .with_flex_parent_data(FlexParentData::inflexible())
    });

    let spec = box_node(IntrinsicQueryingDriver)
        .child(box_node(RenderFlex::row()).children(flex_children));

    let mut owner = PipelineOwner::new();
    let (root_id, _) = tree::mount(&mut owner, spec);
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(bench_constraints()));
    owner.into_layout()
}

// ============================================================================
// Benchmark
// ============================================================================

/// Measures `run_layout` on a tree whose root queries a child's intrinsic
/// during `perform_layout`, exercising the `build_intrinsic_child_parent_data`
/// slice-build path for N = {1, 8, 64} flex children.
///
/// Each iteration receives a freshly-built, cache-empty tree via `iter_batched`
/// so the full borrowed-intrinsic walk (including slice construction) fires on
/// every sample.
fn bench_borrowed_intrinsic_parent_data(c: &mut Criterion) {
    let mut group = c.benchmark_group("intrinsic/borrowed_parent_data_slice");
    for &n in &[1_usize, 8, 64] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter_batched(
                || build_intrinsic_bench_tree(n),
                |mut owner| {
                    owner
                        .run_layout()
                        .expect("run_layout must succeed on a valid intrinsic-query tree");
                    // Prevent the optimizer from discarding the owner.
                    black_box(owner)
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }
    group.finish();
}

criterion_group!(benches, bench_borrowed_intrinsic_parent_data);
criterion_main!(benches);
