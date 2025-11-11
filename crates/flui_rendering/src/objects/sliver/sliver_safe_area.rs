//! RenderSliverSafeArea - Adds safe area insets to sliver content

use flui_core::element::ElementTree;
use flui_core::render::{Arity, LayoutContext, PaintContext, Render};
use flui_painting::Canvas;
use flui_types::prelude::*;
use flui_types::{SliverConstraints, SliverGeometry};

/// RenderObject that adds safe area padding to sliver content
///
/// Safe areas account for system UI elements like:
/// - Notches (iPhone X+)
/// - Status bars
/// - Navigation bars
/// - Home indicators
/// - Rounded corners
///
/// This ensures content doesn't get obscured by system UI.
///
/// # Use Cases
///
/// - Scrollable content that should avoid system UI
/// - Lists and grids that need safe area padding
/// - Content that extends edge-to-edge
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverSafeArea;
/// use flui_types::EdgeInsets;
///
/// // Add safe area padding
/// let safe_area = RenderSliverSafeArea::new(
///     EdgeInsets::new(20.0, 44.0, 20.0, 0.0), // left, top, right, bottom
/// );
/// ```
#[derive(Debug)]
pub struct RenderSliverSafeArea {
    /// Safe area insets
    pub insets: EdgeInsets,
    /// Whether to apply minimum padding
    pub minimum: EdgeInsets,
    /// Whether to maintain bottom view padding
    pub maintain_bottom_view_padding: bool,

    // Layout cache
    child_size: Size,
    sliver_geometry: SliverGeometry,
}

