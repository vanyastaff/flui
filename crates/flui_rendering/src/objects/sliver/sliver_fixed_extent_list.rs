//! RenderSliverFixedExtentList - Optimized list with fixed item size

use crate::core::{
    FullRenderTree,
    ChildrenAccess, LayoutContext, LayoutTree, PaintContext, PaintTree, SliverProtocol,
    SliverRender, Variable,
};
use flui_types::{SliverConstraints, SliverGeometry};

/// RenderObject for lists where all items have the same fixed extent
///
/// This is an optimized version of RenderSliverList for the common case
/// where all items have the same size. Because the size is known in advance,
/// we can:
/// - Calculate item positions instantly (no measurement needed)
/// - Compute visible range with simple math
/// - Determine which items to build without iteration
///
/// # Performance
///
/// - O(1) layout calculation (vs O(n) for variable sizes)
/// - Instant scrolling to any position
/// - Minimal memory usage (no size cache needed)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverFixedExtentList;
///
/// // All items will be 50px tall
/// let list = RenderSliverFixedExtentList::new(50.0);
/// ```
#[derive(Debug)]
pub struct RenderSliverFixedExtentList {
    /// Fixed extent (height for vertical) for each item
    pub item_extent: f32,

    // Layout cache (set during layout, used during paint)
    cached_is_vertical: bool,
}

impl RenderSliverFixedExtentList {
    /// Create new fixed extent list
    ///
    /// # Arguments
    /// * `item_extent` - Fixed size for each item on the main axis
    pub fn new(item_extent: f32) -> Self {
        Self {
            item_extent,
            cached_is_vertical: true,
        }
    }

    /// Set item extent
    pub fn set_item_extent(&mut self, extent: f32) {
        self.item_extent = extent;
    }

    /// Calculate which items are visible
    ///
    /// Returns (first_visible_index, last_visible_index) inclusive
    pub fn visible_range(
        &self,
        constraints: &SliverConstraints,
        child_count: usize,
    ) -> (usize, usize) {
        if child_count == 0 || self.item_extent <= 0.0 {
            return (0, 0);
        }

        let scroll_offset = constraints.scroll_offset.max(0.0);
        let remaining_extent = constraints.remaining_paint_extent;

        // Calculate first visible item
        let first_index = (scroll_offset / self.item_extent).floor() as usize;
        let first_index = first_index.min(child_count.saturating_sub(1));

        // Calculate last visible item
        let last_offset = scroll_offset + remaining_extent;
        let last_index = (last_offset / self.item_extent).ceil() as usize;
        let last_index = last_index.min(child_count);

        (first_index, last_index)
    }

    /// Calculate sliver geometry
    fn calculate_sliver_geometry(
        &self,
        constraints: &SliverConstraints,
        child_count: usize,
    ) -> SliverGeometry {
        if child_count == 0 || self.item_extent <= 0.0 {
            return SliverGeometry::default();
        }

        let scroll_offset = constraints.scroll_offset;
        let remaining_extent = constraints.remaining_paint_extent;

        let total_extent = self.item_extent * child_count as f32;

        // Calculate visible portion
        let leading_scroll_offset = scroll_offset.max(0.0);
        let trailing_scroll_offset = (scroll_offset + remaining_extent).min(total_extent);

        let paint_extent = (trailing_scroll_offset - leading_scroll_offset).max(0.0);

        SliverGeometry {
            scroll_extent: total_extent,
            paint_extent,
            paint_origin: 0.0,
            layout_extent: paint_extent,
            max_paint_extent: total_extent,
            max_scroll_obstruction_extent: 0.0,
            visible_fraction: if total_extent > 0.0 {
                (paint_extent / total_extent).min(1.0)
            } else {
                0.0
            },
            cross_axis_extent: constraints.cross_axis_extent,
            cache_extent: paint_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: total_extent > paint_extent,
            hit_test_extent: Some(paint_extent),
            scroll_offset_correction: None,
        }
    }
}

