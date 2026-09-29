//! Harness tests for the `RenderSliverFillRemaining` family.

use flui_foundation::Leaf;
use flui_foundation::geometry::{Offset, Rect, Size};
use flui_objects::RenderSliverFillRemainingWithScrollable;
use flui_rendering::{
    constraints::SliverConstraints,
    context::{BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext},
    parent_data::BoxParentData,
    pipeline::PipelineOwner,
    testing::{inspect, sliver as sliver_presets},
    traits::RenderBox,
};

use crate::common::{
    BoxedRenderObject, BoxedSliverObject, laid_out_tight_300x100 as laid_out, sliver_geometry,
};

fn vertical_constraints(
    scroll_offset: f64,
    preceding_scroll_extent: f64,
    remaining_paint_extent: f64,
    overlap: f64,
) -> SliverConstraints {
    sliver_presets::vertical()
        .scroll_offset(scroll_offset)
        .preceding_scroll_extent(preceding_scroll_extent)
        .overlap(overlap)
        .remaining_paint_extent(remaining_paint_extent)
        .cross_axis_extent(300.0)
        .viewport_main_axis_extent(100.0)
        .remaining_cache_extent(120.0)
        .cache_origin(-20.0)
        .build()
}

fn box_size(
    owner: &PipelineOwner<flui_rendering::pipeline::phase::Layout>,
    id: flui_foundation::RenderId,
) -> Size {
    inspect::box_geometry(owner, id).expect("box geometry is committed")
}

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

    fn compute_max_intrinsic_width(&self, _height: f64, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        self.desired.width
    }

    fn compute_max_intrinsic_height(&self, _width: f64, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        self.desired.height
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

pub(crate) fn sliver_fill_remaining_with_scrollable_sizes_child_to_remaining_paint_extent() {
    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(SliverHost {
        constraints: vertical_constraints(0.0, 30.0, 70.0, 0.0),
    }) as BoxedRenderObject);
    let sliver_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(RenderSliverFillRemainingWithScrollable::new()) as BoxedSliverObject,
        )
        .expect("fill remaining sliver");
    let child_id = owner
        .render_tree_mut()
        .insert_box_child(
            sliver_id,
            Box::new(FixedHitBox::new(50.0, 10.0)) as BoxedRenderObject,
        )
        .expect("box child");

    let owner = laid_out(owner, root_id);
    let geometry = sliver_geometry(&owner, sliver_id);

    assert_eq!(box_size(&owner, child_id), Size::new(300.0, 70.0));
    assert_eq!(geometry.scroll_extent, 100.0);
    assert_eq!(geometry.paint_extent, 70.0);
    assert_eq!(geometry.max_paint_extent, 70.0);
    assert_eq!(geometry.hit_test_extent, 70.0);
    assert_eq!(geometry.cache_extent, 100.0);
    assert_eq!(render_offset(&owner, child_id), Offset::ZERO);
    assert!(!geometry.has_visual_overflow);
}
