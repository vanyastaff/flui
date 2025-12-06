//! RenderSliverConstrainedCrossAxis - Constrains cross-axis extent for slivers
//!
//! Implements cross-axis width/height limiting for slivers. While slivers scroll on their main
//! axis, this widget constrains the perpendicular cross-axis (width for vertical scroll, height
//! for horizontal scroll). Essential for responsive design and Material Design width constraints.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderSliverConstrainedCrossAxis` | `RenderSliverConstrainedCrossAxis` from `package:flutter/src/rendering/sliver.dart` |
//! | `max_extent` property | `maxExtent` property |
//! | Cross-axis clamping | `min(parentCrossAxis, maxExtent)` logic |
//!
//! # Layout Protocol
//!
//! 1. **Calculate constrained cross-axis**
//!    - `constrained = min(parent_cross_axis_extent, max_extent)`
//!
//! 2. **Create child constraints with reduced cross-axis**
//!    - All fields pass through unchanged except cross_axis_extent
//!    - Main-axis constraints (scroll_offset, paint_extent) unchanged
//!
//! 3. **Layout child with constrained extent**
//!    - Child receives reduced cross-axis constraint
//!
//! 4. **Return geometry with constrained cross-axis**
//!    - All fields from child except cross_axis_extent
//!    - cross_axis_extent set to constrained value
//!
//! # Paint Protocol
//!
//! 1. **Paint child at current offset**
//!    - Child painted normally
//!    - Constraint only affects layout, not paint
//!
//! # Performance
//!
//! - **Layout**: O(child) - pass-through with constraint modification
//! - **Paint**: O(child) - pass-through proxy
//! - **Memory**: 4 bytes (f32 max_extent) + 48 bytes (SliverGeometry cache)
//!
//! # Use Cases
//!
//! - **Responsive lists**: Limit list width on wide screens
//! - **Centered content**: Narrow scrollable content in wide viewports
//! - **Material Design**: Enforce maximum width constraints (600dp for lists)
//! - **Multi-column layouts**: Control column widths in responsive grids
//! - **Reading optimization**: Limit line length for readability
//!
//! # Comparison with Related Objects
//!
//! - **vs SliverPadding**: Padding adds insets, ConstrainedCrossAxis limits max width
//! - **vs BoxConstrainedBox**: ConstrainedCrossAxis is for slivers, ConstrainedBox is for boxes
//! - **vs SliverCrossAxisGroup**: Group manages multiple slivers, Constrained limits single one
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderSliverConstrainedCrossAxis;
//!
//! // Limit list to 600px width (Material Design)
//! let constrained_list = RenderSliverConstrainedCrossAxis::new(600.0);
//!
//! // No constraint (infinite - default)
//! let unconstrained = RenderSliverConstrainedCrossAxis::default();
//!
//! // Responsive: narrow on mobile, capped on desktop
//! let mut responsive = RenderSliverConstrainedCrossAxis::new(800.0);
//! // ... on screen resize ...
//! responsive.set_max_extent(1200.0);
//! ```

use crate::core::{RuntimeArity, LegacySliverRender, SliverSliver};
use flui_painting::Canvas;
use flui_types::{SliverConstraints, SliverGeometry};