impl SliverRender<Variable> for RenderSliverFixedExtentList {
    fn layout<T>(&mut self, ctx: LayoutContext<'_, T, Variable, SliverProtocol>) -> SliverGeometry
    where
        T: LayoutTree,
    {
        use flui_types::layout::AxisDirection;

        let constraints = ctx.constraints;
        let child_count = ctx.children.len();

        // Cache axis direction for paint
        self.cached_is_vertical = matches!(
            constraints.axis_direction,
            AxisDirection::TopToBottom | AxisDirection::BottomToTop
        );

        // Calculate sliver geometry using child count
        // In a full implementation, we would:
        // 1. Determine visible range using visible_range()
        // 2. Layout only visible children
        // 3. Use item_extent for positioning
        self.calculate_sliver_geometry(&constraints, child_count)
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Variable>)
    where
        T: PaintTree,
    {
        use flui_types::geometry::Offset;

        // Use cached axis direction from layout
        let is_vertical = self.cached_is_vertical;

        // Paint visible children at their calculated positions
        // Each child is positioned at item_extent * index from the start
        let children = ctx.children.iter().collect::<Vec<_>>();

        for (index, &child_id) in children.iter().enumerate() {
            // Calculate child offset based on index, item extent, and axis direction
            let child_offset = if is_vertical {
                // Vertical scrolling: y = index * item_extent
                Offset::new(0.0, index as f32 * self.item_extent)
            } else {
                // Horizontal scrolling: x = index * item_extent
                Offset::new(index as f32 * self.item_extent, 0.0)
            };

            // Paint child at calculated position
            ctx.paint_child(child_id, ctx.offset + child_offset);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::constraints::{GrowthDirection, ScrollDirection};
    use flui_types::layout::AxisDirection;

    #[test]
    fn test_render_sliver_fixed_extent_list_new() {
        let list = RenderSliverFixedExtentList::new(50.0);

        assert_eq!(list.item_extent, 50.0);
    }

    #[test]
    fn test_set_item_extent() {
        let mut list = RenderSliverFixedExtentList::new(50.0);
        list.set_item_extent(100.0);

        assert_eq!(list.item_extent, 100.0);
    }

    #[test]
    fn test_visible_range_empty() {
        let list = RenderSliverFixedExtentList::new(50.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset: 0.0,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
            ..SliverConstraints::default()
        };

        let (first, last) = list.visible_range(&constraints, 0);
        assert_eq!(first, 0);
        assert_eq!(last, 0);
    }

    #[test]
    fn test_visible_range_all_visible() {
        let list = RenderSliverFixedExtentList::new(50.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset: 0.0,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
            ..SliverConstraints::default()
        };

        // 10 items * 50px = 500px (all fit in 600px viewport)
        let (first, last) = list.visible_range(&constraints, 10);
        assert_eq!(first, 0);
        assert_eq!(last, 10); // Last index is exclusive
    }

    #[test]
    fn test_visible_range_partial() {
        let list = RenderSliverFixedExtentList::new(50.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset: 0.0,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
            ..SliverConstraints::default()
        };

        // 20 items * 50px = 1000px (only first 600px visible = 12 items)
        let (first, last) = list.visible_range(&constraints, 20);
        assert_eq!(first, 0);
        assert_eq!(last, 12); // ceil(600/50) = 12
    }

    #[test]
    fn test_visible_range_scrolled() {
        let list = RenderSliverFixedExtentList::new(50.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            scroll_offset: 100.0, // Scrolled past 2 items
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
            ..SliverConstraints::default()
        };

        let (first, last) = list.visible_range(&constraints, 20);
        assert_eq!(first, 2); // floor(100/50) = 2
        assert_eq!(last, 14); // ceil((100+600)/50) = 14
    }

    #[test]
    fn test_visible_range_scrolled_to_end() {
        let list = RenderSliverFixedExtentList::new(50.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            scroll_offset: 500.0, // Scrolled to last items
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
            ..SliverConstraints::default()
        };

        // 15 items * 50px = 750px total
        let (first, last) = list.visible_range(&constraints, 15);
        assert_eq!(first, 10); // floor(500/50) = 10
        assert_eq!(last, 15); // Capped at child count
    }

    #[test]
    fn test_calculate_sliver_geometry_empty() {
        let list = RenderSliverFixedExtentList::new(50.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset: 0.0,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
            ..SliverConstraints::default()
        };

        let geometry = list.calculate_sliver_geometry(&constraints, 0);

        assert_eq!(geometry.scroll_extent, 0.0);
        assert_eq!(geometry.paint_extent, 0.0);
        assert!(!geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_all_visible() {
        let list = RenderSliverFixedExtentList::new(50.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset: 0.0,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
            ..SliverConstraints::default()
        };

        let geometry = list.calculate_sliver_geometry(&constraints, 5);

        // 5 items * 50px = 250px
        assert_eq!(geometry.scroll_extent, 250.0);
        assert_eq!(geometry.paint_extent, 250.0);
        assert!(geometry.visible);
        assert_eq!(geometry.visible_fraction, 1.0);
    }

    #[test]
    fn test_calculate_sliver_geometry_partially_visible() {
        let list = RenderSliverFixedExtentList::new(50.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            scroll_offset: 0.0,
            remaining_paint_extent: 300.0, // Only 300px visible
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
            ..SliverConstraints::default()
        };

        let geometry = list.calculate_sliver_geometry(&constraints, 20);

        // 20 items * 50px = 1000px total
        assert_eq!(geometry.scroll_extent, 1000.0);
        // Only 300px visible
        assert_eq!(geometry.paint_extent, 300.0);
        assert!(geometry.visible);
        assert_eq!(geometry.visible_fraction, 0.3);
    }

    #[test]
    fn test_calculate_sliver_geometry_scrolled() {
        let list = RenderSliverFixedExtentList::new(50.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            scroll_offset: 200.0, // Scrolled 200px
            remaining_paint_extent: 300.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
            ..SliverConstraints::default()
        };

        let geometry = list.calculate_sliver_geometry(&constraints, 20);

        // 20 items * 50px = 1000px total
        assert_eq!(geometry.scroll_extent, 1000.0);
        // From 200 to 500 = 300px visible
        assert_eq!(geometry.paint_extent, 300.0);
        assert!(geometry.visible);
    }
}
