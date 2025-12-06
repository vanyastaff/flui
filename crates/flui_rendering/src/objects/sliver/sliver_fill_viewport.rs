//! RenderSliverFillViewport - Sliver where each child fills the viewport
//!
//! Implements Flutter's SliverFillViewport pattern for creating full-page scrollable content.
//! Each child is sized to fill exactly viewport_fraction of the viewport's main axis extent.
//! Commonly used for page views, carousels, onboarding flows, and full-screen image galleries.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderSliverFillViewport` | `RenderSliverFillViewport` from `package:flutter/src/rendering/sliver_fill.dart` |
//! | `viewport_fraction` property | `viewportFraction` property (default 1.0) |
//! | Geometry calculation | Flutter's scroll extent calculation |
//! | Child sizing | Each child fills viewport_fraction * viewport_extent |
//!
//! # Layout Protocol (Intended)
//!
//! 1. **Calculate child extent**
//!    - `child_extent = viewport_main_axis_extent * viewport_fraction`
//!    - Typically viewport_fraction = 1.0 (full viewport)
//!    - Can be < 1.0 for partial viewport children (e.g., 0.8 for peek effect)
//!
//! 2. **Determine visible range**
//!    - Calculate which children are in viewport based on scroll_offset
//!    - `first_visible_index = floor(scroll_offset / child_extent)`
//!    - Layout visible children with BoxConstraints
//!
//! 3. **Layout each visible child**
//!    - Convert to BoxConstraints: `width = cross_axis_extent, height = child_extent`
//!    - Position child at `main_axis_offset = index * child_extent`
//!
//! 4. **Calculate sliver geometry**
//!    - scroll_extent: `child_count * child_extent`
//!    - paint_extent: min(total_extent - scroll_offset, remaining_paint_extent)
//!
//! # Paint Protocol (Intended)
//!
//! 1. **Determine visible children**
//!    - Calculate index range within viewport
//!
//! 2. **Paint each visible child**
//!    - Calculate child offset based on index
//!    - Paint child at calculated position
//!
//! # Performance
//!
//! - **Layout**: O(visible_children) - only layout children in viewport (when implemented)
//! - **Paint**: O(visible_children) - only paint children in viewport (when implemented)
//! - **Memory**: 4 bytes (f32 viewport_fraction) + 48 bytes (SliverGeometry) = 52 bytes
//! - **Viewport culling**: Automatically skips offscreen children (when implemented)
//!
//! # Use Cases
//!
//! - **Page views**: Full-screen page transitions (e.g., onboarding)
//! - **Image carousels**: Full-viewport image galleries
//! - **Story viewers**: Instagram/Snapchat-style story scrolling
//! - **Tutorial slides**: Full-screen instructional content
//! - **Partial viewport**: Set viewport_fraction < 1.0 for peek effect
//! - **Calendar pages**: Month-by-month scrolling views
//!
//! # ⚠️ CRITICAL IMPLEMENTATION ISSUES
//!
//! This implementation has **MAJOR INCOMPLETE FUNCTIONALITY**:
//!
//! 1. **❌ Children are NEVER laid out** (line 109-142)
//!    - No calls to `layout_child()` anywhere
//!    - Child sizes are undefined
//!    - Only geometry calculation, no actual layout
//!
//! 2. **❌ Paint not implemented** (line 144-153)
//!    - Returns empty canvas
//!    - TODO comment: "Paint visible children at their viewport-filling positions"
//!    - Children are never painted
//!
//! 3. **❌ Duplicate code** (line 109-142 vs 53-99)
//!    - `layout()` duplicates `calculate_sliver_geometry()` logic
//!    - Unused method that should be called
//!
//! 4. **❌ Dead code** (line 53-99)
//!    - `calculate_sliver_geometry()` method exists but is never called
//!    - Should be used by `layout()`
//!
//! **This RenderObject is a STUB - geometry only, no layout or paint!**
//!
//! # Comparison with Related Objects
//!
//! - **vs SliverFillRemaining**: FillRemaining fills remaining space, FillViewport fills viewport per child
//! - **vs SliverFixedExtentList**: FixedExtent has fixed child size, FillViewport sizes to viewport
//! - **vs PageView (widget)**: PageView uses SliverFillViewport internally
//! - **vs SliverList**: List has variable child sizes, FillViewport has uniform viewport-based sizing
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderSliverFillViewport;
//!
//! // Full-screen page view (each page fills viewport)
//! let page_view = RenderSliverFillViewport::new(1.0);
//! // Add pages as children - each will be sized to fill viewport
//!
//! // Partial viewport with peek effect (80% of viewport)
//! let peek_carousel = RenderSliverFillViewport::new(0.8);
//! // Users can see edges of adjacent items
//!
//! // Half-viewport children (2 items per screen)
//! let half_page = RenderSliverFillViewport::new(0.5);
//! ```

