//! RenderSliver - Sliver Protocol Render Trait (Flutter Model)
//!
//! This module provides `RenderSliver<A>`, the core trait for sliver protocol render
//! objects following Flutter's exact rendering model:
//!
//! - **Layout**: `perform_layout(constraints) -> SliverGeometry` - constraints as parameters
//! - **Paint**: `paint(ctx, offset)` - PaintingContext for canvas and layers
//! - **Hit Test**: `hit_test(result, position) -> bool` - SliverHitTestResult directly
//!
//! # Flutter Equivalence
//!
//! ```dart
//! // Flutter
//! abstract class RenderSliver extends RenderObject {
//!   @override
//!   void performLayout() {
//!     geometry = SliverGeometry(scrollExtent: 100.0, paintExtent: 50.0);
//!   }
//!
//!   @override
//!   void paint(PaintingContext context, Offset offset) {
//!     context.paintChild(child!, offset);
//!   }
//!
//!   @override
//!   bool hitTest(SliverHitTestResult result, {required double mainAxisPosition, required double crossAxisPosition}) {
//!     return hitTestChildren(result, mainAxisPosition: mainAxisPosition, crossAxisPosition: crossAxisPosition);
//!   }
//! }
//! ```
//!
//! # Sliver Protocol
//!
//! Slivers are specialized render objects for scrollable content:
//! - **One-dimensional**: Scroll in main axis (vertical or horizontal)
//! - **Lazy loading**: Only layout/paint visible portions
//! - **Viewport clipping**: Automatically clip to visible region
//! - **Composability**: Multiple slivers in a single scrollable

use std::fmt;

use crate::hit_test::SliverHitTestResult;
use flui_foundation::ElementId;
use flui_types::{Offset, Rect, SliverConstraints, SliverGeometry};

use super::object::RenderObject;
use super::painting_context::PaintingContext;
use flui_tree::arity::Arity;

// ============================================================================
// CORE RENDER SLIVER TRAIT (Flutter Model)
// ============================================================================

/// Render trait for sliver protocol with compile-time arity validation.
///
/// This trait follows Flutter's `RenderSliver` protocol exactly:
///
/// 1. **Constraints go down**: Parent passes `SliverConstraints` to `perform_layout()`
/// 2. **Geometry comes up**: `perform_layout()` returns `SliverGeometry`
/// 3. **Parent positions**: Parent positions child after layout
///
/// # Type Parameter
///
/// - `A: Arity` - Compile-time child count validation:
///   - `Leaf` - 0 children (SliverFillRemaining)
///   - `Single` - 1 child (SliverPadding, SliverOpacity)
///   - `Variable` - 0+ children (SliverList, SliverGrid)
///
/// # Required Methods
///
/// ## `perform_layout(constraints) -> SliverGeometry`
///
/// Computes geometry given constraints.
///
/// ## `paint(ctx, offset)`
///
/// Draws to canvas at the given offset.
///
/// # Flutter Contract
///
/// The returned geometry MUST satisfy these invariants:
/// - `paint_extent ≤ constraints.remaining_paint_extent`
/// - `layout_extent ≤ paint_extent`
/// - `paint_extent ≤ scroll_extent` (unless pinned/floating)
/// - `max_paint_extent ≥ paint_extent`
pub trait RenderSliver<A: Arity>: RenderObject + fmt::Debug + Send + Sync {
    // ========================================================================
    // LAYOUT
    // ========================================================================

    /// Computes the geometry of this sliver given constraints.
    ///
    /// This is called during the layout phase. The implementation must:
    ///
    /// 1. Layout any children (using stored child references)
    /// 2. Compute geometry based on constraints and child geometries
    /// 3. Return geometry satisfying invariants
    ///
    /// # Contract
    ///
    /// - **MUST** return geometry satisfying invariants
    /// - **MUST** be idempotent (same constraints → same geometry)
    /// - **SHOULD** layout children before using their geometry
    /// - **SHOULD** store computed geometry for paint/hit_test access
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// @override
    /// void performLayout() {
    ///   geometry = SliverGeometry(
    ///     scrollExtent: 100.0,
    ///     paintExtent: 50.0,
    ///   );
    /// }
    /// ```
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// fn perform_layout(&mut self, constraints: SliverConstraints) -> SliverGeometry {
    ///     let extent = 100.0;
    ///     let scroll_offset = constraints.scroll_offset;
    ///
    ///     let visible = if scroll_offset < extent {
    ///         (extent - scroll_offset).min(constraints.remaining_paint_extent)
    ///     } else {
    ///         0.0
    ///     };
    ///
    ///     SliverGeometry {
    ///         scroll_extent: extent,
    ///         paint_extent: visible,
    ///         layout_extent: Some(visible),
    ///         max_paint_extent: Some(extent),
    ///         ..Default::default()
    ///     }
    /// }
    /// ```
    fn perform_layout(&mut self, constraints: SliverConstraints) -> SliverGeometry;

