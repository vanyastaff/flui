//! Pure sliver layout math shared by render objects and the pipeline.

use flui_foundation::geometry::Axis;
use flui_foundation::geometry::Offset;

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

    // ── grid_child_paint_offset ───────────────────────────────────────────────
}
