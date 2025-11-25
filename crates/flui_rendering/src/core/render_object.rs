//! Type-erased render object trait.
//!
//! This module provides `RenderObject`, the main interface for render objects.
//! It enables type erasure over Protocol and Arity while maintaining type safety
//! at the boundary.
//!
//! # Architecture
//!
//! ```text
//! RenderBox<A> → BoxRenderWrapper → Box<dyn RenderObject>
//! SliverRender<A> → SliverRenderWrapper → Box<dyn RenderObject>
//! ```
//!
//! # Type Erasure
//!
//! The `Constraints` and `Geometry` enums (from `geometry.rs`) provide runtime
//! discrimination between Box and Sliver protocols. The wrappers in `wrappers.rs`
//! convert between typed and type-erased representations.

use flui_foundation::ElementId;
use flui_painting::Canvas;
use flui_types::Offset;
use std::any::Any;
use std::fmt::Debug;

use super::geometry::{Constraints, Geometry};

// ============================================================================
// RenderObject Trait
// ============================================================================

/// Type-erased render object trait.
///
/// This trait provides a uniform interface for all render objects regardless
/// of their protocol (Box/Sliver) or arity (Leaf/Single/Variable).
///
/// # Implementation
///
/// Don't implement this trait directly. Instead, implement `RenderBox<A>` or
/// `SliverRender<A>`, which will be automatically wrapped via `BoxRenderWrapper`
/// or `SliverRenderWrapper`.
///
/// # Design
///
/// The trait is designed to be tree-agnostic. Layout, paint, and hit-test methods
/// receive children as a slice of `ElementId`, allowing the concrete tree
/// implementation to be provided at call time.
pub trait RenderObject: Send + Sync + Debug {
    /// Computes layout and returns geometry.
    ///
    /// Called during layout phase. The implementation should:
    /// 1. Layout children as needed using the provided child IDs
    /// 2. Compute own size based on constraints and child sizes
    /// 3. Return the computed geometry
    ///
    /// # Arguments
    ///
    /// * `children` - Slice of child element IDs
    /// * `constraints` - Type-erased constraints from parent
    /// * `layout_child` - Callback to layout a child and get its geometry
    fn layout(
        &mut self,
        children: &[ElementId],
        constraints: &Constraints,
        layout_child: &mut dyn FnMut(ElementId, Constraints) -> Geometry,
    ) -> Geometry;

    /// Paints to a canvas.
    ///
    /// Called during paint phase. The implementation should:
    /// 1. Draw own content to the canvas
    /// 2. Paint children at appropriate offsets
    /// 3. Return the combined canvas
    ///
    /// # Arguments
    ///
    /// * `children` - Slice of child element IDs
    /// * `offset` - Offset in parent's coordinate space
    /// * `paint_child` - Callback to paint a child and get its canvas
    fn paint(
        &self,
        children: &[ElementId],
        offset: Offset,
        paint_child: &mut dyn FnMut(ElementId, Offset) -> Canvas,
    ) -> Canvas;

    /// Performs hit testing.
    ///
    /// Called during pointer event routing to determine which element was hit.
    ///
    /// # Arguments
    ///
    /// * `children` - Slice of child element IDs
    /// * `position` - Position in local coordinates
    /// * `geometry` - Computed geometry from layout
    ///
    /// # Returns
    ///
    /// `true` if this element or any child was hit
    fn hit_test(&self, children: &[ElementId], position: Offset, geometry: &Geometry) -> bool;

    /// Returns a debug name for this render object.
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Downcasts to concrete type for inspection.
    fn as_any(&self) -> &dyn Any;

    /// Downcasts to mutable concrete type for mutation.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::Size;

    // Simple test render object
    #[derive(Debug)]
    struct TestRenderBox {
        size: Size,
    }

    impl RenderObject for TestRenderBox {
        fn layout(
            &mut self,
            _children: &[ElementId],
            constraints: &Constraints,
            _layout_child: &mut dyn FnMut(ElementId, Constraints) -> Geometry,
        ) -> Geometry {
            let box_constraints = constraints.as_box();
            let size = box_constraints.constrain(self.size);
            Geometry::Box(size)
        }

        fn paint(
            &self,
            _children: &[ElementId],
            _offset: Offset,
            _paint_child: &mut dyn FnMut(ElementId, Offset) -> Canvas,
        ) -> Canvas {
            Canvas::new()
        }

        fn hit_test(&self, _children: &[ElementId], position: Offset, geometry: &Geometry) -> bool {
            let size = geometry.as_box();
            position.dx >= 0.0
                && position.dy >= 0.0
                && position.dx < size.width
                && position.dy < size.height
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    #[test]
    fn test_render_object_layout() {
        let mut render = TestRenderBox {
            size: Size::new(100.0, 50.0),
        };

        let constraints = Constraints::Box(flui_types::constraints::BoxConstraints::tight(
            Size::new(80.0, 40.0),
        ));

        let geometry = render.layout(&[], &constraints, &mut |_, _| Geometry::default());

        assert_eq!(geometry.as_box(), Size::new(80.0, 40.0));
    }

    #[test]
    fn test_render_object_hit_test() {
        let render = TestRenderBox {
            size: Size::new(100.0, 50.0),
        };

        let geometry = Geometry::Box(Size::new(100.0, 50.0));

        assert!(render.hit_test(&[], Offset::new(50.0, 25.0), &geometry));
        assert!(!render.hit_test(&[], Offset::new(150.0, 25.0), &geometry));
        assert!(!render.hit_test(&[], Offset::new(-10.0, 25.0), &geometry));
    }
}
