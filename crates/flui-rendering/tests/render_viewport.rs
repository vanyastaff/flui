//! Core.2 W3.4a: minimal `RenderViewport` driver for sliver children.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use flui_foundation::Diagnosticable;
use flui_rendering::{
    constraints::{BoxConstraints, SliverConstraints, SliverGeometry},
    context::{SliverHitTestContext, SliverLayoutContext},
    hit_testing::HitTestResult,
    objects::RenderViewport,
    parent_data::SliverParentData,
    pipeline::PipelineOwner,
    protocol::SliverProtocol,
    traits::{
        HotReloadCapability, PaintEffectsCapability, RenderObject, RenderSliver,
        SemanticsCapability,
    },
    view::{ScrollableViewportOffset, SliverPaintOrder, ViewportOffset},
};
use flui_tree::Leaf;
use flui_types::{Offset, Point, Rect, Size, geometry::px, layout::AxisDirection};

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
struct FixedSliver {
    scroll_extent: f32,
    paint_extent: f32,
    layout_extent: Option<f32>,
    constraints: SliverConstraints,
    geometry: SliverGeometry,
}

impl FixedSliver {
    fn new(scroll_extent: f32) -> Self {
        Self {
            scroll_extent,
            paint_extent: scroll_extent,
            layout_extent: None,
            constraints: SliverConstraints::default(),
            geometry: SliverGeometry::ZERO,
        }
    }

    fn with_extents(scroll_extent: f32, paint_extent: f32, layout_extent: f32) -> Self {
        Self {
            scroll_extent,
            paint_extent,
            layout_extent: Some(layout_extent),
            constraints: SliverConstraints::default(),
            geometry: SliverGeometry::ZERO,
        }
    }
}

impl Diagnosticable for FixedSliver {}
impl PaintEffectsCapability for FixedSliver {}
impl SemanticsCapability for FixedSliver {}
impl HotReloadCapability for FixedSliver {}

impl RenderSliver for FixedSliver {
    type Arity = Leaf;
    type ParentData = SliverParentData;

    fn perform_layout(&mut self, ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>) {
        self.constraints = *ctx.constraints();
        let paint_extent = self.calculate_paint_offset(&self.constraints, 0.0, self.paint_extent);
        let layout_extent = self.layout_extent.unwrap_or(paint_extent);
        let cache_extent = self.calculate_cache_offset(&self.constraints, 0.0, self.paint_extent);
        self.geometry = SliverGeometry {
            scroll_extent: self.scroll_extent,
            paint_extent,
            layout_extent,
            max_paint_extent: self.paint_extent,
            hit_test_extent: paint_extent,
            cache_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: self.scroll_extent > self.constraints.remaining_paint_extent
                || self.constraints.scroll_offset > 0.0,
            ..SliverGeometry::ZERO
        };
        ctx.complete(self.geometry);
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

    fn hit_test(&self, ctx: &mut SliverHitTestContext<'_, Leaf, Self::ParentData>) -> bool {
        self.hit_test_self(ctx.main_axis(), ctx.cross_axis())
    }

    fn hit_test_self(&self, main: f32, cross: f32) -> bool {
        cross >= 0.0 && cross < self.constraints.cross_axis_extent && main >= 0.0
    }

    fn sliver_paint_bounds(&self) -> Rect {
        Rect::from_origin_size(
            Point::ZERO,
            Size::new(px(100.0), px(self.geometry.paint_extent)),
        )
    }
}

#[derive(Debug)]
struct InvisibleHitSliver {
    constraints: SliverConstraints,
    geometry: SliverGeometry,
}

impl Default for InvisibleHitSliver {
    fn default() -> Self {
        Self {
            constraints: SliverConstraints::default(),
            geometry: SliverGeometry::ZERO,
        }
    }
}

impl Diagnosticable for InvisibleHitSliver {}
impl PaintEffectsCapability for InvisibleHitSliver {}
impl SemanticsCapability for InvisibleHitSliver {}
impl HotReloadCapability for InvisibleHitSliver {}

impl RenderSliver for InvisibleHitSliver {
    type Arity = Leaf;
    type ParentData = SliverParentData;

