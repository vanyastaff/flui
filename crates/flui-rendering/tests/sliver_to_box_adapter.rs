//! Cross-protocol child layout — Sliver parent lays out a Box child.
//!
//! Core.2 W3.3: verifies the reverse bridge of PR #187/#188. A
//! `RenderSliverToBoxAdapter` is a Sliver-protocol parent with a
//! Box-protocol child. It must:
//!
//! 1. derive tight-cross-axis `BoxConstraints` from `SliverConstraints`;
//! 2. lay out the Box child through the pipeline's Sliver -> Box callback;
//! 3. compose Flutter-parity sliver geometry from the child's main-axis size;
//! 4. commit the child's paint offset so hit-test/paint use the same source.

use flui_foundation::Leaf;
use flui_foundation::geometry::{Offset, Rect, Size};
use flui_objects::RenderSliverToBoxAdapter;
use flui_rendering::{
    constraints::SliverConstraints,
    context::{BoxHitTestContext, BoxLayoutContext},
    parent_data::BoxParentData,
    pipeline::PipelineOwner,
    testing::inspect,
    traits::RenderBox,
};

use crate::common::{
    BoxedRenderObject, BoxedSliverObject, laid_out_tight_300x100 as laid_out, sliver_geometry,
    vertical_constraints,
};

fn render_offset(
    owner: &PipelineOwner<flui_rendering::pipeline::phase::Layout>,
    id: flui_foundation::RenderId,
) -> Offset {
    inspect::render_offset(owner, id).expect("node exists")
}

#[derive(Debug)]
struct FixedHitBox {
    desired: Size,
}

impl FixedHitBox {
    fn new(width: f64, height: f64) -> Self {
        Self {
            desired: Size::new(width, height),
        }
    }
}

impl flui_foundation::Diagnosticable for FixedHitBox {}

impl RenderBox for FixedHitBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, Self::ParentData>) -> Size {
        ctx.constraints().constrain(self.desired)
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Leaf, Self::ParentData>) -> bool {
        ctx.is_within_bounds(Rect::from_origin_size(
            flui_foundation::geometry::Point::ZERO,
            ctx.own_size(),
        ))
    }
}

#[derive(Debug)]
struct SliverHost {
    constraints: SliverConstraints,
}

impl flui_foundation::Diagnosticable for SliverHost {}

impl RenderBox for SliverHost {
    type Arity = flui_foundation::Variable;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, flui_foundation::Variable, Self::ParentData>,
    ) -> Size {
        if ctx.child_count() > 0 {
            let _ = ctx.layout_sliver_child(0, self.constraints);
        }
        ctx.constraints().biggest()
    }

    fn hit_test(
        &self,
        ctx: &mut BoxHitTestContext<'_, flui_foundation::Variable, Self::ParentData>,
    ) -> bool {
        ctx.hit_test_child(0, ctx.offset())
    }
}

pub(crate) fn sliver_to_box_adapter_lays_out_box_child_and_commits_geometry() {
    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(SliverHost {
        constraints: vertical_constraints(40.0),
    }) as BoxedRenderObject);
    let adapter_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(RenderSliverToBoxAdapter::new()) as BoxedSliverObject,
        )
        .expect("sliver adapter child");
    let child_id = owner
        .render_tree_mut()
        .insert_box_child(
            adapter_id,
            Box::new(FixedHitBox::new(50.0, 180.0)) as BoxedRenderObject,
        )
        .expect("box child under sliver adapter");

    let owner = laid_out(owner, root_id);

    let geometry = sliver_geometry(&owner, adapter_id);
    assert_eq!(geometry.scroll_extent, 180.0);
    assert_eq!(geometry.paint_extent, 100.0);
    assert_eq!(geometry.cache_extent, 120.0);
    assert_eq!(geometry.max_paint_extent, 180.0);
    assert_eq!(geometry.hit_test_extent, 100.0);
    assert!(
        geometry.has_visual_overflow,
        "child extends beyond remaining paint extent and scroll_offset > 0",
    );
    assert_eq!(
        render_offset(&owner, child_id),
        Offset::new(0.0, -40.0),
        "forward vertical adapter positions the Box child at -scroll_offset",
    );
}
