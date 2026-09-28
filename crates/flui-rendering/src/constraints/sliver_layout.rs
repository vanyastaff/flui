//! Pure sliver layout math shared by render objects and the pipeline.

use flui_types::{Offset, layout::Axis};

use super::{SliverConstraints, SliverGeometry, right_way_up};

/// Computes the paint offset for a box child laid out at `layout_offset` along
/// the sliver main axis with `child_main_extent`.
#[inline]
pub fn child_paint_offset(
    constraints: &SliverConstraints,
    geometry: &SliverGeometry,
    layout_offset: f64,
    child_main_extent: f64,
) -> Offset {
    let layout_offset = layout_offset;
    let child_main_extent = child_main_extent;
    let child_main_axis_position = layout_offset - constraints.scroll_offset;
    let main_axis_delta = if right_way_up(constraints.axis_direction, constraints.growth_direction)
    {
        child_main_axis_position
    } else {
        geometry.paint_extent - child_main_extent - child_main_axis_position
    };

    match constraints.axis_direction.axis() {
        Axis::Horizontal => Offset::new(main_axis_delta, 0.0),
        Axis::Vertical => Offset::new(0.0, main_axis_delta),
    }
}

/// Computes the 2-D paint offset for a grid Box child.
///
/// Extends [`child_paint_offset`] with a `cross_axis_offset` argument, placing
/// the child at `(cross, main)` for a vertical sliver or `(main, cross)` for a
/// horizontal one.  The `cross_axis_offset` is already in the layout's chosen
/// direction (forward or mirrored) because `SliverGridLayout::get_cross_axis_offset_of_child`
/// applies the `reverse_cross_axis` flip before this function is called.
///
/// Flutter parity: `RenderSliverGrid.performLayout` position pass
/// (`.flutter/flutter-master/packages/flutter/lib/src/rendering/sliver_grid.dart`).
#[inline]
pub fn grid_child_paint_offset(
    constraints: &SliverConstraints,
    geometry: &SliverGeometry,
    layout_offset: f64,
    child_main_axis_extent: f64,
    cross_axis_offset: f64,
) -> Offset {
    let layout_offset_f = layout_offset;
    let child_main_axis_extent_f = child_main_axis_extent;
    let cross_f = cross_axis_offset;

    let child_main_axis_position = layout_offset_f - constraints.scroll_offset;
    let main_axis_delta = if right_way_up(constraints.axis_direction, constraints.growth_direction)
    {
        child_main_axis_position
    } else {
        geometry.paint_extent - child_main_axis_extent_f - child_main_axis_position
    };

    match constraints.axis_direction.axis() {
        Axis::Horizontal => Offset::new(main_axis_delta, cross_f),
        Axis::Vertical => Offset::new(cross_f, main_axis_delta),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraints::GrowthDirection;
    use crate::testing::sliver;

    fn vertical_constraints(growth: GrowthDirection, scroll_offset: f64) -> SliverConstraints {
        sliver::vertical()
            .with_growth_direction(growth)
            .scroll_offset(scroll_offset)
            .remaining_paint_extent(200.0)
            .cross_axis_extent(100.0)
            .viewport_main_axis_extent(200.0)
            .remaining_cache_extent(200.0)
            .build()
    }

    fn geometry(paint_extent: f64, scroll_extent: f64) -> SliverGeometry {
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
            Offset::new(0.0, -10.0),
        );
    }

    #[test]
    fn child_paint_offset_reverse_vertical_flips_within_paint_extent() {
        let constraints = vertical_constraints(GrowthDirection::Reverse, 0.0);
        let geom = geometry(40.0, 40.0);

        assert_eq!(
            child_paint_offset(&constraints, &geom, 0.0, 40.0),
            Offset::new(0.0, 0.0),
        );
    }

    #[test]
    fn child_paint_offset_list_child_at_layout_offset() {
        let constraints = vertical_constraints(GrowthDirection::Forward, 0.0);
        let geom = geometry(80.0, 120.0);

        assert_eq!(
            child_paint_offset(&constraints, &geom, 40.0, 30.0),
            Offset::new(0.0, 40.0),
        );
    }

    #[test]
    fn child_paint_offset_horizontal_rtl_reverse_maps_to_x() {
        use flui_types::layout::AxisDirection;

        let constraints = sliver::horizontal()
            .with_axis_direction(AxisDirection::RightToLeft)
            .with_growth_direction(GrowthDirection::Reverse)
            .scroll_offset(5.0)
            .remaining_paint_extent(200.0)
            .cross_axis_extent(100.0)
            .viewport_main_axis_extent(200.0)
            .remaining_cache_extent(200.0)
            .build();
        let geom = geometry(80.0, 100.0);

        assert_eq!(
            child_paint_offset(&constraints, &geom, 0.0, 80.0),
            Offset::new(-5.0, 0.0),
        );
    }

    // ── grid_child_paint_offset ───────────────────────────────────────────────

    #[test]
    fn grid_child_paint_offset_vertical_forward_places_cross_on_x() {
        // Vertical forward sliver, scroll_offset=100.
        // child at layout_offset=100, cross=50 → main_delta = 100-100 = 0
        // Vertical: Offset(cross, main) = (50, 0)
        let constraints = vertical_constraints(GrowthDirection::Forward, 100.0);
        let geom = geometry(200.0, 400.0);

        assert_eq!(
            super::grid_child_paint_offset(&constraints, &geom, 100.0, 100.0, 50.0,),
            Offset::new(50.0, 0.0),
        );
    }

    #[test]
    fn grid_child_paint_offset_vertical_forward_second_row() {
        // child at layout_offset=200, cross=100 → main_delta = 200-100 = 100
        let constraints = vertical_constraints(GrowthDirection::Forward, 100.0);
        let geom = geometry(200.0, 400.0);

        assert_eq!(
            super::grid_child_paint_offset(&constraints, &geom, 200.0, 100.0, 100.0,),
            Offset::new(100.0, 100.0),
        );
    }

    #[test]
    fn grid_child_paint_offset_horizontal_forward_places_cross_on_y() {
        // Horizontal forward sliver, scroll_offset=0.
        // child at layout_offset=0, cross=50 → main_delta=0
        // Horizontal: Offset(main, cross) = (0, 50)
        let constraints = sliver::horizontal()
            .scroll_offset(0.0)
            .remaining_paint_extent(200.0)
            .cross_axis_extent(200.0)
            .viewport_main_axis_extent(200.0)
            .remaining_cache_extent(200.0)
            .build();
        let geom = geometry(200.0, 400.0);

        assert_eq!(
            super::grid_child_paint_offset(&constraints, &geom, 0.0, 100.0, 50.0,),
            Offset::new(0.0, 50.0),
        );

        // Second column (layout_offset=100, cross=0)
        assert_eq!(
            super::grid_child_paint_offset(&constraints, &geom, 100.0, 100.0, 0.0,),
            Offset::new(100.0, 0.0),
        );
    }
}