    fn perform_layout(&mut self, ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>) {
        self.constraints = *ctx.constraints();
        self.geometry = SliverGeometry {
            scroll_extent: 0.0,
            paint_extent: 100.0,
            layout_extent: 0.0,
            max_paint_extent: 100.0,
            hit_test_extent: 100.0,
            visible: false,
            ..SliverGeometry::ZERO
        };
        ctx.complete(self.geometry);
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
        main >= 0.0 && main < self.geometry.hit_test_extent && cross >= 0.0
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

impl Diagnosticable for MainAxisBandSliver {}
impl PaintEffectsCapability for MainAxisBandSliver {}
impl SemanticsCapability for MainAxisBandSliver {}
impl HotReloadCapability for MainAxisBandSliver {}

impl RenderSliver for MainAxisBandSliver {
    type Arity = Leaf;
    type ParentData = SliverParentData;

    fn perform_layout(&mut self, ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>) {
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
        ctx.complete(self.geometry);
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

#[derive(Debug)]
struct CorrectingSliver {
    correction: f32,
    corrected: bool,
    constraints: SliverConstraints,
    geometry: SliverGeometry,
}

impl CorrectingSliver {
    fn new(correction: f32) -> Self {
        Self {
            correction,
            corrected: false,
            constraints: SliverConstraints::default(),
            geometry: SliverGeometry::ZERO,
        }
    }
}

impl Diagnosticable for CorrectingSliver {}
impl PaintEffectsCapability for CorrectingSliver {}
impl SemanticsCapability for CorrectingSliver {}
impl HotReloadCapability for CorrectingSliver {}

impl RenderSliver for CorrectingSliver {
    type Arity = Leaf;
    type ParentData = SliverParentData;

    fn perform_layout(&mut self, ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>) {
        self.constraints = *ctx.constraints();
        if !self.corrected {
            self.corrected = true;
            self.geometry = SliverGeometry::scroll_offset_correction(self.correction);
        } else {
            self.geometry = SliverGeometry {
                scroll_extent: 80.0,
                paint_extent: 80.0,
                layout_extent: 80.0,
                max_paint_extent: 80.0,
                hit_test_extent: 80.0,
                cache_extent: 80.0,
                visible: true,
                ..SliverGeometry::ZERO
            };
        }
        ctx.complete(self.geometry);
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
}

#[derive(Debug)]
struct CountingSliver {
    scroll_extent: f32,
    layouts: Arc<AtomicUsize>,
    constraints: SliverConstraints,
    geometry: SliverGeometry,
}

impl CountingSliver {
    fn new(scroll_extent: f32, layouts: Arc<AtomicUsize>) -> Self {
        Self {
            scroll_extent,
            layouts,
            constraints: SliverConstraints::default(),
            geometry: SliverGeometry::ZERO,
        }
    }
}

impl Diagnosticable for CountingSliver {}
impl PaintEffectsCapability for CountingSliver {}
impl SemanticsCapability for CountingSliver {}
impl HotReloadCapability for CountingSliver {}

impl RenderSliver for CountingSliver {
    type Arity = Leaf;
    type ParentData = SliverParentData;

    fn perform_layout(&mut self, ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>) {
        self.layouts.fetch_add(1, Ordering::SeqCst);
        self.constraints = *ctx.constraints();
        let paint_extent = self.calculate_paint_offset(&self.constraints, 0.0, self.scroll_extent);
        let cache_extent = self.calculate_cache_offset(&self.constraints, 0.0, self.scroll_extent);
        self.geometry = SliverGeometry {
            scroll_extent: self.scroll_extent,
            paint_extent,
            layout_extent: paint_extent,
            max_paint_extent: self.scroll_extent,
            hit_test_extent: paint_extent,
            cache_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: self.constraints.scroll_offset > 0.0,
            ..SliverGeometry::ZERO
        };
        ctx.complete(self.geometry);
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
}

#[derive(Debug)]
struct OutOfBandSliver {
    scroll_extent: f32,
    max_scroll_obstruction_extent: f32,
    has_visual_overflow: bool,
    constraints: SliverConstraints,
    geometry: SliverGeometry,
}

impl OutOfBandSliver {
    fn new(
        scroll_extent: f32,
        max_scroll_obstruction_extent: f32,
        has_visual_overflow: bool,
    ) -> Self {
        Self {
            scroll_extent,
            max_scroll_obstruction_extent,
            has_visual_overflow,
            constraints: SliverConstraints::default(),
            geometry: SliverGeometry::ZERO,
        }
    }
}

impl Diagnosticable for OutOfBandSliver {}
impl PaintEffectsCapability for OutOfBandSliver {}
impl SemanticsCapability for OutOfBandSliver {}
impl HotReloadCapability for OutOfBandSliver {}

impl RenderSliver for OutOfBandSliver {
    type Arity = Leaf;
    type ParentData = SliverParentData;

    fn perform_layout(&mut self, ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>) {
        self.constraints = *ctx.constraints();
        let paint_extent = self.calculate_paint_offset(&self.constraints, 0.0, self.scroll_extent);
        let cache_extent = self.calculate_cache_offset(&self.constraints, 0.0, self.scroll_extent);
        self.geometry = SliverGeometry {
            scroll_extent: self.scroll_extent,
            paint_extent,
            layout_extent: paint_extent,
            max_paint_extent: self.scroll_extent,
            max_scroll_obstruction_extent: self.max_scroll_obstruction_extent,
            hit_test_extent: paint_extent,
            cache_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: self.has_visual_overflow,
            ..SliverGeometry::ZERO
        };
        ctx.complete(self.geometry);
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
}

#[derive(Debug)]
struct DynamicOutOfBandSliver {
    scroll_extent: f32,
    max_scroll_obstruction_extent: Arc<AtomicUsize>,
    has_visual_overflow: Arc<AtomicBool>,
    constraints: SliverConstraints,
    geometry: SliverGeometry,
}

impl DynamicOutOfBandSliver {
    fn new(
        scroll_extent: f32,
        max_scroll_obstruction_extent: Arc<AtomicUsize>,
        has_visual_overflow: Arc<AtomicBool>,
    ) -> Self {
        Self {
            scroll_extent,
            max_scroll_obstruction_extent,
            has_visual_overflow,
            constraints: SliverConstraints::default(),
            geometry: SliverGeometry::ZERO,
        }
    }
}

impl Diagnosticable for DynamicOutOfBandSliver {}
impl PaintEffectsCapability for DynamicOutOfBandSliver {}
impl SemanticsCapability for DynamicOutOfBandSliver {}
impl HotReloadCapability for DynamicOutOfBandSliver {}

impl RenderSliver for DynamicOutOfBandSliver {
    type Arity = Leaf;
    type ParentData = SliverParentData;

    fn perform_layout(&mut self, ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>) {
        self.constraints = *ctx.constraints();
        let paint_extent = self.calculate_paint_offset(&self.constraints, 0.0, self.scroll_extent);
        let cache_extent = self.calculate_cache_offset(&self.constraints, 0.0, self.scroll_extent);
        self.geometry = SliverGeometry {
            scroll_extent: self.scroll_extent,
            paint_extent,
            layout_extent: paint_extent,
            max_paint_extent: self.scroll_extent,
            max_scroll_obstruction_extent: self.max_scroll_obstruction_extent.load(Ordering::SeqCst)
                as f32,
            hit_test_extent: paint_extent,
            cache_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: self.has_visual_overflow.load(Ordering::SeqCst),
            ..SliverGeometry::ZERO
        };
        ctx.complete(self.geometry);
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
}

#[test]
fn viewport_lays_out_forward_slivers_and_applies_content_dimensions() {
    let viewport = RenderViewport::with_offset(
        AxisDirection::TopToBottom,
        AxisDirection::LeftToRight,
        ScrollableViewportOffset::new(40.0),
    );

    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(viewport));
    let first_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(FixedSliver::new(70.0)) as BoxedSliverObject,
        )
        .expect("first sliver");
    let second_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(FixedSliver::new(90.0)) as BoxedSliverObject,
        )
        .expect("second sliver");

    let owner = laid_out(owner, root_id);
    let viewport = owner
        .render_tree()
        .get(root_id)
        .and_then(|node| node.as_box())
        .and_then(|entry| {
            entry
                .render_object()
                .downcast_ref::<RenderViewport<ScrollableViewportOffset>>()
        })
        .expect("root is RenderViewport");

    assert_eq!(viewport.size(), &Size::new(px(100.0), px(100.0)));
    assert_eq!(viewport.offset().viewport_dimension(), 100.0);
    assert_eq!(viewport.offset().max_scroll_extent(), 60.0);
    assert_eq!(viewport.offset().pixels(), 40.0);
    assert_eq!(
        render_offset(&owner, first_id),
        Offset::new(px(0.0), px(0.0)),
        "first forward sliver paints at the viewport origin when scroll_offset is consumed by constraints",
    );
    assert_eq!(
        render_offset(&owner, second_id),
        Offset::new(px(0.0), px(30.0)),
        "second sliver advances by first.layout_extent after the first sliver consumes 40px of scroll",
    );
}

#[test]
fn viewport_tracks_sliver_out_of_band_obstruction_and_overflow() {
    let viewport = RenderViewport::with_offset(
        AxisDirection::TopToBottom,
        AxisDirection::LeftToRight,
        ScrollableViewportOffset::zero(),
    );

    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(viewport));
    owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(OutOfBandSliver::new(40.0, 12.0, false)) as BoxedSliverObject,
        )
        .expect("first sliver");
    owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(OutOfBandSliver::new(50.0, 7.0, true)) as BoxedSliverObject,
        )
        .expect("second sliver");
    owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(OutOfBandSliver::new(60.0, 0.0, false)) as BoxedSliverObject,
        )
        .expect("third sliver");

    let owner = laid_out(owner, root_id);
    let viewport = owner
        .render_tree()
        .get(root_id)
        .and_then(|node| node.as_box())
        .and_then(|entry| {
            entry
                .render_object()
                .downcast_ref::<RenderViewport<ScrollableViewportOffset>>()
        })
        .expect("root is RenderViewport");

    assert_eq!(viewport.min_scroll_extent(), 0.0);
    assert_eq!(viewport.max_scroll_extent(), 150.0);
    assert_eq!(viewport.max_scroll_obstruction_extent(), 19.0);
    assert_eq!(viewport.max_scroll_obstruction_extent_before(0), Some(0.0));
    assert_eq!(viewport.max_scroll_obstruction_extent_before(1), Some(12.0));
    assert_eq!(viewport.max_scroll_obstruction_extent_before(2), Some(19.0));
    assert_eq!(viewport.max_scroll_obstruction_extent_before(3), None);
    assert!(
        viewport.has_visual_overflow(),
        "viewport must retain child-reported visual overflow for clipping",
    );
}

#[test]
fn viewport_resets_sliver_out_of_band_data_between_layout_passes() {
    let viewport = RenderViewport::with_offset(
        AxisDirection::TopToBottom,
        AxisDirection::LeftToRight,
        ScrollableViewportOffset::zero(),
    );
    let obstruction = Arc::new(AtomicUsize::new(18));
    let overflow = Arc::new(AtomicBool::new(true));

    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(viewport));
    owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(DynamicOutOfBandSliver::new(
                40.0,
                Arc::clone(&obstruction),
                Arc::clone(&overflow),
            )) as BoxedSliverObject,
        )
        .expect("dynamic sliver");

