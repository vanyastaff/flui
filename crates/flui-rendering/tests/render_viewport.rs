//! Core.2 W3.4a: minimal `RenderViewport` driver for sliver children.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use flui_rendering::{
    constraints::{BoxConstraints, GrowthDirection, SliverGeometry},
    context::{SliverHitTestContext, SliverLayoutContext},
    impl_sliver_test_caps,
    objects::RenderViewport,
    parent_data::SliverParentData,
    pipeline::PipelineOwner,
    protocol::SliverProtocol,
    testing::inspect,
    traits::{RenderObject, RenderSliver},
    view::{ScrollableViewportOffset, SliverPaintOrder, ViewportOffset},
};
use flui_tree::Leaf;
use flui_types::{Offset, Size, geometry::px, layout::AxisDirection};

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
    inspect::render_offset(owner, id).expect("node exists")
}

fn hits(
    owner: &PipelineOwner<flui_rendering::pipeline::phase::Layout>,
    cross: f32,
    main: f32,
) -> Vec<flui_foundation::RenderId> {
    hits_at(owner, cross, main)
}

fn hits_at(
    owner: &PipelineOwner<flui_rendering::pipeline::phase::Layout>,
    x: f32,
    y: f32,
) -> Vec<flui_foundation::RenderId> {
    inspect::hit_path(owner, x, y)
}

const fn test_cross_axis_direction(axis_direction: AxisDirection) -> AxisDirection {
    match axis_direction {
        AxisDirection::TopToBottom | AxisDirection::BottomToTop => AxisDirection::LeftToRight,
        AxisDirection::LeftToRight | AxisDirection::RightToLeft => AxisDirection::TopToBottom,
    }
}

fn laid_out_viewport_with_sliver(
    viewport: RenderViewport<ScrollableViewportOffset>,
    extent: f32,
) -> (
    PipelineOwner<flui_rendering::pipeline::phase::Layout>,
    flui_foundation::RenderId,
    flui_foundation::RenderId,
) {
    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(viewport));
    let sliver_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(FixedSliver::new(extent)) as BoxedSliverObject,
        )
        .expect("sliver");
    let owner = laid_out(owner, root_id);
    (owner, root_id, sliver_id)
}

fn viewport_from_owner(
    owner: &PipelineOwner<flui_rendering::pipeline::phase::Layout>,
    root_id: flui_foundation::RenderId,
) -> &RenderViewport<ScrollableViewportOffset> {
    owner
        .render_tree()
        .get(root_id)
        .and_then(|node| node.as_box())
        .and_then(|entry| {
            entry
                .render_object()
                .downcast_ref::<RenderViewport<ScrollableViewportOffset>>()
        })
        .expect("root is RenderViewport")
}

fn fixed_sliver_from_owner(
    owner: &PipelineOwner<flui_rendering::pipeline::phase::Layout>,
    sliver_id: flui_foundation::RenderId,
) -> &FixedSliver {
    owner
        .render_tree()
        .get(sliver_id)
        .and_then(|node| node.as_sliver())
        .and_then(|entry| entry.render_object().downcast_ref::<FixedSliver>())
        .expect("FixedSliver")
}

#[derive(Debug)]
struct FixedSliver {
    scroll_extent: f32,
    paint_extent: f32,
    layout_extent: Option<f32>,
    /// Cross-axis extent captured at layout, read by the `&self`-only
    /// `hit_test_self` (the sliver hit-test context does not carry it).
    cross_axis_extent: f32,
    /// When `Some`, updated each layout with the child's growth direction.
    recorded_growth_direction: Option<GrowthDirection>,
}

impl FixedSliver {
    fn new(scroll_extent: f32) -> Self {
        Self {
            scroll_extent,
            paint_extent: scroll_extent,
            layout_extent: None,
            cross_axis_extent: 0.0,
            recorded_growth_direction: None,
        }
    }

