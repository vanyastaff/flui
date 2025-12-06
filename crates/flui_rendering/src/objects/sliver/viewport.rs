//! RenderViewport - Scrollable viewport container for sliver children
//!
//! Core container that manages scrolling slivers (lists, grids, custom scrollables).
//! Converts scroll offset into SliverConstraints for children, manages layout of multiple
//! slivers in sequence, handles viewport clipping, and coordinates cache extent for smooth
//! scrolling. Essential building block for CustomScrollView, ListView, GridView.
//!
//! # Flutter Equivalence
//!
//! | FLUI | Flutter |
//! |------|---------|
//! | `RenderViewport` | `RenderViewport` from `package:flutter/src/rendering/viewport.dart` |
//! | `scroll_offset` | Current scroll position |
//! | `viewport_main_axis_extent` | Viewport size (height for vertical) |
//! | `cache_extent` | Buffer for prebuilding off-screen children |
//! | `calculate_sliver_constraints()` | Converts scroll offset to SliverConstraints |
//! | `layout_slivers()` | Sequential sliver layout with remaining extent |
//!
//! # Layout Protocol (Intended)
//!
//! 1. **Calculate viewport size from box constraints**
//!    - Extract width and height from BoxConstraints
//!    - Determine main/cross axis based on axis_direction
//!
//! 2. **Layout slivers sequentially** (NOT IMPLEMENTED)
//!    - Start with remaining_paint_extent = viewport_main_axis_extent
//!    - For each child:
//!      - Create SliverConstraints with current scroll_offset and remaining extent
//!      - Layout sliver child
//!      - Get SliverGeometry from child
//!      - Reduce remaining_paint_extent by paint_extent
//!      - Adjust scroll_offset for next child
//!    - Stop when remaining_paint_extent <= 0 (viewport full)
//!
//! 3. **Return viewport size**
//!    - Width and height from box constraints
//!
//! # Paint Protocol (Intended)
//!
//! 1. **Apply clipping** (NOT IMPLEMENTED)
//!    - Clip to viewport bounds based on clip_behavior
//!
//! 2. **Paint sliver children** (NOT IMPLEMENTED)
//!    - Paint each sliver at calculated offset
//!    - Only visible slivers (paint_extent > 0)
//!
//! # Performance
//!
//! - **Layout**: O(s) where s = visible slivers - stops when viewport full
//! - **Paint**: O(s) - only paints visible slivers
//! - **Memory**: 32 bytes (fields) + Vec<SliverGeometry> for child geometries
//! - **Scroll updates**: O(1) to update scroll_offset, then O(s) relayout
//!
//! # Use Cases
//!
//! - **CustomScrollView**: Generic scrollable with mixed sliver types
//! - **ListView**: Scrollable list of items
//! - **GridView**: Scrollable 2D grid
//! - **PageView**: Swipeable pages
//! - **NestedScrollView**: Nested scrolling with coordinated headers
//!
//! # Scroll Offset Behavior
//!
//! ```text
//! scroll_offset = 0:      [VIEWPORT] ← Top of content visible
//!                         [Content...]
//!
//! scroll_offset = 100:    [Content...] ← Scrolled down 100px
//!                         [VIEWPORT]
//!                         [Content...]
//!
//! scroll_offset = 500:    [Content...]
//!                         [Content...]
//!                         [VIEWPORT] ← Far down in content
//! ```
//!
//! # Sequential Sliver Layout
//!
//! ```text
//! Viewport height: 600px, scroll_offset: 0
//!
//! Sliver #1 (SliverAppBar):
//!   SliverConstraints: { scroll_offset: 0, remaining: 600 }
//!   SliverGeometry: { paint_extent: 100 }
//!   → remaining = 500
//!
//! Sliver #2 (SliverList):
//!   SliverConstraints: { scroll_offset: 0, remaining: 500 }
//!   SliverGeometry: { paint_extent: 400 }
//!   → remaining = 100
//!
//! Sliver #3 (SliverGrid):
//!   SliverConstraints: { scroll_offset: 0, remaining: 100 }
//!   SliverGeometry: { paint_extent: 100 }
//!   → remaining = 0 (STOP - viewport full)
//! ```
//!
//! # ⚠️ CRITICAL IMPLEMENTATION ISSUES
//!
//! This implementation has **MAJOR INCOMPLETE FUNCTIONALITY**:
//!
//! 1. **❌ Sliver children NEVER laid out** (line 151-161)
//!    - layout_slivers() creates PLACEHOLDER geometry only (line 163-178)
//!    - Comments say "In real implementation" (line 157-161)
//!    - No actual child layout calls
//!
//! 2. **❌ layout() doesn't call layout_slivers** (line 204-227)
//!    - TODO comment on line 223-224
//!    - layout_slivers() method exists but never called!
//!    - Children never laid out
//!
//! 3. **❌ Paint NOT IMPLEMENTED** (line 229-237)
//!    - Returns empty Canvas
//!    - TODO comments for clipping and child painting
//!    - Slivers never painted
//!
//! 4. **❌ No clipping applied** (line 233 TODO)
//!    - clip_behavior field exists but unused
//!    - Content can overflow viewport bounds
//!
//! 5. **✅ calculate_sliver_constraints() CORRECT** (line 119-138)
//!    - Properly converts scroll offset to SliverConstraints
//!    - Correct cross-axis direction logic
//!
//! **This RenderObject is BROKEN - core viewport functionality missing!**
//!
//! # Comparison with Related Objects
//!
//! - **vs RenderBox**: Viewport IS a box containing slivers, Box contains boxes
//! - **vs RenderSliverToBoxAdapter**: Adapter wraps box IN sliver, Viewport contains slivers
//! - **vs RenderScrollView**: ScrollView is higher-level widget, Viewport is render object
//! - **vs RenderSliverList**: List IS a sliver, Viewport CONTAINS slivers
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_rendering::RenderViewport;
//! use flui_types::layout::AxisDirection;
//!
//! // Vertical scrolling viewport (like ListView)
//! let viewport = RenderViewport::new(
//!     AxisDirection::TopToBottom,
//!     600.0,  // viewport height
//!     100.0,  // scrolled down 100px
//! );
//! // WARNING: children never laid out or painted!
//!
//! // Horizontal scrolling (like PageView)
//! let horizontal = RenderViewport::new(
//!     AxisDirection::LeftToRight,
//!     800.0,  // viewport width
//!     0.0,    // at start
//! );
//! // WARNING: stub implementation!
//!
//! // With custom cache extent
//! let mut viewport = RenderViewport::new(
//!     AxisDirection::TopToBottom,
//!     600.0,
//!     0.0,
//! );
//! viewport.set_cache_extent(500.0); // 500px buffer
//! // WARNING: cache extent calculated but children never built!
//! ```

