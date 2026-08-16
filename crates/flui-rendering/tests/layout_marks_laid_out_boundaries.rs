//! Every object that re-lays out must be queued for paint, not just the
//! dirty layout root.
//!
//! Flutter does this per object: `RenderObject.layout` ends with
//! `_needsLayout = false; markNeedsPaint();`
//! (`.flutter/packages/flutter/lib/src/rendering/object.dart`, and again in
//! `_layoutWithoutResize`). FLUI shortcut it to one `mark_needs_paint` per
//! dirty layout root, with the reason stated in `run_layout`: "run_paint walks
//! the whole tree from the root, so triggering the phase is what matters."
//!
//! That shortcut is invisible today and load-bearing tomorrow. `run_paint`
//! builds a dirty set and threads it through the walk without reading it
//! (measured in `benches/paint.rs` — eight dirty boundaries cost the same as
//! one). The moment a clean boundary is allowed to reuse its previous layers,
//! a boundary that re-laid out but was never queued shows stale pixels.
//!
//! `mark_needs_paint` walks UP to the nearest established boundary and returns
//! early on an already-dirty node, so a boundary nested INSIDE a relayout
//! subtree is never reached from that subtree's root.

use flui_objects::{RenderColoredBox, RenderPadding, RenderRepaintBoundary};
use flui_rendering::{
    constraints::BoxConstraints,
    pipeline::PipelineOwner,
    testing::{box_node, tree},
};
use flui_types::{EdgeInsets, Size, geometry::px};

/// Change a `RenderPadding`'s inset and report the impact, the same way an
/// element update does.
fn set_padding(owner: &mut PipelineOwner, id: flui_foundation::RenderId, value: f32) {
    let impact = {
        let entry = owner
            .render_tree_mut()
            .get_mut(id)
            .expect("padding node")
            .as_box_mut()
            .expect("box entry");
        entry
            .render_object_mut()
            .as_any_mut()
            .downcast_mut::<RenderPadding>()
            .expect("RenderPadding")
            .set_padding(EdgeInsets::all(px(value)))
    };
    owner.apply_render_update_impact(id, impact);
}

/// Relayout an outer subtree that contains an inner repaint boundary; the
/// inner boundary must be queued for paint.
///
/// Tree: padding root → outer boundary → padding → inner boundary → leaf.
///
/// The root is a `RenderPadding`, not a `RenderColoredBox`: a coloured box
/// paints no children, so the paint walk would stop at it and no boundary
/// below would ever record `was_repaint_boundary` — which
/// `mark_needs_paint` reads to decide whether a boundary owns a retained
/// layer.
///
/// The padding is marked for layout, so the walk re-lays out the padding, the
/// inner boundary, and the leaf. The inner boundary's content is therefore
/// stale, and it has to be in the paint queue for a retaining paint pass to
/// know that.
#[test]
fn a_boundary_nested_in_a_relayout_subtree_is_queued_for_paint() {
    let mut owner = PipelineOwner::new();
    let (root_id, registry) = tree::mount(
        &mut owner,
        box_node(RenderPadding::all(0.0)).child(
            box_node(RenderRepaintBoundary::new()).label("outer").child(
                box_node(RenderPadding::all(0.0)).label("padding").child(
                    box_node(RenderRepaintBoundary::new())
                        .label("inner")
                        .child(box_node(RenderColoredBox::red(1.0, 1.0)).label("leaf")),
                ),
            ),
        ),
    );
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));

    let padding_id = registry.get("padding").expect("padding is labelled");
    let inner_id = registry.get("inner").expect("inner boundary is labelled");

    // Settle the first frame so nothing is dirty from construction.
    let (owner_idle, result) = owner.run_frame();
    result.expect("the first frame must succeed");
    let mut owner = owner_idle;

    assert!(
        owner.nodes_needing_paint().is_empty(),
        "precondition: the settled frame leaves nothing queued for paint"
    );

    // Change the padding. This is the shape that exposes the bug, and the
    // three cheaper shapes do not:
    //
    // - dirtying the LEAF makes the leaf its own relayout root (its
    //   constraints are tight), so `mark_needs_paint` walks UP from it and
    //   correctly reaches the inner boundary on the way;
    // - dirtying the padding WITHOUT changing it leaves the inner boundary
    //   clean with unchanged constraints, so layout short-circuits it — its
    //   content is not stale and it must NOT be queued;
    // - only a parent that hands its child DIFFERENT constraints forces the
    //   inner boundary to re-lay out while the relayout root sits above it.
    set_padding(&mut owner, padding_id, 4.0);

    let mut layout_owner = owner.into_layout();
    layout_owner.run_layout().expect("relayout must succeed");

    let queued: Vec<_> = layout_owner
        .nodes_needing_paint()
        .iter()
        .map(|d| d.id)
        .collect();

    // The queue and the per-node flags must agree. Enqueueing the boundary
    // directly would satisfy the assertion below while leaving this false —
    // and `mark_needs_paint` is also the only place that knows a boundary
    // introduced this frame owns no retained layer yet and must be walked
    // past (see `mark_needs_paint_walks_past_a_new_repaint_boundary`).
    assert!(
        layout_owner
            .render_tree()
            .get(inner_id)
            .expect("inner boundary is in the tree")
            .needs_paint(),
        "the inner boundary must carry NEEDS_PAINT, not just sit in the queue"
    );

    assert!(
        queued.contains(&inner_id),
        "the inner boundary re-laid out, so it must be queued for paint; \
         queue held {queued:?} and the inner boundary is {inner_id:?} — \
         `mark_needs_paint` walks UP from the relayout root, so a boundary \
         nested inside that subtree is never reached"
    );
}

