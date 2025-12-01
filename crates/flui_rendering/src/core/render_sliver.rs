//! Sliver protocol render trait.
//!
//! This module provides the `SliverRender<A>` trait for implementing render objects
//! that participate in scrollable layouts (viewports).
//!
//! # Sliver vs Box
//!
//! - **Box**: Fixed 2D layout with `BoxConstraints` → `Size`
//! - **Sliver**: Scrollable layout with `SliverConstraints` → `SliverGeometry`
//!
//! # Architecture
//!
//! ```text
//! SliverRender<A> trait
//! ├── layout() → SliverGeometry
//! ├── paint() → Canvas
//! └── hit_test() → bool
//! ```

use flui_foundation::ElementId;
use flui_painting::Canvas;
use flui_types::{Offset, SliverConstraints, SliverGeometry};
use std::fmt::Debug;

use super::arity::Arity;

// ============================================================================
// SLIVER RENDER TRAIT
// ============================================================================

/// Sliver protocol render trait.
///
/// Implement this trait for render objects that participate in scrollable layouts.
///
/// # Type Parameters
///
/// - `A`: Arity - compile-time child count (Leaf, Single, Variable, etc.)
///
/// # Example
///
/// ```rust,ignore
/// impl SliverRender<Variable> for RenderSliverList {
///     fn layout(
///         &mut self,
///         constraints: SliverConstraints,
///         children: &[ElementId],
///         layout_child: &mut dyn FnMut(ElementId, SliverConstraints) -> SliverGeometry,
///     ) -> SliverGeometry {
///         // Layout children and compute sliver geometry
///         SliverGeometry {
///             scroll_extent: total_height,
///             paint_extent: visible_height,
///             ..Default::default()
///         }
///     }
/// }
/// ```
pub trait SliverRender<A: Arity>: Send + Sync + Debug + 'static {
    /// Computes the sliver geometry given constraints.
    ///
    /// # Arguments
    ///
    /// * `constraints` - Sliver constraints from viewport
    /// * `children` - Slice of child element IDs
    /// * `layout_child` - Callback to layout a child: `(child_id, constraints) -> SliverGeometry`
    ///
    /// # Returns
    ///
    /// `SliverGeometry` describing scroll extent, paint extent, layout extent,
    /// and other properties for viewport integration.
    fn layout(
        &mut self,
        constraints: SliverConstraints,
        children: &[ElementId],
        layout_child: &mut dyn FnMut(ElementId, SliverConstraints) -> SliverGeometry,
    ) -> SliverGeometry;

    /// Paints the sliver to a canvas.
    ///
    /// # Arguments
    ///
    /// * `offset` - Offset in parent's coordinate space
    /// * `children` - Slice of child element IDs
    /// * `paint_child` - Callback to paint a child: `(child_id, offset) -> Canvas`
    ///
    /// # Returns
    ///
    /// A canvas with all drawing operations.
    fn paint(
        &self,
        offset: Offset,
        children: &[ElementId],
        paint_child: &mut dyn FnMut(ElementId, Offset) -> Canvas,
    ) -> Canvas;

    /// Performs hit testing for pointer events.
    ///
    /// # Arguments
    ///
    /// * `position` - Position in local coordinates
    /// * `geometry` - Computed geometry from layout
    /// * `children` - Slice of child element IDs
    /// * `hit_test_child` - Callback to hit test a child: `(child_id, position) -> bool`
    ///
    /// # Returns
    ///
    /// `true` if this element or any child was hit.
    fn hit_test(
        &self,
        position: Offset,
        geometry: &SliverGeometry,
        children: &[ElementId],
        hit_test_child: &mut dyn FnMut(ElementId, Offset) -> bool,
    ) -> bool {
        // Default: test children first
        for &child in children {
            if hit_test_child(child, position) {
                return true;
            }
        }
        self.hit_test_self(position, geometry)
    }

    /// Tests if the position hits this sliver (excluding children).
    ///
    /// Default returns `false` (transparent to hit testing).
    fn hit_test_self(&self, _position: Offset, _geometry: &SliverGeometry) -> bool {
        false
    }

    /// Returns a debug name for this render object.
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Downcasts to concrete type for inspection.
    fn as_any(&self) -> &dyn std::any::Any;

    /// Downcasts to mutable concrete type for mutation.
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

// ============================================================================
// EXTENSION TRAIT
// ============================================================================

/// Extension trait for ergonomic sliver render operations.
pub trait SliverRenderExt<A: Arity>: SliverRender<A> {
    /// Checks if position is within the paint extent.
    fn is_in_paint_extent(&self, position: Offset, geometry: &SliverGeometry) -> bool {
        position.dy >= 0.0 && position.dy < geometry.paint_extent
    }
}

impl<A: Arity, R: SliverRender<A>> SliverRenderExt<A> for R {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::arity::Leaf;

    #[derive(Debug)]
    struct TestSliverBox {
        extent: f32,
    }

    impl SliverRender<Leaf> for TestSliverBox {
        fn layout(
            &mut self,
            constraints: SliverConstraints,
            _children: &[ElementId],
            _layout_child: &mut dyn FnMut(ElementId, SliverConstraints) -> SliverGeometry,
        ) -> SliverGeometry {
            let paint_extent = self.extent.min(constraints.remaining_paint_extent);
            SliverGeometry {
                scroll_extent: self.extent,
                paint_extent,
                layout_extent: paint_extent,
                max_paint_extent: self.extent,
                ..Default::default()
            }
        }

        fn paint(
            &self,
            _offset: Offset,
            _children: &[ElementId],
            _paint_child: &mut dyn FnMut(ElementId, Offset) -> Canvas,
        ) -> Canvas {
            Canvas::new()
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    #[test]
    fn test_sliver_layout() {
        let mut sliver = TestSliverBox { extent: 200.0 };

        let constraints = SliverConstraints {
            remaining_paint_extent: 100.0,
            ..Default::default()
        };

        let geometry = sliver.layout(constraints, &[], &mut |_, _| SliverGeometry::default());

        assert_eq!(geometry.scroll_extent, 200.0);
        assert_eq!(geometry.paint_extent, 100.0);
    }
}
