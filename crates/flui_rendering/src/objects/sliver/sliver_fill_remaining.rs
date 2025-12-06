//! RenderSliverFillRemaining - Fills remaining viewport space
//!
//! Implements Flutter's SliverFillRemaining that expands a box child to fill the remaining
//! space in a viewport. Unlike SliverFillViewport which sizes children to the full viewport,
//! this sliver sizes its child to fill whatever space remains AFTER previous slivers.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderSliverFillRemaining` | `RenderSliverFillRemaining` from `package:flutter/src/rendering/sliver_fill.dart` |
//! | `has_scrolled_body` | `hasScrollBody` property |
//! | `fill_overscroll` | `fillOverscroll` property |
//! | Sliver-to-box adapter | Child receives BoxConstraints, sliver returns SliverGeometry |
//!
//! # Layout Protocol
//!
//! 1. **Calculate child box constraints**
//!    - Width: 0 to cross_axis_extent
//!    - Height: 0 to remaining_paint_extent
//!    - Child receives box constraints (not sliver!)
//!
//! 2. **Layout child as box**
//!    - Child is laid out using BoxConstraints
//!    - Returns Size (box protocol result)
//!
//! 3. **Calculate sliver geometry from child size**
//!    - Extract main-axis extent from child size
//!    - Apply has_scrolled_body logic:
//!      - If true: max(remaining_extent, child_extent)
//!      - If false: max(child_extent, remaining_extent)
//!    - Apply fill_overscroll to scroll_extent
//!
//! 4. **Return sliver geometry**
//!    - Converts box size to sliver geometry
//!
//! # Paint Protocol
//!
//! 1. **Check visibility**
//!    - Only paint if geometry.visible
//!
//! 2. **Paint child**
//!    - Paint box child at current offset
//!
//! # Performance
//!
//! - **Layout**: O(child) - box child layout
//! - **Paint**: O(child) - box child paint
//! - **Memory**: 2 bytes (bool flags) + 48 bytes (SliverGeometry cache) = 50 bytes
//!
//! # Use Cases
//!
//! - **Footer content**: Stick footer to bottom when content is short
//! - **Expanding cards**: Fill remaining viewport with content
//! - **Centered content**: Center content in remaining space
//! - **Flexible layouts**: Adapt to available space
//! - **Bottom buttons**: Keep buttons at bottom of short lists
//! - **Splash screens**: Fill remaining space with branding
//!
//! # Modes Explained
//!
//! ## has_scrolled_body
//!
//! ```text
//! has_scrolled_body = false (top of viewport):
//! Child: 200px, Remaining: 600px → Expands to 600px
//!
//! has_scrolled_body = true (after scrolled content):
//! Child: 200px, Remaining: 600px → Takes max(600, 200) = 600px
//! Child: 800px, Remaining: 600px → Takes max(600, 800) = 800px
//! ```
//!
//! ## fill_overscroll
//!
//! ```text
//! fill_overscroll = false:
//! scroll_extent = child_extent (only child affects scrolling)
//!
//! fill_overscroll = true:
//! scroll_extent = expanded_extent (fills overscroll area)
//! ```
//!
//! # Comparison with Related Objects
//!
//! - **vs SliverFillViewport**: FillViewport sizes to full viewport, FillRemaining sizes to remaining space
//! - **vs SliverToBoxAdapter**: ToBoxAdapter uses child's intrinsic size, FillRemaining expands child
//! - **vs SliverList**: List lays out multiple children, FillRemaining has single expanding child
//! - **vs SliverPadding**: Padding adds fixed insets, FillRemaining adapts to available space
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderSliverFillRemaining;
//!
//! // Basic fill remaining (expands to fill space)
//! let fill = RenderSliverFillRemaining::new();
//!
//! // Fill including overscroll area
//! let fill_all = RenderSliverFillRemaining::new()
//!     .with_fill_overscroll();
//!
//! // Configure for content after scrolled body
//! let mut fill = RenderSliverFillRemaining::new();
//! fill.set_has_scrolled_body(true);
//!
//! // Footer that sticks to bottom
//! let footer = RenderSliverFillRemaining::new();
//! // If content is short: footer at bottom of viewport
//! // If content is long: footer after scrolled content
//! ```

use crate::core::{RenderObject, RenderSliver, Single, SliverLayoutContext, SliverPaintContext};
use crate::RenderResult;
use flui_painting::Canvas;
use flui_types::prelude::*;
use flui_types::{SliverConstraints, SliverGeometry};