    let owner = laid_out(owner, root_id);
    let viewport = owner
        .render_tree()
        .get(root_id)
        .and_then(|node| node.as_box())
        .and_then(|entry| {
            entry
                .render_object()
                .downcast_ref::<RenderViewport<ScrollableViewportOffset>>()
        })
        .expect("root is RenderViewport");
    assert_eq!(viewport.max_scroll_obstruction_extent(), 18.0);
    assert!(viewport.has_visual_overflow());

    obstruction.store(0, Ordering::SeqCst);
    overflow.store(false, Ordering::SeqCst);
    let mut owner = owner.into_idle();
    owner.mark_needs_layout(root_id);
    let mut owner = owner.into_layout();
    owner.run_layout().expect("second layout succeeds");
    let viewport = owner
        .render_tree()
        .get(root_id)
        .and_then(|node| node.as_box())
        .and_then(|entry| {
            entry
                .render_object()
                .downcast_ref::<RenderViewport<ScrollableViewportOffset>>()
        })
        .expect("root is RenderViewport");

    assert_eq!(viewport.max_scroll_obstruction_extent(), 0.0);
    assert_eq!(viewport.max_scroll_obstruction_extent_before(0), Some(0.0));
    assert!(
        !viewport.has_visual_overflow(),
        "out-of-band overflow must be recomputed from the current layout pass",
    );
}

