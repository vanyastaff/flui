use flui_rendering::{
    constraints::{BoxConstraints, GrowthDirection, SliverConstraints, SliverGeometry},
    context::{BoxHitTestContext, BoxLayoutContext},
    hit_testing::HitTestResult,
    objects::RenderSliverFillRemainingWithScrollable,
    parent_data::BoxParentData,
    pipeline::PipelineOwner,
    protocol::{BoxProtocol, SliverProtocol},
    traits::{
        HotReloadCapability, PaintEffectsCapability, RenderBox, RenderObject, SemanticsCapability,
    },
    view::ScrollDirection,
};
use flui_tree::Leaf;
use flui_types::{Offset, Rect, Size, geometry::px, layout::AxisDirection};

type BoxedRenderObject = Box<dyn RenderObject<BoxProtocol>>;
type BoxedSliverObject = Box<dyn RenderObject<SliverProtocol>>;

fn vertical_constraints(
    scroll_offset: f32,
    preceding_scroll_extent: f32,
    remaining_paint_extent: f32,
    overlap: f32,
) -> SliverConstraints {
    SliverConstraints {
        axis_direction: AxisDirection::TopToBottom,
        cross_axis_direction: AxisDirection::LeftToRight,
        growth_direction: GrowthDirection::Forward,
        user_scroll_direction: ScrollDirection::Idle,
        scroll_offset,
        preceding_scroll_extent,
        overlap,
        remaining_paint_extent,
        cross_axis_extent: 300.0,
        viewport_main_axis_extent: 100.0,
        remaining_cache_extent: 120.0,
        cache_origin: -20.0,
    }
}

fn laid_out(
    mut owner: PipelineOwner,
    root: flui_foundation::RenderId,
) -> PipelineOwner<flui_rendering::pipeline::phase::Layout> {
    owner.set_root_id(Some(root));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(300.0), px(100.0)))));
    let mut owner = owner.into_layout();
    owner.run_layout().expect("layout succeeds");
    owner
}

fn sliver_geometry(
    owner: &PipelineOwner<flui_rendering::pipeline::phase::Layout>,
    id: flui_foundation::RenderId,
) -> SliverGeometry {
    owner
        .render_tree()
        .get(id)
        .and_then(|node| node.as_sliver())
        .and_then(|entry| entry.state().geometry())
        .expect("sliver geometry is committed")
}

fn box_size(
    owner: &PipelineOwner<flui_rendering::pipeline::phase::Layout>,
    id: flui_foundation::RenderId,
) -> Size {
    owner
        .render_tree()
        .get(id)
        .and_then(|node| node.as_box())
        .and_then(|entry| entry.state().geometry())
        .expect("box geometry is committed")
}

fn render_offset(
    owner: &PipelineOwner<flui_rendering::pipeline::phase::Layout>,
    id: flui_foundation::RenderId,
) -> Offset {
    owner
        .render_tree()
        .get(id)
        .map(flui_rendering::storage::RenderNode::offset)
        .expect("node exists")
}

fn hits(
    owner: &PipelineOwner<flui_rendering::pipeline::phase::Layout>,
    cross: f32,
    main: f32,
) -> Vec<flui_foundation::RenderId> {
    let mut result = HitTestResult::new();
    owner.hit_test(Offset::new(px(cross), px(main)), &mut result);
    result.path().iter().map(|entry| entry.target).collect()
}

#[derive(Debug)]
struct FixedHitBox {
    desired: Size,
    size: Size,
}

impl FixedHitBox {
    fn new(width: f32, height: f32) -> Self {
        Self {
            desired: Size::new(px(width), px(height)),
            size: Size::ZERO,
        }
    }
}

impl flui_foundation::Diagnosticable for FixedHitBox {}
impl PaintEffectsCapability for FixedHitBox {}
impl SemanticsCapability for FixedHitBox {}
impl HotReloadCapability for FixedHitBox {}

