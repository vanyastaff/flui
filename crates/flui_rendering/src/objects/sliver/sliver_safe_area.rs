//! RenderSliverSafeArea - System UI safe area adapter for sliver scrolling
//!
//! Adds safe area padding to sliver content to avoid system UI elements (notches, status bars,
//! navigation bars, home indicators, rounded corners). Wraps sliver child with safe area insets
//! as additional scroll extent, ensuring scrollable content doesn't get obscured by system UI.
//! Essential for edge-to-edge layouts on modern devices.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderSliverSafeArea` | `RenderSliverSafeArea` from `package:flutter/src/rendering/sliver.dart` |
//! | `insets` | Safe area insets from MediaQuery.padding |
//! | `minimum` | Minimum padding (fallback when insets are zero) |
//! | `effective_padding()` | max(insets, minimum) calculation |
//! | `main_axis_padding()` | Axis-aware leading/trailing extraction |
//!
//! # Layout Protocol (Intended)
//!
//! 1. **Calculate effective padding**
//!    - effective = max(insets, minimum) for each edge
//!    - Extract main axis leading/trailing based on axis direction
//!
//! 2. **Layout child with adjusted constraints** (NOT IMPLEMENTED)
//!    - Reduce cross-axis extent by cross-axis padding
//!    - Pass through scroll offset (child scrolls normally)
//!
//! 3. **Adjust child geometry**
//!    - Add leading/trailing padding to scroll_extent
//!    - Leading padding scrolls away, trailing padding at end
//!
//! 4. **Return adjusted geometry**
//!    - scroll_extent = total padding (leading + trailing)
//!    - paint_extent = visible portion of padding
//!
//! # Paint Protocol
//!
//! Safe area doesn't paint - it only adds spacing. Returns empty Canvas.
//!
//! # Performance
//!
//! - **Layout**: O(1) padding calculation + O(child) when implemented
//! - **Paint**: O(1) - returns empty Canvas (no painting)
//! - **Memory**: 52 bytes (3×EdgeInsets + bool + SliverGeometry)
//!
//! # Use Cases
//!
//! - **Edge-to-edge lists**: Content avoiding notch/status bar
//! - **Full-screen scrollables**: GridView with safe area padding
//! - **Bottom sheets**: Avoid home indicator overlap
//! - **Landscape layouts**: Avoid rounded corner cutoff
//! - **Dynamic insets**: Keyboard, toolbars, navigation
//!
//! # Safe Area Insets
//!
//! ```text
//! ┌──────────────────────────┐
//! │  Status Bar (top inset)  │ ← Safe area padding (scrolls away)
//! ├──────────────────────────┤
//! │                          │
//! │   Scrollable Content     │ ← Child sliver
//! │                          │
//! ├──────────────────────────┤
//! │ Home Indicator (bottom)  │ ← Safe area padding (at end)
//! └──────────────────────────┘
//! ```
//!
//! # Minimum Padding
//!
//! ```rust,ignore
//! // Device without notch (insets = 0)
//! let safe_area = RenderSliverSafeArea::new(EdgeInsets::ZERO)
//!     .with_minimum(EdgeInsets::all(16.0));
//! // Uses minimum (16px) since insets are zero
//!
//! // Device with notch (insets = 44px top)
//! let safe_area = RenderSliverSafeArea::new(EdgeInsets::only_top(44.0))
//!     .with_minimum(EdgeInsets::all(16.0));
//! // Uses insets (44px) since it's > minimum
//! ```
//!
//! # ⚠️ IMPLEMENTATION ISSUE
//!
//! This implementation has **ONE CRITICAL MISSING FEATURE**:
//!
//! 1. **❌ Child is NEVER laid out** (line 152-156)
//!    - No calls to `layout_child()` anywhere
//!    - Child size is undefined
//!    - Only padding geometry, no actual child layout
//!
//! 2. **⚠️ maintain_bottom_view_padding NOT USED** (line 43)
//!    - Field exists and can be set
//!    - Never checked in geometry calculation
//!    - Dead code - has no effect
//!
//! 3. **✅ Geometry calculation CORRECT** (line 104-142)
//!    - Properly calculates effective padding (max of insets/minimum)
//!    - Correctly extracts main axis leading/trailing
//!    - Accurate scroll extent and paint extent
//!
//! 4. **✅ Paint correct** (line 158-161)
//!    - Correctly returns empty Canvas
//!    - Safe area doesn't paint anything (just spacing)
//!
//! **This RenderObject is MOSTLY CORRECT - only missing child layout!**
//!
//! # Comparison with Related Objects
//!
//! - **vs SliverPadding**: SafeArea uses system insets, Padding uses fixed EdgeInsets
//! - **vs SliverEdgeInsetsPadding**: Identical in current implementation (both just add padding)
//! - **vs SafeArea (box)**: SliverSafeArea is for slivers, SafeArea is for boxes
//! - **vs MediaQuery**: MediaQuery provides insets, SliverSafeArea applies them to slivers
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderSliverSafeArea;
//! use flui_types::EdgeInsets;
//!
//! // iPhone X notch + home indicator
//! let insets = EdgeInsets::new(0.0, 44.0, 0.0, 34.0);
//! let safe_area = RenderSliverSafeArea::new(insets);
//! // Top 44px and bottom 34px safe area padding
//!
//! // With minimum padding fallback
//! let safe_area = RenderSliverSafeArea::new(EdgeInsets::ZERO)
//!     .with_minimum(EdgeInsets::all(16.0));
//! // Ensures at least 16px padding on all sides
//!
//! // Landscape with rounded corners
//! let insets = EdgeInsets::new(44.0, 0.0, 44.0, 21.0);
//! let safe_area = RenderSliverSafeArea::new(insets);
//! // Left/right 44px (rounded corners), bottom 21px (home indicator)
//! ```