    // ========================================================================
    // PAINT
    // ========================================================================

    /// Paints this sliver and its children.
    ///
    /// Called during the paint phase with a `PaintingContext` for canvas access
    /// and layer composition, plus an offset for positioning.
    ///
    /// # Arguments
    ///
    /// - `ctx` - Context for canvas access and child painting
    /// - `offset` - Position of this sliver in parent coordinates
    ///
    /// # Contract
    ///
    /// - **MUST NOT** call layout (use cached geometry from layout phase)
    /// - **SHOULD** use `geometry.paint_extent` to determine visible region
    /// - **SHOULD** skip painting if `paint_extent == 0.0`
    /// - **SHOULD** paint children via `ctx.paint_child()`
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// @override
    /// void paint(PaintingContext context, Offset offset) {
    ///   if (geometry!.visible) {
    ///     context.paintChild(child!, offset);
    ///   }
    /// }
    /// ```
    fn paint(&self, ctx: &mut PaintingContext, offset: Offset);

    // ========================================================================
    // HIT TESTING
    // ========================================================================

    /// Hit tests this sliver at the given position.
    ///
    /// Returns `true` if this sliver or any descendant was hit.
    ///
    /// # Arguments
    ///
    /// - `result` - Accumulator for hit test entries
    /// - `main_axis_position` - Position along main axis
    /// - `cross_axis_position` - Position along cross axis
    ///
    /// # Default Implementation
    ///
    /// Default performs bounds check and delegates to children.
    ///
    /// # Flutter Equivalence
    ///
    /// ```dart
    /// @override
    /// bool hitTest(SliverHitTestResult result, {
    ///   required double mainAxisPosition,
    ///   required double crossAxisPosition,
    /// }) {
    ///   return geometry!.hitTestExtent > 0 &&
    ///     mainAxisPosition >= 0 &&
    ///     mainAxisPosition < geometry!.hitTestExtent &&
    ///     hitTestChildren(result,
    ///       mainAxisPosition: mainAxisPosition,
    ///       crossAxisPosition: crossAxisPosition);
    /// }
    /// ```
    fn hit_test(
        &self,
        result: &mut SliverHitTestResult,
        main_axis_position: f32,
        cross_axis_position: f32,
    ) -> bool {
        // Default: check if within hit test extent
        let geometry = self.geometry();
        let hit_test_extent = geometry.hit_test_extent.unwrap_or(0.0);
        if hit_test_extent <= 0.0 {
            return false;
        }

        if main_axis_position < 0.0 || main_axis_position >= hit_test_extent {
            return false;
        }

        // Test children
        self.hit_test_children(result, main_axis_position, cross_axis_position)
    }

    /// Hit tests children.
    ///
    /// Default implementation returns `false` (no children or leaf node).
    fn hit_test_children(
        &self,
        _result: &mut SliverHitTestResult,
        _main_axis_position: f32,
        _cross_axis_position: f32,
    ) -> bool {
        false // Default: no children
    }

    // ========================================================================
    // GEOMETRY ACCESS
    // ========================================================================

    /// Returns the computed geometry from layout.
    ///
    /// This should return the geometry computed during `perform_layout()`.
    /// Implementations must store the geometry during layout.
    fn geometry(&self) -> SliverGeometry;

    /// Bounding rectangle in local coordinates.
    ///
    /// For slivers, typically uses paint_extent for height/width.
    fn local_bounds(&self) -> Rect {
        Rect::ZERO // Default: empty bounds
    }

    // ========================================================================
    // CHILD POSITIONING (Flutter Methods)
    // ========================================================================

    /// Distance from parent's zero scroll offset to child's zero scroll offset.
    ///
    /// # When to override
    ///
    /// Override if children are positioned anywhere other than scroll offset zero.
    fn child_scroll_offset(&self, _child_id: ElementId) -> Option<f32> {
        Some(0.0) // Default: child aligned with parent's zero offset
    }

    /// Distance from parent's visible leading edge to child's visible leading edge.
    fn child_main_axis_position(&self, _child_id: ElementId) -> Option<f32> {
        Some(0.0) // Default: child at visible leading edge
    }

    /// Distance along cross axis from parent's edge to child's edge.
    fn child_cross_axis_position(&self, _child_id: ElementId) -> Option<f32> {
        Some(0.0) // Default: aligned to parent's cross-axis edge
    }

    // ========================================================================
    // VIEWPORT CALCULATIONS
    // ========================================================================

    /// Computes the visible portion of region from `from` to `to`.
    fn calculate_paint_offset(&self, constraints: &SliverConstraints, from: f32, to: f32) -> f32 {
        let target_last = to;
        let target_first = from;
        let scroll_offset = constraints.scroll_offset;
        let remaining_paint = constraints.remaining_paint_extent;

        (target_last - scroll_offset.max(target_first))
            .max(0.0)
            .min(remaining_paint)
    }

