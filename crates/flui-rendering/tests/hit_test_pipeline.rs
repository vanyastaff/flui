//! Pipeline hit-test walk — the query twin of the fragment paint walk.
//!
//! Pins the bridge that used to be dead end-to-end (`hit_test_raw`
//! blanket returned `false`, the ctx child recursion was a stub, the
//! registry `RenderView::hit_test` answered `true` with no entries):
//!
//! 1. hits recurse through real children with leaf-first entries;
//! 2. children are tested at their laid-out `RenderState.offset` —
//!    parents no longer mirror offsets in their own fields
//!    (`hit_test_child_at_layout_offset`, Flex's `Vec<Offset>` is gone);
//! 3. a transform parent hit-tests through the INVERSE of its paint
//!    matrix; child descent records paint offsets on the result
//!    transform stack for gesture dispatch.

use flui_foundation::geometry::{Offset, Size};
use flui_foundation::{Leaf, Variable};
use flui_objects::{
    RenderColoredBox, RenderFlex, RenderPadding, RenderSliverIgnorePointer, RenderTransform,
};
use flui_rendering::constraints::AxisDirection;
use flui_rendering::{
    constraints::{GrowthDirection, SliverConstraints, SliverGeometry},
    context::{BoxHitTestContext, BoxLayoutContext, SliverHitTestContext, SliverLayoutContext},
    parent_data::{BoxParentData, SliverParentData},
    pipeline::PipelineOwner,
    testing::inspect,
    traits::{RenderBox, RenderSliver},
    view::ScrollDirection,
};

use crate::common::{BoxedRenderObject, BoxedSliverObject, laid_out_loose_200x200 as laid_out};

fn hits(
    owner: &flui_rendering::pipeline::PipelineOwner<flui_rendering::pipeline::phase::Layout>,
    x: f64,
    y: f64,
) -> Vec<flui_foundation::RenderId> {
    inspect::hit_path(owner, x, y)
}

fn render_offset(
    owner: &flui_rendering::pipeline::PipelineOwner<flui_rendering::pipeline::phase::Layout>,
    id: flui_foundation::RenderId,
) -> Offset {
    inspect::render_offset(owner, id).expect("node exists")
}

// ============================================================================
// 1. Leaf-first recursion through a positioned child
// ============================================================================

#[test]
fn padding_child_hits_leaf_first_at_laid_out_offset() {
    let mut owner = PipelineOwner::new();
    let padding_id = owner.insert(Box::new(RenderPadding::all(5.0)) as BoxedRenderObject);
    let child_id = owner
        .insert_child_render_object(padding_id, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("child insert");
    let owner = laid_out(owner, padding_id);

    // (20,20) → child-local (15,15) inside the 40×40 box.
    assert_eq!(
        hits(&owner, 20.0, 20.0),
        vec![child_id, padding_id],
        "hit path must be leaf-first: the colored child, then padding",
    );

    // (3,3) → child-local (-2,-2): inside padding's own area but the
    // padding is hit-transparent (Flutter parity — it forwards to the
    // child only).
    assert!(
        hits(&owner, 3.0, 3.0).is_empty(),
        "padding's own border area claims no hit",
    );
}

// ============================================================================
// 2. Variadic children hit at RenderState offsets (no parent-side Vec)
// ============================================================================

// ============================================================================
// 3. D8 gate: hit-test under transform walks the inverse paint matrix
// ============================================================================

#[test]
fn transform_child_hits_through_inverse_matrix() {
    let mut owner = PipelineOwner::new();
    let transform_id =
        owner.insert(Box::new(RenderTransform::scale(2.0, 2.0)) as BoxedRenderObject);
    let child_id = owner
        .insert_child_render_object(transform_id, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("child insert");
    let owner = laid_out(owner, transform_id);

    // Visual (50,50) under scale(2,2) came from child-local (25,25) —
    // inside the 40×40 child.
    assert_eq!(
        hits(&owner, 50.0, 50.0),
        vec![child_id, transform_id],
        "the child must receive the inverse-transformed point",
    );

    // Visual (90,90) → child-local (45,45) — outside the child even
    // though it is inside the SCALED visual bounds; without the
    // inverse the naive point would still hit.
    assert!(
        hits(&owner, 90.0, 90.0).is_empty(),
        "outside the inverse-mapped child bounds → miss",
    );
}

// ============================================================================
// 4. RenderFlex itself — FlexParentData through the erased driver
// ============================================================================

/// The production walk's parent-data storage is erased; the typed
/// bridge creates FlexParentData slots lazily. Before that, this exact
/// tree PANICKED in from_erased (the walk hardcoded BoxParentData) —
/// Flex/Stack were impossible in production layout.
#[test]
fn flex_lays_out_and_hits_children_at_layout_offsets() {
    let mut owner = PipelineOwner::new();
    let flex_id = owner.insert(Box::new(RenderFlex::row()) as BoxedRenderObject);
    let first = owner
        .insert_child_render_object(flex_id, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("child 0");
    let second = owner
        .insert_child_render_object(flex_id, Box::new(RenderColoredBox::blue(40.0, 40.0)))
        .expect("child 1");
    let owner = laid_out(owner, flex_id);

    // Layout committed real offsets: the second child sits at x=40.
    let second_offset = owner
        .render_tree()
        .get(second)
        .and_then(|n| n.as_box())
        .map(|e| e.state().offset())
        .expect("child 1 state");
    assert_eq!(
        second_offset,
        Offset::new(40.0, 0.0),
        "row layout must commit the second child's offset to RenderState",
    );

    assert_eq!(
        hits(&owner, 10.0, 10.0).first().copied(),
        Some(first),
        "(10,10) lands in the first flex child",
    );
    assert_eq!(
        hits(&owner, 50.0, 10.0).first().copied(),
        Some(second),
        "(50,10) lands in the second flex child at its laid-out offset",
    );
}

// ============================================================================
// 5. Sliver subtree hit-testing through a Box host
// ============================================================================

fn sliver_hit_constraints() -> SliverConstraints {
    SliverConstraints {
        axis_direction: AxisDirection::TopToBottom,
        cross_axis_direction: AxisDirection::LeftToRight,
        growth_direction: GrowthDirection::Forward,
        user_scroll_direction: ScrollDirection::Idle,
        scroll_offset: 0.0,
        preceding_scroll_extent: 0.0,
        overlap: 0.0,
        remaining_paint_extent: 200.0,
        cross_axis_extent: 100.0,
        viewport_main_axis_extent: 200.0,
        remaining_cache_extent: 200.0,
        cache_origin: 0.0,
    }
}

#[derive(Debug)]
struct SliverHitHost {
    constraints: SliverConstraints,
}

impl flui_foundation::Diagnosticable for SliverHitHost {}

impl RenderBox for SliverHitHost {
    type Arity = Variable;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>) -> Size {
        if ctx.child_count() > 0 {
            let _ = ctx.layout_sliver_child(0, self.constraints);
        }
        ctx.constraints().biggest()
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Variable, BoxParentData>) -> bool {
        ctx.hit_test_child(0, ctx.offset())
    }
}

#[derive(Debug)]
struct PositionedSliverHitHost {
    constraints: SliverConstraints,
    offset: Offset,
    position_child: bool,
}

impl flui_foundation::Diagnosticable for PositionedSliverHitHost {}

impl RenderBox for PositionedSliverHitHost {
    type Arity = Variable;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>) -> Size {
        if ctx.child_count() > 0 {
            let _ = ctx.layout_sliver_child(0, self.constraints);
            if self.position_child {
                ctx.position_child(0, self.offset);
            }
        }
        ctx.constraints().biggest()
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Variable, BoxParentData>) -> bool {
        ctx.hit_test_child_at_layout_offset(0)
    }
}

#[derive(Debug, Default)]
struct HitLeafSliver {
    /// Cross-axis extent captured at layout, read by the `&self`-only
    /// `hit_test` (the sliver hit-test context does not carry it).
    cross_axis_extent: f64,
}

impl flui_foundation::Diagnosticable for HitLeafSliver {}

impl RenderSliver for HitLeafSliver {
    type Arity = Leaf;
    type ParentData = SliverParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>,
    ) -> SliverGeometry {
        self.cross_axis_extent = ctx.constraints().cross_axis_extent;
        SliverGeometry {
            scroll_extent: 80.0,
            paint_extent: 80.0,
            layout_extent: 80.0,
            max_paint_extent: 80.0,
            hit_test_extent: 80.0,
            visible: true,
            ..SliverGeometry::ZERO
        }
    }

    fn hit_test(&self, ctx: &mut SliverHitTestContext<'_, Leaf, Self::ParentData>) -> bool {
        // The geometry's hit_test_extent is the fixed 80.0 this double reports.
        ctx.is_within_main_axis_range(0.0, 80.0)
            && ctx.is_within_cross_axis_range(0.0, self.cross_axis_extent)
    }
}

