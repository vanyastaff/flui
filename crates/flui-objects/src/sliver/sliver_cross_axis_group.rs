//! Side-by-side sliver tracks, sharing one scroll coordinate.

use flui_foundation::{
    Variable,
    geometry::{Axis, Offset},
};
use flui_rendering::{
    RenderError,
    constraints::SliverGeometry,
    context::{SliverHitTestContext, SliverLayoutContext},
    parent_data::{SliverCrossAxisAllocation, SliverCrossAxisParentData},
    traits::RenderSliver,
};

/// Lays out content-sized tracks first, then shares the remainder by weight.
///
/// Children scroll together; the longest child determines scrollable extent.
/// Each geometry component is composed independently so a shorter pinned header
/// still contributes obstruction. Child positions publish only after all
/// children accept layout without requesting a scroll correction.
#[derive(Debug, Default, Clone)]
pub struct RenderSliverCrossAxisGroup {
    child_count: usize,
}

impl RenderSliverCrossAxisGroup {
    /// Creates a group whose ordinary children receive equal flexible tracks.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl flui_foundation::inspect::Inspect for RenderSliverCrossAxisGroup {
    fn kind(&self) -> &'static str {
        "RenderSliverCrossAxisGroup"
    }

    fn inspect(&self, _out: &mut flui_foundation::inspect::Fields<'_>) {}
}

impl RenderSliver for RenderSliverCrossAxisGroup {
    type Arity = Variable;
    type ParentData = SliverCrossAxisParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Variable, Self::ParentData>,
    ) -> flui_rendering::RenderResult<SliverGeometry> {
        let constraints = *ctx.constraints();
        let count = ctx.child_count();
        if count == 0 {
            self.child_count = 0;
            return Ok(SliverGeometry::ZERO);
        }
        if !constraints.cross_axis_extent.is_finite() {
            return Err(RenderError::invalid_constraints(
                "cross-axis sliver tracks require finite available extent",
            ));
        }
        let allocations: Vec<_> = (0..count)
            .map(|index| {
                ctx.child_parent_data(index)
                    .copied()
                    .unwrap_or_default()
                    .allocation
            })
            .collect();
        let mut extents = vec![0.0; count];
        let mut geometries = vec![SliverGeometry::ZERO; count];
        let mut remaining = constraints.cross_axis_extent;
        let mut total_weight = 0.0;
        for (index, allocation) in allocations.iter().enumerate() {
            match allocation {
                SliverCrossAxisAllocation::Content => {
                    let mut child_constraints = constraints;
                    child_constraints.cross_axis_extent = remaining;
                    let geometry = ctx.layout_child(index, child_constraints)?;
                    if geometry.scroll_offset_correction.is_some() {
                        return Ok(geometry);
                    }
                    let Some(extent) = geometry.cross_axis_extent.filter(|extent| {
                        extent.is_finite() && *extent >= 0.0 && *extent <= remaining
                    }) else {
                        return Err(RenderError::invalid_constraints(
                            "content-sized sliver track must report an extent within its available cross axis",
                        ));
                    };
                    extents[index] = extent;
                    geometries[index] = geometry;
                    remaining -= extent;
                }
                SliverCrossAxisAllocation::Flex(weight) => total_weight += f64::from(weight.get()),
            }
        }
        for (index, allocation) in allocations.iter().enumerate() {
            if let SliverCrossAxisAllocation::Flex(weight) = allocation {
                // Divide before multiplying: finite available extent must not
                // overflow while calculating a proportional share.
                let extent = remaining * (f64::from(weight.get()) / total_weight);
                let mut child_constraints = constraints;
                child_constraints.cross_axis_extent = extent;
                let geometry = ctx.layout_child(index, child_constraints)?;
                if geometry.scroll_offset_correction.is_some() {
                    return Ok(geometry);
                }
                extents[index] = extent;
                geometries[index] = geometry;
            }
        }
        let mut geometry = SliverGeometry::ZERO;
        // The furthest track end, measured from the group's layout offset as
        // each track's own `paint_origin` is.
        let mut paint_end = 0.0_f64;
        for child in &geometries {
            geometry.scroll_extent = geometry.scroll_extent.max(child.scroll_extent);
            geometry.paint_origin = geometry.paint_origin.min(child.paint_origin);
            paint_end = paint_end.max(child.paint_origin + child.paint_extent);
            geometry.layout_extent = geometry.layout_extent.max(child.layout_extent);
            geometry.max_paint_extent = geometry.max_paint_extent.max(child.max_paint_extent);
            geometry.max_scroll_obstruction_extent = geometry
                .max_scroll_obstruction_extent
                .max(child.max_scroll_obstruction_extent);
            geometry.hit_test_extent = geometry.hit_test_extent.max(child.hit_test_extent);
            geometry.cache_extent = geometry.cache_extent.max(child.cache_extent);
            geometry.visible |= child.visible;
            geometry.has_visual_overflow |= child.has_visual_overflow;
        }
        let remaining_scroll = (geometry.scroll_extent - constraints.scroll_offset).max(0.0);
        // A track painting past the group's remaining scroll is pulled back
        // to end there; the group paints from the earliest track origin.
        let paint_end = paint_end.min(remaining_scroll);
        geometry.paint_extent = (paint_end - geometry.paint_origin).max(0.0);
        geometry.layout_extent = geometry.layout_extent.min(geometry.paint_extent);
        geometry.hit_test_extent = geometry.hit_test_extent.min(geometry.paint_extent);
        geometry.visible &= geometry.paint_extent > 0.0;
        let reversed_main = constraints
            .growth_direction
            .apply_to_axis_direction(constraints.axis_direction)
            .is_reversed();
        let mut cross = 0.0;
        let mut positions = Vec::with_capacity(count);
        for (index, child) in geometries.iter().enumerate() {
            let extent = extents[index];
            let cross_position = if constraints.cross_axis_direction.is_reversed() {
                (constraints.cross_axis_extent - cross - extent).max(0.0)
            } else {
                cross
            };
            // The track's start inside the group's painted interval: its own
            // origin relative to the group's, less any pull-back.
            let correction = (child.paint_origin + child.paint_extent - paint_end).max(0.0);
            let start = child.paint_origin - geometry.paint_origin - correction;
            let main_position = if reversed_main {
                geometry.paint_extent - start - child.paint_extent
            } else {
                start
            };
            let cross_end = cross + extent;
            if !cross_end.is_finite()
                || !cross_position.is_finite()
                || !(cross_position + extent).is_finite()
                || !main_position.is_finite()
            {
                return Err(RenderError::invalid_constraints(
                    "sliver track positions exceed representable physical geometry",
                ));
            }
            positions.push(match constraints.axis() {
                Axis::Vertical => Offset::new(cross_position, main_position),
                Axis::Horizontal => Offset::new(main_position, cross_position),
            });
            cross = cross_end;
        }
        for (index, position) in positions.into_iter().enumerate() {
            ctx.position_child(index, position);
        }
        self.child_count = count;
        Ok(geometry)
    }

    fn hit_test_children(
        &self,
        ctx: &mut SliverHitTestContext<'_, Variable, Self::ParentData>,
    ) -> bool {
        (0..self.child_count)
            .rev()
            .any(|index| ctx.hit_test_child_at_layout_offset(index))
    }
}
