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

use flui_layer::{Layer, LayerTree};
use flui_painting::DisplayListCore;
use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxHitTestContext, BoxLayoutContext},
    objects::{RenderClipRect, RenderColoredBox, RenderPadding, RenderRepaintBoundary},
    parent_data::BoxParentData,
    pipeline::PipelineOwner,
    traits::{HotReloadCapability, PaintEffectsCapability, RenderBox, SemanticsCapability},
};
use flui_tree::Variable;
use flui_types::{Offset, Point, Rect, Size, geometry::px};

type BoxedRenderObject =
    Box<dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>>;

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

/// Collects `(depth, variant-name)` pairs in DFS order — the
/// structural snapshot.
fn structure(tree: &LayerTree) -> Vec<(usize, &'static str)> {
    fn walk(
        tree: &LayerTree,
        id: flui_foundation::LayerId,
        depth: usize,
        out: &mut Vec<(usize, &'static str)>,
    ) {
        let node = tree.get(id).expect("walk only visits live ids");
        let name = match node.layer() {
            Layer::Offset(_) => "Offset",
            Layer::Picture(_) => "Picture",
            Layer::ClipRect(_) => "ClipRect",
            Layer::ClipRRect(_) => "ClipRRect",
            Layer::ClipPath(_) => "ClipPath",
            Layer::Opacity(_) => "Opacity",
            Layer::Transform(_) => "Transform",
            _ => "Other",
        };
        out.push((depth, name));
        for &child in node.children() {
            walk(tree, child, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    if let Some(root) = tree.root() {
        walk(tree, root, 0, &mut out);
    }
    out
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
    find(tree, tree.root().expect("tree has a root")).expect("tree contains a picture layer")
}

// ============================================================================
// 1+2. Inline subtree merges into one picture, z-ordered, origin-baked
// ============================================================================

/// Variable-arity container: lays out children loose and positions
/// child `i` at `(i*50, 0)`. No paint override — the default
/// pass-through splices children, so their draws must merge into the
/// parent's picture space.
#[derive(Debug)]
struct SimpleRow {
    size: Size,
}

impl flui_foundation::Diagnosticable for SimpleRow {}
impl PaintEffectsCapability for SimpleRow {}
impl SemanticsCapability for SimpleRow {}
impl HotReloadCapability for SimpleRow {}

impl RenderBox for SimpleRow {
    type Arity = Variable;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>) {
        let constraints = *ctx.constraints();
        for i in 0..ctx.child_count() {
            let _ = ctx.layout_child(i, constraints);
            #[allow(clippy::cast_precision_loss)] // test fixture, i < 3
            ctx.position_child(i, Offset::new(px(i as f32 * 50.0), px(0.0)));
        }
        self.size = constraints.constrain(Size::new(px(150.0), px(50.0)));
        ctx.complete_with_size(self.size);
    }

    fn size(&self) -> &Size {
        &self.size
    }

    fn size_mut(&mut self) -> &mut Size {
        &mut self.size
    }

    fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Variable, BoxParentData>) -> bool {
        false
    }
}

#[test]
fn inline_siblings_merge_into_one_origin_baked_picture() {
    let mut owner = PipelineOwner::new();
    let row_id = owner.insert(Box::new(SimpleRow { size: Size::ZERO }) as BoxedRenderObject);
    owner
        .insert_child_render_object(row_id, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("child 0");
    owner
        .insert_child_render_object(row_id, Box::new(RenderColoredBox::blue(40.0, 40.0)))
        .expect("child 1");

    owner.set_root_id(Some(row_id));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(300.0),
        px(0.0),
        px(300.0),
    )));

    let (tree, _owner) = paint_frame(owner);

    assert_eq!(
        structure(&tree),
        vec![(0, "Offset"), (1, "Picture")],
        "two inline sibling draws must merge into ONE PictureLayer under \
         the root — no per-node layer explosion",
    );

    let picture = first_picture(&tree);
    assert_eq!(
        picture.len(),
        2,
        "one DrawRect per ColoredBox, merged in z-order",
    );
    assert_eq!(
        picture.bounds(),
        Rect::from_ltrb(px(0.0), px(0.0), px(90.0), px(40.0)),
        "record-time bounds must reflect the committed child offsets: \
         child 0 at (0,0)-(40,40), child 1 at (50,0)-(90,40)",
    );
}

// ============================================================================
// 3. Repaint-boundary child splits into a rebased OffsetLayer
// ============================================================================

#[test]
fn repaint_boundary_child_splits_into_rebased_offset_layer() {
    let mut owner = PipelineOwner::new();
    let padding_id = owner.insert(Box::new(RenderPadding::all(5.0)) as BoxedRenderObject);
    let boundary_id = owner
        .insert_child_render_object(padding_id, Box::new(RenderRepaintBoundary::new()))
        .expect("boundary insert");
    owner
        .insert_child_render_object(boundary_id, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("colored insert");

    owner.set_root_id(Some(padding_id));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(200.0),
    )));

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
        Rect::from_origin_size(Point::ZERO, Size::new(px(40.0), px(40.0))),
        "boundary-subtree coordinates must be rebased to Offset::ZERO",
    );
}

// ============================================================================
// 4. Clip render object produces a real clip layer over the child
// ============================================================================

#[test]
fn clip_rect_object_brackets_child_in_clip_layer() {
    let mut owner = PipelineOwner::new();
    let clip_id = owner.insert(Box::new(RenderClipRect::hard_edge()) as BoxedRenderObject);
    owner
        .insert_child_render_object(clip_id, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("colored insert");

    owner.set_root_id(Some(clip_id));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(200.0),
    )));

    let (tree, _owner) = paint_frame(owner);

    assert_eq!(
        structure(&tree),
        vec![(0, "Offset"), (1, "ClipRect"), (2, "Picture")],
        "RenderClipRect must produce a ClipRect LAYER covering the \
         child's picture — canvas clips are run-local and would never \
         reach the child",
    );
}
