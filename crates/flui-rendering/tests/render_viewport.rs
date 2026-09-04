//! Core.2 W3.4a: minimal `RenderViewport` driver for sliver children.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use flui_objects::RenderViewport;
use flui_rendering::{
    constraints::{GrowthDirection, SliverGeometry},
    context::{SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverParentData,
    pipeline::PipelineOwner,
    testing::inspect,
    traits::RenderSliver,
    view::{ScrollableViewportOffset, SliverPaintOrder, ViewportOffset},
};
use flui_tree::Leaf;
use flui_types::{Offset, Size, geometry::px, layout::AxisDirection};

use crate::common::{BoxedSliverObject, laid_out_tight_100x100 as laid_out};

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

impl flui_foundation::Diagnosticable for FixedSliver {}

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

impl flui_foundation::Diagnosticable for InvisibleHitSliver {}

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

impl flui_foundation::Diagnosticable for MainAxisBandSliver {}

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

impl flui_foundation::Diagnosticable for GeometrySliver {}

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

impl flui_foundation::Diagnosticable for CorrectingSliver {}

impl RenderSliver for CorrectingSliver {
    type Arity = Leaf;
    type ParentData = SliverParentData;

    fn perform_layout(
        &mut self,
        _ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>,
    ) -> SliverGeometry {
        if self.corrected {
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
        } else {
            self.corrected = true;
            SliverGeometry::scroll_offset_correction(self.correction)
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

impl flui_foundation::Diagnosticable for CountingSliver {}

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

impl flui_foundation::Diagnosticable for OutOfBandSliver {}

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

impl flui_foundation::Diagnosticable for DynamicOutOfBandSliver {}

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
    let laid_out_size = owner
        .render_tree()
        .get(root_id)
        .and_then(flui_rendering::storage::RenderNode::geometry_box)
        .expect("root viewport has committed box geometry");
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

    assert_eq!(laid_out_size, Size::new(px(100.0), px(100.0)));
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

// `sliver_obstruction_extents` must be keyed by absolute child index, not by
// the order `layout_child_sequence` visits children in — with a reverse
// group, that visit order is `(center-1, center-2, ..., 0)`, which disagrees
// with index order whenever the reverse group has more than one child.
// Three children, `center: Some(2)`: the reverse group `[0, 2)` is visited
// index 1 THEN index 0 (walking backwards from `center - 1`); the forward
// group `[2, 3)` visits index 2. A push-in-visit-order vec would read back
// [obstruction(1), obstruction(0), obstruction(2)] — right values, wrong
// slots — silently swapping what `max_scroll_obstruction_extent_before`
// reports for indices 0 and 1.
//
// Hand arithmetic (`anchor: 0.5`, `offset: 0.0`, 100px viewport, each
// child's `scroll_extent: 10.0`, obstruction extents 5.0 / 7.0 / 11.0 for
// indices 0 / 1 / 2): `max_scroll_extent = 10` (only child 2 is forward),
// `min_scroll_extent = -20` (children 0 and 1 are reverse); the accepted
// offset range is `min = (-20 + 100*0.5).min(0) = 0`,
// `max = (10 - 100*0.5).max(0) = 0` — `0.0` is the ONLY accepted offset, so
// this converges without any correction cycle. Slot-indexed, the vec reads
// [5.0, 7.0, 11.0] (index order, regardless of visit order) and:
//   before(0): child 0 is reverse (< center); sum over (0, center) = {1} = 7.0
//   before(1): child 1 is reverse; sum over (1, center) = {} = 0.0
//   before(2): child 2 is the first forward child; sum over [center, 2) = {} = 0.0
// A push-in-visit-order vec would instead read [7.0, 5.0, 11.0] (index 1's
// value landing in slot 0, index 0's in slot 1) and the OLD
// `.take(child_index).sum()` formula would report before(0) = 0.0,
// before(1) = 7.0, before(2) = 12.0 — every one of the three wrong.
#[test]
fn viewport_max_scroll_obstruction_extent_before_is_keyed_by_slot_not_layout_order() {
    let mut viewport = RenderViewport::with_offset(
        AxisDirection::TopToBottom,
        AxisDirection::LeftToRight,
        ScrollableViewportOffset::zero(),
    );
    assert_eq!(
        viewport.set_center(Some(2)),
        flui_rendering::RenderUpdateImpact::LAYOUT,
    );
    assert_eq!(
        viewport.set_anchor(0.5),
        flui_rendering::RenderUpdateImpact::LAYOUT,
    );

    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(viewport));
    owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(OutOfBandSliver::new(10.0, 5.0, false)) as BoxedSliverObject,
        )
        .expect("child 0 (reverse)");
    owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(OutOfBandSliver::new(10.0, 7.0, false)) as BoxedSliverObject,
        )
        .expect("child 1 (reverse)");
    owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(OutOfBandSliver::new(10.0, 11.0, false)) as BoxedSliverObject,
        )
        .expect("child 2 (forward)");

    let owner = laid_out(owner, root_id);
    let viewport = viewport_from_owner(&owner, root_id);

    assert_eq!(viewport.max_scroll_obstruction_extent_before(0), Some(7.0));
    assert_eq!(viewport.max_scroll_obstruction_extent_before(1), Some(0.0));
    assert_eq!(viewport.max_scroll_obstruction_extent_before(2), Some(0.0));
}