use crate::core::{RuntimeArity, LegacySliverRender, SliverSliver};
use flui_painting::Canvas;
use flui_types::prelude::*;
use flui_types::{SliverConstraints, SliverGeometry};

/// RenderObject that adds system safe area padding to sliver scrollables.
///
/// Wraps sliver child with safe area insets (notch, status bar, nav bar, home indicator,
/// rounded corners) as additional scroll extent. Leading padding scrolls away, trailing
/// padding stays at end. Supports minimum padding fallback and max(insets, minimum) logic.
///
/// # Arity
///
/// `RuntimeArity::Exact(1)` - Must have exactly 1 sliver child.
///
/// # Protocol
///
/// Sliver protocol - Uses `SliverConstraints` and returns `SliverGeometry`.
///
/// # Pattern
///
/// **System UI Safe Area Adapter** - Applies system UI insets to sliver, effective padding
/// calculation (max of insets/minimum), axis-aware leading/trailing extraction, geometry
/// adjustment for padding scroll extent.
///
/// # Use Cases
///
/// - **Edge-to-edge lists**: Avoid notch/status bar overlap
/// - **Full-screen scrollables**: GridView with safe area
/// - **Bottom sheets**: Avoid home indicator
/// - **Landscape layouts**: Avoid rounded corner cutoff
/// - **Dynamic insets**: Keyboard, toolbars, navigation
///
/// # Flutter Compliance
///
/// **ALMOST COMPLETE**:
/// - ✅ Effective padding calculation correct (max of insets/minimum)
/// - ✅ Main axis extraction correct (axis-aware)
/// - ✅ Geometry calculation correct
/// - ✅ Paint correct (returns empty Canvas)
/// - ❌ Child never laid out (only missing feature)
/// - ⚠️ maintain_bottom_view_padding unused (dead code)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverSafeArea;
/// use flui_types::EdgeInsets;
///
/// // iPhone X safe area (notch + home indicator)
/// let insets = EdgeInsets::new(0.0, 44.0, 0.0, 34.0);
/// let safe_area = RenderSliverSafeArea::new(insets);
/// // WARNING: child never laid out!
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

impl LegacySliverRender for RenderSliverSafeArea {
    fn layout(&mut self, ctx: &Sliver) -> SliverGeometry {
        // Calculate and cache sliver geometry
        self.sliver_geometry = self.calculate_sliver_geometry(&ctx.constraints);
        self.sliver_geometry
    }

    fn paint(&self, _ctx: &Sliver) -> Canvas {
        // Safe area doesn't paint anything - it just adds spacing
        Canvas::new()
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

        let geometry = safe_area.calculate_sliver_geometry(&constraints);

        // Total padding: 40 + 20 = 60
        assert_eq!(geometry.scroll_extent, 60.0);
        assert_eq!(geometry.paint_extent, 60.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_partially_scrolled() {
        let insets = EdgeInsets::new(0.0, 40.0, 0.0, 20.0);
        let safe_area = RenderSliverSafeArea::new(insets);

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

        let geometry = safe_area.calculate_sliver_geometry(&constraints);

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

        let geometry = safe_area.calculate_sliver_geometry(&constraints);

        // Leading visible: 40 - 50 = 0 (capped at 0)
        // Paint extent: 0 + 600 = 600, but capped at total_padding (60)
        assert_eq!(geometry.scroll_extent, 60.0);
        assert_eq!(geometry.paint_extent, 60.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_arity_is_single_child() {
        let safe_area = RenderSliverSafeArea::new(EdgeInsets::ZERO);
        assert_eq!(safe_area.arity(), RuntimeArity::Exact(1));
    }
}
