//! RenderSliverPadding - Adds padding around sliver content
//!
//! Implements Flutter's SliverPadding that adds insets around a sliver child. This is the
//! sliver protocol equivalent of RenderPadding for box protocol, adjusting both constraints
//! and geometry to account for padding space.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderSliverPadding` | `RenderSliverPadding` from `package:flutter/src/rendering/sliver_padding.dart` |
//! | `padding` property | `padding` property (EdgeInsetsGeometry) |
//! | `child_constraints()` | `getChildConstraints()` method |
//! | `child_to_parent_geometry()` | Geometry adjustment logic |
//! | `symmetric()`, `all()`, `only()` | Constructor helpers (FLUI extension) |
//!
//! # Layout Protocol
//!
//! 1. **Calculate child constraints**
//!    - Subtract padding from parent constraints
//!    - Main axis: reduce scroll_offset, remaining_paint_extent, cache_extent
//!    - Cross axis: reduce cross_axis_extent
//!    - Clamp all values to non-negative (max with 0)
//!
//! 2. **Layout child with adjusted constraints**
//!    - Child receives reduced constraints accounting for padding
//!    - Child doesn't know about padding (encapsulation)
//!
//! 3. **Convert child geometry to parent geometry**
//!    - Add padding back to all extent values
//!    - scroll_extent, paint_extent, layout_extent, max_paint_extent all increase
//!    - cross_axis_extent also increases by cross-axis padding
//!    - Preserve other properties (visible_fraction, etc.)
//!
//! # Paint Protocol
//!
//! 1. **Calculate padding offset**
//!    - Use padding.left and padding.top as offset
//!
//! 2. **Paint child at offset position**
//!    - Child painted inset by padding amount
//!
//! # Performance
//!
//! - **Layout**: O(1) + child layout - simple constraint adjustment
//! - **Paint**: O(1) + child paint - offset calculation only
//! - **Memory**: 40 bytes (EdgeInsets + SliverGeometry cache)
//!
//! # Use Cases
//!
//! - **List spacing**: Add space before/after sliver lists
//! - **Section insets**: Indent sliver content from edges
//! - **Viewport margins**: Create margins around viewport content
//! - **Grouped content**: Separate sliver groups visually
//! - **Safe areas**: Inset content from screen edges
//! - **Header/footer spacing**: Add space around persistent headers
//!
//! # Comparison with Related Objects
//!
//! - **vs RenderPadding**: SliverPadding is sliver protocol, Padding is box protocol
//! - **vs SliverEdgeInsetsPadding**: SliverPadding is simpler base implementation
//! - **vs SliverSafeArea**: SafeArea adjusts for device notches, Padding uses fixed values
//! - **vs SliverToBoxAdapter + Padding**: Direct sliver padding is more efficient
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderSliverPadding;
//! use flui_types::EdgeInsets;
//!
//! // Uniform padding
//! let uniform = RenderSliverPadding::all(16.0);
//!
//! // Symmetric padding (horizontal, vertical)
//! let symmetric = RenderSliverPadding::symmetric(24.0, 16.0);
//!
//! // Custom per-side padding
//! let custom = RenderSliverPadding::only(
//!     8.0,   // left
//!     16.0,  // top
//!     8.0,   // right
//!     24.0   // bottom
//! );
//!
//! // Dynamic padding updates
//! let mut padding = RenderSliverPadding::all(10.0);
//! padding.set_padding(EdgeInsets::symmetric(20.0, 15.0));
//! ```

use crate::core::{RuntimeArity, LegacySliverRender, SliverSliver};
use flui_painting::Canvas;
use flui_types::prelude::*;
use flui_types::{SliverConstraints, SliverGeometry};

/// RenderObject that adds padding around sliver content.
///
/// Insets the child sliver by specified EdgeInsets, adjusting both layout constraints
/// (reducing available space) and geometry results (adding padding back to extents). This
/// enables consistent spacing around sliver content in scrollable viewports.
///
/// # Arity
///
/// `RuntimeArity::Exact(1)` - Must have exactly 1 child sliver.
///
/// # Protocol
///
/// Sliver protocol - Uses `SliverConstraints` and returns `SliverGeometry`.
///
/// # Pattern
///
/// **Sliver Proxy** - Passes adjusted constraints to child (reduced by padding),
/// then adds padding back to child geometry. Similar to RenderPadding but for
/// sliver protocol.
///
/// # Use Cases
///
/// - **List margins**: Space around entire list content
/// - **Section spacing**: Separate sliver sections visually
/// - **Safe area insets**: Respect device safe areas in slivers
/// - **Viewport padding**: Add margins to scrollable content
/// - **Header/footer gaps**: Space around persistent headers
/// - **Content indentation**: Indent sliver content from edges
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderSliverPadding behavior:
/// - Adjusts child constraints by subtracting padding
/// - Adds padding back to child geometry for parent
/// - Clamps adjusted constraints to non-negative values
/// - Paints child offset by padding.left and padding.top
/// - Handles both main-axis and cross-axis padding correctly
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverPadding;
/// use flui_types::EdgeInsets;
///
/// // Add uniform spacing around sliver list
/// let padded_list = RenderSliverPadding::all(16.0);
///
/// // Asymmetric padding for safe areas
/// let safe_padding = RenderSliverPadding::only(
///     0.0,   // left
///     44.0,  // top (status bar)
///     0.0,   // right
///     34.0   // bottom (home indicator)
/// );
/// ```
#[derive(Debug)]
pub struct RenderSliverPadding {
    /// Padding to apply
    pub padding: EdgeInsets,

