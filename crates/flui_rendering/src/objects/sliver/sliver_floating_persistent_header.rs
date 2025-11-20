//! RenderSliverFloatingPersistentHeader - Header that floats and can scroll off

use flui_core::render::{RuntimeArity, LegacySliverRender, SliverLayoutContext, SliverPaintContext};
use flui_painting::Canvas;
use flui_types::{SliverConstraints, SliverGeometry};

/// RenderObject for a floating persistent header
///
/// A floating header appears immediately when scrolling in reverse direction,
/// even if the content hasn't scrolled far enough to reveal it naturally.
///
/// This is different from a pinned header which stays visible once reached.
/// A floating header can scroll completely off-screen when scrolling forward,
/// but appears immediately when scrolling backward.
///
/// # Use Cases
///
/// - App bars that appear on scroll up
/// - Search bars that hide/show based on scroll direction
/// - Toolbars that disappear when scrolling content
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverFloatingPersistentHeader;
///
/// // Floating header that appears on scroll up
/// let header = RenderSliverFloatingPersistentHeader::new(80.0);
/// ```
#[derive(Debug)]
pub struct RenderSliverFloatingPersistentHeader {
    /// Height/extent of the header
    pub extent: f32,
    /// Whether to snap the header (show fully or hide fully)
    pub snap: bool,

    // Layout cache
    sliver_geometry: SliverGeometry,
}

impl RenderSliverFloatingPersistentHeader {
    /// Create new floating persistent header
    ///
    /// # Arguments
    /// * `extent` - Height of the header in pixels
    pub fn new(extent: f32) -> Self {
        Self {
            extent,
            snap: false,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Set snap behavior
    pub fn set_snap(&mut self, snap: bool) {
        self.snap = snap;
    }

    /// Create with snap behavior
    pub fn with_snap(mut self) -> Self {
        self.snap = true;
        self
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

    /// Calculate sliver geometry for floating behavior
    ///
    /// Floating headers appear immediately on reverse scroll but can
    /// scroll completely off-screen when scrolling forward.
    fn calculate_sliver_geometry(&self, constraints: &SliverConstraints) -> SliverGeometry {
        let scroll_offset = constraints.scroll_offset;
        let remaining_extent = constraints.remaining_paint_extent;

        // For floating headers, visibility depends on scroll direction
        // Since we don't have scroll direction here, we use a simplified model:
        // - If scroll offset is 0, header is fully visible
        // - If scroll offset >= extent, header can scroll off
        // - In between, header is partially visible

        let visible_extent = if scroll_offset < self.extent {
            // Header is in view
            (self.extent - scroll_offset).max(0.0)
        } else {
            // Header has scrolled off (but can float back)
            // For floating, we don't pin it, so it's truly off
            0.0
        };

        let paint_extent = visible_extent.min(remaining_extent);

        SliverGeometry {
            scroll_extent: self.extent,
            paint_extent,
            paint_origin: 0.0,
            layout_extent: paint_extent,
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

impl Default for RenderSliverFloatingPersistentHeader {
    fn default() -> Self {
        Self::new(56.0) // Material Design standard app bar height
    }
}

impl LegacySliverRender for RenderSliverFloatingPersistentHeader {
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
    fn test_render_sliver_floating_persistent_header_new() {
        let header = RenderSliverFloatingPersistentHeader::new(80.0);

        assert_eq!(header.extent, 80.0);
        assert!(!header.snap);
    }

    #[test]
    fn test_render_sliver_floating_persistent_header_default() {
        let header = RenderSliverFloatingPersistentHeader::default();

        assert_eq!(header.extent, 56.0);
        assert!(!header.snap);
    }

    #[test]
    fn test_set_snap() {
        let mut header = RenderSliverFloatingPersistentHeader::new(80.0);
        header.set_snap(true);

        assert!(header.snap);
    }

    #[test]
    fn test_with_snap() {
        let header = RenderSliverFloatingPersistentHeader::new(80.0).with_snap();

        assert!(header.snap);
    }

    #[test]
    fn test_calculate_sliver_geometry_fully_visible() {
        let header = RenderSliverFloatingPersistentHeader::new(80.0);

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

        // Header should be fully visible
        assert_eq!(geometry.scroll_extent, 80.0);
        assert_eq!(geometry.paint_extent, 80.0);
        assert!(geometry.visible);
        assert_eq!(geometry.visible_fraction, 1.0);
    }

    #[test]
    fn test_calculate_sliver_geometry_partially_visible() {
        let header = RenderSliverFloatingPersistentHeader::new(80.0);

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

        // Header should be half visible (80 - 40 = 40px)
        assert_eq!(geometry.scroll_extent, 80.0);
        assert_eq!(geometry.paint_extent, 40.0);
        assert!(geometry.visible);
        assert_eq!(geometry.visible_fraction, 0.5);
    }

    #[test]
    fn test_calculate_sliver_geometry_scrolled_off() {
        let header = RenderSliverFloatingPersistentHeader::new(80.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 100.0, // Scrolled past header
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = header.calculate_sliver_geometry(&constraints);

        // Header should be scrolled off (floating, not pinned)
        assert_eq!(geometry.scroll_extent, 80.0);
        assert_eq!(geometry.paint_extent, 0.0);
        assert!(!geometry.visible);
        assert_eq!(geometry.visible_fraction, 0.0);
    }

    #[test]
    fn test_arity_is_single_child() {
        let header = RenderSliverFloatingPersistentHeader::new(80.0);
        assert_eq!(header.arity(), RuntimeArity::Exact(1));
    }
}