    fn recording_growth(scroll_extent: f32) -> Self {
        Self {
            recorded_growth_direction: Some(GrowthDirection::Forward),
            ..Self::new(scroll_extent)
        }
    }

    fn with_extents(scroll_extent: f32, paint_extent: f32, layout_extent: f32) -> Self {
        Self {
            scroll_extent,
            paint_extent,
            layout_extent: Some(layout_extent),
            cross_axis_extent: 0.0,
            recorded_growth_direction: None,
        }
    }

    fn last_growth_direction(&self) -> GrowthDirection {
        self.recorded_growth_direction
            .expect("FixedSliver::recording_growth was not used")
    }
}

impl_sliver_test_caps!(FixedSliver);

impl RenderSliver for FixedSliver {
    type Arity = Leaf;
    type ParentData = SliverParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>,
    ) -> SliverGeometry {
        let constraints = *ctx.constraints();
        self.cross_axis_extent = constraints.cross_axis_extent;
        if self.recorded_growth_direction.is_some() {
            self.recorded_growth_direction = Some(constraints.growth_direction);
        }
        let paint_extent = self.calculate_paint_offset(&constraints, 0.0, self.paint_extent);
        let layout_extent = self.layout_extent.unwrap_or(paint_extent);
        let cache_extent = self.calculate_cache_offset(&constraints, 0.0, self.paint_extent);
        SliverGeometry {
            scroll_extent: self.scroll_extent,
            paint_extent,
            layout_extent,
            max_paint_extent: self.paint_extent,
            hit_test_extent: paint_extent,
            cache_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: self.scroll_extent > constraints.remaining_paint_extent
                || constraints.scroll_offset > 0.0,
            ..SliverGeometry::ZERO
        }
    }

    fn hit_test(&self, ctx: &mut SliverHitTestContext<'_, Leaf, Self::ParentData>) -> bool {
        self.hit_test_self(ctx.main_axis(), ctx.cross_axis())
    }

    fn hit_test_self(&self, main: f32, cross: f32) -> bool {
        cross >= 0.0 && cross < self.cross_axis_extent && main >= 0.0
    }
}

#[derive(Debug, Default)]
struct InvisibleHitSliver;

impl_sliver_test_caps!(InvisibleHitSliver);

impl RenderSliver for InvisibleHitSliver {
    type Arity = Leaf;
    type ParentData = SliverParentData;

    fn perform_layout(
        &mut self,
        _ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>,
    ) -> SliverGeometry {
        SliverGeometry {
            scroll_extent: 0.0,
            paint_extent: 100.0,
            layout_extent: 0.0,
            max_paint_extent: 100.0,
            hit_test_extent: 100.0,
            visible: false,
            ..SliverGeometry::ZERO
        }
    }

    fn hit_test_self(&self, main: f32, cross: f32) -> bool {
        // The geometry's hit_test_extent is the fixed 100.0 this double reports.
        (0.0..100.0).contains(&main) && cross >= 0.0
    }
}

#[derive(Debug)]
struct MainAxisBandSliver {
    extent: f32,
    hit_start: f32,
    hit_end: f32,
    /// Cross-axis extent captured at layout, read by the `&self`-only
    /// `hit_test_self` (the sliver hit-test context does not carry it).
    cross_axis_extent: f32,
}

