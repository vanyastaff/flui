use flui_rendering::{
    constraints::{BoxConstraints, SliverConstraints, SliverGeometry},
    context::{BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext, SliverLayoutContext},
    objects::{
        RenderSliverFillRemaining, RenderSliverFillRemainingAndOverscroll,
        RenderSliverFillRemainingWithScrollable,
    },
    parent_data::{BoxParentData, SliverPhysicalParentData},
    pipeline::PipelineOwner,
    protocol::{BoxProtocol, SliverProtocol},
    storage::IntrinsicDimension,
    testing::{inspect, sliver as sliver_presets},
    traits::{RenderBox, RenderObject, RenderSliver},
};
use flui_tree::{Leaf, Single};
use flui_types::{Offset, Rect, Size, geometry::px, layout::AxisDirection};

type BoxedRenderObject = Box<dyn RenderObject<BoxProtocol>>;
type BoxedSliverObject = Box<dyn RenderObject<SliverProtocol>>;

fn vertical_constraints(
    scroll_offset: f32,
    preceding_scroll_extent: f32,
    remaining_paint_extent: f32,
    overlap: f32,
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
    inspect::sliver_geometry(owner, id).expect("sliver geometry is committed")
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

fn hits(
    owner: &PipelineOwner<flui_rendering::pipeline::phase::Layout>,
    cross: f32,
    main: f32,
) -> Vec<flui_foundation::RenderId> {
    inspect::hit_path(owner, cross, main)
}

#[derive(Debug)]
struct FixedHitBox {
    desired: Size,
}

impl FixedHitBox {
    fn new(width: f32, height: f32) -> Self {
        Self {
            desired: Size::new(px(width), px(height)),
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
            flui_types::Point::ZERO,
            ctx.own_size(),
        ))
    }

    fn compute_max_intrinsic_width(&self, _height: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.desired.width.get()
    }

    fn compute_max_intrinsic_height(&self, _width: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.desired.height.get()
    }
}

#[derive(Debug)]
struct ExpandingHitBox {
    intrinsic: Size,
}

impl ExpandingHitBox {
    fn new(width: f32, height: f32) -> Self {
        Self {
            intrinsic: Size::new(px(width), px(height)),
        }
    }
}

impl flui_foundation::Diagnosticable for ExpandingHitBox {}

impl RenderBox for ExpandingHitBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, Self::ParentData>) -> Size {
        ctx.constraints().biggest()
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Leaf, Self::ParentData>) -> bool {
        ctx.is_within_bounds(Rect::from_origin_size(
            flui_types::Point::ZERO,
            ctx.own_size(),
        ))
    }

    fn compute_max_intrinsic_width(&self, _height: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.intrinsic.width.get()
    }

    fn compute_max_intrinsic_height(&self, _width: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.intrinsic.height.get()
    }
}

#[derive(Debug, Default)]
struct IntrinsicProbeSliver;

impl flui_foundation::Diagnosticable for IntrinsicProbeSliver {}

impl RenderSliver for IntrinsicProbeSliver {
    type Arity = flui_tree::Single;
    type ParentData = SliverPhysicalParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Single, Self::ParentData>,
    ) -> SliverGeometry {
        let constraints = *ctx.constraints();
        let child_extent = ctx.box_child_intrinsic(
            0,
            IntrinsicDimension::MaxHeight,
            constraints.cross_axis_extent,
        );
        if ctx.child_count() > 0 {
            ctx.layout_box_child(
                0,
                constraints.as_box_constraints(child_extent, child_extent, None),
            );
        }
        let paint_extent = self.calculate_paint_offset(&constraints, 0.0, child_extent);
        SliverGeometry {
            scroll_extent: child_extent,
            paint_extent,
            layout_extent: paint_extent,
            max_paint_extent: paint_extent,
            hit_test_extent: paint_extent,
            cache_extent: self.calculate_cache_offset(&constraints, 0.0, child_extent),
            visible: paint_extent > 0.0,
            ..SliverGeometry::ZERO
        }
    }
}

#[derive(Debug)]
struct SliverHost {
    constraints: SliverConstraints,
}

impl flui_foundation::Diagnosticable for SliverHost {}

impl RenderBox for SliverHost {
    type Arity = flui_tree::Variable;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, flui_tree::Variable, Self::ParentData>,
    ) -> Size {
        if ctx.child_count() > 0 {
            let _ = ctx.layout_sliver_child(0, self.constraints);
        }
        ctx.constraints().biggest()
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

#[test]
fn sliver_layout_context_queries_box_child_intrinsics() {
    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(SliverHost {
        constraints: vertical_constraints(0.0, 0.0, 100.0, 0.0),
    }) as BoxedRenderObject);
    let sliver_id = owner
        .render_tree_mut()
        .insert_sliver_child(root_id, Box::new(IntrinsicProbeSliver) as BoxedSliverObject)
        .expect("probe sliver");
    let child_id = owner
        .render_tree_mut()
        .insert_box_child(
            sliver_id,
            Box::new(FixedHitBox::new(300.0, 140.0)) as BoxedRenderObject,
        )
        .expect("box child");

    let owner = laid_out(owner, root_id);
    let geometry = sliver_geometry(&owner, sliver_id);

    assert_eq!(box_size(&owner, child_id), Size::new(px(300.0), px(140.0)));
    assert_eq!(geometry.scroll_extent, 140.0);
    assert_eq!(geometry.paint_extent, 100.0);
}