impl RenderBox for FixedHitBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, Self::ParentData>) {
        self.size = ctx.constraints().constrain(self.desired);
        ctx.complete_with_size(self.size);
    }

    fn size(&self) -> &Size {
        &self.size
    }

    fn size_mut(&mut self) -> &mut Size {
        &mut self.size
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Leaf, Self::ParentData>) -> bool {
        ctx.is_within_bounds(Rect::from_origin_size(flui_types::Point::ZERO, self.size))
    }
}

#[derive(Debug)]
struct SliverHost {
    constraints: SliverConstraints,
    size: Size,
}

impl flui_foundation::Diagnosticable for SliverHost {}
impl PaintEffectsCapability for SliverHost {}
impl SemanticsCapability for SliverHost {}
impl HotReloadCapability for SliverHost {}

impl RenderBox for SliverHost {
    type Arity = flui_tree::Variable;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, flui_tree::Variable, Self::ParentData>,
    ) {
        if ctx.child_count() > 0 {
            let _ = ctx.layout_sliver_child(0, self.constraints);
        }
        self.size = ctx.constraints().biggest();
        ctx.complete_with_size(self.size);
    }

    fn size(&self) -> &Size {
        &self.size
    }

    fn size_mut(&mut self) -> &mut Size {
        &mut self.size
    }

    fn hit_test(
        &self,
        ctx: &mut BoxHitTestContext<'_, flui_tree::Variable, Self::ParentData>,
    ) -> bool {
        ctx.hit_test_child(0, ctx.offset())
    }
}

#[test]
fn sliver_fill_remaining_with_scrollable_sizes_child_to_remaining_paint_extent() {
    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(SliverHost {
        constraints: vertical_constraints(0.0, 30.0, 70.0, 0.0),
        size: Size::ZERO,
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

    assert_eq!(box_size(&owner, child_id), Size::new(px(300.0), px(70.0)));
    assert_eq!(geometry.scroll_extent, 100.0);
    assert_eq!(geometry.paint_extent, 70.0);
    assert_eq!(geometry.max_paint_extent, 70.0);
    assert_eq!(geometry.hit_test_extent, 70.0);
    assert_eq!(geometry.cache_extent, 100.0);
    assert_eq!(render_offset(&owner, child_id), Offset::ZERO);
    assert!(!geometry.has_visual_overflow);
}

#[test]
fn sliver_fill_remaining_with_scrollable_includes_negative_overlap_in_child_extent() {
    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(SliverHost {
        constraints: vertical_constraints(0.0, 0.0, 80.0, -20.0),
        size: Size::ZERO,
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

    assert_eq!(box_size(&owner, child_id), Size::new(px(300.0), px(100.0)));
    assert_eq!(geometry.scroll_extent, 100.0);
    assert_eq!(geometry.paint_extent, 80.0);
    assert_eq!(geometry.max_paint_extent, 80.0);
    assert_eq!(geometry.hit_test_extent, 80.0);
    assert!(
        geometry.has_visual_overflow,
        "extent includes 20px negative overlap and exceeds the remaining paint extent",
    );
}

#[test]
fn sliver_fill_remaining_with_scrollable_keeps_zero_extent_child_in_cache_window() {
    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(SliverHost {
        constraints: vertical_constraints(110.0, 100.0, 0.0, 0.0),
        size: Size::ZERO,
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

    assert_eq!(
        box_size(&owner, child_id),
        Size::new(px(300.0), px(10.0)),
        "when visible extent is zero but cache extent is non-zero, Flutter uses cache extent as maxExtent",
    );
    assert_eq!(geometry.scroll_extent, 100.0);
    assert_eq!(geometry.paint_extent, 0.0);
    assert_eq!(geometry.max_paint_extent, 0.0);
    assert_eq!(geometry.hit_test_extent, 0.0);
    assert_eq!(geometry.cache_extent, 10.0);
    assert!(geometry.has_visual_overflow);
    assert!(
        hits(&owner, 10.0, 0.0).is_empty(),
        "zero hit_test_extent gates child hits even while cache keeps layout alive",
    );
}