// Flutter's `center` is always a direct child (`center!.parent == this`), so
// a lone reverse-growth sliver — FLUI's old `center_sliver_index(Some(0))`,
// which meant "every child reverse" — is unrepresentable under the new
// model: with one child, the only valid `center` is `0`, and `center == 0`
// means every child grows FORWARD (empty reverse group). The reverse rows
// below model it as the smallest representable tree instead: a reverse
// child (index 0) before a forward filler (index 1), `center: Some(1)`,
// with `anchor: 1.0` so the reverse group claims the WHOLE viewport
// (`center_offset == main_axis_extent * 1.0 == main_axis_extent`) — giving
// `forward_remaining_paint_extent == 0` (the filler paints nothing but
// still lays out) and `layout_offset == main_axis_extent * (1 - anchor) ==
// 0` for the reverse group, so its physical offset —
// `size - layout_offset(0) - paint_extent(40) == size - 40` on the
// paint-origin axis — is EXACTLY the old all-reverse row's expected value.
#[test]
fn viewport_positions_first_sliver_for_axis_and_growth_matrix() {
    let cases = [
        (
            AxisDirection::TopToBottom,
            GrowthDirection::Forward,
            Offset::new(px(0.0), px(0.0)),
        ),
        (
            AxisDirection::TopToBottom,
            GrowthDirection::Reverse,
            Offset::new(px(0.0), px(60.0)),
        ),
        (
            AxisDirection::BottomToTop,
            GrowthDirection::Forward,
            Offset::new(px(0.0), px(60.0)),
        ),
        (
            AxisDirection::BottomToTop,
            GrowthDirection::Reverse,
            Offset::new(px(0.0), px(0.0)),
        ),
        (
            AxisDirection::LeftToRight,
            GrowthDirection::Forward,
            Offset::new(px(0.0), px(0.0)),
        ),
        (
            AxisDirection::LeftToRight,
            GrowthDirection::Reverse,
            Offset::new(px(60.0), px(0.0)),
        ),
        (
            AxisDirection::RightToLeft,
            GrowthDirection::Forward,
            Offset::new(px(60.0), px(0.0)),
        ),
        (
            AxisDirection::RightToLeft,
            GrowthDirection::Reverse,
            Offset::new(px(0.0), px(0.0)),
        ),
    ];

    for (axis_direction, growth, expected_offset) in cases {
        let mut viewport = RenderViewport::with_offset(
            axis_direction,
            test_cross_axis_direction(axis_direction),
            ScrollableViewportOffset::zero(),
        );
        if growth == GrowthDirection::Reverse {
            assert_eq!(
                viewport.set_center(Some(1)),
                flui_rendering::RenderUpdateImpact::LAYOUT,
            );
            assert_eq!(
                viewport.set_anchor(1.0),
                flui_rendering::RenderUpdateImpact::LAYOUT,
            );
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
        if growth == GrowthDirection::Reverse {
            owner
                .render_tree_mut()
                .insert_sliver_child(
                    root_id,
                    Box::new(FixedSliver::new(20.0)) as BoxedSliverObject,
                )
                .expect("forward filler");
        }

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
    // Same restructuring as `viewport_positions_first_sliver_for_axis_and_growth_matrix`:
    // a lone reverse sliver is unrepresentable under Flutter's model, so this
    // is one reverse child (index 0, asserted below) before a forward
    // filler (index 1), with `anchor: 1.0` reproducing the old all-reverse
    // offset (y = 100*1.0 - 40 = 60) exactly.
    assert_eq!(
        viewport.set_center(Some(1)),
        flui_rendering::RenderUpdateImpact::LAYOUT,
    );
    assert_eq!(
        viewport.set_anchor(1.0),
        flui_rendering::RenderUpdateImpact::LAYOUT,
    );

    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(viewport));
    let sliver = FixedSliver::recording_growth(40.0);
    let sliver_id = owner
        .render_tree_mut()
        .insert_sliver_child(root_id, Box::new(sliver) as BoxedSliverObject)
        .expect("sliver");
    owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(FixedSliver::new(20.0)) as BoxedSliverObject,
        )
        .expect("forward filler");

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

// Under FLUI's old `center_sliver_index`, `Some(1)` meant "children [0,1)
// forward, [1,2) reverse" — child 0 forward, child 1 reverse. Under
// Flutter's model `center` is the first FORWARD child, so the SAME value,
// `Some(1)`, now means the opposite: child 0 (before center) is the reverse
// group, child 1 (at center) is the forward group. `anchor: 0.5` splits the
// 100px viewport evenly (`center_offset == 50`), giving each 30px child
// room to lay out at its full extent on its own side of the center line:
//   reverse (child 0): layout_offset == forward_remaining_paint_extent ==
//     50, so its physical offset is `size - 50 - 30 == 20`.
//   forward (child 1): layout_offset == reverse_remaining_paint_extent ==
//     50 (center_offset < main_axis_extent), so its physical offset is the
//     center line itself, `50` — exactly where the reverse child's far edge
//     (`20 + 30 == 50`) ends, with no gap and no overlap.
#[test]
fn viewport_center_partition_lays_out_forward_then_reverse() {
    let mut viewport = RenderViewport::with_offset(
        AxisDirection::TopToBottom,
        AxisDirection::LeftToRight,
        ScrollableViewportOffset::zero(),
    );
    assert_eq!(
        viewport.set_center(Some(1)),
        flui_rendering::RenderUpdateImpact::LAYOUT,
    );
    assert_eq!(
        viewport.set_anchor(0.5),
        flui_rendering::RenderUpdateImpact::LAYOUT,
    );

    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(viewport));
    let s0 = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(FixedSliver::recording_growth(30.0)) as BoxedSliverObject,
        )
        .expect("reverse sliver");
    let s1 = owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(FixedSliver::recording_growth(30.0)) as BoxedSliverObject,
        )
        .expect("forward sliver");

    let owner = laid_out(owner, root_id);
    let rev = fixed_sliver_from_owner(&owner, s0);
    let fwd = fixed_sliver_from_owner(&owner, s1);

    assert_eq!(rev.last_growth_direction(), GrowthDirection::Reverse);
    assert_eq!(fwd.last_growth_direction(), GrowthDirection::Forward);
    assert_eq!(render_offset(&owner, s0), Offset::new(px(0.0), px(20.0)));
    assert_eq!(render_offset(&owner, s1), Offset::new(px(0.0), px(50.0)));
}