#[test]
fn viewport_retries_after_child_scroll_offset_correction() {
    let viewport = RenderViewport::with_offset(
        AxisDirection::TopToBottom,
        AxisDirection::LeftToRight,
        ScrollableViewportOffset::new(20.0),
    );

    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(viewport));
    owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(CorrectingSliver::new(-20.0)) as BoxedSliverObject,
        )
        .expect("correcting sliver");

    let owner = laid_out(owner, root_id);
    let viewport = owner
        .render_tree()
        .get(root_id)
        .and_then(|node| node.as_box())
        .and_then(|entry| {
            entry
                .render_object()
                .downcast_ref::<RenderViewport<ScrollableViewportOffset>>()
        })
        .expect("root is RenderViewport");

    assert_eq!(
        viewport.offset().pixels(),
        0.0,
        "child correction must be applied through ViewportOffset::correct_by and layout retried",
    );
}

#[test]
fn viewport_hit_tests_in_opposite_paint_order() {
    let mut viewport = RenderViewport::with_offset(
        AxisDirection::TopToBottom,
        AxisDirection::LeftToRight,
        ScrollableViewportOffset::zero(),
    );
    viewport.set_paint_order(SliverPaintOrder::LastIsTop);

    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(viewport));
    let _first_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(FixedSliver::with_extents(0.0, 100.0, 0.0)) as BoxedSliverObject,
        )
        .expect("first sliver");
    let second_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(FixedSliver::with_extents(0.0, 100.0, 0.0)) as BoxedSliverObject,
        )
        .expect("second sliver");

    let owner = laid_out(owner, root_id);

    assert_eq!(
        hits(&owner, 10.0, 10.0),
        vec![second_id, root_id],
        "LastIsTop paints later children on top, so hit testing must visit them first",
    );

    let mut viewport = RenderViewport::with_offset(
        AxisDirection::TopToBottom,
        AxisDirection::LeftToRight,
        ScrollableViewportOffset::zero(),
    );
    viewport.set_paint_order(SliverPaintOrder::FirstIsTop);

    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(viewport));
    let first_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(FixedSliver::with_extents(0.0, 100.0, 0.0)) as BoxedSliverObject,
        )
        .expect("first sliver");
    let _second_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(FixedSliver::with_extents(0.0, 100.0, 0.0)) as BoxedSliverObject,
        )
        .expect("second sliver");

    let owner = laid_out(owner, root_id);

    assert_eq!(
        hits(&owner, 10.0, 10.0),
        vec![first_id, root_id],
        "FirstIsTop paints earlier children on top, so hit testing must visit them first",
    );
}