use flui_core::element::ElementTree;
// TODO: Migrate to Render<A>
// use crate::core::{RuntimeArity, BoxPaintCtx, LegacyRender};
use flui_painting::Canvas;
use flui_types::prelude::*;
use flui_types::layout::{Axis, AxisDirection};
use flui_types::{SliverConstraints, SliverGeometry};

/// RenderObject for scrollable viewport containing sliver children.
///
/// Core container for sliver-based scrolling. Converts scroll offset into SliverConstraints,
/// layouts slivers sequentially with remaining extent tracking, manages viewport clipping,
/// and coordinates cache extent for smooth scrolling. Essential for CustomScrollView,
/// ListView, GridView, PageView.
///
/// # Arity
///
/// `RuntimeArity::Variable` - Can have multiple sliver children (0+).
///
/// # Protocol
///
/// Box protocol - Uses `BoxConstraints` and returns `Size`.
/// **Children use sliver protocol** - Layouts children with `SliverConstraints`.
///
/// # Pattern
///
/// **Scroll-Aware Multi-Sliver Container** - Protocol bridge (box-to-sliver), sequential
/// layout with remaining extent tracking, scroll offset to constraints conversion, viewport
/// clipping, cache extent coordination for prebuilding off-screen content.
///
/// # Use Cases
///
/// - **CustomScrollView**: Mixed sliver types (lists, grids, headers, etc.)
/// - **ListView**: Scrollable vertical/horizontal list
/// - **GridView**: Scrollable 2D grid layout
/// - **PageView**: Swipeable page navigation
/// - **NestedScrollView**: Coordinated nested scrolling
///
/// # Flutter Compliance
///
/// **BROKEN IMPLEMENTATION**:
/// - ❌ Sliver children never laid out (placeholder geometry only)
/// - ❌ layout() doesn't call layout_slivers (TODO comment)
/// - ❌ Paint not implemented (returns empty Canvas)
/// - ❌ Clipping not applied (clip_behavior unused)
/// - ✅ calculate_sliver_constraints() correct
/// - ⚠️ Stub implementation - only constraint calculation works
///
/// # Implementation Status
///
/// | Feature | Status | Notes |
/// |---------|--------|-------|
/// | Constraint calculation | ✅ Complete | Correctly converts scroll to SliverConstraints |
/// | Sequential layout | ❌ Missing | layout_slivers() exists but creates placeholders |
/// | Layout integration | ❌ Missing | layout() doesn't call layout_slivers (TODO) |
/// | Paint | ❌ Missing | Returns empty Canvas with TODO |
/// | Clipping | ❌ Missing | clip_behavior field exists but unused |
/// | Cache extent | ⚠️ Partial | Calculated but children not prebuilt |
/// | Scroll offset | ✅ Works | Can update, but no relayout happens |
///
/// # Coordinate System
///
/// - `scroll_offset = 0` means top/left of content visible
/// - Positive scroll_offset scrolls content upward/leftward
/// - `viewport_main_axis_extent` is viewport size (height for vertical, width for horizontal)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderViewport;
/// use flui_types::layout::AxisDirection;
///
/// // Vertical scrolling viewport
/// let viewport = RenderViewport::new(
///     AxisDirection::TopToBottom,
///     600.0,  // viewport height
///     100.0,  // scroll offset
/// );
/// // WARNING: children never laid out or painted!
/// ```
#[derive(Debug)]
pub struct RenderViewport {
    /// Direction of the main axis
    pub axis_direction: AxisDirection,
    /// Main axis extent (height for vertical, width for horizontal)
    pub viewport_main_axis_extent: f32,
    /// Cross axis extent
    pub cross_axis_extent: f32,
    /// Current scroll offset
    pub scroll_offset: f32,
    /// Cache extent for off-screen rendering
    pub cache_extent: f32,
    /// Whether to clip content to viewport bounds
    pub clip_behavior: ClipBehavior,

