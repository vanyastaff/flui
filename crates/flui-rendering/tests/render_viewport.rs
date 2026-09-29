//! Core.2 W3.4a: minimal `RenderViewport` driver for sliver children.

use flui_foundation::Leaf;
use flui_foundation::geometry::{Offset, Size};
use flui_objects::RenderViewport;
use flui_rendering::constraints::AxisDirection;
use flui_rendering::{
    constraints::{GrowthDirection, SliverGeometry},
    context::{SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverParentData,
    pipeline::PipelineOwner,
    testing::inspect,
    traits::RenderSliver,
    view::{ScrollableViewportOffset, ViewportOffset},
};

use crate::common::{BoxedSliverObject, laid_out_tight_100x100 as laid_out};

fn render_offset(
    owner: &PipelineOwner<flui_rendering::pipeline::phase::Layout>,
    id: flui_foundation::RenderId,
) -> Offset {
    inspect::render_offset(owner, id).expect("node exists")
}

fn hits_at(
    owner: &PipelineOwner<flui_rendering::pipeline::phase::Layout>,
    x: f64,
    y: f64,
) -> Vec<flui_foundation::RenderId> {
    inspect::hit_path(owner, x, y)
}

const fn test_cross_axis_direction(axis_direction: AxisDirection) -> AxisDirection {
    match axis_direction {
        AxisDirection::TopToBottom | AxisDirection::BottomToTop => AxisDirection::LeftToRight,
        AxisDirection::LeftToRight | AxisDirection::RightToLeft => AxisDirection::TopToBottom,
    }
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
    scroll_extent: f64,
    paint_extent: f64,
    layout_extent: Option<f64>,
    /// Cross-axis extent captured at layout, read by the `&self`-only
    /// `hit_test_self` (the sliver hit-test context does not carry it).
    cross_axis_extent: f64,
    /// When `Some`, updated each layout with the child's growth direction.
    recorded_growth_direction: Option<GrowthDirection>,
}

impl FixedSliver {
    fn new(scroll_extent: f64) -> Self {
        Self {
            scroll_extent,
            paint_extent: scroll_extent,
            layout_extent: None,
            cross_axis_extent: 0.0,
            recorded_growth_direction: None,
        }
    }

    fn recording_growth(scroll_extent: f64) -> Self {
        Self {
            recorded_growth_direction: Some(GrowthDirection::Forward),
            ..Self::new(scroll_extent)
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

    fn hit_test_self(&self, main: f64, cross: f64) -> bool {
        cross >= 0.0 && cross < self.cross_axis_extent && main >= 0.0
    }
}

#[derive(Debug)]
struct MainAxisBandSliver {
    extent: f64,
    hit_start: f64,
    hit_end: f64,
    /// Cross-axis extent captured at layout, read by the `&self`-only
    /// `hit_test_self` (the sliver hit-test context does not carry it).
    cross_axis_extent: f64,
}

impl MainAxisBandSliver {
    fn new(extent: f64, hit_start: f64, hit_end: f64) -> Self {
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

    fn hit_test_self(&self, main: f64, cross: f64) -> bool {
        main >= self.hit_start
            && main < self.hit_end
            && cross >= 0.0
            && cross < self.cross_axis_extent
    }
}

#[derive(Debug)]
struct CorrectingSliver {
    correction: f64,
    corrected: bool,
}

impl CorrectingSliver {
    fn new(correction: f64) -> Self {
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

    assert_eq!(laid_out_size, Size::new(100.0, 100.0));
    assert_eq!(viewport.offset().viewport_dimension(), 100.0);
    assert_eq!(viewport.offset().max_scroll_extent(), 60.0);
    assert_eq!(viewport.offset().pixels(), 40.0);
    assert_eq!(
        render_offset(&owner, first_id),
        Offset::new(0.0, 0.0),
        "first forward sliver paints at the viewport origin when scroll_offset is consumed by constraints",
    );
    assert_eq!(
        render_offset(&owner, second_id),
        Offset::new(0.0, 30.0),
        "second sliver advances by first.layout_extent after the first sliver consumes 40px of scroll",
    );
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
            Offset::new(0.0, 0.0),
        ),
        (
            AxisDirection::TopToBottom,
            GrowthDirection::Reverse,
            Offset::new(0.0, 60.0),
        ),
        (
            AxisDirection::BottomToTop,
            GrowthDirection::Forward,
            Offset::new(0.0, 60.0),
        ),
        (
            AxisDirection::BottomToTop,
            GrowthDirection::Reverse,
            Offset::new(0.0, 0.0),
        ),
        (
            AxisDirection::LeftToRight,
            GrowthDirection::Forward,
            Offset::new(0.0, 0.0),
        ),
        (
            AxisDirection::LeftToRight,
            GrowthDirection::Reverse,
            Offset::new(60.0, 0.0),
        ),
        (
            AxisDirection::RightToLeft,
            GrowthDirection::Forward,
            Offset::new(60.0, 0.0),
        ),
        (
            AxisDirection::RightToLeft,
            GrowthDirection::Reverse,
            Offset::new(0.0, 0.0),
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
    assert_eq!(render_offset(&owner, s0), Offset::new(0.0, 20.0));
    assert_eq!(render_offset(&owner, s1), Offset::new(0.0, 50.0));
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
            Offset::new(10.0, 10.0),
            Offset::new(10.0, 30.0),
        ),
        (
            AxisDirection::BottomToTop,
            None,
            Offset::new(10.0, 90.0),
            Offset::new(10.0, 70.0),
        ),
        (
            AxisDirection::LeftToRight,
            None,
            Offset::new(10.0, 10.0),
            Offset::new(30.0, 10.0),
        ),
        (
            AxisDirection::RightToLeft,
            None,
            Offset::new(90.0, 10.0),
            Offset::new(70.0, 10.0),
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
            Offset::new(10.0, 90.0),
            Offset::new(10.0, 70.0),
        ),
        (
            AxisDirection::BottomToTop,
            Some(1),
            Offset::new(10.0, 10.0),
            Offset::new(10.0, 30.0),
        ),
        (
            AxisDirection::LeftToRight,
            Some(1),
            Offset::new(90.0, 10.0),
            Offset::new(70.0, 10.0),
        ),
        (
            AxisDirection::RightToLeft,
            Some(1),
            Offset::new(10.0, 10.0),
            Offset::new(30.0, 10.0),
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
            hits_at(&owner, hit_position.dx, hit_position.dy),
            vec![sliver_id, root_id],
            "{axis_direction:?} center={center:?} must map the leading hit band into sliver main-axis space",
        );
        assert!(
            hits_at(&owner, miss_position.dx, miss_position.dy).is_empty(),
            "{axis_direction:?} center={center:?} must miss outside the sliver's leading hit band",
        );
    }
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
