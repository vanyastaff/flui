//! `RenderSliverFixedExtentList` — direct Box children with a fixed main-axis extent.

use flui_foundation::Leaf;
use flui_foundation::geometry::{Offset, Rect, Size};
use flui_objects::RenderSliverFixedExtentList;
use flui_rendering::{
    constraints::SliverConstraints,
    context::{BoxHitTestContext, BoxLayoutContext},
    parent_data::{BoxParentData, SliverMultiBoxAdaptorParentData},
    pipeline::PipelineOwner,
    testing::inspect,
    traits::RenderBox,
};

use crate::common::{
    BoxedRenderObject, BoxedSliverObject, laid_out_tight_300x100 as laid_out, sliver_geometry,
    vertical_constraints,
};

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
        ctx.hit_test_child_at_layout_offset(0)
    }
}

fn fixed_extent_tree(
    constraints: SliverConstraints,
    item_extent: f64,
    child_count: usize,
) -> (
    PipelineOwner<flui_rendering::pipeline::phase::Layout>,
    flui_foundation::RenderId,
    flui_foundation::RenderId,
    Vec<flui_foundation::RenderId>,
) {
    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(SliverHost { constraints }) as BoxedRenderObject);
    let sliver_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(RenderSliverFixedExtentList::new(item_extent, child_count))
                as BoxedSliverObject,
        )
        .expect("fixed extent sliver");
    let mut child_ids = Vec::with_capacity(child_count);
    for index in 0..child_count {
        let child_id = owner
            .render_tree_mut()
            .insert_box_child(
                sliver_id,
                Box::new(FixedHitBox::new(1000.0, 1000.0)) as BoxedRenderObject,
            )
            .expect("box child");
        // The logical index the element tree stamps at adoption.
        owner
            .render_tree_mut()
            .get_mut(child_id)
            .expect("just inserted")
            .set_parent_data(Box::new(SliverMultiBoxAdaptorParentData::new(index)));
        child_ids.push(child_id);
    }

    (laid_out(owner, root_id), root_id, sliver_id, child_ids)
}

pub(crate) fn sliver_fixed_extent_list_sizes_children_to_item_extent() {
    let (owner, _root_id, sliver_id, child_ids) =
        fixed_extent_tree(vertical_constraints(25.0), 30.0, 4);

    let geometry = sliver_geometry(&owner, sliver_id);
    assert_eq!(geometry.scroll_extent, 120.0);
    assert_eq!(geometry.paint_extent, 95.0);
    assert_eq!(geometry.layout_extent, 95.0);
    assert_eq!(geometry.max_paint_extent, 120.0);
    assert_eq!(geometry.hit_test_extent, 95.0);
    assert_eq!(geometry.cache_extent, 115.0);
    assert!(geometry.has_visual_overflow);

    // The Diagnosticable-backed dump surfaces the sliver's committed
    // geometry (cross-protocol: sliver nodes carry a `geometry` property,
    // box nodes carry `size`). Exercises `PipelineOwner::debug_diagnostics_tree`
    // and the foundation `DiagnosticsNode` query getters.
    let diagnostics = owner
        .debug_diagnostics_tree()
        .expect("laid-out tree has a diagnostics root");
    let sliver = diagnostics
        .find_descendant("RenderSliverFixedExtentList")
        .expect("the host's sliver child appears in the diagnostics tree");
    assert!(
        sliver.get_property("geometry").is_some(),
        "the sliver self-reports its committed geometry in the dump",
    );

    for &child_id in &child_ids {
        assert_eq!(box_size(&owner, child_id), Size::new(300.0, 30.0));
    }
    assert_eq!(render_offset(&owner, child_ids[0]), Offset::new(0.0, -25.0),);
    assert_eq!(render_offset(&owner, child_ids[1]), Offset::new(0.0, 5.0),);
    assert_eq!(render_offset(&owner, child_ids[2]), Offset::new(0.0, 35.0),);
    assert_eq!(render_offset(&owner, child_ids[3]), Offset::new(0.0, 65.0),);
}