use flui_core::element::ElementTree;
use crate::core::{RuntimeArity, SliverSliverBoxPaintCtx, LegacySliverRender};
use flui_painting::Canvas;
use flui_types::{SliverConstraints, SliverGeometry};

/// RenderObject where each child fills a fraction of the viewport.
///
/// Implements full-page scrolling where each child occupies viewport_fraction of the
/// viewport's main axis extent. Commonly used for page views, carousels, onboarding
/// flows, and image galleries.
///
/// # Arity
///
/// `RuntimeArity::Variable` - Supports multiple box children (N ≥ 0).
///
/// # Protocol
///
/// **Sliver-to-Box Adapter** - Uses `SliverConstraints`, but children use **BoxConstraints**
/// and return **Size** (not sliver protocol). Similar to SliverFillRemaining.
///
/// # Pattern
///
/// **Viewport-Filling Multi-Child Layout** - Sizes each child to viewport_fraction * viewport_extent,
/// positions children sequentially along main axis, and calculates total scroll extent as
/// child_count * child_extent.
///
/// # Use Cases
///
/// - **Full-screen pages**: Onboarding flows with viewport_fraction = 1.0
/// - **Image carousels**: Full-viewport photo galleries
/// - **Peek carousels**: Set viewport_fraction = 0.8 to show edges of adjacent items
/// - **Story viewers**: Instagram-style vertical story scrolling
/// - **Tutorial slides**: Sequential instructional screens
/// - **Calendar pages**: Monthly calendar with horizontal swipe
///
/// # Flutter Compliance
///
/// **INCOMPLETE IMPLEMENTATION** - Major features missing:
/// - ❌ Child layout not implemented
/// - ❌ Paint not implemented
/// - ❌ Viewport culling not implemented
/// - ✅ Geometry calculation correct
///
/// # Implementation Status
///
/// **Current State (STUB):**
/// - ✅ Geometry calculation (scroll_extent, paint_extent)
/// - ❌ Child layout (no layout_child calls)
/// - ❌ Child paint (returns empty canvas)
/// - ❌ Viewport culling optimization
///
/// **Missing from Flutter:**
/// - Layout visible children with BoxConstraints
/// - Paint children at calculated positions
/// - Optimize by skipping offscreen children
///
/// **⚠️ WARNING**: This RenderObject currently only calculates geometry.
/// Children are never laid out or painted!
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderSliverFillViewport;
///
/// // Full-screen page view
/// let page_view = RenderSliverFillViewport::new(1.0);
/// // Note: Children won't actually render until layout/paint implemented!
///
/// // Peek effect carousel (80% viewport per item)
/// let peek_carousel = RenderSliverFillViewport::new(0.8);
/// ```
#[derive(Debug)]
pub struct RenderSliverFillViewport {
    /// Fraction of viewport each child should occupy (typically 1.0)
    pub viewport_fraction: f32,

    // Layout cache
    sliver_geometry: SliverGeometry,
}

impl RenderSliverFillViewport {
    /// Create new sliver fill viewport
    ///
    /// # Arguments
    /// * `viewport_fraction` - Fraction of viewport each child occupies (1.0 = full viewport)
    pub fn new(viewport_fraction: f32) -> Self {
        Self {
            viewport_fraction,
            sliver_geometry: SliverGeometry::default(),
        }
    }

