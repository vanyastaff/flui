//! Integration matrix for sliver hit-test coordinate conversion (Wave 3.2).
//!
//! Exercises 4 axis directions × 2 growth directions through a Box host that
//! lays out a band sliver with explicit [`SliverConstraints`].

use flui_rendering::{
    constraints::{BoxConstraints, GrowthDirection, SliverConstraints, SliverGeometry},
    context::{BoxHitTestContext, BoxLayoutContext, SliverLayoutContext},
    impl_sliver_test_caps,
    objects::RenderViewport,
    parent_data::{BoxParentData, SliverParentData},
    pipeline::PipelineOwner,
    protocol::SliverProtocol,
    testing::{inspect, sliver},
    traits::{RenderBox, RenderObject, RenderSliver},
    view::ScrollableViewportOffset,
};
use flui_tree::{Leaf, Variable};
use flui_types::{Offset, Size, geometry::px, layout::AxisDirection};

type BoxedRenderObject = Box<dyn RenderObject<flui_rendering::protocol::BoxProtocol>>;
type BoxedSliverObject = Box<dyn RenderObject<SliverProtocol>>;

fn laid_out(
    mut owner: PipelineOwner,
    root: flui_foundation::RenderId,
) -> PipelineOwner<flui_rendering::pipeline::phase::Layout> {
    owner.set_root_id(Some(root));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(100.0), px(100.0)))));
    let mut owner = owner.into_layout();
    owner.run_layout().expect("layout succeeds");
    owner
}

fn hits_at(
    owner: &PipelineOwner<flui_rendering::pipeline::phase::Layout>,
    x: f32,
    y: f32,
) -> Vec<flui_foundation::RenderId> {
    inspect::hit_path(owner, x, y)
}

fn sliver_hit_constraints(axis: AxisDirection, growth: GrowthDirection) -> SliverConstraints {
    let builder = match axis {
        AxisDirection::TopToBottom | AxisDirection::BottomToTop => sliver::vertical(),
        AxisDirection::LeftToRight | AxisDirection::RightToLeft => sliver::horizontal(),
    };
    builder
        .with_axis_direction(axis)
        .with_growth_direction(growth)
        .remaining_paint_extent(100.0)
        .cross_axis_extent(100.0)
        .viewport_main_axis_extent(100.0)
        .remaining_cache_extent(100.0)
        .build()
}

#[derive(Debug)]
struct SliverHitHost {
    constraints: SliverConstraints,
    size: Size,
}

impl_sliver_test_caps!(SliverHitHost);

impl RenderBox for SliverHitHost {
    type Arity = Variable;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>) -> Size {
        if ctx.child_count() > 0 {
            let _ = ctx.layout_sliver_child(0, self.constraints);
        }
        self.size = ctx.constraints().biggest();
        self.size
    }

    fn size(&self) -> &Size {
        &self.size
    }

    fn size_mut(&mut self) -> &mut Size {
        &mut self.size
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Variable, BoxParentData>) -> bool {
        ctx.hit_test_child_at_layout_offset(0)
    }
}

#[derive(Debug)]
struct MainAxisBandSliver {
    extent: f32,
    hit_start: f32,
    hit_end: f32,
    constraints: SliverConstraints,
    geometry: SliverGeometry,
}

impl MainAxisBandSliver {
    fn new(extent: f32, hit_start: f32, hit_end: f32) -> Self {
        Self {
            extent,
            hit_start,
            hit_end,
            constraints: SliverConstraints::default(),
            geometry: SliverGeometry::ZERO,
        }
    }
}

impl_sliver_test_caps!(MainAxisBandSliver);