    // Layout cache
    sliver_geometries: Vec<SliverGeometry>,
}

/// Clipping behavior for viewport
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipBehavior {
    /// No clipping
    None,
    /// Clip content to viewport bounds
    HardEdge,
    /// Clip with anti-aliasing
    AntiAlias,
    /// Clip with anti-aliasing and handle edge bleeding
    AntiAliasWithSaveLayer,
}

impl RenderViewport {
    /// Create new viewport
    ///
    /// # Arguments
    /// * `axis_direction` - Direction of scrolling axis
    /// * `viewport_main_axis_extent` - Size of viewport on main axis
    /// * `scroll_offset` - Current scroll position
    pub fn new(
        axis_direction: AxisDirection,
        viewport_main_axis_extent: f32,
        scroll_offset: f32,
    ) -> Self {
        Self {
            axis_direction,
            viewport_main_axis_extent,
            cross_axis_extent: 0.0,
            scroll_offset,
            cache_extent: 250.0, // Default cache extent
            clip_behavior: ClipBehavior::HardEdge,
            sliver_geometries: Vec::new(),
        }
    }

    /// Set scroll offset
    pub fn set_scroll_offset(&mut self, offset: f32) {
        self.scroll_offset = offset;
    }

    /// Set viewport extent
    pub fn set_viewport_extent(&mut self, extent: f32) {
        self.viewport_main_axis_extent = extent;
    }

    /// Set cache extent
    pub fn set_cache_extent(&mut self, extent: f32) {
        self.cache_extent = extent;
    }

    /// Set clip behavior
    pub fn set_clip_behavior(&mut self, behavior: ClipBehavior) {
        self.clip_behavior = behavior;
    }

    /// Get the axis (vertical or horizontal)
    pub fn axis(&self) -> Axis {
        self.axis_direction.axis()
    }

    /// Calculate sliver constraints for children
    fn calculate_sliver_constraints(
        &self,
        remaining_paint_extent: f32,
        scroll_offset: f32,
    ) -> SliverConstraints {
        SliverConstraints {
            axis_direction: self.axis_direction,
            grow_direction_reversed: false,
            scroll_offset,
            remaining_paint_extent,
            cross_axis_extent: self.cross_axis_extent,
            cross_axis_direction: match self.axis_direction.axis() {
                Axis::Vertical => AxisDirection::LeftToRight,
                Axis::Horizontal => AxisDirection::TopToBottom,
            },
            viewport_main_axis_extent: self.viewport_main_axis_extent,
            remaining_cache_extent: self.cache_extent,
            cache_origin: 0.0,
        }
    }