/// The same rule holds on the sliver walk.
///
/// The two protocols have separate layout walks, and the sweep this replaced
/// was protocol-agnostic — it walked `children()` and checked a flag, so it
/// covered both. Recording inside the walks does not, unless both walks
/// record. A custom `RenderSliver` that is a repaint boundary re-lays out
/// under a viewport relayout, and the mark the viewport contributes walks
/// UPWARD, so it can never reach a boundary below it.
///
/// The sliver walk has no short-circuit at all — sliver constraints change
/// with scroll position every frame — so arriving in the walk IS the condition
/// the box path has to test for.
#[test]
fn a_sliver_repaint_boundary_that_laid_out_is_queued_for_paint() {
    use flui_objects::RenderViewport;
    use flui_rendering::{
        constraints::SliverGeometry,
        context::{SliverHitTestContext, SliverLayoutContext},
        testing::sliver_node,
        traits::RenderSliver,
    };
    use flui_tree::Leaf;
    use flui_types::layout::AxisDirection;

    /// A sliver that declares itself a repaint boundary and produces a fixed
    /// extent, so the viewport lays it out for real.
    #[derive(Debug, Default)]
    struct BoundarySliver;

    impl flui_foundation::Diagnosticable for BoundarySliver {}

    impl RenderSliver for BoundarySliver {
        type Arity = Leaf;
        type ParentData = flui_rendering::parent_data::SliverParentData;

        fn perform_layout(
            &mut self,
            ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>,
        ) -> SliverGeometry {
            let extent = 40.0_f32.min(ctx.constraints().remaining_paint_extent);
            SliverGeometry::new(40.0, extent, 0.0)
        }

        fn hit_test(&self, _ctx: &mut SliverHitTestContext<'_, Leaf, Self::ParentData>) -> bool {
            false
        }

        fn is_repaint_boundary(&self) -> bool {
            true
        }
    }

    let mut owner = PipelineOwner::new();
    let (root_id, registry) = tree::mount(
        &mut owner,
        box_node(RenderViewport::new(AxisDirection::TopToBottom))
            .child(sliver_node(BoundarySliver).label("sliver-boundary")),
    );
    owner.set_root_id(Some(root_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(200.0), px(200.0)))));

    let sliver_id = registry
        .get("sliver-boundary")
        .expect("sliver boundary is labelled");

    let (owner_idle, result) = owner.run_frame();
    result.expect("the first frame must succeed");
    let mut owner = owner_idle;

    assert!(
        owner.nodes_needing_paint().is_empty(),
        "precondition: the settled frame leaves nothing queued for paint"
    );

    // Dirty the VIEWPORT, above the sliver boundary.
    owner.mark_needs_layout(root_id);

    let mut layout_owner = owner.into_layout();
    layout_owner.run_layout().expect("relayout must succeed");

    assert!(
        layout_owner
            .render_tree()
            .get(sliver_id)
            .expect("sliver boundary is in the tree")
            .needs_paint(),
        "a sliver repaint boundary that re-laid out must be marked for paint — \
         the viewport's own mark walks upward and cannot reach it"
    );
}