impl MainAxisBandSliver {
    fn new(extent: f32, hit_start: f32, hit_end: f32) -> Self {
        Self {
            extent,
            hit_start,
            hit_end,
            cross_axis_extent: 0.0,
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
        let constraints = *ctx.constraints();
        self.cross_axis_extent = constraints.cross_axis_extent;
        let paint_extent = self.calculate_paint_offset(&constraints, 0.0, self.extent);
        SliverGeometry {
            scroll_extent: self.extent,
            paint_extent,
            layout_extent: paint_extent,
            max_paint_extent: self.extent,
            hit_test_extent: paint_extent,
            cache_extent: self.calculate_cache_offset(&constraints, 0.0, self.extent),
            visible: paint_extent > 0.0,
            ..SliverGeometry::ZERO
        }
    }

    fn hit_test_self(&self, main: f32, cross: f32) -> bool {
        main >= self.hit_start
            && main < self.hit_end
            && cross >= 0.0
            && cross < self.cross_axis_extent
    }
}

#[derive(Debug)]
struct GeometrySliver {
    scroll_extent: f32,
    paint_origin: f32,
    paint_extent: f32,
    layout_extent: f32,
    hit_test_extent: f32,
    /// Cross-axis extent captured at layout, read by the `&self`-only
    /// `hit_test_self` (the sliver hit-test context does not carry it).
    cross_axis_extent: f32,
}

impl GeometrySliver {
    fn new(
        scroll_extent: f32,
        paint_origin: f32,
        paint_extent: f32,
        layout_extent: f32,
        hit_test_extent: f32,
    ) -> Self {
        Self {
            scroll_extent,
            paint_origin,
            paint_extent,
            layout_extent,
            hit_test_extent,
            cross_axis_extent: 0.0,
        }
    }
}

impl_sliver_test_caps!(GeometrySliver);

impl RenderSliver for GeometrySliver {
    type Arity = Leaf;
    type ParentData = SliverParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>,
    ) -> SliverGeometry {
        self.cross_axis_extent = ctx.constraints().cross_axis_extent;
        SliverGeometry {
            scroll_extent: self.scroll_extent,
            paint_origin: self.paint_origin,
            paint_extent: self.paint_extent,
            layout_extent: self.layout_extent,
            max_paint_extent: self.paint_extent,
            hit_test_extent: self.hit_test_extent,
            cache_extent: self.paint_extent,
            visible: self.paint_extent > 0.0,
            ..SliverGeometry::ZERO
        }
    }

    fn hit_test_self(&self, main: f32, cross: f32) -> bool {
        main >= 0.0 && main < self.hit_test_extent && cross >= 0.0 && cross < self.cross_axis_extent
    }
}

#[derive(Debug)]
struct CorrectingSliver {
    correction: f32,
    corrected: bool,
}

impl CorrectingSliver {
    fn new(correction: f32) -> Self {
        Self {
            correction,
            corrected: false,
        }
    }
}

impl_sliver_test_caps!(CorrectingSliver);

impl RenderSliver for CorrectingSliver {
    type Arity = Leaf;
    type ParentData = SliverParentData;

    fn perform_layout(
        &mut self,
        _ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>,
    ) -> SliverGeometry {
        if !self.corrected {
            self.corrected = true;
            SliverGeometry::scroll_offset_correction(self.correction)
        } else {
            SliverGeometry {
                scroll_extent: 80.0,
                paint_extent: 80.0,
                layout_extent: 80.0,
                max_paint_extent: 80.0,
                hit_test_extent: 80.0,
                cache_extent: 80.0,
                visible: true,
                ..SliverGeometry::ZERO
            }
        }
    }

    fn hit_test(&self, _ctx: &mut SliverHitTestContext<'_, Leaf, Self::ParentData>) -> bool {
        false
    }
}

#[derive(Debug)]
struct CountingSliver {
    scroll_extent: f32,
    layouts: Arc<AtomicUsize>,
}

impl CountingSliver {
    fn new(scroll_extent: f32, layouts: Arc<AtomicUsize>) -> Self {
        Self {
            scroll_extent,
            layouts,
        }
    }
}

impl_sliver_test_caps!(CountingSliver);

