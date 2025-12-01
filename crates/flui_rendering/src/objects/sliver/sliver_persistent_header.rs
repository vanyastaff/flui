//! RenderSliverPersistentHeader - Sticky header that stays visible during scroll

use flui_core::render::{RuntimeArity, LegacySliverRender, SliverLayoutContext, SliverPaintContext};
use flui_painting::Canvas;
use flui_types::{SliverConstraints, SliverGeometry};

/// RenderObject for a persistent header that sticks during scrolling
///
/// Persistent headers remain visible at the top/leading edge of the viewport
/// as content scrolls underneath them. Common use cases:
/// - Section headers in lists
/// - Sticky table headers
/// - Category separators
/// - Date headers in message lists
///
/// # Behavior
///
/// - **Pinned**: Always visible once reached
/// - **Floating**: Can scroll off-screen but reappears on reverse scroll
/// - **Neither**: Scrolls away normally
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverPersistentHeader;
///
/// // Always visible once scrolled into view
/// let pinned = RenderSliverPersistentHeader::new(50.0, true);
///
/// // Can scroll away
/// let floating = RenderSliverPersistentHeader::new(50.0, false);
/// ```
#[derive(Debug)]
pub struct RenderSliverPersistentHeader {
    /// Height of the header
    pub extent: f32,
    /// Whether header is pinned (stays visible)
    pub pinned: bool,
    /// Whether header floats (reappears on scroll up)
    pub floating: bool,

    // Layout cache
    sliver_geometry: SliverGeometry,
}

impl RenderSliverPersistentHeader {
    /// Create new persistent header
    ///
    /// # Arguments
    /// * `extent` - Height of the header
    /// * `pinned` - Whether to pin header once visible
    pub fn new(extent: f32, pinned: bool) -> Self {
        Self {
            extent,
            pinned,
            floating: false,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Set whether header is pinned
    pub fn set_pinned(&mut self, pinned: bool) {
        self.pinned = pinned;
    }

    /// Set whether header is floating
    pub fn set_floating(&mut self, floating: bool) {
        self.floating = floating;
    }

    /// Create with floating behavior
    pub fn with_floating(mut self) -> Self {
        self.floating = true;
        self
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

    /// Calculate sliver geometry
    fn calculate_sliver_geometry(
        &self,
        constraints: &SliverConstraints,
    ) -> SliverGeometry {
        let scroll_offset = constraints.scroll_offset;
        let remaining_extent = constraints.remaining_paint_extent;

        // Calculate how much of the header is visible
        let paint_extent = if self.pinned {
            // Pinned: Always visible once reached
            if scroll_offset >= self.extent {
                // Header has been reached, now it sticks
                self.extent.min(remaining_extent)
            } else {
                // Not yet reached the header
                let visible = (self.extent - scroll_offset).max(0.0);
                visible.min(remaining_extent)
            }
        } else {
            // Not pinned: Regular scrolling behavior
            let visible = (self.extent - scroll_offset).max(0.0);
            visible.min(remaining_extent)
        };

        // Layout extent is what affects following slivers
        let layout_extent = if self.pinned && scroll_offset >= self.extent {
            // When pinned and past scroll offset, we take up space
            self.extent.min(remaining_extent)
        } else {
            paint_extent
        };

        SliverGeometry {
            scroll_extent: self.extent,
            paint_extent,
            paint_origin: 0.0,
            layout_extent,
            max_paint_extent: self.extent,
            max_scroll_obsolescence: 0.0,
            visible_fraction: if self.extent > 0.0 {
                (paint_extent / self.extent).min(1.0)
            } else {
                0.0
            },
            cross_axis_extent: constraints.cross_axis_extent,
            cache_extent: paint_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: self.extent > paint_extent,
            hit_test_extent: Some(paint_extent),
            scroll_offset_correction: None,
        }
    }
}

impl LegacySliverRender for RenderSliverPersistentHeader {
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

    fn arity(&self) -> RuntimeArity {
        RuntimeArity::Exact(1) // Single child (header content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::layout::AxisDirection;

    #[test]
    fn test_render_sliver_persistent_header_new_pinned() {
        let header = RenderSliverPersistentHeader::new(50.0, true);

        assert_eq!(header.extent, 50.0);
        assert!(header.pinned);
        assert!(!header.floating);
    }

    #[test]
    fn test_render_sliver_persistent_header_new_not_pinned() {
        let header = RenderSliverPersistentHeader::new(50.0, false);

        assert_eq!(header.extent, 50.0);
        assert!(!header.pinned);
        assert!(!header.floating);
    }

    #[test]
    fn test_set_pinned() {
        let mut header = RenderSliverPersistentHeader::new(50.0, false);
        header.set_pinned(true);

        assert!(header.pinned);
    }

    #[test]
    fn test_set_floating() {
        let mut header = RenderSliverPersistentHeader::new(50.0, true);
        header.set_floating(true);

        assert!(header.floating);
    }

    #[test]
    fn test_with_floating() {
        let header = RenderSliverPersistentHeader::new(50.0, true).with_floating();

        assert!(header.pinned);
        assert!(header.floating);
    }

    #[test]
    fn test_calculate_sliver_geometry_not_scrolled() {
        let header = RenderSliverPersistentHeader::new(50.0, false);

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

        // Full header visible
        assert_eq!(geometry.scroll_extent, 50.0);
        assert_eq!(geometry.paint_extent, 50.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_partially_scrolled() {
        let header = RenderSliverPersistentHeader::new(50.0, false);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 25.0, // Scrolled halfway
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = header.calculate_sliver_geometry(&constraints);

        // Half visible
        assert_eq!(geometry.scroll_extent, 50.0);
        assert_eq!(geometry.paint_extent, 25.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_scrolled_past_not_pinned() {
        let header = RenderSliverPersistentHeader::new(50.0, false);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 60.0, // Scrolled past
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = header.calculate_sliver_geometry(&constraints);

        // Not visible when not pinned
        assert_eq!(geometry.scroll_extent, 50.0);
        assert_eq!(geometry.paint_extent, 0.0);
        assert!(!geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_scrolled_past_pinned() {
        let header = RenderSliverPersistentHeader::new(50.0, true);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 60.0, // Scrolled past
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = header.calculate_sliver_geometry(&constraints);

        // Still visible when pinned!
        assert_eq!(geometry.scroll_extent, 50.0);
        assert_eq!(geometry.paint_extent, 50.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_pinned_before_reached() {
        let header = RenderSliverPersistentHeader::new(50.0, true);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 25.0, // Before fully scrolled
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = header.calculate_sliver_geometry(&constraints);

        // Partially visible, not yet pinned
        assert_eq!(geometry.scroll_extent, 50.0);
        assert_eq!(geometry.paint_extent, 25.0); // 50 - 25
        assert!(geometry.visible);
    }

    #[test]
    fn test_arity_is_single_child() {
        let header = RenderSliverPersistentHeader::new(50.0, true);
        assert_eq!(header.arity(), RuntimeArity::Exact(1));
    }
}