    /// Layout sliver children
    fn layout_slivers(
        &mut self,
        _tree: &ElementTree,
        children: &[flui_core::element::ElementId],
    ) {
        self.sliver_geometries.clear();

        let mut remaining_paint_extent = self.viewport_main_axis_extent;
        let mut current_scroll_offset = self.scroll_offset;

        for _child_id in children {
            let constraints = self.calculate_sliver_constraints(
                remaining_paint_extent,
                current_scroll_offset,
            );

            // In real implementation:
            // 1. Set sliver constraints on child
            // 2. Call child.layout()
            // 3. Get child's SliverGeometry
            // 4. Update remaining_paint_extent and current_scroll_offset

            // For now, create placeholder geometry
            let geometry = SliverGeometry {
                scroll_extent: 100.0,
                paint_extent: remaining_paint_extent.min(100.0),
                layout_extent: remaining_paint_extent.min(100.0),
                max_paint_extent: 100.0,
                visible: remaining_paint_extent > 0.0,
                visible_fraction: 1.0,
                paint_origin: 0.0,
                cross_axis_extent: constraints.cross_axis_extent,
                cache_extent: remaining_paint_extent.min(100.0),
                has_visual_overflow: false,
                hit_test_extent: Some(remaining_paint_extent.min(100.0)),
                scroll_offset_correction: None,
                max_scroll_obsolescence: 0.0,
            };

            self.sliver_geometries.push(geometry);

            remaining_paint_extent -= geometry.paint_extent;
            current_scroll_offset = (current_scroll_offset - geometry.scroll_extent).max(0.0);

            if remaining_paint_extent <= 0.0 {
                break;
            }
        }
    }

    /// Get geometry for child at index
    pub fn geometry_at(&self, index: usize) -> Option<&SliverGeometry> {
        self.sliver_geometries.get(index)
    }
}

impl Default for RenderViewport {
    fn default() -> Self {
        Self::new(AxisDirection::TopToBottom, 600.0, 0.0)
    }
}

impl LegacyRender for RenderViewport {
    fn layout(&mut self, ctx: &) -> Size {
        let constraints = &ctx.constraints;

        // Viewport takes up the space given by box constraints
        let width = constraints.max_width;
        let height = constraints.max_height;

        // Determine cross axis extent based on axis direction
        match self.axis_direction.axis() {
            Axis::Vertical => {
                self.cross_axis_extent = width;
                self.viewport_main_axis_extent = height;
            }
            Axis::Horizontal => {
                self.cross_axis_extent = height;
                self.viewport_main_axis_extent = width;
            }
        }

        // TODO: Layout sliver children
        // self.layout_slivers(tree, children);

        Size::new(width, height)
    }

