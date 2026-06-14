//! `RenderSliverFillViewport` — direct Box children with viewport-sized extents.

use flui_rendering::{
    constraints::{BoxConstraints, SliverConstraints, SliverGeometry},
    context::{BoxHitTestContext, BoxLayoutContext},
    objects::RenderSliverFillViewport,
    parent_data::BoxParentData,
    pipeline::PipelineOwner,
    protocol::{BoxProtocol, SliverProtocol},
    testing::{inspect, sliver as sliver_presets},
    traits::{
        HotReloadCapability, PaintEffectsCapability, RenderBox, RenderObject, SemanticsCapability,
    },
};
use flui_tree::Leaf;
use flui_types::{Offset, Rect, Size, geometry::px, layout::AxisDirection};

type BoxedRenderObject = Box<dyn RenderObject<BoxProtocol>>;
type BoxedSliverObject = Box<dyn RenderObject<SliverProtocol>>;

fn vertical_constraints(scroll_offset: f32) -> SliverConstraints {
    sliver_presets::vertical()
        .scroll_offset(scroll_offset)
        .remaining_paint_extent(100.0)
        .cross_axis_extent(300.0)
        .viewport_main_axis_extent(100.0)
        .remaining_cache_extent(120.0)
        .cache_origin(-20.0)
        .build()
}

fn horizontal_constraints(scroll_offset: f32) -> SliverConstraints {
    sliver_presets::horizontal()
        .scroll_offset(scroll_offset)
        .remaining_paint_extent(300.0)
        .cross_axis_extent(100.0)
        .viewport_main_axis_extent(300.0)
        .remaining_cache_extent(320.0)
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
impl PaintEffectsCapability for FixedHitBox {}
impl SemanticsCapability for FixedHitBox {}
impl HotReloadCapability for FixedHitBox {}

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
}

#[derive(Debug)]
struct SliverHost {
    constraints: SliverConstraints,
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
        ctx.hit_test_child_at_layout_offset(0)
    }
}

fn fill_viewport_tree(
    constraints: SliverConstraints,
    viewport_fraction: f32,
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
            Box::new(RenderSliverFillViewport::new(viewport_fraction)) as BoxedSliverObject,
        )
        .expect("fill viewport sliver");
    let mut child_ids = Vec::with_capacity(child_count);
    for _ in 0..child_count {
        let child_id = owner
            .render_tree_mut()
            .insert_box_child(
                sliver_id,
                Box::new(FixedHitBox::new(1000.0, 1000.0)) as BoxedRenderObject,
            )
            .expect("box child");
        child_ids.push(child_id);
    }

    (laid_out(owner, root_id), root_id, sliver_id, child_ids)
}

#[test]
fn sliver_fill_viewport_sizes_children_to_viewport_fraction() {
    let (owner, _root_id, sliver_id, child_ids) =
        fill_viewport_tree(vertical_constraints(40.0), 0.5, 3);

    let geometry = sliver_geometry(&owner, sliver_id);
    assert_eq!(geometry.scroll_extent, 150.0);
    assert_eq!(geometry.paint_extent, 100.0);
    assert_eq!(geometry.layout_extent, 100.0);
    assert_eq!(geometry.max_paint_extent, 150.0);
    assert_eq!(geometry.hit_test_extent, 100.0);
    assert_eq!(geometry.cache_extent, 120.0);
    assert!(geometry.has_visual_overflow);

    assert_eq!(
        box_size(&owner, child_ids[0]),
        Size::new(px(300.0), px(50.0))
    );
    assert_eq!(
        box_size(&owner, child_ids[1]),
        Size::new(px(300.0), px(50.0))
    );
    assert_eq!(
        box_size(&owner, child_ids[2]),
        Size::new(px(300.0), px(50.0))
    );
    assert_eq!(
        render_offset(&owner, child_ids[0]),
        Offset::new(px(0.0), px(-40.0)),
    );
    assert_eq!(
        render_offset(&owner, child_ids[1]),
        Offset::new(px(0.0), px(10.0)),
    );
    assert_eq!(
        render_offset(&owner, child_ids[2]),
        Offset::new(px(0.0), px(60.0)),
    );
}

#[test]
fn sliver_fill_viewport_hit_tests_visible_page_children() {
    let (owner, root_id, sliver_id, child_ids) =
        fill_viewport_tree(vertical_constraints(40.0), 0.5, 3);

    assert_eq!(
        hits(&owner, 10.0, 20.0),
        vec![child_ids[1], sliver_id, root_id],
        "global y=20 maps to child 1 after the 40px scroll offset",
    );
    assert_eq!(
        hits(&owner, 10.0, 70.0),
        vec![child_ids[2], sliver_id, root_id],
        "global y=70 maps to child 2 after the 40px scroll offset",
    );
}

#[test]
fn sliver_fill_viewport_supports_horizontal_axis() {
    let (owner, _root_id, _sliver_id, child_ids) =
        fill_viewport_tree(horizontal_constraints(30.0), 0.25, 2);

    assert_eq!(
        box_size(&owner, child_ids[0]),
        Size::new(px(75.0), px(100.0))
    );
    assert_eq!(
        box_size(&owner, child_ids[1]),
        Size::new(px(75.0), px(100.0))
    );
    assert_eq!(
        render_offset(&owner, child_ids[0]),
        Offset::new(px(-30.0), px(0.0)),
    );
    assert_eq!(
        render_offset(&owner, child_ids[1]),
        Offset::new(px(45.0), px(0.0)),
    );
}

#[test]
fn sliver_fill_viewport_reverse_axis_uses_right_way_up_offsets() {
    let mut constraints = vertical_constraints(40.0);
    constraints.axis_direction = AxisDirection::BottomToTop;
    let (owner, root_id, sliver_id, child_ids) = fill_viewport_tree(constraints, 0.5, 3);

    assert_eq!(
        render_offset(&owner, child_ids[0]),
        Offset::new(px(0.0), px(90.0)),
    );
    assert_eq!(
        render_offset(&owner, child_ids[1]),
        Offset::new(px(0.0), px(40.0)),
    );
    assert_eq!(
        render_offset(&owner, child_ids[2]),
        Offset::new(px(0.0), px(-10.0)),
    );
    assert_eq!(
        hits(&owner, 10.0, 20.0),
        vec![child_ids[2], sliver_id, root_id]
    );
    assert_eq!(
        hits(&owner, 10.0, 70.0),
        vec![child_ids[1], sliver_id, root_id]
    );
}
