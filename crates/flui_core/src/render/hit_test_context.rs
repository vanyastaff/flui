//! Hit test contexts for render objects
//!
//! This module provides context structs for hit testing operations,
//! similar to how LayoutContext and PaintContext work for layout and paint.
//!
//! # Architecture
//!
//! Hit testing follows a similar pattern to layout/paint:
//! 1. RenderObject receives a HitTestContext
//! 2. Context contains position, tree access, and children
//! 3. RenderObject tests self and children
//! 4. Results are accumulated in HitTestResult
//!
//! # Box vs Sliver
//!
//! - `BoxHitTestContext`: For box-based rendering (standard UI elements)
//! - `SliverHitTestContext`: For sliver-based rendering (scrollable lists)

use crate::element::{ElementId, ElementTree};
use crate::render::Children;
use flui_types::layout::AxisDirection;
use flui_types::{Offset, Size, SliverGeometry};

// ============================================================================
// BoxHitTestContext
// ============================================================================

/// Context for box hit testing operations
///
/// Provides all necessary information for a render object to perform
/// hit testing on itself and its children.
///
/// # Lifetime
///
/// The `'a` lifetime ensures that the context cannot outlive the
/// element tree or children it references.
///
/// # Examples
///
/// ```rust,ignore
/// impl Render for RenderAbsorbPointer {
///     fn hit_test(&self, ctx: &BoxHitTestContext, result: &mut BoxHitTestResult) -> bool {
///         if self.absorbing {
///             // Absorb hit - add self but don't test children
///             result.add(ctx.element_id, BoxHitTestEntry::new(ctx.position, ctx.size));
///             return true;
///         }
///         // Normal - test children
///         self.hit_test_children(ctx, result)
///     }
/// }
/// ```
#[derive(Debug)]
pub struct BoxHitTestContext<'a> {
    /// Reference to the element tree
    ///
    /// Provides access to all elements for child hit testing.
    pub tree: &'a ElementTree,

    /// Position in local coordinates
    ///
    /// The hit test position transformed into this element's coordinate space.
    pub position: Offset,

    /// Size of the element
    ///
    /// The size computed during layout (from RenderState).
    pub size: Size,

    /// Children of this render object
    ///
    /// Encoded as a `Children` enum which can be:
    /// - `Children::None` for leaf nodes
    /// - `Children::Single(id)` for single-child wrappers
    /// - `Children::Multi(ids)` for multi-child layouts
    pub children: &'a Children,

    /// Element ID being tested
    ///
    /// The ID of the element that owns this render object.
    pub element_id: ElementId,
}

impl<'a> BoxHitTestContext<'a> {
    /// Create a new box hit test context
    ///
    /// # Parameters
    ///
    /// - `tree`: Reference to the element tree
    /// - `position`: Hit position in local coordinates
    /// - `size`: Size of the element from layout
    /// - `children`: Reference to children enum
    /// - `element_id`: ID of the element being tested
    pub fn new(
        tree: &'a ElementTree,
        position: Offset,
        size: Size,
        children: &'a Children,
        element_id: ElementId,
    ) -> Self {
        Self {
            tree,
            position,
            size,
            children,
            element_id,
        }
    }

    /// Create a modified context with a new position
    ///
    /// Useful for transforms where you need to test with a transformed position.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // In RenderTransform
    /// let inverse = self.transform.inverse()?;
    /// let transformed_pos = inverse.transform_point(ctx.position);
    /// let new_ctx = ctx.with_position(transformed_pos);
    /// self.hit_test_children(&new_ctx, result)
    /// ```
    pub fn with_position(&self, position: Offset) -> Self {
        Self {
            tree: self.tree,
            position,
            size: self.size,
            children: self.children,
            element_id: self.element_id,
        }
    }

    /// Check if position is within bounds (simple box check)
    ///
    /// Returns true if the position is inside the element's bounding box.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if !ctx.is_in_bounds() {
    ///     return false; // Early exit - hit is outside
    /// }
    /// ```
    #[inline]
    pub fn is_in_bounds(&self) -> bool {
        self.position.dx >= 0.0
            && self.position.dy >= 0.0
            && self.position.dx <= self.size.width
            && self.position.dy <= self.size.height
    }
}

// ============================================================================
// SliverHitTestContext
// ============================================================================