impl RenderSliver for MainAxisBandSliver {
    type Arity = Leaf;
    type ParentData = SliverParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>,
    ) -> SliverGeometry {
        self.constraints = *ctx.constraints();
        let paint_extent = self.calculate_paint_offset(&self.constraints, 0.0, self.extent);
        self.geometry = SliverGeometry {
            scroll_extent: self.extent,
            paint_extent,
            layout_extent: paint_extent,
            max_paint_extent: self.extent,
            hit_test_extent: paint_extent,
            cache_extent: self.calculate_cache_offset(&self.constraints, 0.0, self.extent),
            visible: paint_extent > 0.0,
            ..SliverGeometry::ZERO
        };
        self.geometry
    }

    fn constraints(&self) -> &SliverConstraints {
        &self.constraints
    }

    fn geometry(&self) -> &SliverGeometry {
        &self.geometry
    }

    fn set_geometry(&mut self, geometry: SliverGeometry) {
        self.geometry = geometry;
    }

    fn hit_test_self(&self, main: f32, cross: f32) -> bool {
        main >= self.hit_start
            && main < self.hit_end
            && cross >= 0.0
            && cross < self.constraints.cross_axis_extent
    }
}

#[test]
fn sliver_hit_direction_matrix_through_box_host() {
    let cases = [
        (
            AxisDirection::TopToBottom,
            GrowthDirection::Forward,
            Offset::new(px(10.0), px(10.0)),
            Offset::new(px(10.0), px(30.0)),
        ),
        (
            AxisDirection::TopToBottom,
            GrowthDirection::Reverse,
            Offset::new(px(10.0), px(30.0)),
            Offset::new(px(10.0), px(10.0)),
        ),
        (
            AxisDirection::BottomToTop,
            GrowthDirection::Forward,
            Offset::new(px(10.0), px(30.0)),
            Offset::new(px(10.0), px(10.0)),
        ),
        (
            AxisDirection::BottomToTop,
            GrowthDirection::Reverse,
            Offset::new(px(10.0), px(10.0)),
            Offset::new(px(10.0), px(30.0)),
        ),
        (
            AxisDirection::LeftToRight,
            GrowthDirection::Forward,
            Offset::new(px(10.0), px(10.0)),
            Offset::new(px(30.0), px(10.0)),
        ),
        (
            AxisDirection::LeftToRight,
            GrowthDirection::Reverse,
            Offset::new(px(30.0), px(10.0)),
            Offset::new(px(10.0), px(10.0)),
        ),
        (
            AxisDirection::RightToLeft,
            GrowthDirection::Forward,
            Offset::new(px(30.0), px(10.0)),
            Offset::new(px(10.0), px(10.0)),
        ),
        (
            AxisDirection::RightToLeft,
            GrowthDirection::Reverse,
            Offset::new(px(10.0), px(10.0)),
            Offset::new(px(30.0), px(10.0)),
        ),
    ];

    for (axis, growth, hit_position, miss_position) in cases {
        let mut owner = PipelineOwner::new();
        let host_id = owner.insert(Box::new(SliverHitHost {
            constraints: sliver_hit_constraints(axis, growth),
            size: Size::ZERO,
        }) as BoxedRenderObject);
        let sliver_id = owner
            .render_tree_mut()
            .insert_sliver_child(
                host_id,
                Box::new(MainAxisBandSliver::new(40.0, 0.0, 15.0)) as BoxedSliverObject,
            )
            .expect("band sliver");

        let owner = laid_out(owner, host_id);

        assert_eq!(
            hits_at(&owner, hit_position.dx.get(), hit_position.dy.get()),
            vec![sliver_id, host_id],
            "{axis:?} {growth:?} must hit inside the leading main-axis band",
        );
        assert!(
            hits_at(&owner, miss_position.dx.get(), miss_position.dy.get()).is_empty(),
            "{axis:?} {growth:?} must miss outside the leading main-axis band",
        );
    }
}

#[test]
fn viewport_hit_direction_matrix_matches_box_host_semantics() {
    let mut owner = PipelineOwner::new();
    let mut viewport = RenderViewport::with_offset(
        AxisDirection::TopToBottom,
        AxisDirection::LeftToRight,
        ScrollableViewportOffset::zero(),
    );
    viewport.set_center_sliver_index(Some(0));
    let root_id = owner.insert(Box::new(viewport));
    let sliver_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(MainAxisBandSliver::new(40.0, 0.0, 15.0)) as BoxedSliverObject,
        )
        .expect("sliver");

    let owner = laid_out(owner, root_id);

    assert_eq!(
        hits_at(&owner, 10.0, 90.0),
        vec![sliver_id, root_id],
        "viewport reverse TTB must align with box-host reverse hit semantics",
    );
}