    // Layout cache
    sliver_geometry: SliverGeometry,
}

impl RenderSliverPadding {
    /// Create new sliver padding
    pub fn new(padding: EdgeInsets) -> Self {
        Self {
            padding,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Create with all sides equal
    pub fn all(amount: f32) -> Self {
        Self::new(EdgeInsets::all(amount))
    }

    /// Create with symmetric padding
    pub fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self::new(EdgeInsets::symmetric(horizontal, vertical))
    }

    /// Create with individual sides
    pub fn only(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self::new(EdgeInsets::new(left, top, right, bottom))
    }

    /// Set padding
    pub fn set_padding(&mut self, padding: EdgeInsets) {
        self.padding = padding;
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

    /// Calculate adjusted sliver constraints for child
    fn child_constraints(&self, constraints: &SliverConstraints) -> SliverConstraints {
        // Adjust constraints to account for padding
        let main_axis_padding = match constraints.axis_direction.axis() {
            Axis::Vertical => self.padding.vertical_total(),
            Axis::Horizontal => self.padding.horizontal_total(),
        };

        let cross_axis_padding = match constraints.axis_direction.axis() {
            Axis::Vertical => self.padding.horizontal_total(),
            Axis::Horizontal => self.padding.vertical_total(),
        };

        SliverConstraints {
            axis_direction: constraints.axis_direction,
            grow_direction_reversed: constraints.grow_direction_reversed,
            scroll_offset: (constraints.scroll_offset - main_axis_padding).max(0.0),
            remaining_paint_extent: (constraints.remaining_paint_extent - main_axis_padding)
                .max(0.0),
            cross_axis_extent: (constraints.cross_axis_extent - cross_axis_padding).max(0.0),
            cross_axis_direction: constraints.cross_axis_direction,
            viewport_main_axis_extent: constraints.viewport_main_axis_extent,
            remaining_cache_extent: (constraints.remaining_cache_extent - main_axis_padding)
                .max(0.0),
            cache_origin: constraints.cache_origin,
        }
    }

    /// Calculate sliver geometry from child geometry
    fn child_to_parent_geometry(
        &self,
        child_geometry: SliverGeometry,
        constraints: &SliverConstraints,
    ) -> SliverGeometry {
        // Determine main-axis and cross-axis padding based on scroll direction
        let (main_axis_padding, cross_axis_padding) = match constraints.axis_direction.axis() {
            Axis::Vertical => (self.padding.vertical_total(), self.padding.horizontal_total()),
            Axis::Horizontal => (self.padding.horizontal_total(), self.padding.vertical_total()),
        };

        SliverGeometry {
            scroll_extent: child_geometry.scroll_extent + main_axis_padding,
            paint_extent: child_geometry.paint_extent + main_axis_padding,
            paint_origin: child_geometry.paint_origin,
            layout_extent: child_geometry.layout_extent + main_axis_padding,
            max_paint_extent: child_geometry.max_paint_extent + main_axis_padding,
            max_scroll_obsolescence: child_geometry.max_scroll_obsolescence,
            visible_fraction: child_geometry.visible_fraction,
            cross_axis_extent: child_geometry.cross_axis_extent + cross_axis_padding,
            cache_extent: child_geometry.cache_extent + main_axis_padding,
            visible: child_geometry.visible,
            has_visual_overflow: child_geometry.has_visual_overflow,
            hit_test_extent: child_geometry
                .hit_test_extent
                .map(|extent| extent + main_axis_padding),
            scroll_offset_correction: child_geometry.scroll_offset_correction,
        }
    }
}

impl LegacySliverRender for RenderSliverPadding {
    fn layout(&mut self, ctx: &Sliver) -> SliverGeometry {
        let child_id = ctx.children.single();

        // Adjust constraints for padding
        let child_constraints = self.child_constraints(&ctx.constraints);

        // Layout child
        let child_geometry = ctx.layout_child(child_id, child_constraints);

        // Add padding to geometry (pass constraints for axis determination)
        let geometry = self.child_to_parent_geometry(child_geometry, &ctx.constraints);

        // Cache geometry
        self.sliver_geometry = geometry;

        geometry
    }

    fn paint(&self, ctx: &Sliver) -> Canvas {
        let child_id = ctx.children.single();

        // Paint child with padding offset
        let padding_offset = Offset::new(self.padding.left, self.padding.top);
        let child_offset = ctx.offset + padding_offset;

        ctx.paint_child(child_id, child_offset)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> RuntimeArity {
        RuntimeArity::Exact(1) // Single child sliver
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::layout::AxisDirection;

    #[test]
    fn test_render_sliver_padding_new() {
        let padding = RenderSliverPadding::new(EdgeInsets::all(10.0));

        assert_eq!(padding.padding, EdgeInsets::all(10.0));
    }

    #[test]
    fn test_render_sliver_padding_all() {
        let padding = RenderSliverPadding::all(15.0);

        assert_eq!(padding.padding, EdgeInsets::all(15.0));
    }

    #[test]
    fn test_render_sliver_padding_symmetric() {
        let padding = RenderSliverPadding::symmetric(20.0, 10.0);

        assert_eq!(padding.padding, EdgeInsets::symmetric(20.0, 10.0));
    }

    #[test]
    fn test_render_sliver_padding_only() {
        let padding = RenderSliverPadding::only(5.0, 10.0, 15.0, 20.0);

        assert_eq!(
            padding.padding,
            EdgeInsets::new(5.0, 10.0, 15.0, 20.0)
        );
    }

    #[test]
    fn test_set_padding() {
        let mut padding = RenderSliverPadding::all(10.0);
        padding.set_padding(EdgeInsets::all(20.0));

        assert_eq!(padding.padding, EdgeInsets::all(20.0));
    }

    #[test]
    fn test_child_constraints_vertical() {
        let padding = RenderSliverPadding::symmetric(10.0, 20.0); // h, v

        let parent_constraints = SliverConstraints {
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

        let child_constraints = padding.child_constraints(&parent_constraints);

        // Vertical padding = 40 (20 top + 20 bottom)
        assert_eq!(child_constraints.scroll_offset, 60.0); // 100 - 40
        assert_eq!(child_constraints.remaining_paint_extent, 560.0); // 600 - 40
        assert_eq!(child_constraints.remaining_cache_extent, 960.0); // 1000 - 40

        // Horizontal padding = 20 (10 left + 10 right)
        assert_eq!(child_constraints.cross_axis_extent, 380.0); // 400 - 20
    }

    #[test]
    fn test_child_constraints_clamped_to_zero() {
        let padding = RenderSliverPadding::all(1000.0); // Huge padding

        let parent_constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 50.0,
            remaining_paint_extent: 100.0,
            cross_axis_extent: 100.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 200.0,
            cache_origin: 0.0,
        };

        let child_constraints = padding.child_constraints(&parent_constraints);

        // Should be clamped to 0, not negative
        assert_eq!(child_constraints.scroll_offset, 0.0);
        assert_eq!(child_constraints.remaining_paint_extent, 0.0);
        assert_eq!(child_constraints.cross_axis_extent, 0.0);
        assert_eq!(child_constraints.remaining_cache_extent, 0.0);
    }

    #[test]
    fn test_child_to_parent_geometry() {
        let padding = RenderSliverPadding::symmetric(10.0, 20.0); // h=20, v=40

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom, // Vertical
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let child_geometry = SliverGeometry {
            scroll_extent: 500.0,
            paint_extent: 300.0,
            paint_origin: 0.0,
            layout_extent: 300.0,
            max_paint_extent: 500.0,
            max_scroll_obsolescence: 0.0,
            visible_fraction: 0.6,
            cross_axis_extent: 360.0,
            cache_extent: 300.0,
            visible: true,
            has_visual_overflow: false,
            hit_test_extent: Some(300.0),
            scroll_offset_correction: None,
        };

        let parent_geometry = padding.child_to_parent_geometry(child_geometry, &constraints);

        // For vertical axis: main_axis_padding = vertical_total = 40
        assert_eq!(parent_geometry.scroll_extent, 540.0); // 500 + 40
        assert_eq!(parent_geometry.paint_extent, 340.0); // 300 + 40
        assert_eq!(parent_geometry.layout_extent, 340.0);
        assert_eq!(parent_geometry.max_paint_extent, 540.0);

        // For vertical axis: cross_axis_padding = horizontal_total = 20
        assert_eq!(parent_geometry.cross_axis_extent, 380.0); // 360 + 20

        // Cache extent
        assert_eq!(parent_geometry.cache_extent, 340.0);

        // Hit test extent
        assert_eq!(parent_geometry.hit_test_extent, Some(340.0));

        // Other properties preserved
        assert_eq!(parent_geometry.visible_fraction, 0.6);
        assert!(parent_geometry.visible);
    }

    #[test]
    fn test_child_to_parent_geometry_zero_child() {
        let padding = RenderSliverPadding::all(10.0);

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

        let child_geometry = SliverGeometry::default();
        let parent_geometry = padding.child_to_parent_geometry(child_geometry, &constraints);

        // Even with zero child, padding adds extent (vertical = 20 for all(10))
        assert_eq!(parent_geometry.scroll_extent, 20.0); // 0 + 20 (top+bottom)
        assert_eq!(parent_geometry.paint_extent, 20.0);
    }

    #[test]
    fn test_arity_is_single_child() {
        let padding = RenderSliverPadding::all(10.0);
        assert_eq!(padding.arity(), RuntimeArity::Exact(1));
    }
}
