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

use flui_rendering::{
    constraints::{BoxConstraints, GrowthDirection, SliverConstraints, SliverGeometry},
    context::{BoxHitTestContext, BoxLayoutContext},
    objects::RenderSliverToBoxAdapter,
    parent_data::BoxParentData,
    pipeline::PipelineOwner,
    protocol::{BoxProtocol, SliverProtocol},
    testing::{inspect, sliver as sliver_presets},
    traits::{
        HotReloadCapability, PaintEffectsCapability, RenderBox, RenderObject, SemanticsCapability,
    },
};
use flui_tree::Leaf;
use flui_types::{Offset, Rect, Size, geometry::px};

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
struct VerticalBandHitBox;

impl VerticalBandHitBox {
    fn new() -> Self {
        Self
    }
}

impl flui_foundation::Diagnosticable for VerticalBandHitBox {}
impl PaintEffectsCapability for VerticalBandHitBox {}
impl SemanticsCapability for VerticalBandHitBox {}
impl HotReloadCapability for VerticalBandHitBox {}

impl RenderBox for VerticalBandHitBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, Self::ParentData>) -> Size {
        ctx.constraints().constrain(Size::new(px(300.0), px(180.0)))
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Leaf, Self::ParentData>) -> bool {
        let local = ctx.offset();
        local.dx >= px(0.0)
            && local.dx < ctx.own_size().width
            && local.dy >= px(120.0)
            && local.dy < px(140.0)
    }
}

#[derive(Debug)]
struct SliverHost {
    constraints: SliverConstraints,
}

#[test]
fn sliver_to_box_adapter_reverse_growth_hit_tests_box_child_right_way_up() {
    let mut constraints = vertical_constraints(40.0);
    constraints.growth_direction = GrowthDirection::Reverse;

    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(SliverHost { constraints }) as BoxedRenderObject);
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
            Box::new(VerticalBandHitBox::new()) as BoxedRenderObject,
        )
        .expect("box child under sliver adapter");

    let owner = laid_out(owner, root_id);

    assert_eq!(
        hits(&owner, 10.0, 10.0),
        vec![child_id, adapter_id, root_id],
        "reverse growth must mirror Flutter RenderSliverHelpers::hitTestBoxChild: \
         main=10 maps to child-local y=130, not y=50",
    );
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
        ctx.hit_test_child(0, ctx.offset())
    }
}

#[test]
fn sliver_constraints_as_box_constraints_tightens_cross_axis_vertically() {
    let constraints = vertical_constraints(0.0);

    let box_constraints = constraints.as_box_constraints(0.0, f32::INFINITY, None);

    assert_eq!(box_constraints.min_width, px(300.0));
    assert_eq!(box_constraints.max_width, px(300.0));
    assert_eq!(box_constraints.min_height, px(0.0));
    assert_eq!(box_constraints.max_height, px(f32::INFINITY));
}

#[test]
fn sliver_to_box_adapter_lays_out_box_child_and_commits_geometry() {
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
        Offset::new(px(0.0), px(-40.0)),
        "forward vertical adapter positions the Box child at -scroll_offset",
    );
}

#[test]
fn sliver_to_box_adapter_hit_tests_box_child_leaf_first() {
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

    assert_eq!(
        hits(&owner, 10.0, 10.0),
        vec![child_id, adapter_id, root_id],
        "global main=10 maps to child-local y=50 through the committed \
         -scroll_offset paint offset",
    );
    assert!(
        hits(&owner, 10.0, 120.0).is_empty(),
        "per-level sliver gate rejects points beyond geometry.hit_test_extent",
    );
}