/// RenderObject that constrains the cross-axis extent of sliver content.
///
/// While slivers primarily deal with main-axis scrolling (vertical or horizontal),
/// they also have a perpendicular cross-axis (width for vertical scroll, height for
/// horizontal scroll). This RenderObject constrains that cross-axis extent to a
/// maximum value, enabling responsive layouts that adapt to wide viewports.
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
/// **Cross-Axis Constraint Proxy** - Passes constraints to child with reduced
/// cross_axis_extent (clamped to max_extent), then returns child geometry with
/// constrained cross-axis. Layout and paint are otherwise transparent.
///
/// # Use Cases
///
/// - **Responsive lists**: Limit list width on desktop while full-width on mobile
/// - **Centered content**: Narrow scrollable content in wide viewports (e.g., reading apps)
/// - **Material Design**: Enforce maximum width constraints (600dp recommendation)
/// - **Multi-column layouts**: Control column widths in responsive grid systems
/// - **Reading optimization**: Limit line length for better readability
/// - **Adaptive design**: Different max widths for different breakpoints
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderSliverConstrainedCrossAxis behavior:
/// - Clamps cross_axis_extent to min(parent, max_extent) ✅
/// - Passes all other constraint fields unchanged ✅
/// - Returns child geometry with constrained cross-axis ✅
/// - Preserves child's main-axis geometry (scroll_extent, paint_extent) ✅
/// - Paint is transparent proxy ✅
///
/// # Behavior Details
///
/// | Scenario | parent_cross_axis | max_extent | child_cross_axis | Result |
/// |----------|-------------------|------------|------------------|--------|
/// | No constraint | 1000.0 | ∞ | 1000.0 | Pass through |
/// | Within limit | 400.0 | 600.0 | 400.0 | No change |
/// | Exceeds limit | 1000.0 | 600.0 | 600.0 | Clamped to max |
/// | Exact match | 600.0 | 600.0 | 600.0 | At limit |
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverConstrainedCrossAxis;
///
/// // Material Design: limit list to 600px width
/// let constrained_list = RenderSliverConstrainedCrossAxis::new(600.0);
///
/// // Responsive design with breakpoints
/// let mut responsive = RenderSliverConstrainedCrossAxis::new(800.0);
/// // On screen resize...
/// if viewport_width > 1200.0 {
///     responsive.set_max_extent(1000.0); // Wide desktop
/// } else {
///     responsive.set_max_extent(600.0);  // Tablet/mobile
/// }
///
/// // No constraint (pass through)
/// let unconstrained = RenderSliverConstrainedCrossAxis::default(); // f32::INFINITY
/// ```
#[derive(Debug)]
pub struct RenderSliverConstrainedCrossAxis {
    /// Maximum cross-axis extent
    pub max_extent: f32,

    // Layout cache
    sliver_geometry: SliverGeometry,
}