    /// Set viewport fraction
    pub fn set_viewport_fraction(&mut self, fraction: f32) {
        self.viewport_fraction = fraction;
    }

    /// Get the sliver geometry from last layout
    pub fn geometry(&self) -> SliverGeometry {
        self.sliver_geometry
    }

    /// Calculate sliver geometry
    fn calculate_sliver_geometry(
        &self,
        constraints: &SliverConstraints,
        _tree: &ElementTree,
        children: &[flui_core::element::ElementId],
    ) -> SliverGeometry {
        if children.is_empty() {
            return SliverGeometry::default();
        }

        let scroll_offset = constraints.scroll_offset;
        let remaining_extent = constraints.remaining_paint_extent;
        let viewport_extent = constraints.viewport_main_axis_extent;

        // Each child takes up viewport_fraction * viewport_extent
        let child_extent = viewport_extent * self.viewport_fraction;

        // Total extent is child_extent * number of children
        let child_count = children.len();
        let total_extent = child_extent * child_count as f32;

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
            max_scroll_obsolescence: 0.0,
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

impl Default for RenderSliverFillViewport {
    fn default() -> Self {
        Self::new(1.0) // Default to filling entire viewport
    }
}

impl LegacySliverRender for RenderSliverFillViewport {
    fn layout(&mut self, ctx: &Sliver) -> SliverGeometry {
        let constraints = &ctx.constraints;

        // Each child fills the viewport based on viewport_fraction
        let child_extent = constraints.viewport_main_axis_extent * self.viewport_fraction;
        let child_count = ctx.children.as_slice().len();
        let total_extent = child_extent * child_count as f32;

        // Calculate visible portion
        let scroll_offset = constraints.scroll_offset;
        let remaining_extent = constraints.remaining_paint_extent;

        let paint_extent = (total_extent - scroll_offset).max(0.0).min(remaining_extent);

        SliverGeometry {
            scroll_extent: total_extent,
            paint_extent,
            paint_origin: 0.0,
            layout_extent: paint_extent,
            max_paint_extent: total_extent,
            max_scroll_obsolescence: 0.0,
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

    fn paint(&self, _ctx: &Sliver) -> Canvas {
        let canvas = Canvas::new();

        // Children are painted by viewport
        // Each child is painted at its calculated position

        // TODO: Paint visible children at their viewport-filling positions

        canvas
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> RuntimeArity {
        RuntimeArity::Variable // Multiple children
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::layout::AxisDirection;

    #[test]
    fn test_render_sliver_fill_viewport_new() {
        let viewport = RenderSliverFillViewport::new(1.0);

        assert_eq!(viewport.viewport_fraction, 1.0);
    }

    #[test]
    fn test_render_sliver_fill_viewport_default() {
        let viewport = RenderSliverFillViewport::default();

        assert_eq!(viewport.viewport_fraction, 1.0);
    }

    #[test]
    fn test_set_viewport_fraction() {
        let mut viewport = RenderSliverFillViewport::new(1.0);
        viewport.set_viewport_fraction(0.5);

        assert_eq!(viewport.viewport_fraction, 0.5);
    }

    #[test]
    fn test_calculate_sliver_geometry_empty() {
        let viewport = RenderSliverFillViewport::new(1.0);
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

        let geometry = viewport.calculate_sliver_geometry(&constraints, &tree, &children);

        assert_eq!(geometry.scroll_extent, 0.0);
        assert_eq!(geometry.paint_extent, 0.0);
        assert!(!geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_single_child_full_viewport() {
        let viewport = RenderSliverFillViewport::new(1.0);
        let tree = ElementTree::new();
        let children = vec![flui_core::element::ElementId::new(1)];

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

        let geometry = viewport.calculate_sliver_geometry(&constraints, &tree, &children);

        // 1 child * 600px = 600px
        assert_eq!(geometry.scroll_extent, 600.0);
        assert_eq!(geometry.paint_extent, 600.0);
        assert!(geometry.visible);
        assert_eq!(geometry.visible_fraction, 1.0);
    }

    #[test]
    fn test_calculate_sliver_geometry_multiple_children() {
        let viewport = RenderSliverFillViewport::new(1.0);
        let tree = ElementTree::new();
        let children = vec![
            flui_core::element::ElementId::new(1),
            flui_core::element::ElementId::new(2),
            flui_core::element::ElementId::new(3),
        ];

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

        let geometry = viewport.calculate_sliver_geometry(&constraints, &tree, &children);

        // 3 children * 600px = 1800px total
        assert_eq!(geometry.scroll_extent, 1800.0);
        // Only 600px visible (first child)
        assert_eq!(geometry.paint_extent, 600.0);
        assert!(geometry.visible);
        assert!((geometry.visible_fraction - 0.333).abs() < 0.01); // 600/1800 ≈ 0.33
    }

    #[test]
    fn test_calculate_sliver_geometry_half_viewport() {
        let viewport = RenderSliverFillViewport::new(0.5);
        let tree = ElementTree::new();
        let children = vec![
            flui_core::element::ElementId::new(1),
            flui_core::element::ElementId::new(2),
        ];

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

        let geometry = viewport.calculate_sliver_geometry(&constraints, &tree, &children);

        // 2 children * (600 * 0.5) = 600px total
        assert_eq!(geometry.scroll_extent, 600.0);
        assert_eq!(geometry.paint_extent, 600.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_scrolled() {
        let viewport = RenderSliverFillViewport::new(1.0);
        let tree = ElementTree::new();
        let children = vec![
            flui_core::element::ElementId::new(1),
            flui_core::element::ElementId::new(2),
            flui_core::element::ElementId::new(3),
        ];

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 600.0, // Scrolled past first child
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = viewport.calculate_sliver_geometry(&constraints, &tree, &children);

        // 3 children * 600px = 1800px total
        assert_eq!(geometry.scroll_extent, 1800.0);
        // From offset 600 to 1200 = 600px (second child fully visible)
        assert_eq!(geometry.paint_extent, 600.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_partially_scrolled() {
        let viewport = RenderSliverFillViewport::new(1.0);
        let tree = ElementTree::new();
        let children = vec![
            flui_core::element::ElementId::new(1),
            flui_core::element::ElementId::new(2),
        ];

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 300.0, // Halfway through first child
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = viewport.calculate_sliver_geometry(&constraints, &tree, &children);

        // 2 children * 600px = 1200px total
        assert_eq!(geometry.scroll_extent, 1200.0);
        // From offset 300 to 900 = 600px
        // (half of first child + half of second child)
        assert_eq!(geometry.paint_extent, 600.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_calculate_sliver_geometry_scrolled_off() {
        let viewport = RenderSliverFillViewport::new(1.0);
        let tree = ElementTree::new();
        let children = vec![
            flui_core::element::ElementId::new(1),
            flui_core::element::ElementId::new(2),
        ];

        let constraints = SliverConstraints {
            axis_direction: AxisDirection::TopToBottom,
            grow_direction_reversed: false,
            scroll_offset: 2000.0, // Scrolled past all children
            remaining_paint_extent: 600.0,
            cross_axis_extent: 400.0,
            cross_axis_direction: AxisDirection::LeftToRight,
            viewport_main_axis_extent: 600.0,
            remaining_cache_extent: 1000.0,
            cache_origin: 0.0,
        };

        let geometry = viewport.calculate_sliver_geometry(&constraints, &tree, &children);

        // 2 children * 600px = 1200px total
        assert_eq!(geometry.scroll_extent, 1200.0);
        // Nothing visible
        assert_eq!(geometry.paint_extent, 0.0);
        assert!(!geometry.visible);
    }

    #[test]
    fn test_arity_is_variable() {
        let viewport = RenderSliverFillViewport::new(1.0);
        assert_eq!(viewport.arity(), RuntimeArity::Variable);
    }
}
