//! Runtime validation for `SliverGeometry` layout results.

use std::sync::{Arc, Mutex};

use flui_foundation::Diagnosticable;
use flui_rendering::{
    constraints::{BoxConstraints, GrowthDirection, SliverConstraints, SliverGeometry},
    context::{BoxLayoutContext, SliverHitTestContext, SliverLayoutContext},
    error::RenderError,
    parent_data::{BoxParentData, SliverParentData},
    pipeline::PipelineOwner,
    protocol::{BoxProtocol, SliverProtocol},
    traits::{
        HotReloadCapability, PaintEffectsCapability, RenderBox, RenderObject, RenderSliver,
        SemanticsCapability,
    },
    view::ScrollDirection,
};
use flui_tree::{Leaf, Variable};
use flui_types::{Point, Rect, Size, geometry::px, layout::AxisDirection};

type BoxedRenderObject = Box<dyn RenderObject<BoxProtocol>>;
type BoxedSliverObject = Box<dyn RenderObject<SliverProtocol>>;

fn sliver_constraints() -> SliverConstraints {
    SliverConstraints {
        axis_direction: AxisDirection::TopToBottom,
        cross_axis_direction: AxisDirection::LeftToRight,
        growth_direction: GrowthDirection::Forward,
        user_scroll_direction: ScrollDirection::Idle,
        scroll_offset: 0.0,
        preceding_scroll_extent: 0.0,
        overlap: 0.0,
        remaining_paint_extent: 100.0,
        cross_axis_extent: 300.0,
        viewport_main_axis_extent: 100.0,
        remaining_cache_extent: 120.0,
        cache_origin: -20.0,
    }
}

fn invalid_layout_exceeds_paint_geometry() -> SliverGeometry {
    SliverGeometry {
        scroll_extent: 100.0,
        paint_extent: 10.0,
        layout_extent: 20.0,
        max_paint_extent: 100.0,
        hit_test_extent: 10.0,
        cache_extent: 20.0,
        visible: true,
        ..SliverGeometry::ZERO
    }
}

fn assert_invalid_geometry(err: RenderError, expected_reason: &'static str) {
    match err {
        RenderError::InvalidGeometry {
            render_object: _,
            reason,
        } => assert_eq!(reason, expected_reason),
        other => panic!("expected InvalidGeometry, got {other:?}"),
    }
}

#[derive(Debug)]
struct BadGeometrySliver {
    constraints: SliverConstraints,
    geometry: SliverGeometry,
}

impl BadGeometrySliver {
    fn new(geometry: SliverGeometry) -> Self {
        Self {
            constraints: SliverConstraints::default(),
            geometry,
        }
    }
}

impl Diagnosticable for BadGeometrySliver {}
impl PaintEffectsCapability for BadGeometrySliver {}
impl SemanticsCapability for BadGeometrySliver {}
impl HotReloadCapability for BadGeometrySliver {}

impl RenderSliver for BadGeometrySliver {
    type Arity = Leaf;
    type ParentData = SliverParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>,
    ) -> SliverGeometry {
        self.constraints = *ctx.constraints();
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

    fn hit_test(&self, _ctx: &mut SliverHitTestContext<'_, Leaf, Self::ParentData>) -> bool {
        false
    }

    fn sliver_paint_bounds(&self) -> Rect {
        Rect::from_origin_size(Point::ZERO, Size::ZERO)
    }
}

#[derive(Debug)]
struct BoxWithSliverChild {
    sliver_constraints: SliverConstraints,
    captured: Arc<Mutex<Option<SliverGeometry>>>,
    size: Size,
}

impl BoxWithSliverChild {
    fn new(
        sliver_constraints: SliverConstraints,
        captured: Arc<Mutex<Option<SliverGeometry>>>,
    ) -> Self {
        Self {
            sliver_constraints,
            captured,
            size: Size::ZERO,
        }
    }
}

impl Diagnosticable for BoxWithSliverChild {}
impl PaintEffectsCapability for BoxWithSliverChild {}
impl SemanticsCapability for BoxWithSliverChild {}
impl HotReloadCapability for BoxWithSliverChild {}

impl RenderBox for BoxWithSliverChild {
    type Arity = Variable;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, Self::ParentData>,
    ) -> Size {
        let geometry = ctx.layout_sliver_child(0, self.sliver_constraints);
        *self.captured.lock().unwrap() = Some(geometry);
        self.size = ctx.constraints().biggest();
        self.size
    }

    fn size(&self) -> &Size {
        &self.size
    }

    fn size_mut(&mut self) -> &mut Size {
        &mut self.size
    }
}

#[test]
fn sliver_leaf_layout_rejects_invalid_geometry_before_state_commit() {
    let mut owner = PipelineOwner::new();
    let sliver_id = owner
        .render_tree_mut()
        .insert_sliver(Box::new(BadGeometrySliver::new(
            invalid_layout_exceeds_paint_geometry(),
        )) as BoxedSliverObject);

    let entry = owner
        .render_tree_mut()
        .get_mut(sliver_id)
        .and_then(|node| node.as_sliver_mut())
        .expect("sliver entry");
    let err = entry
        .layout_leaf_only(sliver_constraints())
        .expect_err("invalid sliver geometry must fail layout");

    assert_invalid_geometry(err, "layout_extent exceeds paint_extent");
    assert!(
        entry.state().geometry().is_none(),
        "invalid geometry must not be committed to RenderState"
    );
    assert!(
        entry.needs_layout(),
        "failed sliver layout must stay dirty for retry"
    );
}

#[test]
fn sliver_descendant_invalid_geometry_returns_zero_and_keeps_parent_dirty() {
    let captured: Arc<Mutex<Option<SliverGeometry>>> = Arc::new(Mutex::new(None));
    let parent_obj: BoxedRenderObject = Box::new(BoxWithSliverChild::new(
        sliver_constraints(),
        Arc::clone(&captured),
    ));
    let sliver_obj: BoxedSliverObject = Box::new(BadGeometrySliver::new(
        invalid_layout_exceeds_paint_geometry(),
    ));

    let mut pipeline = PipelineOwner::new().into_layout();
    let parent_id = pipeline.render_tree_mut().insert_box(parent_obj);
    let sliver_id = pipeline
        .render_tree_mut()
        .insert_sliver_child(parent_id, sliver_obj)
        .expect("tree accepts sliver child");

    pipeline
        .layout_dirty_root(
            parent_id,
            BoxConstraints::new(px(0.0), px(800.0), px(0.0), px(600.0)),
        )
        .expect("parent layout still completes; descendant error is isolated");

    assert_eq!(
        captured.lock().unwrap().expect("parent saw child layout"),
        SliverGeometry::ZERO,
        "invalid descendant geometry must be replaced with ZERO for the parent"
    );

    let parent_node = pipeline
        .render_tree()
        .get(parent_id)
        .expect("parent remains in tree");
    assert!(
        parent_node.needs_layout(),
        "descendant InvalidGeometry must keep the parent dirty for retry"
    );

    let sliver_entry = pipeline
        .render_tree()
        .get(sliver_id)
        .and_then(|node| node.as_sliver())
        .expect("sliver entry remains in tree");
    assert!(
        sliver_entry.state().geometry().is_none(),
        "invalid descendant geometry must not be committed"
    );
    assert!(
        sliver_entry.needs_layout(),
        "invalid descendant must stay dirty for retry"
    );
}