impl RenderSliverConstrainedCrossAxis {
    /// Create new sliver constrained cross axis
    ///
    /// # Arguments
    /// * `max_extent` - Maximum cross-axis extent
    pub fn new(max_extent: f32) -> Self {
        Self {
            max_extent,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Set maximum extent
    pub fn set_max_extent(&mut self, max_extent: f32) {
        self.max_extent = max_extent;
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

    /// Calculate child constraints with limited cross-axis extent
    fn child_constraints(&self, constraints: &SliverConstraints) -> SliverConstraints {
        let constrained_cross_axis = constraints.cross_axis_extent.min(self.max_extent);

        SliverConstraints {
            axis_direction: constraints.axis_direction,
            grow_direction_reversed: constraints.grow_direction_reversed,
            scroll_offset: constraints.scroll_offset,
            remaining_paint_extent: constraints.remaining_paint_extent,
            cross_axis_extent: constrained_cross_axis,
            cross_axis_direction: constraints.cross_axis_direction,
            viewport_main_axis_extent: constraints.viewport_main_axis_extent,
            remaining_cache_extent: constraints.remaining_cache_extent,
            cache_origin: constraints.cache_origin,
        }
    }

    /// Calculate sliver geometry from child
    fn calculate_sliver_geometry(
        &self,
        constraints: &SliverConstraints,
        child_geometry: SliverGeometry,
    ) -> SliverGeometry {
        // Pass through child geometry, but with constrained cross-axis
        let constrained_cross_axis = constraints.cross_axis_extent.min(self.max_extent);

        SliverGeometry {
            scroll_extent: child_geometry.scroll_extent,
            paint_extent: child_geometry.paint_extent,
            paint_origin: child_geometry.paint_origin,
            layout_extent: child_geometry.layout_extent,
            max_paint_extent: child_geometry.max_paint_extent,
            max_scroll_obsolescence: child_geometry.max_scroll_obsolescence,
            visible_fraction: child_geometry.visible_fraction,
            cross_axis_extent: constrained_cross_axis,
            cache_extent: child_geometry.cache_extent,
            visible: child_geometry.visible,
            has_visual_overflow: child_geometry.has_visual_overflow,
            hit_test_extent: child_geometry.hit_test_extent,
            scroll_offset_correction: child_geometry.scroll_offset_correction,
        }
    }
}

impl Default for RenderSliverConstrainedCrossAxis {
    fn default() -> Self {
        Self::new(f32::INFINITY) // No constraint by default
    }
}

impl LegacySliverRender for RenderSliverConstrainedCrossAxis {
    fn layout(&mut self, ctx: &Sliver) -> SliverGeometry {
        let constraints = &ctx.constraints;

        // Constrain cross-axis for child
        let child_constraints = self.child_constraints(constraints);

        // Layout child
        let child_geometry = if let Some(child_id) = ctx.children.try_single() {
            ctx.tree.layout_sliver_child(child_id, child_constraints)
        } else {
            SliverGeometry::default()
        };

        // Calculate and cache geometry with constrained cross-axis
        self.sliver_geometry = self.calculate_sliver_geometry(constraints, child_geometry);
        self.sliver_geometry
    }

    fn paint(&self, ctx: &Sliver) -> Canvas {
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
        RuntimeArity::Exact(1) // Single child sliver
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::layout::AxisDirection;

    #[test]
    fn test_render_sliver_constrained_cross_axis_new() {
        let constrained = RenderSliverConstrainedCrossAxis::new(600.0);

        assert_eq!(constrained.max_extent, 600.0);
    }

    #[test]
    fn test_render_sliver_constrained_cross_axis_default() {
        let constrained = RenderSliverConstrainedCrossAxis::default();

        assert_eq!(constrained.max_extent, f32::INFINITY);
    }

    #[test]
    fn test_set_max_extent() {
        let mut constrained = RenderSliverConstrainedCrossAxis::new(600.0);
        constrained.set_max_extent(800.0);

        assert_eq!(constrained.max_extent, 800.0);
    }

    #[test]
    fn test_child_constraints_within_limit() {
        let constrained = RenderSliverConstrainedCrossAxis::new(600.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: 800.0,
            cross_axis_extent: 400.0, // Less than max
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 800.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let child_constraints = constrained.child_constraints(&constraints);

        // Cross-axis should remain unchanged (400 < 600)
        assert_eq!(child_constraints.cross_axis_extent, 400.0);
    }

    #[test]
    fn test_child_constraints_exceeds_limit() {
        let constrained = RenderSliverConstrainedCrossAxis::new(600.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: 800.0,
            cross_axis_extent: 1000.0, // Exceeds max
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 800.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let child_constraints = constrained.child_constraints(&constraints);

        // Cross-axis should be clamped to max
        assert_eq!(child_constraints.cross_axis_extent, 600.0);
    }

    #[test]
    fn test_child_constraints_exactly_at_limit() {
        let constrained = RenderSliverConstrainedCrossAxis::new(600.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: 800.0,
            cross_axis_extent: 600.0, // Exactly at max
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 800.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let child_constraints = constrained.child_constraints(&constraints);

        // Cross-axis should be exactly max
        assert_eq!(child_constraints.cross_axis_extent, 600.0);
    }

    #[test]
    fn test_calculate_sliver_geometry() {
        let constrained = RenderSliverConstrainedCrossAxis::new(600.0);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: 800.0,
            cross_axis_extent: 1000.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 800.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let child_geometry = SliverGeometry {
            scroll_extent: 500.0,
            paint_extent: 500.0,
            layout_extent: 500.0,
            max_paint_extent: 500.0,
            visible: true,
            visible_fraction: 1.0,
            paint_origin: 0.0,
            cross_axis_extent: 1000.0, // Child reports unconstrained
            cache_extent: 500.0,
            has_visual_overflow: false,
            hit_test_extent: Some(500.0),
            scroll_offset_correction: None,
            max_scroll_obsolescence: 0.0,
        };

        let geometry = constrained.calculate_sliver_geometry(&constraints, child_geometry);

        // Cross-axis should be constrained
        assert_eq!(geometry.cross_axis_extent, 600.0);
        // Other properties pass through
        assert_eq!(geometry.scroll_extent, 500.0);
        assert_eq!(geometry.paint_extent, 500.0);
    }

    #[test]
    fn test_calculate_sliver_geometry_no_constraint() {
        let constrained = RenderSliverConstrainedCrossAxis::new(f32::INFINITY);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: 800.0,
            cross_axis_extent: 1000.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 800.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let child_geometry = SliverGeometry {
            scroll_extent: 500.0,
            paint_extent: 500.0,
            layout_extent: 500.0,
            max_paint_extent: 500.0,
            visible: true,
            visible_fraction: 1.0,
            paint_origin: 0.0,
            cross_axis_extent: 1000.0,
            cache_extent: 500.0,
            has_visual_overflow: false,
            hit_test_extent: Some(500.0),
            scroll_offset_correction: None,
            max_scroll_obsolescence: 0.0,
        };

        let geometry = constrained.calculate_sliver_geometry(&constraints, child_geometry);

        // Cross-axis should pass through unchanged
        assert_eq!(geometry.cross_axis_extent, 1000.0);
    }

    #[test]
    fn test_arity_is_single_child() {
        let constrained = RenderSliverConstrainedCrossAxis::new(600.0);
        assert_eq!(constrained.arity(), RuntimeArity::Exact(1));
    }
}
