//! Box protocol render trait.
//!
//! This module provides the `RenderBox<A>` trait for implementing render objects
//! that use the standard 2D box layout protocol.
//!
//! # Architecture
//!
//! ```text
//! RenderBox<A> trait
//! ├── layout() → Size
//! ├── paint() → Canvas
//! └── hit_test() → bool
//! ```
//!
//! # Design
//!
//! `RenderBox` is generic over:
//! - `A`: Arity - compile-time child count (Leaf, Single, Optional, Variable)
//! - `T`: Tree type - the tree implementation providing child access
//!
//! This design keeps the trait independent of concrete tree implementations.

use flui_foundation::ElementId;
use flui_interaction::HitTestResult;
use flui_painting::Canvas;
use flui_types::{Offset, Size};
use std::fmt::Debug;

use super::arity::{Arity, Leaf, Optional, Single, Variable};
use super::protocol::BoxConstraints;

// ============================================================================
// RENDER BOX TRAIT
// ============================================================================

/// Box protocol render trait.
///
/// Implement this trait for render objects that use the standard 2D box layout.
///
/// # Type Parameters
///
/// - `A`: Arity - compile-time child count (Leaf, Single, Optional, Variable)
///
/// # Example
///
/// ```rust,ignore
/// impl RenderBox<Leaf> for RenderColoredBox {
///     fn layout(&mut self, constraints: BoxConstraints, _children: &[ElementId]) -> Size {
///         constraints.constrain(Size::new(100.0, 100.0))
///     }
///
///     fn paint(&self, offset: Offset, _children: &[ElementId]) -> Canvas {
///         let mut canvas = Canvas::new();
///         canvas.draw_rect(Rect::from_size(self.size), self.color);
///         canvas
///     }
/// }
/// ```
pub trait RenderBox<A: Arity>: Send + Sync + Debug + 'static {
    /// Computes the size of this render object given constraints.
    ///
    /// # Arguments
    ///
    /// * `constraints` - Box constraints from parent
    /// * `children` - Slice of child element IDs
    /// * `layout_child` - Callback to layout a child: `(child_id, constraints) -> Size`
    ///
    /// # Returns
    ///
    /// The computed size that satisfies the constraints.
    fn layout(
        &mut self,
        constraints: BoxConstraints,
        children: &[ElementId],
        layout_child: &mut dyn FnMut(ElementId, BoxConstraints) -> Size,
    ) -> Size;

    /// Paints this render object to a canvas.
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
    /// * `size` - Computed size from layout
    /// * `children` - Slice of child element IDs
    /// * `hit_test_child` - Callback to hit test a child: `(child_id, position) -> bool`
    ///
    /// # Returns
    ///
    /// `true` if this element or any child was hit.
    fn hit_test(
        &self,
        position: Offset,
        size: Size,
        children: &[ElementId],
        hit_test_child: &mut dyn FnMut(ElementId, Offset) -> bool,
    ) -> bool {
        // Default: test children first, then self
        for &child in children {
            if hit_test_child(child, position) {
                return true;
            }
        }
        self.hit_test_self(position, size)
    }

    /// Tests if the position hits this render object (excluding children).
    ///
    /// Override for opaque hit testing (e.g., buttons, interactive areas).
    /// Default returns `false` (transparent to hit testing).
    fn hit_test_self(&self, _position: Offset, _size: Size) -> bool {
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

/// Extension trait for ergonomic render box operations.
pub trait RenderBoxExt<A: Arity>: RenderBox<A> {
    /// Checks if position is within the given size bounds.
    fn contains(&self, position: Offset, size: Size) -> bool {
        position.dx >= 0.0
            && position.dy >= 0.0
            && position.dx < size.width
            && position.dy < size.height
    }
}

impl<A: Arity, R: RenderBox<A>> RenderBoxExt<A> for R {}

// ============================================================================
// EMPTY RENDER
// ============================================================================

/// Empty render object with zero size.
///
/// Used for `Option::None` and placeholder elements.
#[derive(Debug, Clone, Copy, Default)]
pub struct EmptyRender;

impl RenderBox<Leaf> for EmptyRender {
    fn layout(
        &mut self,
        _constraints: BoxConstraints,
        _children: &[ElementId],
        _layout_child: &mut dyn FnMut(ElementId, BoxConstraints) -> Size,
    ) -> Size {
        Size::ZERO
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

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::Color;

    #[derive(Debug)]
    struct TestColoredBox {
        color: Color,
        preferred_size: Size,
    }

    impl RenderBox<Leaf> for TestColoredBox {
        fn layout(
            &mut self,
            constraints: BoxConstraints,
            _children: &[ElementId],
            _layout_child: &mut dyn FnMut(ElementId, BoxConstraints) -> Size,
        ) -> Size {
            constraints.constrain(self.preferred_size)
        }

        fn paint(
            &self,
            _offset: Offset,
            _children: &[ElementId],
            _paint_child: &mut dyn FnMut(ElementId, Offset) -> Canvas,
        ) -> Canvas {
            let mut canvas = Canvas::new();
            // Would draw colored rect here
            let _ = self.color;
            canvas
        }

        fn hit_test_self(&self, position: Offset, size: Size) -> bool {
            self.contains(position, size)
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    #[test]
    fn test_empty_render() {
        let mut empty = EmptyRender;
        let constraints = BoxConstraints::default();
        let size = empty.layout(constraints, &[], &mut |_, _| Size::ZERO);
        assert_eq!(size, Size::ZERO);
    }

    #[test]
    fn test_colored_box_layout() {
        let mut colored = TestColoredBox {
            color: Color::RED,
            preferred_size: Size::new(100.0, 50.0),
        };

        let constraints = BoxConstraints::tight(Size::new(80.0, 40.0));
        let size = colored.layout(constraints, &[], &mut |_, _| Size::ZERO);
        assert_eq!(size, Size::new(80.0, 40.0));
    }

    #[test]
    fn test_hit_test_self() {
        let colored = TestColoredBox {
            color: Color::BLUE,
            preferred_size: Size::new(100.0, 50.0),
        };

        let size = Size::new(100.0, 50.0);
        assert!(colored.hit_test_self(Offset::new(50.0, 25.0), size));
        assert!(!colored.hit_test_self(Offset::new(150.0, 25.0), size));
        assert!(!colored.hit_test_self(Offset::new(-10.0, 25.0), size));
    }
}
