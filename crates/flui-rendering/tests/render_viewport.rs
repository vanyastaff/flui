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

const fn test_cross_axis_direction(axis_direction: AxisDirection) -> AxisDirection {
    match axis_direction {
        AxisDirection::TopToBottom | AxisDirection::BottomToTop => AxisDirection::LeftToRight,
        AxisDirection::LeftToRight | AxisDirection::RightToLeft => AxisDirection::TopToBottom,
    }
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

pub(crate) fn viewport_lays_out_forward_slivers_and_applies_content_dimensions() {
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

// The `center` is always a direct child, so
// a lone reverse-growth sliver — the old `center_sliver_index(Some(0))`,
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
pub(crate) fn viewport_positions_first_sliver_for_axis_and_growth_matrix() {
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

// `viewport_center_at_child_count_behaves_like_no_center` tested FLUI's old
// "no center" spelling, `center_sliver_index(Some(child_count))` — under
// the current model `center` is always a direct child, so `Some(n) ==
// child_count` is invalid configuration, not a synonym for `None`. That
// state does not exist any more; there is nothing left for this test to
// pin, so it is deleted rather than rewritten.