#[test]
fn viewport_skips_invisible_sliver_children_during_hit_testing() {
    let viewport = RenderViewport::with_offset(
        AxisDirection::TopToBottom,
        AxisDirection::LeftToRight,
        ScrollableViewportOffset::zero(),
    );

    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(viewport));
    let invisible_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(InvisibleHitSliver::default()) as BoxedSliverObject,
        )
        .expect("invisible sliver");
    let visible_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(FixedSliver::with_extents(0.0, 100.0, 0.0)) as BoxedSliverObject,
        )
        .expect("visible sliver");

    let owner = laid_out(owner, root_id);

    assert_eq!(
        render_offset(&owner, invisible_id),
        Offset::ZERO,
        "fixture sanity: invisible and visible slivers overlap in paint space",
    );
    assert_eq!(
        hits(&owner, 10.0, 10.0),
        vec![visible_id, root_id],
        "RenderViewport must mirror Flutter and skip geometry.visible=false \
         slivers before hit-testing them",
    );
}

#[test]
fn viewport_hit_test_flips_reverse_axis_into_sliver_main_axis() {
    let viewport = RenderViewport::with_offset(
        AxisDirection::BottomToTop,
        AxisDirection::LeftToRight,
        ScrollableViewportOffset::zero(),
    );

    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(viewport));
    let sliver_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(MainAxisBandSliver::new(40.0, 0.0, 15.0)) as BoxedSliverObject,
        )
        .expect("reverse-axis sliver");

    let owner = laid_out(owner, root_id);

    assert_eq!(
        render_offset(&owner, sliver_id),
        Offset::new(px(0.0), px(60.0)),
        "bottom-to-top viewport paints the first 40px sliver at the bottom edge",
    );
    assert_eq!(
        hits(&owner, 10.0, 90.0),
        vec![sliver_id, root_id],
        "parent y=90 must map to sliver main=10, inside the leading hit band",
    );
    assert!(
        hits(&owner, 10.0, 70.0).is_empty(),
        "parent y=70 maps to sliver main=30 and must miss the leading hit band",
    );
}