#[test]
fn sliver_fill_remaining_uses_child_intrinsic_when_larger_than_remaining_viewport() {
    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(SliverHost {
        constraints: vertical_constraints(0.0, 30.0, 70.0, 0.0),
    }) as BoxedRenderObject);
    let sliver_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(RenderSliverFillRemaining::new()) as BoxedSliverObject,
        )
        .expect("fill remaining sliver");
    let child_id = owner
        .render_tree_mut()
        .insert_box_child(
            sliver_id,
            Box::new(FixedHitBox::new(300.0, 120.0)) as BoxedRenderObject,
        )
        .expect("box child");

    let owner = laid_out(owner, root_id);
    let geometry = sliver_geometry(&owner, sliver_id);

    assert_eq!(box_size(&owner, child_id), Size::new(px(300.0), px(120.0)));
    assert_eq!(geometry.scroll_extent, 120.0);
    assert_eq!(geometry.paint_extent, 70.0);
    assert_eq!(geometry.max_paint_extent, 70.0);
    assert_eq!(geometry.hit_test_extent, 70.0);
    assert_eq!(geometry.cache_extent, 120.0);
}

#[test]
fn sliver_fill_remaining_uses_viewport_remainder_when_child_is_smaller() {
    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(SliverHost {
        constraints: vertical_constraints(0.0, 30.0, 70.0, 0.0),
    }) as BoxedRenderObject);
    let sliver_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(RenderSliverFillRemaining::new()) as BoxedSliverObject,
        )
        .expect("fill remaining sliver");
    let child_id = owner
        .render_tree_mut()
        .insert_box_child(
            sliver_id,
            Box::new(FixedHitBox::new(300.0, 20.0)) as BoxedRenderObject,
        )
        .expect("box child");

    let owner = laid_out(owner, root_id);
    let geometry = sliver_geometry(&owner, sliver_id);

    assert_eq!(box_size(&owner, child_id), Size::new(px(300.0), px(70.0)));
    assert_eq!(geometry.scroll_extent, 70.0);
    assert_eq!(geometry.paint_extent, 70.0);
    assert_eq!(geometry.max_paint_extent, 70.0);
    assert_eq!(geometry.hit_test_extent, 70.0);
}

#[test]
fn sliver_fill_remaining_overscroll_expands_max_paint_extent() {
    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(SliverHost {
        constraints: vertical_constraints(0.0, 20.0, 90.0, -30.0),
    }) as BoxedRenderObject);
    let sliver_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(RenderSliverFillRemainingAndOverscroll::new()) as BoxedSliverObject,
        )
        .expect("fill remaining overscroll sliver");
    let child_id = owner
        .render_tree_mut()
        .insert_box_child(
            sliver_id,
            Box::new(FixedHitBox::new(300.0, 40.0)) as BoxedRenderObject,
        )
        .expect("box child");

    let owner = laid_out(owner, root_id);
    let geometry = sliver_geometry(&owner, sliver_id);

    assert_eq!(box_size(&owner, child_id), Size::new(px(300.0), px(80.0)));
    assert_eq!(geometry.scroll_extent, 80.0);
    assert_eq!(geometry.paint_extent, 90.0);
    assert_eq!(geometry.max_paint_extent, 120.0);
    assert_eq!(geometry.hit_test_extent, 90.0);
    assert_eq!(geometry.cache_extent, 80.0);
}

#[test]
fn sliver_fill_remaining_overscroll_reverse_axis_positions_actual_child_extent() {
    let mut constraints = vertical_constraints(0.0, 20.0, 90.0, -30.0);
    constraints.axis_direction = AxisDirection::BottomToTop;

    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(SliverHost { constraints }) as BoxedRenderObject);
    let sliver_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(RenderSliverFillRemainingAndOverscroll::new()) as BoxedSliverObject,
        )
        .expect("fill remaining overscroll sliver");
    let child_id = owner
        .render_tree_mut()
        .insert_box_child(
            sliver_id,
            Box::new(ExpandingHitBox::new(300.0, 40.0)) as BoxedRenderObject,
        )
        .expect("box child");

    let owner = laid_out(owner, root_id);
    let geometry = sliver_geometry(&owner, sliver_id);

    assert_eq!(box_size(&owner, child_id), Size::new(px(300.0), px(120.0)));
    assert_eq!(geometry.scroll_extent, 80.0);
    assert_eq!(geometry.paint_extent, 90.0);
    assert_eq!(geometry.max_paint_extent, 120.0);
    assert_eq!(
        render_offset(&owner, child_id),
        Offset::new(px(0.0), px(-30.0))
    );
}
