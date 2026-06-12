//! Shared sliver layout helpers used by multi-child sliver objects.
//!
//! Box-child hit testing stays in [`crate::pipeline::PipelineOwner::hit_test_sliver_subtree_impl`];
//! sliver objects delegate via `hit_test_child_at_layout_offset` on the hit-test context.

use flui_types::{Offset, geometry::px, layout::Axis};

use crate::constraints::{SliverConstraints, SliverGeometry, right_way_up};

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