#[test]
fn viewport_reuses_clean_cached_tail_extents_after_cache_window() {
    let viewport = RenderViewport::with_offset(
        AxisDirection::TopToBottom,
        AxisDirection::LeftToRight,
        ScrollableViewportOffset::zero(),
    );

    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(viewport));
    let layout_counts = (0..8)
        .map(|_| Arc::new(AtomicUsize::new(0)))
        .collect::<Vec<_>>();

    for counter in &layout_counts {
        owner
            .render_tree_mut()
            .insert_sliver_child(
                root_id,
                Box::new(CountingSliver::new(100.0, Arc::clone(counter))) as BoxedSliverObject,
            )
            .expect("counting sliver");
    }

    let owner = laid_out(owner, root_id);
    assert!(
        layout_counts
            .iter()
            .all(|counter| counter.load(Ordering::SeqCst) == 1),
        "first layout seeds geometry for every direct sliver child",
    );

    let mut owner = owner.into_idle();
    owner.mark_needs_layout(root_id);
    let mut owner = owner.into_layout();
    owner.run_layout().expect("second layout succeeds");

    assert!(
        layout_counts
            .iter()
            .take(4)
            .all(|counter| counter.load(Ordering::SeqCst) == 2),
        "second layout still drives the visible/cache window",
    );
    assert!(
        layout_counts
            .iter()
            .skip(4)
            .all(|counter| counter.load(Ordering::SeqCst) == 1),
        "clean slivers after the cache window should reuse cached scroll extents",
    );

    let viewport = owner
        .render_tree()
        .get(root_id)
        .and_then(|node| node.as_box())
        .and_then(|entry| {
            entry
                .render_object()
                .downcast_ref::<RenderViewport<ScrollableViewportOffset>>()
        })
        .expect("root is RenderViewport");
    assert_eq!(
        viewport.offset().max_scroll_extent(),
        700.0,
        "reusing cached tail extents must preserve the full scroll range",
    );
}