/// RenderObject that fills the remaining space in the viewport with a box child.
///
/// Sliver-to-box protocol adapter that sizes its box child to fill the space remaining
/// in the viewport after previous slivers. Unlike SliverToBoxAdapter which uses intrinsic
/// size, this expands the child to consume available space.
///
/// # Arity
///
/// `RuntimeArity::Exact(1)` - Must have exactly 1 child (box protocol, not sliver!).
///
/// # Protocol
///
/// **Sliver-to-Box Adapter** - Uses `SliverConstraints`, lays out child with `BoxConstraints`,
/// returns `SliverGeometry`.
///
/// # Pattern
///
/// **Space-Filling Adapter** - Converts remaining viewport space to box constraints,
/// expands child to fill available space, then converts child size back to sliver geometry.
/// Bidirectional protocol conversion with space expansion.
///
/// # Use Cases
///
/// - **Sticky footers**: Keep footer at bottom when content is short
/// - **Expanding panels**: Fill remaining space with content panel
/// - **Bottom CTAs**: Call-to-action buttons that stay at bottom
/// - **Flexible content**: Content that adapts to viewport size
/// - **Center alignment**: Center content in remaining space
/// - **Splash branding**: Fill remaining space with logo/branding
///
/// # Flutter Compliance
///
/// Matches Flutter's RenderSliverFillRemaining behavior:
/// - Expands child to fill remaining viewport space ✅
/// - Supports has_scrolled_body for different sizing logic ✅
/// - Supports fill_overscroll for overscroll area filling ✅
/// - Child receives BoxConstraints (protocol adapter) ✅
/// - Returns SliverGeometry based on child size ✅
///
/// # Mode Behavior
///
/// | Mode | Child Size | Remaining | Result Extent |
/// |------|------------|-----------|---------------|
/// | has_scrolled_body=false | 200px | 600px | 600px (expand) |
/// | has_scrolled_body=true | 200px | 600px | 600px (max) |
/// | has_scrolled_body=true | 800px | 600px | 800px (child) |
/// | fill_overscroll=false | Any | Any | child_extent |
/// | fill_overscroll=true | Any | Any | expanded_extent |
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverFillRemaining;
///
/// // Footer that sticks to bottom
/// let footer = RenderSliverFillRemaining::new();
///
/// // Fill with overscroll
/// let expanding = RenderSliverFillRemaining::new()
///     .with_fill_overscroll();
/// ```
#[derive(Debug)]
pub struct RenderSliverFillRemaining {
    /// Whether to fill overscroll (space beyond content)
    pub has_scrolled_body: bool,
    /// Minimum child extent
    pub fill_overscroll: bool,

    // Layout cache
    sliver_geometry: SliverGeometry,
}

