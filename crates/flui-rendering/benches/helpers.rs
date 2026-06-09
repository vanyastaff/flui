//! Shared tree-construction helpers for `flui-rendering` benchmarks.
//!
//! Each helper builds a real `PipelineOwner` tree via the same public API as
//! the integration tests, ensuring benchmarks measure the genuine production
//! contract rather than mocks or shortcuts (bench-fidelity discipline).

// Shared across the `layout` and `paint` benches via `mod helpers;`; each bench
// uses only the subset it needs, so unused-in-this-unit helpers are expected.
#![allow(dead_code)]

use flui_rendering::{
    constraints::BoxConstraints,
    objects::{RenderColoredBox, RenderFlex, RenderPadding},
    pipeline::{Compositing, Layout, PaintPhase, PipelineOwner},
    protocol::BoxProtocol,
    traits::RenderObject,
};
use flui_types::{Size, geometry::px};

/// Tight 200×200 root constraint used across all bench tree shapes.
pub fn root_constraints() -> BoxConstraints {
    BoxConstraints::tight(Size::new(px(200.0), px(200.0)))
}

// ============================================================================
// Flat: 1 RenderFlex root + N RenderColoredBox leaves
// ============================================================================

/// Build a flat tree: one `RenderFlex` row root + `n` leaf `RenderColoredBox`
/// children, returned in the `Layout` phase ready for `run_layout`.
///
/// Measures the cost of one root-layout pass across `n` same-depth children.
pub fn build_flat(n: usize) -> PipelineOwner<Layout> {
    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(RenderFlex::row()) as Box<dyn RenderObject<BoxProtocol>>);
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(root_constraints()));

    for _ in 0..n {
        owner
            .insert_child_render_object(root_id, Box::new(RenderColoredBox::red(1.0, 1.0)))
            .expect("child insert must succeed: parent id was just inserted and is valid");
    }
    owner.into_layout()
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
/// Construction strategy (API constraint): `insert_child_render_object`
/// creates a *new* node as child — it cannot adopt an existing node. We
/// therefore build top-down: insert the root padding first, then at each step
/// insert the next level as a child of the previous level. The final level
/// inserts the leaf `ColoredBox`.
pub fn build_deep(depth: usize) -> PipelineOwner<Layout> {
    let mut owner = PipelineOwner::new();

    if depth == 0 {
        // Degenerate: single leaf, no wrappers.
        let root_id =
            owner.insert(
                Box::new(RenderColoredBox::red(1.0, 1.0)) as Box<dyn RenderObject<BoxProtocol>>
            );
        owner.set_root_id(Some(root_id));
        owner.set_root_constraints(Some(root_constraints()));
        return owner.into_layout();
    }

    // Insert the first (top) padding as root.
    let root_id =
        owner.insert(Box::new(RenderPadding::all(0.0)) as Box<dyn RenderObject<BoxProtocol>>);
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(root_constraints()));

    let mut current = root_id;

    // Add `depth - 1` intermediate padding nodes after the root padding, then
    // one ColoredBox leaf — `depth` padding wrappers total. When depth==1 the
    // range is empty and we fall straight through to the leaf insert below.
    //
    // Invariant: `current` is always a Padding node that was just inserted and
    // has no children yet, so every insert_child_render_object call succeeds.
    for _ in 1..depth {
        let next = owner
            .insert_child_render_object(current, Box::new(RenderPadding::all(0.0)))
            .expect(
                "chain insert must succeed: current is a valid padding node just inserted \
                 and has no children yet",
            );
        current = next;
    }

    // Always insert one ColoredBox leaf as child of the deepest padding.
    owner
        .insert_child_render_object(current, Box::new(RenderColoredBox::red(1.0, 1.0)))
        .expect(
            "leaf insert must succeed: current is a valid padding node just inserted \
             and has no children yet",
        );

    owner.into_layout()
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
