//! RenderSliverMainAxisGroup - Groups multiple slivers along main axis

use crate::core::{RuntimeArity, LegacySliverRender, SliverSliver};
use flui_painting::Canvas;
use flui_types::{Offset, SliverGeometry};

/// RenderObject that groups multiple slivers along the main axis
///
/// Places sliver children in a linear array, one after another along the
/// main axis (scroll direction). Acts as a container that treats multiple
/// slivers as a single unit.
///
/// # Behavior
///
/// - Lays out children sequentially along main axis
/// - Tracks cumulative scroll extent across all children
/// - Adjusts paint offsets for each child
/// - When group scrolls out of view, all children (including pinned
///   elements like SliverAppBar) scroll away together
///
/// # Layout Algorithm
///
/// 1. **Offset Tracking**: Maintains cumulative scrollExtent of laid-out children
/// 2. **Child Constraints**: Calculates remainingPaintExtent and scrollOffset for each child
/// 3. **Paint Offset Adjustment**: Ensures children don't paint beyond group bounds
/// 4. **Geometry Assignment**: Accumulates total scrollExtent, paintExtent, maxPaintExtent
///
/// # Use Cases
///
/// - Grouping related slivers (header + content + footer)
/// - Creating collapsible sliver sections
/// - Managing pinned headers within a group
/// - Coordinating multiple slivers as a unit
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverMainAxisGroup;
///
/// // Groups app bar, list, and footer as single scrollable unit
/// let group = RenderSliverMainAxisGroup::new();
/// // Add children: [SliverAppBar, SliverList, SliverToBoxAdapter]
/// ```
#[derive(Debug)]
pub struct RenderSliverMainAxisGroup {
    /// Computed geometry from last layout
    sliver_geometry: SliverGeometry,

    /// Paint offsets for each child (computed during layout)
    child_paint_offsets: Vec<f32>,
}

impl RenderSliverMainAxisGroup {
    /// Create new main axis group
    pub fn new() -> Self {
        Self {
            sliver_geometry: SliverGeometry::default(),
            child_paint_offsets: Vec::new(),
        }
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

    /// Calculate sliver geometry by accumulating children
    ///
    /// Lays out children sequentially and accumulates their geometry
    /// to compute the group's total geometry.
    fn calculate_sliver_geometry(
        &mut self,
        ctx: &Sliver,
    ) -> SliverGeometry {
        let children = ctx.children.as_slice();

        if children.is_empty() {
            self.child_paint_offsets.clear();
            return SliverGeometry::default();
        }

        // Step 1: Track offset as cumulative scrollExtent
        let mut offset = 0.0;
        let mut total_scroll_extent = 0.0;
        let mut total_paint_extent = 0.0;
        let mut max_paint_extent = 0.0;
        let mut total_cache_extent = 0.0;
        let mut any_visible = false;

        self.child_paint_offsets.clear();
        self.child_paint_offsets.reserve(children.len());

        for &child_id in children {
            // Step 2: Calculate child constraints
            let remaining_paint_extent =
                (ctx.constraints.remaining_paint_extent - total_paint_extent).max(0.0);
            let scroll_offset = (ctx.constraints.scroll_offset - offset).max(0.0);

            // Create new constraints for child
            let child_constraints = flui_types::SliverConstraints {
                scroll_offset,
                remaining_paint_extent,
                ..ctx.constraints
            };

            // Layout child
            let child_geometry = ctx.tree.layout_sliver_child(child_id, child_constraints);

            // Store paint offset for this child
            self.child_paint_offsets.push(total_paint_extent);

            // Accumulate geometry
            total_scroll_extent += child_geometry.scroll_extent;
            total_paint_extent += child_geometry.paint_extent;
            max_paint_extent += child_geometry.max_paint_extent;
            total_cache_extent += child_geometry.cache_extent;

            if child_geometry.visible {
                any_visible = true;
            }

            // Update offset for next child
            offset += child_geometry.scroll_extent;

            // If no more paint extent, stop (optimization)
            if remaining_paint_extent <= 0.0 {
                break;
            }
        }

        // Step 4: Assign geometry
        SliverGeometry {
            scroll_extent: total_scroll_extent,
            paint_extent: total_paint_extent.min(ctx.constraints.remaining_paint_extent),
            paint_origin: 0.0,
            layout_extent: total_paint_extent.min(ctx.constraints.remaining_paint_extent),
            max_paint_extent,
            max_scroll_obsolescence: 0.0,
            visible_fraction: if any_visible { 1.0 } else { 0.0 },
            cross_axis_extent: ctx.constraints.cross_axis_extent,
            cache_extent: total_cache_extent,
            visible: any_visible,
            has_visual_overflow: total_paint_extent > ctx.constraints.remaining_paint_extent,
            hit_test_extent: Some(total_paint_extent),
            scroll_offset_correction: None,
        }
    }
}

impl Default for RenderSliverMainAxisGroup {
    fn default() -> Self {
        Self::new()
    }
}

impl LegacySliverRender for RenderSliverMainAxisGroup {
    fn layout(&mut self, ctx: &Sliver) -> SliverGeometry {
        self.sliver_geometry = self.calculate_sliver_geometry(ctx);
        self.sliver_geometry
    }

    fn paint(&self, ctx: &Sliver) -> Canvas {
        let mut canvas = Canvas::new();
        let children = ctx.children.as_slice();

        // Paint children with their respective offsets
        for (i, &child_id) in children.iter().enumerate() {
            if i < self.child_paint_offsets.len() {
                let paint_offset = self.child_paint_offsets[i];

                // Calculate child's offset along main axis
                // For simplicity, assume vertical scrolling (TopToBottom)
                // In a full implementation, would check axis direction
                let child_offset = Offset::new(ctx.offset.dx, ctx.offset.dy + paint_offset);

                let child_canvas = ctx.tree.paint_child(child_id, child_offset);
                canvas.append_canvas(child_canvas);
            }
        }

        canvas
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> RuntimeArity {
        RuntimeArity::Variable // Multiple sliver children
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sliver_main_axis_group_creation() {
        let group = RenderSliverMainAxisGroup::new();
        assert_eq!(group.child_paint_offsets.len(), 0);
    }

    #[test]
    fn test_sliver_main_axis_group_default() {
        let group = RenderSliverMainAxisGroup::default();
        assert_eq!(group.child_paint_offsets.len(), 0);
    }

    #[test]
    fn test_geometry_getter() {
        let group = RenderSliverMainAxisGroup::new();
        let geometry = group.geometry();
        assert_eq!(geometry.scroll_extent, 0.0);
    }

    #[test]
    fn test_arity_multiple_children() {
        let group = RenderSliverMainAxisGroup::new();
        assert_eq!(group.arity(), RuntimeArity::Variable);
    }
}