    /// Computes the cached portion of region from `from` to `to`.
    fn calculate_cache_offset(&self, _constraints: &SliverConstraints, from: f32, to: f32) -> f32 {
        let cache_origin: f32 = 0.0;
        let cache_extent: f32 = f32::INFINITY;

        let target_last = to;
        let target_first = from;

        (target_last - cache_origin.max(target_first))
            .max(0.0)
            .min(cache_extent)
    }

    /// Offset applied to viewport's center sliver (for centering).
    fn center_offset_adjustment(&self) -> f32 {
        0.0 // Default: no adjustment
    }

    /// Whether this sliver has visual overflow beyond its paint bounds.
    fn has_visual_overflow(&self) -> bool {
        false // Default: no overflow
    }

    // ========================================================================
    // DEBUG UTILITIES
    // ========================================================================

    // Note: debug_fill_properties() is inherited from Diagnosticable supertrait

    /// Paints debug visualization.
    #[cfg(debug_assertions)]
    fn debug_paint(&self, _canvas: &mut flui_painting::Canvas, _geometry: &SliverGeometry) {
        // Override for custom debug visualization
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Leaf;
    use flui_foundation::DiagnosticsProperty;

    #[derive(Debug)]
    struct TestRenderSliver {
        extent: f32,
        cached_geometry: SliverGeometry,
    }

    impl TestRenderSliver {
        fn new(extent: f32) -> Self {
            Self {
                extent,
                cached_geometry: SliverGeometry::zero(),
            }
        }
    }

    impl RenderSliver<Leaf> for TestRenderSliver {
        fn perform_layout(&mut self, constraints: SliverConstraints) -> SliverGeometry {
            let visible = if constraints.scroll_offset < self.extent {
                (self.extent - constraints.scroll_offset).min(constraints.remaining_paint_extent)
            } else {
                0.0
            };

            self.cached_geometry = SliverGeometry {
                scroll_extent: self.extent,
                paint_extent: visible,
                layout_extent: Some(visible),
                max_paint_extent: Some(self.extent),
                ..Default::default()
            };
            self.cached_geometry
        }

        fn paint(&self, _ctx: &mut PaintingContext, _offset: Offset) {
            // No-op for test
        }

        fn geometry(&self) -> SliverGeometry {
            self.cached_geometry
        }
    }

    impl flui_foundation::Diagnosticable for TestRenderSliver {
        #[cfg(debug_assertions)]
        fn debug_fill_properties(&self, properties: &mut Vec<DiagnosticsProperty>) {
            properties.push(DiagnosticsProperty::new("extent", self.extent));
        }
    }

    impl flui_interaction::HitTestTarget for TestRenderSliver {
        fn handle_event(
            &self,
            _event: &flui_types::events::PointerEvent,
            _entry: &flui_interaction::HitTestEntry,
        ) {
        }
    }

    impl RenderObject for TestRenderSliver {}

    #[test]
    fn test_calculate_paint_offset() {
        let sliver = TestRenderSliver::new(100.0);
        let constraints = SliverConstraints {
            scroll_offset: 50.0,
            remaining_paint_extent: 200.0,
            ..Default::default()
        };

        // Fully visible region
        let visible = sliver.calculate_paint_offset(&constraints, 50.0, 150.0);
        assert_eq!(visible, 100.0);

        // Partially visible
        let visible = sliver.calculate_paint_offset(&constraints, 30.0, 80.0);
        assert_eq!(visible, 30.0);

        // Not visible
        let visible = sliver.calculate_paint_offset(&constraints, 0.0, 40.0);
        assert_eq!(visible, 0.0);
    }

    #[test]
    fn test_child_positioning_defaults() {
        let sliver = TestRenderSliver::new(100.0);
        let child_id = ElementId::new(1);

        assert_eq!(sliver.child_scroll_offset(child_id), Some(0.0));
        assert_eq!(sliver.child_main_axis_position(child_id), Some(0.0));
        assert_eq!(sliver.child_cross_axis_position(child_id), Some(0.0));
    }

    #[test]
    fn test_geometry_helpers() {
        let zero = SliverGeometry::zero();
        assert_eq!(zero.scroll_extent, 0.0);
        assert_eq!(zero.paint_extent, 0.0);
        assert!(!zero.is_visible());

        let visible = SliverGeometry {
            scroll_extent: 100.0,
            paint_extent: 50.0,
            visible: true,
            ..Default::default()
        };
        assert!(visible.is_visible());
    }

    #[cfg(debug_assertions)]
    #[test]
    fn test_debug_properties() {
        use flui_foundation::Diagnosticable;

        let sliver = TestRenderSliver::new(100.0);
        let mut props = Vec::new();
        sliver.debug_fill_properties(&mut props);

        assert_eq!(props.len(), 1);
        assert_eq!(props[0].name(), "extent");
    }
}