impl RenderSliver for CountingSliver {
    type Arity = Leaf;
    type ParentData = SliverParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>,
    ) -> SliverGeometry {
        self.layouts.fetch_add(1, Ordering::SeqCst);
        let constraints = *ctx.constraints();
        let paint_extent = self.calculate_paint_offset(&constraints, 0.0, self.scroll_extent);
        let cache_extent = self.calculate_cache_offset(&constraints, 0.0, self.scroll_extent);
        SliverGeometry {
            scroll_extent: self.scroll_extent,
            paint_extent,
            layout_extent: paint_extent,
            max_paint_extent: self.scroll_extent,
            hit_test_extent: paint_extent,
            cache_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: constraints.scroll_offset > 0.0,
            ..SliverGeometry::ZERO
        }
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
        }
    }
}

impl_sliver_test_caps!(OutOfBandSliver);

impl RenderSliver for OutOfBandSliver {
    type Arity = Leaf;
    type ParentData = SliverParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>,
    ) -> SliverGeometry {
        let constraints = *ctx.constraints();
        let paint_extent = self.calculate_paint_offset(&constraints, 0.0, self.scroll_extent);
        let cache_extent = self.calculate_cache_offset(&constraints, 0.0, self.scroll_extent);
        SliverGeometry {
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
        }
    }
}

#[derive(Debug)]
struct DynamicOutOfBandSliver {
    scroll_extent: f32,
    max_scroll_obstruction_extent: Arc<AtomicUsize>,
    has_visual_overflow: Arc<AtomicBool>,
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
        }
    }
}

impl_sliver_test_caps!(DynamicOutOfBandSliver);

impl RenderSliver for DynamicOutOfBandSliver {
    type Arity = Leaf;
    type ParentData = SliverParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>,
    ) -> SliverGeometry {
        let constraints = *ctx.constraints();
        let paint_extent = self.calculate_paint_offset(&constraints, 0.0, self.scroll_extent);
        let cache_extent = self.calculate_cache_offset(&constraints, 0.0, self.scroll_extent);
        SliverGeometry {
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
        }
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
fn viewport_positions_first_sliver_for_axis_and_growth_matrix() {
    let cases = [
        (
            AxisDirection::TopToBottom,
            GrowthDirection::Forward,
            None,
            Offset::new(px(0.0), px(0.0)),
        ),
        (
            AxisDirection::TopToBottom,
            GrowthDirection::Reverse,
            Some(0),
            Offset::new(px(0.0), px(60.0)),
        ),
        (
            AxisDirection::BottomToTop,
            GrowthDirection::Forward,
            None,
            Offset::new(px(0.0), px(60.0)),
        ),
        (
            AxisDirection::BottomToTop,
            GrowthDirection::Reverse,
            Some(0),
            Offset::new(px(0.0), px(0.0)),
        ),
        (
            AxisDirection::LeftToRight,
            GrowthDirection::Forward,
            None,
            Offset::new(px(0.0), px(0.0)),
        ),
        (
            AxisDirection::LeftToRight,
            GrowthDirection::Reverse,
            Some(0),
            Offset::new(px(60.0), px(0.0)),
        ),
        (
            AxisDirection::RightToLeft,
            GrowthDirection::Forward,
            None,
            Offset::new(px(60.0), px(0.0)),
        ),
        (
            AxisDirection::RightToLeft,
            GrowthDirection::Reverse,
            Some(0),
            Offset::new(px(0.0), px(0.0)),
        ),
    ];

    for (axis_direction, growth, center, expected_offset) in cases {
        let mut viewport = RenderViewport::with_offset(
            axis_direction,
            test_cross_axis_direction(axis_direction),
            ScrollableViewportOffset::zero(),
        );
        if growth == GrowthDirection::Reverse {
            viewport.set_center_sliver_index(center);
        }

        let mut owner = PipelineOwner::new();
        let root_id = owner.insert(Box::new(viewport));
        let sliver_id = owner
            .render_tree_mut()
            .insert_sliver_child(
                root_id,
                Box::new(FixedSliver::new(40.0)) as BoxedSliverObject,
            )
            .expect("sliver");

        let owner = laid_out(owner, root_id);

        assert_eq!(
            render_offset(&owner, sliver_id),
            expected_offset,
            "{axis_direction:?} {growth:?} must place a 40px sliver at the expected paint offset",
        );
    }
}

#[test]
fn viewport_reverse_section_passes_reverse_growth_to_slivers() {
    let mut viewport = RenderViewport::with_offset(
        AxisDirection::TopToBottom,
        AxisDirection::LeftToRight,
        ScrollableViewportOffset::zero(),
    );
    viewport.set_center_sliver_index(Some(0));

    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(viewport));
    let sliver = FixedSliver::recording_growth(40.0);
    let sliver_id = owner
        .render_tree_mut()
        .insert_sliver_child(root_id, Box::new(sliver) as BoxedSliverObject)
        .expect("sliver");

    let owner = laid_out(owner, root_id);
    let sliver = owner
        .render_tree()
        .get(sliver_id)
        .and_then(|node| node.as_sliver())
        .and_then(|entry| entry.render_object().downcast_ref::<FixedSliver>())
        .expect("FixedSliver");

    assert_eq!(
        sliver.last_growth_direction(),
        GrowthDirection::Reverse,
        "reverse-side viewport children must receive GrowthDirection::Reverse",
    );
    assert_eq!(
        render_offset(&owner, sliver_id),
        Offset::new(px(0.0), px(60.0))
    );
}

