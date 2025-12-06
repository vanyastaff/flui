//! RenderSliverEdgeInsetsPadding - EdgeInsets-based padding for slivers
//!
//! Implements Flutter's SliverPadding pattern specifically optimized for EdgeInsets padding.
//! Wraps sliver content with rectangular insets (left, top, right, bottom), adjusting both
//! layout constraints and paint positioning to create visual spacing around the child.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderSliverEdgeInsetsPadding` | `RenderSliverPadding` specialized for EdgeInsets |
//! | `padding` property | `padding` property (EdgeInsetsGeometry) |
//! | `main_axis_padding()` | Axis-aware padding extraction |
//! | `child_constraints()` | Constraint adjustment by padding |
//! | `calculate_sliver_geometry()` | Geometry adjustment by padding |
//! | Paint offset by (left, top) | Absolute direction offset ✅ |
//!
//! # Layout Protocol
//!
//! 1. **Calculate axis-relative padding**
//!    - Extract main-axis padding (leading + trailing) based on scroll direction
//!    - Extract cross-axis padding total
//!    - Vertical scroll: main=(top, bottom), cross=(left + right)
//!    - Horizontal scroll: main=(left, right), cross=(top + bottom)
//!
//! 2. **Create child constraints with padding removed**
//!    - scroll_offset: `max(parent_offset - leading_padding, 0)`
//!    - remaining_paint_extent: `max(parent_extent - total_padding, 0)`
//!    - cross_axis_extent: `max(parent_cross - cross_padding, 0)`
//!    - remaining_cache_extent: `max(parent_cache - total_padding, 0)`
//!
//! 3. **Layout child with reduced constraints**
//!    - Child receives smaller constraint space
//!    - Child determines its own geometry
//!
//! 4. **Add padding back to child geometry**
//!    - scroll_extent: `child + total_padding`
//!    - paint_extent: `min(child + total_padding, remaining_paint_extent)`
//!    - max_paint_extent: `child + total_padding`
//!    - cache_extent: `child + total_padding`
//!    - hit_test_extent: `child.map(|e| e + total_padding)`
//!
//! # Paint Protocol
//!
//! 1. **Check visibility**
//!    - Only paint if sliver_geometry.visible
//!
//! 2. **Calculate paint offset**
//!    - Offset child by (padding.left, padding.top)
//!    - Absolute directions work correctly for both axes
//!    - Vertical scroll: left=cross, top=main-leading ✅
//!    - Horizontal scroll: left=main-leading, top=cross ✅
//!
//! 3. **Paint child at offset position**
//!    - Paint child with adjusted offset
//!
//! # Performance
//!
//! - **Layout**: O(child) - pass-through with constraint modification
//! - **Paint**: O(child) - pass-through with offset adjustment
//! - **Memory**: 16 bytes (EdgeInsets) + 48 bytes (SliverGeometry cache) = 64 bytes
//! - **Optimization**: Axis-aware methods avoid redundant calculations
//!
//! # Use Cases
//!
//! - **List margins**: Add spacing around scrollable list content
//! - **Material Design spacing**: 16dp/8dp standard margins
//! - **Safe area insets**: Respect device notches and system UI
//! - **Breathing room**: Visual separation from viewport edges
//! - **Asymmetric padding**: Different insets on each side
//! - **Responsive margins**: Adaptive spacing based on viewport size
//!
//! # Comparison with Related Objects
//!
//! - **vs RenderSliverPadding**: EdgeInsetsPadding is specialized, SliverPadding is generic
//! - **vs RenderPadding (box)**: EdgeInsetsPadding is sliver protocol, Padding is box protocol
//! - **vs SliverSafeArea**: SafeArea adds device insets, EdgeInsetsPadding adds explicit values
//! - **vs SliverToBoxAdapter + Padding**: Adapter is protocol conversion, EdgeInsetsPadding is direct
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderSliverEdgeInsetsPadding;
//! use flui_types::EdgeInsets;
//!
//! // Symmetric padding (Material Design standard)
//! let list_padding = RenderSliverEdgeInsetsPadding::new(
//!     EdgeInsets::all(16.0),
//! );
//!
//! // Horizontal padding only
//! let horizontal = RenderSliverEdgeInsetsPadding::new(
//!     EdgeInsets::symmetric_horizontal(20.0),
//! );
//!
//! // Asymmetric padding (more bottom for FAB clearance)
//! let fab_clearance = RenderSliverEdgeInsetsPadding::new(
//!     EdgeInsets::new(16.0, 16.0, 16.0, 80.0), // left, top, right, bottom
//! );
//!
//! // Responsive padding based on screen width
//! let responsive_padding = if screen_width > 600.0 {
//!     EdgeInsets::symmetric_horizontal(48.0) // Desktop margins
//! } else {
//!     EdgeInsets::symmetric_horizontal(16.0) // Mobile margins
//! };
//! let responsive = RenderSliverEdgeInsetsPadding::new(responsive_padding);
//! ```

