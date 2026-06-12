//! `RenderSliverFixedExtentList` — direct Box children with a fixed main-axis extent.

use flui_rendering::{
    constraints::{BoxConstraints, SliverConstraints, SliverGeometry},
    context::{BoxHitTestContext, BoxLayoutContext},
    objects::RenderSliverFixedExtentList,
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
    x: f32,
    y: f32,
) -> Vec<flui_foundation::RenderId> {
    inspect::hit_path(owner, x, y)
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
        ctx.hit_test_child_at_layout_offset(0)
    }
}

fn fixed_extent_tree(
    constraints: SliverConstraints,
    item_extent: f32,
    child_count: usize,
) -> (
    PipelineOwner<flui_rendering::pipeline::phase::Layout>,
    flui_foundation::RenderId,
    flui_foundation::RenderId,
    Vec<flui_foundation::RenderId>,
) {
    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(SliverHost {
        constraints,
        size: Size::ZERO,
    }) as BoxedRenderObject);
    let sliver_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(RenderSliverFixedExtentList::new(item_extent)) as BoxedSliverObject,
        )
        .expect("fixed extent sliver");
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
fn sliver_fixed_extent_list_sizes_children_to_item_extent() {
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
        assert_eq!(box_size(&owner, child_id), Size::new(px(300.0), px(30.0)));
    }
    assert_eq!(
        render_offset(&owner, child_ids[0]),
        Offset::new(px(0.0), px(-25.0)),
    );
    assert_eq!(
        render_offset(&owner, child_ids[1]),
        Offset::new(px(0.0), px(5.0)),
    );
    assert_eq!(
        render_offset(&owner, child_ids[2]),
        Offset::new(px(0.0), px(35.0)),
    );
    assert_eq!(
        render_offset(&owner, child_ids[3]),
        Offset::new(px(0.0), px(65.0)),
    );
}

#[test]
fn sliver_fixed_extent_list_hit_tests_visible_children() {
    let (owner, root_id, sliver_id, child_ids) =
        fixed_extent_tree(vertical_constraints(25.0), 30.0, 4);

    assert_eq!(
        hits(&owner, 10.0, 10.0),
        vec![child_ids[1], sliver_id, root_id],
        "global y=10 maps to child 1 after the 25px scroll offset",
    );
    assert_eq!(
        hits(&owner, 10.0, 50.0),
        vec![child_ids[2], sliver_id, root_id],
        "global y=50 maps to child 2 after the 25px scroll offset",
    );
    assert!(
        hits(&owner, 10.0, 110.0).is_empty(),
        "per-level sliver gate rejects points beyond geometry.hit_test_extent",
    );
}

#[test]
fn sliver_fixed_extent_list_supports_horizontal_axis() {
    let (owner, _root_id, _sliver_id, child_ids) =
        fixed_extent_tree(horizontal_constraints(30.0), 80.0, 2);

    for &child_id in &child_ids {
        assert_eq!(box_size(&owner, child_id), Size::new(px(80.0), px(100.0)));
    }
    assert_eq!(
        render_offset(&owner, child_ids[0]),
        Offset::new(px(-30.0), px(0.0)),
    );
    assert_eq!(
        render_offset(&owner, child_ids[1]),
        Offset::new(px(50.0), px(0.0)),
    );
}

#[test]
fn sliver_fixed_extent_list_reverse_axis_uses_right_way_up_offsets() {
    let mut constraints = vertical_constraints(25.0);
    constraints.axis_direction = AxisDirection::BottomToTop;
    let (owner, _root_id, _sliver_id, child_ids) = fixed_extent_tree(constraints, 30.0, 4);

    assert_eq!(
        render_offset(&owner, child_ids[0]),
        Offset::new(px(0.0), px(90.0)),
    );
    assert_eq!(
        render_offset(&owner, child_ids[1]),
        Offset::new(px(0.0), px(60.0)),
    );
    assert_eq!(
        render_offset(&owner, child_ids[2]),
        Offset::new(px(0.0), px(30.0)),
    );
    assert_eq!(
        render_offset(&owner, child_ids[3]),
        Offset::new(px(0.0), px(0.0)),
    );
}