#[test]
fn box_host_hit_tests_sliver_proxy_subtree_leaf_first() {
    let mut owner = PipelineOwner::new();
    let host_id = owner.insert(Box::new(SliverHitHost {
        constraints: sliver_hit_constraints(),
    }) as BoxedRenderObject);
    let proxy_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            host_id,
            Box::new(RenderSliverIgnorePointer::new(false)) as BoxedSliverObject,
        )
        .expect("sliver proxy child");
    let leaf_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            proxy_id,
            Box::new(HitLeafSliver::default()) as BoxedSliverObject,
        )
        .expect("sliver leaf child");

    let owner = laid_out(owner, host_id);

    assert_eq!(
        hits(&owner, 10.0, 10.0),
        vec![leaf_id, proxy_id, host_id],
        "hit path must cross Box -> SliverIgnorePointer -> leaf Sliver and remain leaf-first",
    );
    assert!(
        hits(&owner, 10.0, 120.0).is_empty(),
        "main-axis position beyond the leaf sliver's hit extent must miss",
    );
}

#[test]
fn box_parent_preserves_unpositioned_sliver_child_offset_across_relayout() {
    let mut owner = PipelineOwner::new();
    let host_id = owner.insert(Box::new(PositionedSliverHitHost {
        constraints: sliver_hit_constraints(),
        offset: Offset::new(0.0, 20.0),
        position_child: true,
    }) as BoxedRenderObject);
    let leaf_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            host_id,
            Box::new(HitLeafSliver::default()) as BoxedSliverObject,
        )
        .expect("sliver leaf child");

    let owner = laid_out(owner, host_id);
    assert_eq!(render_offset(&owner, leaf_id), Offset::new(0.0, 20.0));

    let mut owner = owner.into_idle();
    {
        let node = owner
            .render_tree_mut()
            .get_mut(host_id)
            .expect("host in tree");
        let entry = node.as_box_mut().expect("box entry");
        let host = entry
            .render_object_mut()
            .as_any_mut()
            .downcast_mut::<PositionedSliverHitHost>()
            .expect("positioned host downcast");
        host.position_child = false;
    }
    owner.mark_needs_layout(host_id);
    let mut owner = owner.into_layout();
    owner.run_layout().expect("relayout succeeds");

    assert_eq!(
        render_offset(&owner, leaf_id),
        Offset::new(0.0, 20.0),
        "a Box parent that lays out a Sliver child without re-positioning \
         it must preserve the child's previous offset",
    );
}
