//! Headless paint-fragment snapshot tests — no GPU, no window.
//!
//! The sans-IO paint model makes the frame's output an inspectable
//! value: `run_paint` produces a `LayerTree` whose pictures are
//! `DisplayList`s with record-time bounds. These tests pin the
//! composition contract:
//!
//! 1. an inline subtree merges into ONE `PictureLayer` with
//!    origin-baked coordinates;
//! 2. sibling inline draws merge into the same picture in z-order;
//! 3. a repaint-boundary child splits into its own `OffsetLayer`
//!    with coordinates rebased to zero;
//! 4. a clip render object produces a real clip layer bracketing the
//!    child's picture.
//!
//! Refs:
//!   * docs/research/2026-06-10-rendering-design-amendments.md §D1/§D9
//!   * crates/flui-rendering/src/context/paint_cx.rs (recording side)

use flui_foundation::Variable;
use flui_foundation::geometry::{Offset, Point, Rect, Size};
use flui_layer::{Layer, LayerTree};
use flui_objects::{RenderClipRect, RenderColoredBox, RenderPadding, RenderRepaintBoundary};
use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxHitTestContext, BoxLayoutContext},
    parent_data::BoxParentData,
    pipeline::PipelineOwner,
    testing::inspect,
    traits::RenderBox,
};

use crate::common::BoxedRenderObject;

/// Runs layout → compositing → paint and returns the produced layer
/// tree.
fn paint_frame(
    owner: PipelineOwner,
) -> (
    LayerTree,
    flui_rendering::pipeline::PipelineOwner<flui_rendering::pipeline::phase::PaintPhase>,
) {
    let mut owner = owner.into_layout();
    owner.run_layout().expect("layout succeeds");
    let mut owner = owner.into_compositing();
    owner.run_compositing().expect("compositing succeeds");
    let mut owner = owner.into_paint();
    owner.run_paint().expect("paint succeeds");
    let tree = owner
        .take_layer_tree()
        .expect("run_paint must produce a layer tree");
    (tree, owner)
}

/// Collects `(depth, variant-name)` pairs in DFS order — the structural
/// snapshot. Delegates to the shared inspection surface.
fn structure(tree: &LayerTree) -> Vec<(usize, &'static str)> {
    inspect::layer_structure_with_depth(tree)
}

/// First picture's display list in DFS order.
fn first_picture(tree: &LayerTree) -> &flui_painting::DisplayList {
    fn find(tree: &LayerTree, id: flui_foundation::LayerId) -> Option<&flui_painting::DisplayList> {
        let node = tree.get(id)?;
        if let Layer::Picture(p) = node.layer() {
            return Some(p.picture());
        }
        node.children().iter().find_map(|&c| find(tree, c))
    }
    find(tree, tree.root()).expect("tree contains a picture layer")
}

// ============================================================================
// 1+2. Inline subtree merges into one picture, z-ordered, origin-baked
// ============================================================================

/// Variable-arity container: lays out children loose and positions
/// child `i` at `(i*50, 0)`. No paint override — the default
/// pass-through splices children, so their draws must merge into the
/// parent's picture space.
#[derive(Debug)]
struct SimpleRow;

impl flui_foundation::Diagnosticable for SimpleRow {}

impl RenderBox for SimpleRow {
    type Arity = Variable;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        for i in 0..ctx.child_count() {
            let _ = ctx.layout_child(i, constraints);
            #[expect(clippy::cast_precision_loss)] // test fixture, i < 3
            ctx.position_child(i, Offset::new(i as f64 * 50.0, 0.0));
        }
        constraints.constrain(Size::new(150.0, 50.0))
    }

    fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Variable, BoxParentData>) -> bool {
        false
    }
}

pub(crate) fn inline_siblings_merge_into_one_origin_baked_picture() {
    let mut owner = PipelineOwner::new();
    let row_id = owner.insert(Box::new(SimpleRow) as BoxedRenderObject);
    owner
        .insert_child_render_object(row_id, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("child 0");
    owner
        .insert_child_render_object(row_id, Box::new(RenderColoredBox::blue(40.0, 40.0)))
        .expect("child 1");

    owner.set_root_id(Some(row_id));
    owner.set_root_constraints(Some(BoxConstraints::new(0.0, 300.0, 0.0, 300.0)));

    let (tree, _owner) = paint_frame(owner);

    assert_eq!(
        structure(&tree),
        vec![(0, "Offset"), (1, "Picture")],
        "two inline sibling draws must merge into ONE PictureLayer under \
         the root — no per-node layer explosion",
    );

    let picture = first_picture(&tree);
    assert_eq!(
        picture
            .iter()
            .filter(|command| matches!(command.op, flui_painting::DrawOp::Rect { .. }))
            .count(),
        2,
        "one DrawRect per ColoredBox, merged in z-order",
    );
    assert_eq!(
        picture.bounds(),
        Some(Rect::from_ltrb(0.0, 0.0, 90.0, 40.0)),
        "record-time bounds must reflect the committed child offsets: \
         child 0 at (0,0)-(40,40), child 1 at (50,0)-(90,40)",
    );
}

// ============================================================================
// 3. Repaint-boundary child splits into a rebased OffsetLayer
// ============================================================================

pub(crate) fn repaint_boundary_child_splits_into_rebased_offset_layer() {
    let mut owner = PipelineOwner::new();
    let padding_id = owner.insert(Box::new(RenderPadding::all(5.0)) as BoxedRenderObject);
    let boundary_id = owner
        .insert_child_render_object(padding_id, Box::new(RenderRepaintBoundary::new()))
        .expect("boundary insert");
    owner
        .insert_child_render_object(boundary_id, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("colored insert");

    owner.set_root_id(Some(padding_id));
    owner.set_root_constraints(Some(BoxConstraints::new(0.0, 200.0, 0.0, 200.0)));

    let (tree, _owner) = paint_frame(owner);

    assert_eq!(
        structure(&tree),
        vec![(0, "Offset"), (1, "Offset"), (2, "Picture")],
        "the boundary subtree must live under its own OffsetLayer",
    );

    // The boundary's OffsetLayer carries the accumulated offset (5,5);
    // the picture inside is REBASED to zero so an offset-only move can
    // later become a layer-property update instead of a repaint.
    let picture = first_picture(&tree);
    assert_eq!(
        picture.bounds(),
        Some(Rect::from_origin_size(Point::ZERO, Size::new(40.0, 40.0))),
        "boundary-subtree coordinates must be rebased to Offset::ZERO",
    );
}

// ============================================================================
// 4. Clip render object produces a real clip layer over the child
// ============================================================================

pub(crate) fn clip_rect_object_brackets_child_in_clip_layer() {
    let mut owner = PipelineOwner::new();
    let clip_id = owner.insert(Box::new(RenderClipRect::hard_edge()) as BoxedRenderObject);
    owner
        .insert_child_render_object(clip_id, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("colored insert");

    owner.set_root_id(Some(clip_id));
    owner.set_root_constraints(Some(BoxConstraints::new(0.0, 200.0, 0.0, 200.0)));

    let (tree, _owner) = paint_frame(owner);

    assert_eq!(
        structure(&tree),
        vec![(0, "Offset"), (1, "ClipRect"), (2, "Picture")],
        "RenderClipRect must produce a ClipRect LAYER covering the \
         child's picture — canvas clips are run-local and would never \
         reach the child",
    );
}

// ============================================================================
// 5. Box host paints a sliver subtree
// ============================================================================