use crate::core::{RenderObject, RenderSliver, Single, SliverLayoutContext, SliverPaintContext};
use crate::RenderResult;
use flui_painting::Canvas;
use flui_types::prelude::*;
use flui_types::{SliverConstraints, SliverGeometry};

/// RenderObject that adds EdgeInsets padding to sliver content.
///
/// Specialized version of SliverPadding optimized for EdgeInsets (rectangular insets
/// with left, top, right, bottom values). Wraps a sliver child with visual spacing,
/// adjusting layout constraints to reduce available space and paint offset to shift
/// the child inward.
///
/// # Arity
///
/// `RuntimeArity::Exact(1)` - Must have exactly 1 child sliver (optional in implementation).
///
/// # Protocol
///
/// Sliver protocol - Uses `SliverConstraints` and returns `SliverGeometry`.
///
/// # Pattern
///
/// **Padding Proxy** - Reduces child constraints by padding amounts, then adds padding
/// back to child geometry. Paint shifts child by (left, top) offset. Axis-aware padding
/// extraction ensures correct behavior for both vertical and horizontal scrolling.
///
/// # Use Cases
///
/// - **List margins**: Standard 16dp spacing around Material lists
/// - **Safe area insets**: Respect system UI and device notches
/// - **Responsive spacing**: Larger margins on desktop, smaller on mobile
/// - **Asymmetric padding**: Extra bottom padding for FAB clearance
/// - **Visual breathing room**: Separation from viewport edges
/// - **Content insets**: Padding for card-style scrollable content
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderSliverPadding behavior for EdgeInsets:
/// - Reduces constraints by padding values ✅
/// - Adds padding to child geometry ✅
/// - Offsets paint position by (left, top) ✅
/// - Correctly handles axis direction for main/cross axis ✅
/// - Clamps adjusted constraints to non-negative ✅
///
/// # Axis-Aware Padding
///
/// | Scroll Direction | Main-Axis Padding | Cross-Axis Padding |
/// |------------------|-------------------|---------------------|
/// | Vertical | (top, bottom) | left + right |
/// | Horizontal | (left, right) | top + bottom |
///
/// The implementation uses absolute EdgeInsets directions (left/top/right/bottom)
/// and correctly maps them to axis-relative positions based on scroll direction.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverEdgeInsetsPadding;
/// use flui_types::EdgeInsets;
///
/// // Material Design standard spacing
/// let list_margin = RenderSliverEdgeInsetsPadding::new(
///     EdgeInsets::all(16.0),
/// );
///
/// // Asymmetric for FAB clearance (extra bottom)
/// let with_fab = RenderSliverEdgeInsetsPadding::new(
///     EdgeInsets::new(16.0, 16.0, 16.0, 80.0), // left, top, right, bottom
/// );
///
/// // Horizontal-only padding
/// let horizontal = RenderSliverEdgeInsetsPadding::new(
///     EdgeInsets::symmetric_horizontal(24.0),
/// );
/// ```
#[derive(Debug)]
pub struct RenderSliverEdgeInsetsPadding {
    /// Edge insets padding
    pub padding: EdgeInsets,

    // Layout cache
    sliver_geometry: SliverGeometry,
}