    fn paint(&self, ctx: &) -> Canvas {
        let _offset = ctx.offset;
        let canvas = Canvas::new();

        // TODO: Apply clipping based on clip_behavior
        // TODO: Paint sliver children at their calculated positions

        canvas
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> RuntimeArity {
        RuntimeArity::Variable // Multiple sliver children
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_viewport_new() {
        let viewport = RenderViewport::new(AxisDirection::TopToBottom, 600.0, 0.0);

        assert_eq!(viewport.axis_direction, AxisDirection::TopToBottom);
        assert_eq!(viewport.viewport_main_axis_extent, 600.0);
        assert_eq!(viewport.scroll_offset, 0.0);
        assert_eq!(viewport.cache_extent, 250.0);
        assert_eq!(viewport.clip_behavior, ClipBehavior::HardEdge);
    }

    #[test]
    fn test_render_viewport_default() {
        let viewport = RenderViewport::default();

        assert_eq!(viewport.axis_direction, AxisDirection::TopToBottom);
        assert_eq!(viewport.viewport_main_axis_extent, 600.0);
        assert_eq!(viewport.scroll_offset, 0.0);
    }

    #[test]
    fn test_set_scroll_offset() {
        let mut viewport = RenderViewport::new(AxisDirection::TopToBottom, 600.0, 0.0);
        viewport.set_scroll_offset(100.0);

        assert_eq!(viewport.scroll_offset, 100.0);
    }

    #[test]
    fn test_set_viewport_extent() {
        let mut viewport = RenderViewport::new(AxisDirection::TopToBottom, 600.0, 0.0);
        viewport.set_viewport_extent(800.0);

        assert_eq!(viewport.viewport_main_axis_extent, 800.0);
    }

    #[test]
    fn test_set_cache_extent() {
        let mut viewport = RenderViewport::new(AxisDirection::TopToBottom, 600.0, 0.0);
        viewport.set_cache_extent(500.0);

        assert_eq!(viewport.cache_extent, 500.0);
    }

    #[test]
    fn test_set_clip_behavior() {
        let mut viewport = RenderViewport::new(AxisDirection::TopToBottom, 600.0, 0.0);
        viewport.set_clip_behavior(ClipBehavior::AntiAlias);

        assert_eq!(viewport.clip_behavior, ClipBehavior::AntiAlias);
    }

    #[test]
    fn test_axis_vertical() {
        let viewport = RenderViewport::new(AxisDirection::TopToBottom, 600.0, 0.0);

        assert_eq!(viewport.axis(), Axis::Vertical);
    }

    #[test]
    fn test_axis_horizontal() {
        let viewport = RenderViewport::new(AxisDirection::LeftToRight, 600.0, 0.0);

        assert_eq!(viewport.axis(), Axis::Horizontal);
    }

    #[test]
    fn test_calculate_sliver_constraints_vertical() {
        let viewport = RenderViewport::new(AxisDirection::TopToBottom, 600.0, 100.0);
        let constraints = viewport.calculate_sliver_constraints(500.0, 100.0);

        assert_eq!(constraints.axis_direction, AxisDirection::TopToBottom);
        assert_eq!(constraints.scroll_offset, 100.0);
        assert_eq!(constraints.remaining_paint_extent, 500.0);
        assert_eq!(constraints.viewport_main_axis_extent, 600.0);
        assert_eq!(constraints.cross_axis_direction, AxisDirection::LeftToRight);
    }

    #[test]
    fn test_calculate_sliver_constraints_horizontal() {
        let viewport = RenderViewport::new(AxisDirection::LeftToRight, 800.0, 50.0);
        let constraints = viewport.calculate_sliver_constraints(700.0, 50.0);

        assert_eq!(constraints.axis_direction, AxisDirection::LeftToRight);
        assert_eq!(constraints.scroll_offset, 50.0);
        assert_eq!(constraints.remaining_paint_extent, 700.0);
        assert_eq!(constraints.viewport_main_axis_extent, 800.0);
        assert_eq!(constraints.cross_axis_direction, AxisDirection::TopToBottom);
    }

    #[test]
    fn test_layout_slivers_single_child() {
        let mut viewport = RenderViewport::new(AxisDirection::TopToBottom, 600.0, 0.0);
        viewport.cross_axis_extent = 400.0;

        let tree = ElementTree::new();
        let children = vec![flui_core::element::ElementId::new(1)];

        viewport.layout_slivers(&tree, &children);

        assert_eq!(viewport.sliver_geometries.len(), 1);
        let geometry = viewport.geometry_at(0).unwrap();
        assert_eq!(geometry.scroll_extent, 100.0);
        assert!(geometry.visible);
    }

    #[test]
    fn test_layout_slivers_multiple_children() {
        let mut viewport = RenderViewport::new(AxisDirection::TopToBottom, 600.0, 0.0);
        viewport.cross_axis_extent = 400.0;

        let tree = ElementTree::new();
        let children = vec![
            flui_core::element::ElementId::new(1),
            flui_core::element::ElementId::new(2),
            flui_core::element::ElementId::new(3),
        ];

        viewport.layout_slivers(&tree, &children);

        // With placeholder geometry, all children should be laid out
        assert_eq!(viewport.sliver_geometries.len(), 3);
    }

    #[test]
    fn test_geometry_at_valid_index() {
        let mut viewport = RenderViewport::new(AxisDirection::TopToBottom, 600.0, 0.0);
        viewport.cross_axis_extent = 400.0;

        let tree = ElementTree::new();
        let children = vec![flui_core::element::ElementId::new(1)];

        viewport.layout_slivers(&tree, &children);

        assert!(viewport.geometry_at(0).is_some());
    }

    #[test]
    fn test_geometry_at_invalid_index() {
        let viewport = RenderViewport::new(AxisDirection::TopToBottom, 600.0, 0.0);

        assert!(viewport.geometry_at(0).is_none());
    }

    #[test]
    fn test_arity_is_variable() {
        let viewport = RenderViewport::new(AxisDirection::TopToBottom, 600.0, 0.0);
        assert_eq!(viewport.arity(), RuntimeArity::Variable);
    }
}