/// Context for sliver hit testing operations
///
/// Provides viewport-aware information for sliver hit testing,
/// including scroll offset, axis direction, and sliver geometry.
///
/// # Lifetime
///
/// The `'a` lifetime ensures that the context cannot outlive the
/// element tree or children it references.
///
/// # Examples
///
/// ```rust,ignore
/// impl RenderSliver for RenderSliverList {
///     fn hit_test(&self, ctx: &SliverHitTestContext, result: &mut SliverHitTestResult) -> bool {
///         // Check if hit is in visible region
///         if ctx.main_axis_position < 0.0 || ctx.main_axis_position >= ctx.geometry.paint_extent {
///             return false;
///         }
///
///         // Test children...
///         self.hit_test_children(ctx, result)
///     }
/// }
/// ```
#[derive(Debug)]
pub struct SliverHitTestContext<'a> {
    /// Reference to the element tree
    ///
    /// Provides access to all elements for child hit testing.
    pub tree: &'a ElementTree,

    /// Position along main axis (scroll direction)
    ///
    /// Distance from the leading edge of the viewport along the scroll axis.
    /// For vertical scrolling, this is the Y distance from top.
    /// For horizontal scrolling, this is the X distance from left.
    pub main_axis_position: f32,

    /// Position along cross axis (perpendicular to scroll)
    ///
    /// Distance perpendicular to the scroll direction.
    /// For vertical scrolling, this is the X coordinate.
    /// For horizontal scrolling, this is the Y coordinate.
    pub cross_axis_position: f32,

    /// Sliver geometry
    ///
    /// The geometry computed during layout (from RenderSliverState).
    /// Contains scroll_extent, paint_extent, cache_extent, etc.
    pub geometry: SliverGeometry,

    /// Current scroll offset
    ///
    /// The scroll position of the viewport. Used to determine
    /// if content is scrolled off-screen.
    pub scroll_offset: f32,

    /// Axis direction (Vertical/Horizontal)
    ///
    /// The direction in which the sliver scrolls.
    pub axis_direction: AxisDirection,

    /// Children of this sliver render object
    ///
    /// Encoded as a `Children` enum which can be:
    /// - `Children::None` for leaf nodes
    /// - `Children::Single(id)` for single-child wrappers
    /// - `Children::Multi(ids)` for multi-child layouts
    pub children: &'a Children,

    /// Element ID being tested
    ///
    /// The ID of the element that owns this sliver render object.
    pub element_id: ElementId,
}