impl RenderSliverSafeArea {
    /// Create new sliver safe area
    ///
    /// # Arguments
    /// * `insets` - Safe area insets (typically from MediaQuery)
    pub fn new(insets: EdgeInsets) -> Self {
        Self {
            insets,
            minimum: EdgeInsets::ZERO,
            maintain_bottom_view_padding: false,
            child_size: Size::ZERO,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Set minimum padding
    pub fn set_minimum(&mut self, minimum: EdgeInsets) {
        self.minimum = minimum;
    }

    /// Set maintain bottom view padding
    pub fn set_maintain_bottom_view_padding(&mut self, maintain: bool) {
        self.maintain_bottom_view_padding = maintain;
    }

    /// Create with minimum padding
    pub fn with_minimum(mut self, minimum: EdgeInsets) -> Self {
        self.minimum = minimum;
        self
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

    /// Calculate effective padding (max of insets and minimum)
    fn effective_padding(&self) -> EdgeInsets {
        EdgeInsets::new(
            self.insets.left.max(self.minimum.left),
            self.insets.top.max(self.minimum.top),
            self.insets.right.max(self.minimum.right),
            self.insets.bottom.max(self.minimum.bottom),
        )
    }

    /// Calculate main axis padding based on axis direction
    fn main_axis_padding(&self, axis: Axis) -> (f32, f32) {
        let padding = self.effective_padding();
        match axis {
            Axis::Vertical => (padding.top, padding.bottom),
            Axis::Horizontal => (padding.left, padding.right),
        }
    }

    /// Calculate sliver geometry
    fn calculate_sliver_geometry(
        &self,
        constraints: &SliverConstraints,
        _tree: &ElementTree,
        _children: &[flui_core::element::ElementId],
    ) -> SliverGeometry {
        let scroll_offset = constraints.scroll_offset;
        let remaining_extent = constraints.remaining_paint_extent;

        let (leading_padding, trailing_padding) = self.main_axis_padding(constraints.axis_direction.axis());
        let total_padding = leading_padding + trailing_padding;

        // Safe area adds padding at start and end
        // Leading padding scrolls away, trailing padding is at the end

        // Calculate how much leading padding is still visible
        let leading_visible = (leading_padding - scroll_offset).max(0.0);

        // Paint extent includes visible leading padding + remaining space
        let paint_extent = (leading_visible + remaining_extent).min(total_padding);

        SliverGeometry {
            scroll_extent: total_padding,
            paint_extent,
            paint_origin: 0.0,
            layout_extent: paint_extent,
            max_paint_extent: total_padding,
            max_scroll_obsolescence: 0.0,
            visible_fraction: if total_padding > 0.0 {
                (paint_extent / total_padding).min(1.0)
            } else {
                0.0
            },
            cross_axis_extent: constraints.cross_axis_extent,
            cache_extent: paint_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: total_padding > paint_extent,
            hit_test_extent: Some(paint_extent),
            scroll_offset_correction: None,
        }
    }
}

impl Default for RenderSliverSafeArea {
    fn default() -> Self {
        Self::new(EdgeInsets::ZERO)
    }
}

impl Render for RenderSliverSafeArea {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let constraints = ctx.constraints;

        let padding = self.effective_padding();

        // Safe area reduces available space
        self.child_size = Size::new(
            (constraints.max_width - padding.horizontal_total()).max(0.0),
            (constraints.max_height - padding.vertical_total()).max(0.0),
        );

        self.child_size
    }

    fn paint(&self, ctx: &PaintContext) -> Canvas {
        let _offset = ctx.offset;
        let canvas = Canvas::new();

        // Child is painted with offset by safe area insets
        // TODO: Offset child by leading padding

        canvas
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Exact(1) // Single child sliver
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::layout::AxisDirection;

    #[test]
    fn test_render_sliver_safe_area_new() {
        let insets = EdgeInsets::new(10.0, 20.0, 10.0, 30.0);
        let safe_area = RenderSliverSafeArea::new(insets);

        assert_eq!(safe_area.insets, insets);
        assert_eq!(safe_area.minimum, EdgeInsets::ZERO);
        assert!(!safe_area.maintain_bottom_view_padding);
    }

    #[test]
    fn test_render_sliver_safe_area_default() {
        let safe_area = RenderSliverSafeArea::default();

        assert_eq!(safe_area.insets, EdgeInsets::ZERO);
    }

    #[test]
    fn test_set_minimum() {
        let mut safe_area = RenderSliverSafeArea::new(EdgeInsets::ZERO);
        let minimum = EdgeInsets::new(5.0, 5.0, 5.0, 5.0);
        safe_area.set_minimum(minimum);

        assert_eq!(safe_area.minimum, minimum);
    }

    #[test]
    fn test_with_minimum() {
        let minimum = EdgeInsets::new(8.0, 8.0, 8.0, 8.0);
        let safe_area = RenderSliverSafeArea::new(EdgeInsets::ZERO).with_minimum(minimum);

        assert_eq!(safe_area.minimum, minimum);
    }

    #[test]
    fn test_effective_padding_no_minimum() {
        let insets = EdgeInsets::new(10.0, 20.0, 10.0, 30.0);
        let safe_area = RenderSliverSafeArea::new(insets);

        let effective = safe_area.effective_padding();
        assert_eq!(effective, insets);
    }

    #[test]
    fn test_effective_padding_with_minimum() {
        let insets = EdgeInsets::new(5.0, 10.0, 5.0, 15.0);
        let minimum = EdgeInsets::new(8.0, 8.0, 8.0, 8.0);
        let safe_area = RenderSliverSafeArea::new(insets).with_minimum(minimum);

        let effective = safe_area.effective_padding();
        // Should be max of insets and minimum
        assert_eq!(effective.left, 8.0);  // max(5, 8)
        assert_eq!(effective.top, 10.0);  // max(10, 8)
        assert_eq!(effective.right, 8.0); // max(5, 8)
        assert_eq!(effective.bottom, 15.0); // max(15, 8)
    }

    #[test]
    fn test_main_axis_padding_vertical() {
        let insets = EdgeInsets::new(10.0, 20.0, 10.0, 30.0);
        let safe_area = RenderSliverSafeArea::new(insets);

        let (leading, trailing) = safe_area.main_axis_padding(Axis::Vertical);
        assert_eq!(leading, 20.0);  // top
        assert_eq!(trailing, 30.0); // bottom
    }

    #[test]
    fn test_main_axis_padding_horizontal() {
        let insets = EdgeInsets::new(10.0, 20.0, 15.0, 30.0);
        let safe_area = RenderSliverSafeArea::new(insets);

        let (leading, trailing) = safe_area.main_axis_padding(Axis::Horizontal);
        assert_eq!(leading, 10.0);  // left
        assert_eq!(trailing, 15.0); // right
    }

    #[test]
    fn test_calculate_sliver_geometry_not_scrolled() {
        let insets = EdgeInsets::new(0.0, 40.0, 0.0, 20.0);
        let safe_area = RenderSliverSafeArea::new(insets);
        let tree = ElementTree::new();
        let children = vec![];

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

        let geometry = safe_area.calculate_sliver_geometry(&constraints, &tree, &children);

        // Total padding: 40 + 20 = 60
        assert_eq!(geometry.scroll_extent, 60.0);
        assert_eq!(geometry.paint_extent, 60.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_partially_scrolled() {
        let insets = EdgeInsets::new(0.0, 40.0, 0.0, 20.0);
        let safe_area = RenderSliverSafeArea::new(insets);
        let tree = ElementTree::new();
        let children = vec![];

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 30.0, // Scrolled 30px
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = safe_area.calculate_sliver_geometry(&constraints, &tree, &children);

        // Leading visible: 40 - 30 = 10
        // Paint extent: 10 (leading) + 600 (remaining) = 610, but capped at total_padding (60)
        assert_eq!(geometry.scroll_extent, 60.0);
        assert_eq!(geometry.paint_extent, 60.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_scrolled_past_leading() {
        let insets = EdgeInsets::new(0.0, 40.0, 0.0, 20.0);
        let safe_area = RenderSliverSafeArea::new(insets);
        let tree = ElementTree::new();
        let children = vec![];

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 50.0, // Scrolled past leading padding
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = safe_area.calculate_sliver_geometry(&constraints, &tree, &children);

        // Leading visible: 40 - 50 = 0 (capped at 0)
        // Paint extent: 0 + 600 = 600, but capped at total_padding (60)
        assert_eq!(geometry.scroll_extent, 60.0);
        assert_eq!(geometry.paint_extent, 60.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_arity_is_single_child() {
        let safe_area = RenderSliverSafeArea::new(EdgeInsets::ZERO);
        assert_eq!(safe_area.arity(), Arity::Exact(1));
    }
}