// A lone reverse sliver (FLUI's old `center_sliver_index(Some(0))` == "all
// reverse") is unrepresentable under Flutter's model — `center` must be a
// direct child, and with one child the only valid `center` is `0`, which
// means every child grows FORWARD. Rewritten as one reverse child (index 0,
// the 50px sliver under test) before one forward filler (index 1), with
// `anchor: 1.0` giving the reverse group the whole viewport so the 50px
// sliver still lays out at its full `scroll_extent` — `min_scroll_extent`
// accumulates `-50.0` from that child regardless of the filler's own
// (zero-room) forward geometry.
#[test]
fn viewport_reverse_slivers_produce_negative_min_scroll_extent() {
    let mut viewport = RenderViewport::with_offset(
        AxisDirection::TopToBottom,
        AxisDirection::LeftToRight,
        ScrollableViewportOffset::zero(),
    );
    assert_eq!(
        viewport.set_center(Some(1)),
        flui_rendering::RenderUpdateImpact::LAYOUT,
    );
    assert_eq!(
        viewport.set_anchor(1.0),
        flui_rendering::RenderUpdateImpact::LAYOUT,
    );

    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(Box::new(viewport));
    owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(FixedSliver::new(50.0)) as BoxedSliverObject,
        )
        .expect("reverse sliver");
    owner
        .render_tree_mut()
        .insert_sliver_child(
            root_id,
            Box::new(FixedSliver::new(20.0)) as BoxedSliverObject,
        )
        .expect("forward filler");
    let owner = laid_out(owner, root_id);
    let viewport = viewport_from_owner(&owner, root_id);

    assert_eq!(
        viewport.min_scroll_extent(),
        -50.0,
        "reverse slivers must accumulate negative min_scroll_extent",
    );
}