impl<'a> SliverHitTestContext<'a> {
    /// Create a new sliver hit test context
    ///
    /// # Parameters
    ///
    /// - `tree`: Reference to the element tree
    /// - `main_axis_position`: Position along scroll axis
    /// - `cross_axis_position`: Position perpendicular to scroll axis
    /// - `geometry`: Sliver geometry from layout
    /// - `scroll_offset`: Current scroll position
    /// - `axis_direction`: Scroll direction
    /// - `children`: Reference to children enum
    /// - `element_id`: ID of the element being tested
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tree: &'a ElementTree,
        main_axis_position: f32,
        cross_axis_position: f32,
        geometry: SliverGeometry,
        scroll_offset: f32,
        axis_direction: AxisDirection,
        children: &'a Children,
        element_id: ElementId,
    ) -> Self {
        Self {
            tree,
            main_axis_position,
            cross_axis_position,
            geometry,
            scroll_offset,
            axis_direction,
            children,
            element_id,
        }
    }

    /// Check if hit is in visible region
    ///
    /// Returns true if the main axis position is within the painted extent
    /// (currently visible on screen).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if !ctx.is_visible() {
    ///     return false; // Hit is scrolled off-screen
    /// }
    /// ```
    #[inline]
    pub fn is_visible(&self) -> bool {
        self.main_axis_position >= 0.0 && self.main_axis_position < self.geometry.paint_extent
    }

    /// Check if hit is in cache extent
    ///
    /// Returns true if the hit is within the cache extent, which includes
    /// the visible region plus an off-screen buffer.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// if ctx.is_in_cache_extent() {
    ///     // Hit is in buffer region - may need to preload content
    /// }
    /// ```
    #[inline]
    pub fn is_in_cache_extent(&self) -> bool {
        self.main_axis_position >= -self.geometry.cache_extent
            && self.main_axis_position < self.geometry.paint_extent + self.geometry.cache_extent
    }

    /// Get local position as Offset
    ///
    /// Converts main/cross axis positions into an Offset,
    /// respecting the axis direction.
    ///
    /// # Returns
    ///
    /// - For vertical scrolling: Offset(cross_axis, main_axis)
    /// - For horizontal scrolling: Offset(main_axis, cross_axis)
    pub fn local_position(&self) -> Offset {
        match self.axis_direction {
            AxisDirection::TopToBottom | AxisDirection::BottomToTop => {
                // Vertical scroll: X = cross, Y = main
                Offset::new(self.cross_axis_position, self.main_axis_position)
            }
            AxisDirection::LeftToRight | AxisDirection::RightToLeft => {
                // Horizontal scroll: X = main, Y = cross
                Offset::new(self.main_axis_position, self.cross_axis_position)
            }
        }
    }

    /// Get distance from viewport edge
    ///
    /// Returns the absolute position of this hit relative to the start
    /// of the scrollable content.
    #[inline]
    pub fn distance_from_viewport_edge(&self) -> f32 {
        self.scroll_offset + self.main_axis_position
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_box_hit_test_context_in_bounds() {
        let tree = ElementTree::new();
        let children = Children::None;
        let element_id = ElementId::new(1);

        let ctx = BoxHitTestContext::new(
            &tree,
            Offset::new(50.0, 50.0),
            Size::new(100.0, 100.0),
            &children,
            element_id,
        );

        assert!(ctx.is_in_bounds());
        assert_eq!(ctx.position, Offset::new(50.0, 50.0));
        assert_eq!(ctx.size, Size::new(100.0, 100.0));
    }

    #[test]
    fn test_box_hit_test_context_out_of_bounds() {
        let tree = ElementTree::new();
        let children = Children::None;
        let element_id = ElementId::new(1);

        let ctx = BoxHitTestContext::new(
            &tree,
            Offset::new(150.0, 50.0),
            Size::new(100.0, 100.0),
            &children,
            element_id,
        );

        assert!(!ctx.is_in_bounds());
    }

    #[test]
    fn test_box_hit_test_context_with_position() {
        let tree = ElementTree::new();
        let children = Children::None;
        let element_id = ElementId::new(1);

        let ctx = BoxHitTestContext::new(
            &tree,
            Offset::new(50.0, 50.0),
            Size::new(100.0, 100.0),
            &children,
            element_id,
        );

        let new_ctx = ctx.with_position(Offset::new(75.0, 75.0));

        assert_eq!(new_ctx.position, Offset::new(75.0, 75.0));
        assert_eq!(new_ctx.size, ctx.size); // Size unchanged
        assert!(new_ctx.is_in_bounds());
    }

    #[test]
    fn test_sliver_hit_test_context_visible() {
        let tree = ElementTree::new();
        let children = Children::None;
        let element_id = ElementId::new(1);

        let geometry = SliverGeometry {
            scroll_extent: 1000.0,
            paint_extent: 600.0,
            cache_extent: 250.0,
            ..Default::default()
        };

        let ctx = SliverHitTestContext::new(
            &tree,
            200.0,  // main_axis_position
            50.0,   // cross_axis_position
            geometry,
            100.0,  // scroll_offset
            AxisDirection::TopToBottom,
            &children,
            element_id,
        );

        assert!(ctx.is_visible());
        assert!(ctx.is_in_cache_extent());
        assert_eq!(ctx.distance_from_viewport_edge(), 300.0); // 100 + 200
    }

    #[test]
    fn test_sliver_hit_test_context_not_visible() {
        let tree = ElementTree::new();
        let children = Children::None;
        let element_id = ElementId::new(1);

        let geometry = SliverGeometry {
            scroll_extent: 1000.0,
            paint_extent: 600.0,
            cache_extent: 250.0,
            ..Default::default()
        };

        let ctx = SliverHitTestContext::new(
            &tree,
            700.0,  // main_axis_position (beyond paint_extent)
            50.0,   // cross_axis_position
            geometry,
            100.0,  // scroll_offset
            AxisDirection::TopToBottom,
            &children,
            element_id,
        );

        assert!(!ctx.is_visible());
    }

    #[test]
    fn test_sliver_hit_test_context_local_position_vertical() {
        let tree = ElementTree::new();
        let children = Children::None;
        let element_id = ElementId::new(1);

        let geometry = SliverGeometry::default();

        let ctx = SliverHitTestContext::new(
            &tree,
            200.0,  // main_axis_position
            50.0,   // cross_axis_position
            geometry,
            0.0,
            AxisDirection::TopToBottom,
            &children,
            element_id,
        );

        let pos = ctx.local_position();
        assert_eq!(pos.dx, 50.0);  // cross axis
        assert_eq!(pos.dy, 200.0); // main axis
    }

    #[test]
    fn test_sliver_hit_test_context_local_position_horizontal() {
        let tree = ElementTree::new();
        let children = Children::None;
        let element_id = ElementId::new(1);

        let geometry = SliverGeometry::default();

        let ctx = SliverHitTestContext::new(
            &tree,
            200.0,  // main_axis_position
            50.0,   // cross_axis_position
            geometry,
            0.0,
            AxisDirection::LeftToRight,
            &children,
            element_id,
        );

        let pos = ctx.local_position();
        assert_eq!(pos.dx, 200.0); // main axis
        assert_eq!(pos.dy, 50.0);  // cross axis
    }
}