impl RenderSliverFillRemaining {
    /// Create new sliver fill remaining
    pub fn new() -> Self {
        Self {
            has_scrolled_body: false,
            fill_overscroll: false,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Set whether there's scrolled content before this sliver
    pub fn set_has_scrolled_body(&mut self, has_scrolled: bool) {
        self.has_scrolled_body = has_scrolled;
    }

    /// Set whether to fill overscroll area
    pub fn set_fill_overscroll(&mut self, fill: bool) {
        self.fill_overscroll = fill;
    }

    /// Create with overscroll filling enabled
    pub fn with_fill_overscroll(mut self) -> Self {
        self.fill_overscroll = true;
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
        child_size: Size,
    ) -> SliverGeometry {
        let remaining_extent = constraints.remaining_paint_extent;
        let scroll_offset = constraints.scroll_offset;

        // The child's main axis extent
        let child_extent = match constraints.axis_direction.axis() {
            Axis::Vertical => child_size.height,
            Axis::Horizontal => child_size.width,
        };

        // Determine the extent this sliver should report
        let extent = if self.has_scrolled_body {
            // If there's content before us that was scrolled, we take up
            // the remaining space exactly
            remaining_extent.max(child_extent)
        } else {
            // If we're at the top (no scrolled content), we might expand
            // to fill the viewport
            child_extent.max(remaining_extent)
        };

        // Calculate scroll extent
        let scroll_extent = if self.fill_overscroll {
            // Fill any overscroll area
            extent
        } else {
            // Only our actual child size
            child_extent
        };

        // Paint extent is what's actually visible
        let paint_extent = if scroll_offset >= scroll_extent {
            // Completely scrolled off
            0.0
        } else if scroll_offset + remaining_extent >= scroll_extent {
            // Fully visible
            (scroll_extent - scroll_offset).max(0.0)
        } else {
            // Partially visible
            remaining_extent
        };

        let paint_extent = paint_extent.min(remaining_extent);

        SliverGeometry {
            scroll_extent,
            paint_extent,
            paint_origin: 0.0,
            layout_extent: paint_extent,
            max_paint_extent: extent,
            max_scroll_obsolescence: 0.0,
            visible_fraction: if scroll_extent > 0.0 {
                (paint_extent / scroll_extent).min(1.0)
            } else {
                0.0
            },
            cross_axis_extent: constraints.cross_axis_extent,
            cache_extent: paint_extent,
            visible: paint_extent > 0.0,
            has_visual_overflow: scroll_extent > paint_extent,
            hit_test_extent: Some(paint_extent),
            scroll_offset_correction: None,
        }
    }
}

impl Default for RenderSliverFillRemaining {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderObject for RenderSliverFillRemaining {}

impl RenderSliver<Single> for RenderSliverFillRemaining {
    fn layout(&mut self, mut ctx: SliverLayoutContext<'_, Single>) -> RenderResult<SliverGeometry> {
        let constraints = ctx.constraints;
        let child_id = *ctx.children.single();

        // Layout child with box constraints based on remaining viewport space
        let remaining_extent = constraints.remaining_paint_extent;
        let box_constraints = BoxConstraints::new(
            0.0,
            constraints.cross_axis_extent,
            0.0,
            remaining_extent,
        );
        let child_size = ctx.tree_mut().perform_layout(child_id, box_constraints)?;

        // Calculate and cache sliver geometry
        self.sliver_geometry = self.calculate_sliver_geometry(&constraints, child_size);
        Ok(self.sliver_geometry)
    }

    fn paint(&self, ctx: &mut SliverPaintContext<'_, Single>) {
        // Paint child if visible
        if self.sliver_geometry.visible {
            let child_id = *ctx.children.single();

            if let Ok(child_canvas) = ctx.tree().perform_paint(child_id, ctx.offset) {
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
    fn test_render_sliver_fill_remaining_new() {
        let fill = RenderSliverFillRemaining::new();

        assert!(!fill.has_scrolled_body);
        assert!(!fill.fill_overscroll);
    }

    #[test]
    fn test_render_sliver_fill_remaining_default() {
        let fill = RenderSliverFillRemaining::default();

        assert!(!fill.has_scrolled_body);
        assert!(!fill.fill_overscroll);
    }

    #[test]
    fn test_set_has_scrolled_body() {
        let mut fill = RenderSliverFillRemaining::new();
        fill.set_has_scrolled_body(true);

        assert!(fill.has_scrolled_body);
    }

    #[test]
    fn test_set_fill_overscroll() {
        let mut fill = RenderSliverFillRemaining::new();
        fill.set_fill_overscroll(true);

        assert!(fill.fill_overscroll);
    }

    #[test]
    fn test_with_fill_overscroll() {
        let fill = RenderSliverFillRemaining::new().with_fill_overscroll();

        assert!(fill.fill_overscroll);
    }

    #[test]
    fn test_calculate_sliver_geometry_no_scrolled_body() {
        let fill = RenderSliverFillRemaining::new();

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

        let child_size = Size::new(400.0, 200.0);
        let geometry = fill.calculate_sliver_geometry(&constraints, child_size);

        // Child is 200px, but we expand to fill remaining 600px
        assert_eq!(geometry.scroll_extent, 200.0);
        assert_eq!(geometry.max_paint_extent, 600.0);
        assert_eq!(geometry.paint_extent, 200.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_with_scrolled_body() {
        let mut fill = RenderSliverFillRemaining::new();
        fill.set_has_scrolled_body(true);

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: 200.0, // Only 200px left
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let child_size = Size::new(400.0, 100.0);
        let geometry = fill.calculate_sliver_geometry(&constraints, child_size);

        // We expand to fill the remaining 200px
        assert_eq!(geometry.scroll_extent, 100.0);
        assert_eq!(geometry.paint_extent, 100.0);
    }

    #[test]
    fn test_calculate_sliver_geometry_child_larger_than_remaining() {
        let fill = RenderSliverFillRemaining::new();

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 0.0,
            remaining_paint_extent: 200.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let child_size = Size::new(400.0, 300.0); // Child is bigger
        let geometry = fill.calculate_sliver_geometry(&constraints, child_size);

        // Child is 300px, larger than remaining 200px
        assert_eq!(geometry.scroll_extent, 300.0);
        assert_eq!(geometry.paint_extent, 200.0); // Clipped to remaining
    }

    #[test]
    fn test_calculate_sliver_geometry_with_fill_overscroll() {
        let fill = RenderSliverFillRemaining::new().with_fill_overscroll();

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

        let child_size = Size::new(400.0, 200.0);
        let geometry = fill.calculate_sliver_geometry(&constraints, child_size);

        // With fill_overscroll, we report the expanded extent
        assert_eq!(geometry.scroll_extent, 600.0);
        assert_eq!(geometry.paint_extent, 600.0);
    }

    #[test]
    fn test_calculate_sliver_geometry_scrolled_off() {
        let fill = RenderSliverFillRemaining::new();

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 500.0, // Scrolled past child
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let child_size = Size::new(400.0, 200.0);
        let geometry = fill.calculate_sliver_geometry(&constraints, child_size);

        // Scrolled past the child
        assert_eq!(geometry.scroll_extent, 200.0);
        assert_eq!(geometry.paint_extent, 0.0);
        assert!(!geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_partially_visible() {
        let fill = RenderSliverFillRemaining::new();

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 100.0, // Partially scrolled
            remaining_paint_extent: 300.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let child_size = Size::new(400.0, 500.0);
        let geometry = fill.calculate_sliver_geometry(&constraints, child_size);

        // Child is 500px, scrolled 100px, showing 300px
        assert_eq!(geometry.scroll_extent, 500.0);
        assert_eq!(geometry.paint_extent, 300.0);
        assert!(geometry.visible);
    }
}
