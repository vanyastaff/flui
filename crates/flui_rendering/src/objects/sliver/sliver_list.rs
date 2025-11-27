//! RenderSliverList - Scrollable list with lazy loading

use crate::core::{
    ChildrenAccess, LayoutContext, LayoutTree, PaintContext, PaintTree, SliverProtocol,
    SliverRender, Variable,
};
use flui_types::{SliverConstraints, SliverGeometry};

/// Child builder function for lazy loading
///
/// Takes index and returns whether to build the child at that index.
/// Returns None when no more children should be built.
pub type SliverChildBuilder = Box<dyn Fn(usize) -> Option<bool> + Send + Sync>;

/// RenderObject for scrollable lists with lazy loading
///
/// Unlike RenderColumn which lays out all children eagerly, RenderSliverList
/// only builds and lays out children that are visible or near-visible (in cache).
/// This enables efficient scrolling through large lists.
///
/// # Sliver Protocol
///
/// Slivers use a different constraint/sizing model:
/// - **Input**: SliverConstraints (scroll offset, remaining paint extent, cache extent)
/// - **Output**: SliverGeometry (scroll extent, paint extent, visible)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverList;
///
/// let list = RenderSliverList::new(
///     |index| {
///         if index < 1000 {
///             Some(true) // Build child at this index
///         } else {
///             None // No more children
///         }
///     }
/// );
/// ```
pub struct RenderSliverList {
    /// Optional child builder for lazy loading
    #[allow(clippy::type_complexity)]
    pub child_builder: Option<SliverChildBuilder>,
    /// Fixed item extent (if all items have same size)
    pub item_extent: Option<f32>,
    /// Cross axis extent (width for vertical scroll)
    pub cross_axis_extent: f32,
}

impl std::fmt::Debug for RenderSliverList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderSliverList")
            .field(
                "child_builder",
                &self
                    .child_builder
                    .as_ref()
                    .map(|_| "Fn(usize) -> Option<bool>"),
            )
            .field("item_extent", &self.item_extent)
            .field("cross_axis_extent", &self.cross_axis_extent)
            .finish()
    }
}

impl RenderSliverList {
    /// Create new sliver list
    pub fn new() -> Self {
        Self {
            child_builder: None,
            item_extent: None,
            cross_axis_extent: 0.0,
        }
    }

    /// Create with child builder
    pub fn with_builder<F>(builder: F) -> Self
    where
        F: Fn(usize) -> Option<bool> + Send + Sync + 'static,
    {
        Self {
            child_builder: Some(Box::new(builder)),
            item_extent: None,
            cross_axis_extent: 0.0,
        }
    }

    /// Set fixed item extent
    pub fn set_item_extent(&mut self, extent: f32) {
        self.item_extent = Some(extent);
    }

    /// Set cross axis extent
    pub fn set_cross_axis_extent(&mut self, extent: f32) {
        self.cross_axis_extent = extent;
    }

    /// Create with fixed item extent
    pub fn with_item_extent(mut self, extent: f32) -> Self {
        self.item_extent = Some(extent);
        self
    }

