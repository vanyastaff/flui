//! Shared tree-construction helpers for `flui-rendering` benchmarks.
//!
//! Each helper builds a real `PipelineOwner` tree via the same public API as
//! the integration tests, ensuring benchmarks measure the genuine production
//! contract rather than mocks or shortcuts (bench-fidelity discipline).

// Shared across the `layout` and `paint` benches via `mod helpers;`; each bench
// uses only the subset it needs, so unused-in-this-unit helpers are expected.
#![allow(dead_code)]

use flui_objects::{RenderColoredBox, RenderFlex, RenderPadding};
use flui_rendering::{
    constraints::BoxConstraints,
    pipeline::{Compositing, Layout, PaintPhase, PipelineOwner},
    testing::{TreeNode, box_node, tree},
};
use flui_types::{Size, geometry::px};

/// Tight 200×200 root constraint used across all bench tree shapes.
pub fn root_constraints() -> BoxConstraints {
    BoxConstraints::tight(Size::new(px(200.0), px(200.0)))
}

/// Mounts a `TreeNode` spec into a fresh owner with the shared root
/// constraints and returns it in the `Layout` phase, ready for `run_layout`.
/// Construction goes through the same `flui_rendering::testing` builder the
/// integration tests use, so benches measure the genuine production contract.
fn mount_layout(spec: TreeNode) -> PipelineOwner<Layout> {
    let mut owner = PipelineOwner::new();
    let (root_id, _registry) = tree::mount(&mut owner, spec);
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(root_constraints()));
    owner.into_layout()
}

// ============================================================================
// Flat: 1 RenderFlex root + N RenderColoredBox leaves
// ============================================================================

/// Build a flat tree: one `RenderFlex` row root + `n` leaf `RenderColoredBox`
/// children, returned in the `Layout` phase ready for `run_layout`.
///
/// Measures the cost of one root-layout pass across `n` same-depth children.
pub fn build_flat(n: usize) -> PipelineOwner<Layout> {
    mount_layout(
        box_node(RenderFlex::row())
            .children((0..n).map(|_| box_node(RenderColoredBox::red(1.0, 1.0)))),
    )
}

// ============================================================================
// Deep: a single-child chain of `depth` RenderPadding wrappers + 1 leaf
// ============================================================================

/// Build a single-child chain of `depth` `RenderPadding` wrappers (padding=0)
/// around a single `RenderColoredBox` leaf, returned in the `Layout` phase.
///
/// Measures the cost of a layout pass that must recurse `depth` levels deep.
/// Padding=0 keeps arithmetic trivial so the bench reflects traversal cost.
///
/// The spec is built leaf-first and wrapped `depth` times, so `depth == 0`
/// degenerates to a bare leaf and `depth == k` yields `k` padding wrappers
/// around one `ColoredBox`.
pub fn build_deep(depth: usize) -> PipelineOwner<Layout> {
    let mut node = box_node(RenderColoredBox::red(1.0, 1.0));
    for _ in 0..depth {
        node = box_node(RenderPadding::all(0.0)).child(node);
    }
    mount_layout(node)
}

// ============================================================================
// Wide: RenderFlex root + N children (same as flat; named separately for
// bench clarity)
// ============================================================================

/// Build a wide tree: one `RenderFlex` row root + `n` leaf children.
///
/// Identical to [`build_flat`]; provided as a named alias so bench code reads
/// as "wide N" rather than "flat N" at the call site.
#[inline]
pub fn build_wide(n: usize) -> PipelineOwner<Layout> {
    build_flat(n)
}

// ============================================================================
// Paint-ready builders: run layout + compositing, return PipelineOwner<PaintPhase>
// ============================================================================

/// Build a flat tree already advanced through layout and compositing, ready
/// for `run_paint`. The layout and compositing phases are run in setup so the
/// paint bench measures only `run_paint` cost.
pub fn build_flat_paint_ready(n: usize) -> PipelineOwner<PaintPhase> {
    advance_to_paint(build_flat(n))
}

/// Build a deep chain already advanced through layout and compositing, ready
/// for `run_paint`.
pub fn build_deep_paint_ready(depth: usize) -> PipelineOwner<PaintPhase> {
    advance_to_paint(build_deep(depth))
}

/// Build a compositing-only-ready owner from a flat tree (layout run, not yet
/// compositing). Used when benching `run_compositing` in isolation.
pub fn build_flat_compositing_ready(n: usize) -> PipelineOwner<Compositing> {
    let mut layout_owner = build_flat(n);
    layout_owner
        .run_layout()
        .expect("run_layout must succeed: tree was built with valid root constraints");
    layout_owner.into_compositing()
}

/// Advance a `Layout`-phase owner through `run_layout` + `run_compositing`,
/// returning a `PaintPhase` owner ready for `run_paint`.
fn advance_to_paint(mut layout_owner: PipelineOwner<Layout>) -> PipelineOwner<PaintPhase> {
    layout_owner
        .run_layout()
        .expect("run_layout must succeed: tree was built with valid root constraints");
    let mut compositing_owner = layout_owner.into_compositing();
    compositing_owner
        .run_compositing()
        .expect("run_compositing must succeed: all nodes were freshly inserted");
    compositing_owner.into_paint()
}