impl RenderSliverEdgeInsetsPadding {
    /// Create new sliver edge insets padding
    ///
    /// # Arguments
    /// * `padding` - EdgeInsets padding values
    pub fn new(padding: EdgeInsets) -> Self {
        Self {
            padding,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Set padding
    pub fn set_padding(&mut self, padding: EdgeInsets) {
        self.padding = padding;
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

    /// Calculate main axis padding
    fn main_axis_padding(&self, axis: Axis) -> (f32, f32) {
        match axis {
            Axis::Vertical => (self.padding.top, self.padding.bottom),
            Axis::Horizontal => (self.padding.left, self.padding.right),
        }
    }

    /// Calculate cross axis padding
    fn cross_axis_padding(&self, axis: Axis) -> f32 {
        match axis {
            Axis::Vertical => self.padding.horizontal_total(),
            Axis::Horizontal => self.padding.vertical_total(),
        }
    }

    /// Calculate child constraints with padding removed
    fn child_constraints(&self, constraints: &SliverConstraints) -> SliverConstraints {
        let (leading_padding, trailing_padding) = self.main_axis_padding(constraints.axis_direction.axis());
        let cross_padding = self.cross_axis_padding(constraints.axis_direction.axis());

        SliverConstraints {
            axis_direction: constraints.axis_direction,
            grow_direction_reversed: constraints.grow_direction_reversed,
            scroll_offset: (constraints.scroll_offset - leading_padding).max(0.0),
            remaining_paint_extent: (constraints.remaining_paint_extent - leading_padding - trailing_padding).max(0.0),
            cross_axis_extent: (constraints.cross_axis_extent - cross_padding).max(0.0),
            cross_axis_direction: constraints.cross_axis_direction,
            viewport_main_axis_extent: constraints.viewport_main_axis_extent,
            remaining_cache_extent: (constraints.remaining_cache_extent - leading_padding - trailing_padding).max(0.0),
            cache_origin: constraints.cache_origin,
        }
    }

    /// Calculate sliver geometry from child
    fn calculate_sliver_geometry(
        &self,
        constraints: &SliverConstraints,
        child_geometry: SliverGeometry,
    ) -> SliverGeometry {
        let (leading_padding, trailing_padding) = self.main_axis_padding(constraints.axis_direction.axis());
        let total_padding = leading_padding + trailing_padding;

        // Add padding to child's geometry
        SliverGeometry {
            scroll_extent: child_geometry.scroll_extent + total_padding,
            paint_extent: (child_geometry.paint_extent + leading_padding + trailing_padding)
                .min(constraints.remaining_paint_extent),
            paint_origin: child_geometry.paint_origin,
            layout_extent: (child_geometry.layout_extent + leading_padding + trailing_padding)
                .min(constraints.remaining_paint_extent),
            max_paint_extent: child_geometry.max_paint_extent + total_padding,
            max_scroll_obsolescence: child_geometry.max_scroll_obsolescence,
            visible_fraction: child_geometry.visible_fraction,
            cross_axis_extent: constraints.cross_axis_extent,
            cache_extent: child_geometry.cache_extent + leading_padding + trailing_padding,
            visible: child_geometry.visible,
            has_visual_overflow: child_geometry.has_visual_overflow,
            hit_test_extent: child_geometry.hit_test_extent.map(|e| e + leading_padding + trailing_padding),
            scroll_offset_correction: child_geometry.scroll_offset_correction,
        }
    }
}

impl Default for RenderSliverEdgeInsetsPadding {
    fn default() -> Self {
        Self::new(EdgeInsets::ZERO)
    }
}

impl RenderObject for RenderSliverEdgeInsetsPadding {}

impl RenderSliver<Single> for RenderSliverEdgeInsetsPadding {
    fn layout(&mut self, mut ctx: SliverLayoutContext<'_, Single>) -> RenderResult<SliverGeometry> {
        let constraints = ctx.constraints;
        let child_id = *ctx.children.single();

        // Adjust constraints for child
        let child_constraints = self.child_constraints(&constraints);

        // Layout child
        let child_geometry = ctx.tree_mut().perform_sliver_layout(child_id, child_constraints)?;

        // Calculate and cache geometry with padding
        self.sliver_geometry = self.calculate_sliver_geometry(&constraints, child_geometry);
        Ok(self.sliver_geometry)
    }

    fn paint(&self, ctx: &mut SliverPaintContext<'_, Single>) {
        // Paint child if visible
        if self.sliver_geometry.visible {
            let child_id = *ctx.children.single();

            // Paint child with padding offset
            let padding_offset = Offset::new(self.padding.left, self.padding.top);
            if let Ok(child_canvas) = ctx.tree().perform_paint(child_id, ctx.offset + padding_offset) {
                *ctx.canvas = child_canvas;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::layout::AxisDirection;

    #[test]
    fn test_render_sliver_edge_insets_padding_new() {
        let padding = EdgeInsets::new(10.0, 20.0, 10.0, 30.0);
        let sliver = RenderSliverEdgeInsetsPadding::new(padding);

        assert_eq!(sliver.padding, padding);
    }

    #[test]
    fn test_render_sliver_edge_insets_padding_default() {
        let sliver = RenderSliverEdgeInsetsPadding::default();

        assert_eq!(sliver.padding, EdgeInsets::ZERO);
    }

    #[test]
    fn test_set_padding() {
        let mut sliver = RenderSliverEdgeInsetsPadding::new(EdgeInsets::ZERO);
        let new_padding = EdgeInsets::new(5.0, 10.0, 5.0, 15.0);
        sliver.set_padding(new_padding);

        assert_eq!(sliver.padding, new_padding);
    }

    #[test]
    fn test_main_axis_padding_vertical() {
        let padding = EdgeInsets::new(10.0, 20.0, 10.0, 30.0);
        let sliver = RenderSliverEdgeInsetsPadding::new(padding);

        let (leading, trailing) = sliver.main_axis_padding(Axis::Vertical);
        assert_eq!(leading, 20.0);  // top
        assert_eq!(trailing, 30.0); // bottom
    }

    #[test]
    fn test_main_axis_padding_horizontal() {
        let padding = EdgeInsets::new(10.0, 20.0, 15.0, 30.0);
        let sliver = RenderSliverEdgeInsetsPadding::new(padding);

        let (leading, trailing) = sliver.main_axis_padding(Axis::Horizontal);
        assert_eq!(leading, 10.0);  // left
        assert_eq!(trailing, 15.0); // right
    }

    #[test]
    fn test_cross_axis_padding_vertical() {
        let padding = EdgeInsets::new(10.0, 20.0, 15.0, 30.0);
        let sliver = RenderSliverEdgeInsetsPadding::new(padding);

        let cross = sliver.cross_axis_padding(Axis::Vertical);
        assert_eq!(cross, 25.0); // left + right = 10 + 15
    }

    #[test]
    fn test_cross_axis_padding_horizontal() {
        let padding = EdgeInsets::new(10.0, 20.0, 15.0, 30.0);
        let sliver = RenderSliverEdgeInsetsPadding::new(padding);

        let cross = sliver.cross_axis_padding(Axis::Horizontal);
        assert_eq!(cross, 50.0); // top + bottom = 20 + 30
    }

    #[test]
    fn test_child_constraints() {
        let padding = EdgeInsets::new(0.0, 40.0, 0.0, 20.0);
        let sliver = RenderSliverEdgeInsetsPadding::new(padding);

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

        let child_constraints = sliver.child_constraints(&constraints);

        // Scroll offset adjusted by leading padding
        assert_eq!(child_constraints.scroll_offset, 60.0); // 100 - 40
        // Remaining paint extent reduced by total padding
        assert_eq!(child_constraints.remaining_paint_extent, 540.0); // 600 - 40 - 20
        // Cross axis unchanged (no horizontal padding)
        assert_eq!(child_constraints.cross_axis_extent, 400.0);
    }

    #[test]
    fn test_calculate_sliver_geometry() {
        let padding = EdgeInsets::new(0.0, 40.0, 0.0, 20.0);
        let sliver = RenderSliverEdgeInsetsPadding::new(padding);

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

        // Simulate child geometry
        let child_geometry = SliverGeometry {
            scroll_extent: 200.0,
            paint_extent: 200.0,
            layout_extent: 200.0,
            max_paint_extent: 200.0,
            visible: true,
            visible_fraction: 1.0,
            paint_origin: 0.0,
            cross_axis_extent: 400.0,
            cache_extent: 200.0,
            has_visual_overflow: false,
            hit_test_extent: Some(200.0),
            scroll_offset_correction: None,
            max_scroll_obsolescence: 0.0,
        };

        let geometry = sliver.calculate_sliver_geometry(&constraints, child_geometry);

        // Scroll extent includes padding
        assert_eq!(geometry.scroll_extent, 260.0); // 200 + 40 + 20
        // Paint extent includes padding
        assert_eq!(geometry.paint_extent, 260.0); // 200 + 40 + 20
    }
}