#[test]
fn viewport_center_partition_lays_out_forward_then_reverse() {
    let mut viewport = RenderViewport::with_offset(
        AxisDirection::TopToBottom,
        AxisDirection::LeftToRight,
        ScrollableViewportOffset::zero(),
    );
    viewport.set_center_sliver_index(Some(1));

    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(viewport));
    let s0 = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(FixedSliver::recording_growth(30.0)) as BoxedSliverObject,
        )
        .expect("forward sliver");
    let s1 = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(FixedSliver::recording_growth(30.0)) as BoxedSliverObject,
        )
        .expect("reverse sliver");

    let owner = laid_out(owner, root_id);
    let fwd = fixed_sliver_from_owner(&owner, s0);
    let rev = fixed_sliver_from_owner(&owner, s1);

    assert_eq!(fwd.last_growth_direction(), GrowthDirection::Forward);
    assert_eq!(rev.last_growth_direction(), GrowthDirection::Reverse);
    assert_eq!(render_offset(&owner, s0), Offset::new(px(0.0), px(0.0)));
    assert_eq!(render_offset(&owner, s1), Offset::new(px(0.0), px(70.0)));
}

#[test]
fn viewport_reverse_slivers_produce_negative_min_scroll_extent() {
    let mut viewport = RenderViewport::with_offset(
        AxisDirection::TopToBottom,
        AxisDirection::LeftToRight,
        ScrollableViewportOffset::zero(),
    );
    viewport.set_center_sliver_index(Some(0));

    let (owner, root_id, _) = laid_out_viewport_with_sliver(viewport, 50.0);
    let viewport = viewport_from_owner(&owner, root_id);

    assert_eq!(
        viewport.min_scroll_extent(),
        -50.0,
        "reverse slivers must accumulate negative min_scroll_extent",
    );
}

#[test]
fn viewport_center_at_child_count_behaves_like_no_center() {
    for axis in [AxisDirection::TopToBottom, AxisDirection::LeftToRight] {
        let mut with_center = RenderViewport::with_offset(
            axis,
            test_cross_axis_direction(axis),
            ScrollableViewportOffset::zero(),
        );
        with_center.set_center_sliver_index(Some(1));

        let without_center = RenderViewport::with_offset(
            axis,
            test_cross_axis_direction(axis),
            ScrollableViewportOffset::zero(),
        );

        let (owner_a, _, sliver_a) = laid_out_viewport_with_sliver(with_center, 40.0);
        let (owner_b, _, sliver_b) = laid_out_viewport_with_sliver(without_center, 40.0);

        assert_eq!(
            render_offset(&owner_a, sliver_a),
            render_offset(&owner_b, sliver_b),
            "{axis:?}: center==child_count must match no-center behavior",
        );
    }
}