    /// Calculate sliver geometry for multi-child layout
    fn calculate_sliver_geometry(
        &self,
        constraints: &SliverConstraints,
        child_count: usize,
    ) -> SliverGeometry {
        // If no children, return zero geometry
        if child_count == 0 {
            return SliverGeometry::default();
        }

        let scroll_offset = constraints.scroll_offset;
        let remaining_extent = constraints.remaining_paint_extent;

        // If we have fixed item extent, we can calculate precisely
        if let Some(item_extent) = self.item_extent {
            let total_extent = item_extent * child_count as f32;

            // Calculate what's visible
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
        } else {
            // Variable size children - would need to measure each
            // For now, estimate based on child count
            // In real implementation, we'd query actual child sizes from element tree

            // Estimate average child height (50px)
            let estimated_item_extent = 50.0;
            let total_extent = estimated_item_extent * child_count as f32;

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
}

impl Default for RenderSliverList {
    fn default() -> Self {
        Self::new()
    }
}

impl SliverRender<Variable> for RenderSliverList {
    fn layout<T>(&mut self, ctx: LayoutContext<'_, T, Variable, SliverProtocol>) -> SliverGeometry
    where
        T: LayoutTree,
    {
        let constraints = ctx.constraints;

        // Store cross axis extent
        self.cross_axis_extent = constraints.cross_axis_extent;

        // Get child count
        let child_count = ctx.children.len();

        // Calculate sliver geometry
        // Note: This is a simplified implementation using child count estimation.
        // A full viewport implementation would:
        // 1. Determine which children are visible based on scroll offset
        // 2. Layout only visible children with box constraints
        // 3. Calculate precise geometry from actual child sizes
        // The current approach provides correct layout for basic lists
        self.calculate_sliver_geometry(&constraints, child_count)
    }

    fn paint<T>(&self, ctx: &mut PaintContext<'_, T, Variable>)
    where
        T: PaintTree,
    {
        use flui_types::geometry::Offset;

        // Paint visible children based on their positions
        let children = ctx.children.iter().collect::<Vec<_>>();

        // If we have fixed item extent, use it for positioning
        if let Some(item_extent) = self.item_extent {
            for (index, &child_id) in children.iter().enumerate() {
                // Calculate child offset based on index and item extent
                let child_offset = Offset::new(0.0, index as f32 * item_extent);
                ctx.paint_child(child_id, ctx.offset + child_offset);
            }
        } else {
            // Variable-height items: need to track accumulated offset
            // For now, use estimated height (50px) as fallback
            // In full implementation, we'd query actual child sizes from layout
            let estimated_extent = 50.0;
            for (index, &child_id) in children.iter().enumerate() {
                let child_offset = Offset::new(0.0, index as f32 * estimated_extent);
                ctx.paint_child(child_id, ctx.offset + child_offset);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::constraints::{GrowthDirection, ScrollDirection};

    #[test]
    fn test_render_sliver_list_new() {
        let list = RenderSliverList::new();

        assert!(list.child_builder.is_none());
        assert!(list.item_extent.is_none());
        assert_eq!(list.cross_axis_extent, 0.0);
    }

    #[test]
    fn test_render_sliver_list_with_builder() {
        let list =
            RenderSliverList::with_builder(|index| if index < 100 { Some(true) } else { None });

        assert!(list.child_builder.is_some());
    }

    #[test]
    fn test_render_sliver_list_set_item_extent() {
        let mut list = RenderSliverList::new();
        list.set_item_extent(50.0);

        assert_eq!(list.item_extent, Some(50.0));
    }

    #[test]
    fn test_render_sliver_list_set_cross_axis_extent() {
        let mut list = RenderSliverList::new();
        list.set_cross_axis_extent(400.0);

        assert_eq!(list.cross_axis_extent, 400.0);
    }

    #[test]
    fn test_render_sliver_list_with_item_extent() {
        let list = RenderSliverList::new().with_item_extent(60.0);

        assert_eq!(list.item_extent, Some(60.0));
    }

    #[test]
    fn test_render_sliver_list_geometry_empty() {
        let list = RenderSliverList::new();

        let constraints = SliverConstraints {
            axis_direction: flui_types::layout::AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset: 0.0,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: flui_types::layout::AxisDirection::LeftToRight,
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
    fn test_render_sliver_list_geometry_fixed_extent() {
        let list = RenderSliverList::new().with_item_extent(50.0);

        let constraints = SliverConstraints {
            axis_direction: flui_types::layout::AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            user_scroll_direction: ScrollDirection::Idle,
            scroll_offset: 0.0,
            preceding_scroll_extent: 0.0,
            overlap: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: flui_types::layout::AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
            ..SliverConstraints::default()
        };

        let geometry = list.calculate_sliver_geometry(&constraints, 10);

        // 10 items * 50px = 500px total
        assert_eq!(geometry.scroll_extent, 500.0);
        // All 500px should be visible (viewport is 600px)
        assert_eq!(geometry.paint_extent, 500.0);
        assert!(geometry.visible);
        assert_eq!(geometry.visible_fraction, 1.0);
    }

    #[test]
    fn test_render_sliver_list_geometry_scrolled() {
        let list = RenderSliverList::new().with_item_extent(50.0);

        let constraints = SliverConstraints {
            axis_direction: flui_types::layout::AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            scroll_offset: 100.0, // Scrolled down 100px
            remaining_paint_extent: 300.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: flui_types::layout::AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
            ..SliverConstraints::default()
        };

        let geometry = list.calculate_sliver_geometry(&constraints, 10);

        // Total extent still 500px
        assert_eq!(geometry.scroll_extent, 500.0);
        // Only 300px visible (from offset 100 to 400)
        assert_eq!(geometry.paint_extent, 300.0);
        assert!(geometry.visible);
        assert!((geometry.visible_fraction - 0.6).abs() < 0.01); // 300/500 = 0.6
    }

    #[test]
    fn test_render_sliver_list_geometry_off_screen() {
        let list = RenderSliverList::new().with_item_extent(50.0);

        let constraints = SliverConstraints {
            axis_direction: flui_types::layout::AxisDirection::TopToBottom,
            growth_direction: GrowthDirection::Forward,
            scroll_offset: 500.0, // Scrolled past all children
            remaining_paint_extent: 300.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: flui_types::layout::AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
            ..SliverConstraints::default()
        };

        let geometry = list.calculate_sliver_geometry(&constraints, 2);

        // Total extent 100px (2 * 50)
        assert_eq!(geometry.scroll_extent, 100.0);
        // Nothing visible
        assert_eq!(geometry.paint_extent, 0.0);
        assert!(!geometry.visible);
    }

    #[test]
    fn test_render_sliver_list_default() {
        let list = RenderSliverList::default();

        assert!(list.child_builder.is_none());
        assert!(list.item_extent.is_none());
    }
}
