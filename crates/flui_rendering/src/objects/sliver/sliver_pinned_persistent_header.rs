//! RenderSliverPinnedPersistentHeader - Pinned header that stays visible

use flui_core::render::{Arity, RenderSliver, SliverLayoutContext, SliverPaintContext};
use flui_painting::Canvas;
use flui_types::{SliverConstraints, SliverGeometry};

/// RenderObject for a pinned persistent header
///
/// A pinned header stays visible at the top/leading edge of the viewport
/// once it has been scrolled into view. Unlike floating headers, it doesn't
/// scroll away even when scrolling forward past it.
///
/// # Differences from other headers
///
/// - **Pinned**: Stays visible once reached (this one)
/// - **Floating**: Can scroll off but reappears immediately on scroll up
/// - **Normal**: Scrolls away naturally
///
/// # Use Cases
///
/// - Section headers that should always be visible
/// - Sticky table headers
/// - Category separators in long lists
/// - Navigation headers that pin to top
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverPinnedPersistentHeader;
///
/// // Pinned header that sticks once scrolled into view
/// let header = RenderSliverPinnedPersistentHeader::new(60.0);
/// ```
#[derive(Debug)]
pub struct RenderSliverPinnedPersistentHeader {
    /// Minimum extent (height when pinned)
    pub min_extent: f32,
    /// Maximum extent (height when fully expanded)
    pub max_extent: f32,

    // Layout cache
    sliver_geometry: SliverGeometry,
}

