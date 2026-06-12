//! Pure sliver layout math shared by render objects and the pipeline.

use flui_types::{Offset, geometry::px, layout::Axis};

use super::{SliverConstraints, SliverGeometry, right_way_up};

/// Computes the paint offset for a box child laid out at `layout_offset` along
/// the sliver main axis with `child_main_extent`.
#[inline]
pub(crate) fn child_paint_offset(
    constraints: &SliverConstraints,
    geometry: &SliverGeometry,
    layout_offset: f32,
    child_main_extent: f32,
) -> Offset {
    let child_main_axis_position = layout_offset - constraints.scroll_offset;
    let main_axis_delta = if right_way_up(constraints.axis_direction, constraints.growth_direction)
    {
        child_main_axis_position
    } else {
        geometry.paint_extent - child_main_extent - child_main_axis_position
    };

    match constraints.axis_direction.axis() {
        Axis::Horizontal => Offset::new(px(main_axis_delta), px(0.0)),
        Axis::Vertical => Offset::new(px(0.0), px(main_axis_delta)),
    }
}

#[cfg(test)]
mod tests {
    use flui_types::layout::AxisDirection;

    use super::*;
    use crate::constraints::GrowthDirection;
    use crate::view::ScrollDirection;

    fn vertical_constraints(growth: GrowthDirection, scroll_offset: f32) -> SliverConstraints {
        SliverConstraints::new(
            AxisDirection::TopToBottom,
            growth,
            ScrollDirection::Idle,
            scroll_offset,
            0.0,
            0.0,
            200.0,
            100.0,
            AxisDirection::LeftToRight,
            200.0,
            200.0,
            0.0,
        )
    }

    fn geometry(paint_extent: f32, scroll_extent: f32) -> SliverGeometry {
        SliverGeometry {
            scroll_extent,
            paint_extent,
            layout_extent: paint_extent,
            max_paint_extent: scroll_extent,
            hit_test_extent: paint_extent,
            visible: paint_extent > 0.0,
            ..SliverGeometry::ZERO
        }
    }

    #[test]
    fn child_paint_offset_forward_vertical_uses_scroll_offset() {
        let constraints = vertical_constraints(GrowthDirection::Forward, 10.0);
        let geom = geometry(80.0, 100.0);

        assert_eq!(
            child_paint_offset(&constraints, &geom, 0.0, 100.0),
            Offset::new(px(0.0), px(-10.0)),
        );
    }

    #[test]
    fn child_paint_offset_reverse_vertical_flips_within_paint_extent() {
        let constraints = vertical_constraints(GrowthDirection::Reverse, 0.0);
        let geom = geometry(40.0, 40.0);

        assert_eq!(
            child_paint_offset(&constraints, &geom, 0.0, 40.0),
            Offset::new(px(0.0), px(0.0)),
        );
    }

    #[test]
    fn child_paint_offset_list_child_at_layout_offset() {
        let constraints = vertical_constraints(GrowthDirection::Forward, 0.0);
        let geom = geometry(80.0, 120.0);

        assert_eq!(
            child_paint_offset(&constraints, &geom, 40.0, 30.0),
            Offset::new(px(0.0), px(40.0)),
        );
    }
}