#[test]
fn viewport_hit_test_maps_each_axis_direction_into_sliver_main_axis() {
    let forward_cases = [
        (
            AxisDirection::TopToBottom,
            None,
            Offset::new(px(10.0), px(10.0)),
            Offset::new(px(10.0), px(30.0)),
        ),
        (
            AxisDirection::BottomToTop,
            None,
            Offset::new(px(10.0), px(90.0)),
            Offset::new(px(10.0), px(70.0)),
        ),
        (
            AxisDirection::LeftToRight,
            None,
            Offset::new(px(10.0), px(10.0)),
            Offset::new(px(30.0), px(10.0)),
        ),
        (
            AxisDirection::RightToLeft,
            None,
            Offset::new(px(90.0), px(10.0)),
            Offset::new(px(70.0), px(10.0)),
        ),
    ];
    let reverse_cases = [
        (
            AxisDirection::TopToBottom,
            Some(0),
            Offset::new(px(10.0), px(90.0)),
            Offset::new(px(10.0), px(70.0)),
        ),
        (
            AxisDirection::BottomToTop,
            Some(0),
            Offset::new(px(10.0), px(10.0)),
            Offset::new(px(10.0), px(30.0)),
        ),
        (
            AxisDirection::LeftToRight,
            Some(0),
            Offset::new(px(90.0), px(10.0)),
            Offset::new(px(70.0), px(10.0)),
        ),
        (
            AxisDirection::RightToLeft,
            Some(0),
            Offset::new(px(10.0), px(10.0)),
            Offset::new(px(30.0), px(10.0)),
        ),
    ];

    for (axis_direction, center, hit_position, miss_position) in
        forward_cases.into_iter().chain(reverse_cases)
    {
        let mut viewport = RenderViewport::with_offset(
            axis_direction,
            test_cross_axis_direction(axis_direction),
            ScrollableViewportOffset::zero(),
        );
        if let Some(center_index) = center {
            viewport.set_center_sliver_index(Some(center_index));
        }

        let mut owner = PipelineOwner::new();
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
            hits_at(&owner, hit_position.dx.get(), hit_position.dy.get()),
            vec![sliver_id, root_id],
            "{axis_direction:?} center={center:?} must map the leading hit band into sliver main-axis space",
        );
        assert!(
            hits_at(&owner, miss_position.dx.get(), miss_position.dy.get()).is_empty(),
            "{axis_direction:?} center={center:?} must miss outside the sliver's leading hit band",
        );
    }
}

#[test]
fn viewport_hit_testing_tracks_paint_origin_and_hit_test_extent() {
    let viewport = RenderViewport::with_offset(
        AxisDirection::TopToBottom,
        AxisDirection::LeftToRight,
        ScrollableViewportOffset::zero(),
    );

    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(viewport));
    let sliver_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(GeometrySliver::new(0.0, 20.0, 30.0, 0.0, 12.0)) as BoxedSliverObject,
        )
        .expect("paint-origin sliver");

    let owner = laid_out(owner, root_id);

    assert_eq!(
        render_offset(&owner, sliver_id),
        Offset::new(px(0.0), px(20.0)),
        "paint_origin shifts the physical sliver paint offset",
    );
    assert_eq!(
        hits_at(&owner, 10.0, 31.0),
        vec![sliver_id, root_id],
        "parent y=31 maps to child main=11 after the 20px paint_origin shift",
    );
    assert!(
        hits_at(&owner, 10.0, 35.0).is_empty(),
        "parent y=35 is still inside paint_extent but beyond hit_test_extent=12",
    );
    assert!(
        hits_at(&owner, 10.0, 10.0).is_empty(),
        "points before the shifted paint origin must miss",
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
        .insert_sliver_child(root_id, Box::new(InvisibleHitSliver) as BoxedSliverObject)
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