impl RenderSliverPinnedPersistentHeader {
    /// Create new pinned persistent header
    ///
    /// # Arguments
    /// * `extent` - Height of the header (both min and max)
    pub fn new(extent: f32) -> Self {
        Self {
            min_extent: extent,
            max_extent: extent,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Create with separate min and max extents
    ///
    /// This allows the header to collapse/expand as it scrolls.
    ///
    /// # Arguments
    /// * `min_extent` - Minimum height when pinned
    /// * `max_extent` - Maximum height when fully expanded
    pub fn with_extents(min_extent: f32, max_extent: f32) -> Self {
        Self {
            min_extent,
            max_extent,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Set minimum extent
    pub fn set_min_extent(&mut self, extent: f32) {
        self.min_extent = extent;
    }

    /// Set maximum extent
    pub fn set_max_extent(&mut self, extent: f32) {
        self.max_extent = extent;
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

    /// Calculate sliver geometry for pinned behavior
    ///
    /// Pinned headers:
    /// - Scroll normally until they reach the top
    /// - Then stick to the top (at min_extent) as content scrolls underneath
    /// - Never scroll off-screen once pinned
    fn calculate_sliver_geometry(&self, constraints: &SliverConstraints) -> SliverGeometry {
        let scroll_offset = constraints.scroll_offset;
        let remaining_extent = constraints.remaining_paint_extent;

        // Calculate how much of the header is scrolled past
        let scrolled_extent = scroll_offset.min(self.max_extent - self.min_extent);

        // Current extent shrinks as we scroll (from max to min)
        let current_extent = self.max_extent - scrolled_extent;

        // Paint extent is what's actually visible
        let paint_extent = current_extent.min(remaining_extent);

        // Layout extent for pinned header is always min_extent
        // This means it always takes up min_extent space at the top
        let layout_extent = self.min_extent.min(remaining_extent);

        SliverGeometry {
            // Scroll extent is the collapsible part (max - min)
            scroll_extent: self.max_extent - self.min_extent,
            paint_extent,
            paint_origin: 0.0,
            layout_extent,
            max_paint_extent: self.max_extent,
            max_scroll_obsolescence: 0.0,
            visible_fraction: if self.max_extent > 0.0 {
                (paint_extent / self.max_extent).min(1.0)
            } else {
                0.0
            },
            cross_axis_extent: constraints.cross_axis_extent,
            cache_extent: paint_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: self.max_extent > paint_extent,
            hit_test_extent: Some(paint_extent),
            scroll_offset_correction: None,
        }
    }
}

impl Default for RenderSliverPinnedPersistentHeader {
    fn default() -> Self {
        Self::new(56.0) // Material Design standard app bar height
    }
}

impl RenderSliver for RenderSliverPinnedPersistentHeader {
    fn layout(&mut self, ctx: &SliverLayoutContext) -> SliverGeometry {
        // Calculate and cache sliver geometry
        self.sliver_geometry = self.calculate_sliver_geometry(&ctx.constraints);
        self.sliver_geometry
    }

    fn paint(&self, ctx: &SliverPaintContext) -> Canvas {
        // Paint child if present and visible
        if let Some(child_id) = ctx.children.try_single() {
            if self.sliver_geometry.visible {
                return ctx.tree.paint_child(child_id, ctx.offset);
            }
        }

        Canvas::new()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Exact(1) // Single child (header content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::layout::AxisDirection;

    #[test]
    fn test_render_sliver_pinned_persistent_header_new() {
        let header = RenderSliverPinnedPersistentHeader::new(60.0);

        assert_eq!(header.min_extent, 60.0);
        assert_eq!(header.max_extent, 60.0);
    }

    #[test]
    fn test_render_sliver_pinned_persistent_header_with_extents() {
        let header = RenderSliverPinnedPersistentHeader::with_extents(40.0, 120.0);

        assert_eq!(header.min_extent, 40.0);
        assert_eq!(header.max_extent, 120.0);
    }

    #[test]
    fn test_render_sliver_pinned_persistent_header_default() {
        let header = RenderSliverPinnedPersistentHeader::default();

        assert_eq!(header.min_extent, 56.0);
        assert_eq!(header.max_extent, 56.0);
    }

    #[test]
    fn test_set_min_extent() {
        let mut header = RenderSliverPinnedPersistentHeader::new(60.0);
        header.set_min_extent(30.0);

        assert_eq!(header.min_extent, 30.0);
    }

    #[test]
    fn test_set_max_extent() {
        let mut header = RenderSliverPinnedPersistentHeader::new(60.0);
        header.set_max_extent(100.0);

        assert_eq!(header.max_extent, 100.0);
    }

    #[test]
    fn test_calculate_sliver_geometry_not_scrolled() {
        let header = RenderSliverPinnedPersistentHeader::with_extents(40.0, 120.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = header.calculate_sliver_geometry(&constraints);

        // Header should be at max extent
        assert_eq!(geometry.scroll_extent, 80.0); // max - min = 120 - 40
        assert_eq!(geometry.paint_extent, 120.0); // Full max extent
        assert_eq!(geometry.layout_extent, 40.0); // Always min_extent for pinned
        assert!(geometry.visible);
        assert_eq!(geometry.visible_fraction, 1.0);
    }

    #[test]
    fn test_calculate_sliver_geometry_partially_scrolled() {
        let header = RenderSliverPinnedPersistentHeader::with_extents(40.0, 120.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 40.0, // Scrolled 40px
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = header.calculate_sliver_geometry(&constraints);

        // Header should be collapsing (120 - 40 = 80px)
        assert_eq!(geometry.scroll_extent, 80.0);
        assert_eq!(geometry.paint_extent, 80.0); // Partially collapsed
        assert_eq!(geometry.layout_extent, 40.0); // Still min_extent
        assert!(geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_fully_collapsed() {
        let header = RenderSliverPinnedPersistentHeader::with_extents(40.0, 120.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 100.0, // Scrolled past collapsible part
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = header.calculate_sliver_geometry(&constraints);

        // Header should be pinned at min extent
        assert_eq!(geometry.scroll_extent, 80.0);
        assert_eq!(geometry.paint_extent, 40.0); // Collapsed to min
        assert_eq!(geometry.layout_extent, 40.0); // At min_extent
        assert!(geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_fixed_extent() {
        let header = RenderSliverPinnedPersistentHeader::new(60.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 100.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = header.calculate_sliver_geometry(&constraints);

        // Fixed extent header (min == max)
        assert_eq!(geometry.scroll_extent, 0.0); // No collapsible part
        assert_eq!(geometry.paint_extent, 60.0);
        assert_eq!(geometry.layout_extent, 60.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_arity_is_single_child() {
        let header = RenderSliverPinnedPersistentHeader::new(60.0);
        assert_eq!(header.arity(), Arity::Exact(1));
    }
}