// `viewport_center_at_child_count_behaves_like_no_center` tested FLUI's old
// "no center" spelling, `center_sliver_index(Some(child_count))` — under
// Flutter's model `center` is always a direct child, so `Some(n) ==
// child_count` is invalid configuration, not a synonym for `None`. That
// state does not exist any more; there is nothing left for this test to
// pin, so it is deleted rather than rewritten.

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
    // `Some(1)` + `anchor: 1.0`, not `Some(0)`: a lone reverse-growth sliver
    // (FLUI's old `center_sliver_index(Some(0))`) is unrepresentable under
    // Flutter's model, since `center` must be a direct child and a single
    // child can only be `center == 0` (all-forward). Two children instead —
    // the band sliver under test (index 0, reverse) before a forward filler
    // (index 1) — with `anchor: 1.0` giving the reverse group the whole
    // viewport, reproduces the exact same physical offset the old
    // all-reverse layout gave this 40px sliver, so every hit/miss position
    // below is unchanged.
    let reverse_cases = [
        (
            AxisDirection::TopToBottom,
            Some(1),
            Offset::new(px(10.0), px(90.0)),
            Offset::new(px(10.0), px(70.0)),
        ),
        (
            AxisDirection::BottomToTop,
            Some(1),
            Offset::new(px(10.0), px(10.0)),
            Offset::new(px(10.0), px(30.0)),
        ),
        (
            AxisDirection::LeftToRight,
            Some(1),
            Offset::new(px(90.0), px(10.0)),
            Offset::new(px(70.0), px(10.0)),
        ),
        (
            AxisDirection::RightToLeft,
            Some(1),
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
            assert_eq!(
                viewport.set_center(Some(center_index)),
                flui_rendering::RenderUpdateImpact::LAYOUT,
            );
            assert_eq!(
                viewport.set_anchor(1.0),
                flui_rendering::RenderUpdateImpact::LAYOUT,
            );
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
        if center.is_some() {
            owner
                .render_tree_mut()
                .insert_sliver_child(
                    root_id,
                    Box::new(MainAxisBandSliver::new(20.0, 0.0, 15.0)) as BoxedSliverObject,
                )
                .expect("forward filler");
        }

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
    assert_eq!(
        viewport.set_paint_order(SliverPaintOrder::LastIsTop),
        flui_rendering::RenderUpdateImpact::PAINT,
    );

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
    assert_eq!(
        viewport.set_paint_order(SliverPaintOrder::FirstIsTop),
        flui_rendering::RenderUpdateImpact::NONE,
    );

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
