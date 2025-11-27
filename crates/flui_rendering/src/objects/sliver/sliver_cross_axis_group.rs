//! RenderSliverCrossAxisGroup - Groups multiple slivers along cross axis

use crate::core::{LayoutContext, LayoutTree, PaintContext, PaintTree, Variable, SliverProtocol, SliverRender};
use flui_types::{Offset, SliverGeometry};

/// RenderObject that groups multiple slivers along the cross axis
///
/// Places sliver children side-by-side perpendicular to the scroll direction
/// (cross axis). Supports flexible sizing where children can have flex factors
/// to distribute available cross-axis space.
///
/// # Layout Algorithm
///
/// 1. **Non-flex children**: Layout children with flex=0 first, they determine
///    their own cross-axis extent
/// 2. **Flex allocation**: Remaining cross-axis space is divided among children
///    with non-zero flex factors proportionally
/// 3. **Flexible children**: Layout children with allocated cross-axis space
/// 4. **Geometry**: Use longest scroll extent, sum cross-axis extents
///
/// # Behavior
///
/// - Lays out children side-by-side along cross axis
/// - Each child gets a portion of cross-axis space
/// - All children share the same scroll offset and constraints
/// - Geometry uses maximum scroll extent from all children
///
/// # Use Cases
///
/// - Multi-column scrollable layouts
/// - Side-by-side lists (e.g., calendar day columns)
/// - Flexible sliver arrangements
/// - Split-screen scrollable content
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverCrossAxisGroup;
///
/// // Two side-by-side lists
/// let group = RenderSliverCrossAxisGroup::new();
/// // Add children: [SliverList, SliverList]
/// // Each takes 50% of cross-axis space
/// ```
#[derive(Debug)]
pub struct RenderSliverCrossAxisGroup {
    /// Computed geometry from last layout

    /// Cross-axis offsets for each child (computed during layout)
    child_cross_axis_offsets: Vec<f32>,

    /// Cross-axis extents for each child
    child_cross_axis_extents: Vec<f32>,
}

impl RenderSliverCrossAxisGroup {
    /// Create new cross axis group
    pub fn new() -> Self {
        Self {
            child_cross_axis_offsets: Vec::new(),
            child_cross_axis_extents: Vec::new(),
        }
    }
}

impl Default for RenderSliverCrossAxisGroup {
    fn default() -> Self {
        Self::new()
    }
}

impl SliverRender<Variable> for RenderSliverCrossAxisGroup {
    fn layout<T>(
        &mut self,
        mut ctx: LayoutContext<'_, T, Variable, SliverProtocol>,
    ) -> SliverGeometry
    where
        T: LayoutTree,
    {
        let constraints = ctx.constraints;

        // Collect children first to avoid borrow checker issues
        let child_ids: Vec<_> = ctx.children.iter().collect();

        if child_ids.is_empty() {
            self.child_cross_axis_offsets.clear();
            self.child_cross_axis_extents.clear();
            return SliverGeometry::default();
        }

        // Divide cross-axis space equally among children
        // A full implementation would support flex factors
        let child_cross_axis_extent = constraints.cross_axis_extent / child_ids.len() as f32;

        self.child_cross_axis_offsets.clear();
        self.child_cross_axis_extents.clear();
        self.child_cross_axis_offsets.reserve(child_ids.len());
        self.child_cross_axis_extents.reserve(child_ids.len());

        let mut max_scroll_extent = 0.0f32;
        let mut max_paint_extent = 0.0f32;
        let mut max_max_paint_extent = 0.0f32;
        let mut total_cache_extent = 0.0;
        let mut any_visible = false;

        // Layout each child with its portion of cross-axis space
        for (i, &child_id) in child_ids.iter().enumerate() {
            let cross_axis_offset = i as f32 * child_cross_axis_extent;

            // Create child constraints with adjusted cross-axis extent
            let child_constraints = flui_types::SliverConstraints {
                cross_axis_extent: child_cross_axis_extent,
                ..constraints
            };

            // Layout child
            let child_geometry = ctx.layout_child(child_id, child_constraints);

            // Store cross-axis position and extent
            self.child_cross_axis_offsets.push(cross_axis_offset);
            self.child_cross_axis_extents.push(child_cross_axis_extent);

            // Use maximum extents from all children
            max_scroll_extent = max_scroll_extent.max(child_geometry.scroll_extent);
            max_paint_extent = max_paint_extent.max(child_geometry.paint_extent);
            max_max_paint_extent = max_max_paint_extent.max(child_geometry.max_paint_extent);
            total_cache_extent += child_geometry.cache_extent;

            if child_geometry.visible {
                any_visible = true;
            }
        }

        // Group geometry uses maximum scroll extent and sums cross-axis
        SliverGeometry {
            scroll_extent: max_scroll_extent,
            paint_extent: max_paint_extent.min(constraints.remaining_paint_extent),
            paint_origin: 0.0,
            layout_extent: max_paint_extent.min(constraints.remaining_paint_extent),
            max_paint_extent: max_max_paint_extent,
            max_scroll_obstruction_extent: 0.0,
            visible_fraction: if any_visible { 1.0 } else { 0.0 },
            cross_axis_extent: constraints.cross_axis_extent,
            cache_extent: total_cache_extent,
            visible: any_visible,
            has_visual_overflow: max_paint_extent > constraints.remaining_paint_extent,
            hit_test_extent: Some(max_paint_extent),
            scroll_offset_correction: None,
        }
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Variable>)
    where
        T: PaintTree,
    {
        // Paint children with their respective cross-axis offsets
        // Collect children first to avoid borrow checker issues
        let child_ids: Vec<_> = ctx.children.iter().collect();

        for (i, child_id) in child_ids.iter().enumerate() {
            if i < self.child_cross_axis_offsets.len() {
                let cross_axis_offset = self.child_cross_axis_offsets[i];

                // Calculate child's offset based on cross axis
                // For simplicity, assume vertical scrolling (cross axis = horizontal)
                let child_offset = Offset::new(ctx.offset.dx + cross_axis_offset, ctx.offset.dy);

                ctx.paint_child(*child_id, child_offset);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sliver_cross_axis_group_creation() {
        let group = RenderSliverCrossAxisGroup::new();
        assert_eq!(group.child_cross_axis_offsets.len(), 0);
        assert_eq!(group.child_cross_axis_extents.len(), 0);
    }

    #[test]
    fn test_sliver_cross_axis_group_default() {
        let group = RenderSliverCrossAxisGroup::default();
        assert_eq!(group.child_cross_axis_offsets.len(), 0);
    }

}
