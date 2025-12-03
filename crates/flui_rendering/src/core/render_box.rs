//! RenderBox trait for box protocol render objects.
//!
//! This module provides the `RenderBox<A>` trait for implementing render objects
//! that use the 2D box layout protocol with compile-time arity validation.
//!
//! # Design Philosophy
//!
//! - **Simple and clean**: Minimal API surface, no unnecessary abstractions
//! - **Arity validation**: Compile-time child count constraints
//! - **Context-based**: All operations use typed contexts
//! - **Progressive disclosure**: Simple defaults, explicit when needed
//!
//! # Trait Hierarchy
//!
//! ```text
//! RenderObject (base trait)
//!     │
//!     └── RenderBox<A> (box protocol with arity A)
//!             │
//!             ├── layout(ctx) -> Size
//!             ├── paint(ctx)
//!             └── hit_test(ctx, result) -> bool
//! ```
//!
//! # Arity System
//!
//! | Arity | Children | Examples |
//! |-------|----------|----------|
//! | `Leaf` | 0 | Text, Image, Icon |
//! | `Single` | 1 | Padding, Transform, Align |
//! | `Variable` | 0+ | Flex, Stack, Column, Row |
//!
//! # Examples
//!
//! ## Leaf Element (0 children)
//!
//! ```rust,ignore
//! use flui_rendering::core::{RenderBox, RenderObject, Leaf};
//!
//! #[derive(Debug)]
//! struct RenderText {
//!     text: String,
//!     color: Color,
//! }
//!
//! impl RenderBox<Leaf> for RenderText {
//!     fn layout(&mut self, ctx: LayoutContext<'_, Leaf>) -> Size {
//!         let measured = self.measure_text(&self.text);
//!         ctx.constraints.constrain(measured)
//!     }
//!
//!     fn paint(&self, ctx: &mut PaintContext<'_, Leaf>) {
//!         ctx.canvas_mut().draw_text(&self.text, ctx.offset, &self.paint);
//!     }
//! }
//!
//! impl RenderObject for RenderText {
//!     fn as_any(&self) -> &dyn std::any::Any { self }
//!     fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
//! }
//! ```
//!
//! ## Single Child Wrapper (1 child)
//!
//! ```rust,ignore
//! #[derive(Debug)]
//! struct RenderPadding {
//!     padding: EdgeInsets,
//! }
//!
//! impl RenderBox<Single> for RenderPadding {
//!     fn layout(&mut self, mut ctx: LayoutContext<'_, Single>) -> Size {
//!         let inner = ctx.constraints.deflate(&self.padding);
//!         let child_size = ctx.layout_single_child_with(|_| inner);
//!         child_size + self.padding.size()
//!     }
//!
//!     fn paint(&self, ctx: &mut PaintContext<'_, Single>) {
//!         ctx.paint_single_child(self.padding.top_left());
//!     }
//! }
//!
//! impl RenderObject for RenderPadding {
//!     fn as_any(&self) -> &dyn std::any::Any { self }
//!     fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
//! }
//! ```
//!
//! ## Multi-Child Layout (Variable children)
//!
//! ```rust,ignore
//! #[derive(Debug)]
//! struct RenderRow {
//!     spacing: f32,
//! }
//!
//! impl RenderBox<Variable> for RenderRow {
//!     fn layout(&mut self, mut ctx: BoxLayoutContext<'_, Variable>) -> Size {
//!         let mut x = 0.0;
//!         let mut max_height = 0.0;
//!
//!         for child_id in ctx.children() {
//!             let size = ctx.layout_child(child_id, ctx.constraints);
//!             ctx.set_child_offset(child_id, Offset::new(x, 0.0));
//!             x += size.width + self.spacing;
//!             max_height = max_height.max(size.height);
//!         }
//!
//!         if x > 0.0 {
//!             x -= self.spacing;
//!         }
//!
//!         Size::new(x, max_height)
//!     }
//!
//!     fn paint(&self, ctx: &mut BoxPaintContext<'_, Variable>) {
//!         ctx.paint_all_children();
//!     }
//! }
//!
//! impl RenderObject for RenderRow {
//!     fn as_any(&self) -> &dyn std::any::Any { self }
//!     fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
//! }
//! ```

use std::fmt;

use super::arity::Arity;
use super::contexts::{BoxHitTestContext, BoxLayoutContext, BoxPaintContext};
use super::render_object::RenderObject;
use crate::RenderResult;
use flui_interaction::HitTestResult;
use flui_types::{BoxConstraints, Rect, Size};

// ============================================================================
// CORE RENDER BOX TRAIT
// ============================================================================

/// Render trait for box protocol with arity validation.
///
/// This trait provides the foundation for implementing render objects that use
/// the 2D box layout protocol.
///
/// # Type Parameters
///
/// - `A`: Arity type constraining the number of children
///
/// # Required Methods
///
/// - [`layout`](Self::layout) - Computes size given constraints
/// - [`paint`](Self::paint) - Draws to canvas
///
/// # Optional Methods
///
/// - [`hit_test`](Self::hit_test) - Pointer event detection (default: bounds check)
///
/// # Context API
///
/// All methods use contexts with sensible defaults:
///
/// ```rust,ignore
/// // Minimal - uses default BoxProtocol
/// fn layout(&mut self, ctx: LayoutContext<'_, Single>) -> Size
///
/// // Type alias - explicit protocol
/// fn layout(&mut self, ctx: BoxLayoutContext<'_, Variable>) -> Size
/// ```
pub trait RenderBox<A: Arity>: RenderObject + fmt::Debug + Send + Sync {
    /// Computes the size of this render object.
    ///
    /// # Context
    ///
    /// - `ctx.constraints` - Layout constraints from parent
    /// - `ctx.children()` - Iterator over child ElementIds
    /// - `ctx.layout_child(id, c)` - Layout a child (returns Size, fallback on error)
    /// - `ctx.set_child_offset(id, offset)` - Position a child
    ///
    /// # Returns
    ///
    /// The computed size. If layout fails, return a fallback size (e.g., Size::ZERO).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Leaf
    /// fn layout(&mut self, ctx: LayoutContext<'_, Leaf>) -> Size {
    ///     let intrinsic = self.compute_intrinsic_size();
    ///     ctx.constraints.constrain(intrinsic)
    /// }
    ///
    /// // Single child
    /// fn layout(&mut self, mut ctx: LayoutContext<'_, Single>) -> Size {
    ///     let child_size = ctx.layout_single_child();
    ///     child_size + self.padding.size()
    /// }
    ///
    /// // Multiple children
    /// fn layout(&mut self, mut ctx: BoxLayoutContext<'_, Variable>) -> Size {
    ///     let mut total = 0.0;
    ///     for child_id in ctx.children() {
    ///         let size = ctx.layout_child(child_id, ctx.constraints);
    ///         total += size.width;
    ///     }
    ///     Size::new(total, 0.0)
    /// }
    /// ```
    fn layout(&mut self, ctx: BoxLayoutContext<'_, A>) -> RenderResult<Size>;

    /// Paints this render object to the canvas.
    ///
    /// # Context
    ///
    /// - `ctx.offset` - Position in parent coordinates
    /// - `ctx.geometry` - Size from layout (this is `Size` for BoxProtocol)
    /// - `ctx.canvas_mut()` - Mutable canvas for drawing
    /// - `ctx.children()` - Iterator over child ElementIds
    /// - `ctx.paint_child(id, offset)` - Paint a child
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Leaf
    /// fn paint(&self, ctx: &mut PaintContext<'_, Leaf>) {
    ///     let size = ctx.geometry;
    ///     ctx.canvas_mut().draw_rect(Rect::from_size(size), &self.paint);
    /// }
    ///
    /// // Single child
    /// fn paint(&self, ctx: &mut PaintContext<'_, Single>) {
    ///     ctx.paint_single_child(Offset::ZERO);
    /// }
    ///
    /// // Multiple children
    /// fn paint(&self, ctx: &mut BoxPaintContext<'_, Variable>) {
    ///     ctx.paint_all_children();
    /// }
    /// ```
    fn paint(&self, ctx: &mut BoxPaintContext<'_, A>);

    /// Performs hit testing for pointer events.
    ///
    /// # Context
    ///
    /// - `ctx.position` - Position to test
    /// - `ctx.geometry` - Size from layout
    /// - `ctx.children()` - Iterator over children
    /// - `ctx.hit_test_child(id, pos, result)` - Test a child
    /// - `ctx.hit_test_self(result)` - Add self to result
    ///
    /// # Default Implementation
    ///
    /// The default checks bounds and tests children in reverse z-order.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // Custom circular hit test
    /// fn hit_test(&self, ctx: &BoxHitTestContext<'_, Leaf>, result: &mut HitTestResult) -> bool {
    ///     let center = ctx.size() / 2.0;
    ///     let radius = center.width.min(center.height);
    ///     let distance = (ctx.position - center).length();
    ///
    ///     if distance <= radius {
    ///         ctx.hit_test_self(result);
    ///         true
    ///     } else {
    ///         false
    ///     }
    /// }
    /// ```
    fn hit_test(&self, ctx: &BoxHitTestContext<'_, A>, result: &mut HitTestResult) -> bool {
        // Default: rectangular hit testing
        if !ctx.contains_position() {
            return false;
        }

        // Test children in reverse z-order (front to back)
        for child_id in ctx.children_reverse() {
            if ctx.hit_test_child(child_id, ctx.position, result) {
                return true;
            }
        }

        // Add self to result
        ctx.hit_test_self(result)
    }

    /// Computes the minimum intrinsic width for a given height.
    ///
    /// Returns `None` by default (no intrinsic size preference).
    fn intrinsic_width(&self, _height: f32) -> Option<f32> {
        None
    }

    /// Computes the minimum intrinsic height for a given width.
    ///
    /// Returns `None` by default (no intrinsic size preference).
    fn intrinsic_height(&self, _width: f32) -> Option<f32> {
        None
    }

    /// Gets the baseline offset for text alignment.
    ///
    /// Returns `None` by default (no meaningful baseline).
    fn baseline_offset(&self) -> Option<f32> {
        None
    }

    /// Performs a "dry layout" without side effects.
    ///
    /// Used for intrinsic sizing calculations. Returns the smallest size by default.
    fn compute_dry_layout(&self, constraints: BoxConstraints) -> Size {
        constraints.smallest()
    }

    /// Gets the local bounding rectangle.
    ///
    /// Default returns an empty rectangle. Override for proper hit testing.
    fn local_bounds(&self) -> Rect {
        Rect::ZERO
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::arity::{Leaf, Single, Variable};
    use flui_types::BoxConstraints;
    use std::marker::PhantomData;

    // Simple test render box
    #[derive(Debug)]
    struct TestRenderBox<A: Arity> {
        size: Size,
        _phantom: PhantomData<A>,
    }

    impl<A: Arity> TestRenderBox<A> {
        fn new(size: Size) -> Self {
            Self {
                size,
                _phantom: PhantomData,
            }
        }
    }

    impl<A: Arity> RenderBox<A> for TestRenderBox<A> {
        fn layout(&mut self, ctx: BoxLayoutContext<'_, A>) -> RenderResult<Size> {
            Ok(ctx.constraints.constrain(self.size))
        }

        fn paint(&self, _ctx: &mut BoxPaintContext<'_, A>) {
            // Do nothing
        }
    }

    impl<A: Arity> RenderObject for TestRenderBox<A> {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    #[test]
    fn test_render_box_arity() {
        let _leaf: TestRenderBox<Leaf> = TestRenderBox::new(Size::new(100.0, 50.0));
        let _single: TestRenderBox<Single> = TestRenderBox::new(Size::new(100.0, 50.0));
        let _variable: TestRenderBox<Variable> = TestRenderBox::new(Size::new(100.0, 50.0));
        // Compiles = arity system works
    }

    #[test]
    fn test_default_intrinsic_size() {
        let render = TestRenderBox::<Leaf>::new(Size::new(100.0, 50.0));
        assert_eq!(render.intrinsic_width(50.0), None);
        assert_eq!(render.intrinsic_height(100.0), None);
    }

    #[test]
    fn test_default_baseline() {
        let render = TestRenderBox::<Leaf>::new(Size::new(100.0, 50.0));
        assert_eq!(render.baseline_offset(), None);
    }

    #[test]
    fn test_compute_dry_layout() {
        let render = TestRenderBox::<Leaf>::new(Size::new(100.0, 50.0));
        let constraints = BoxConstraints::tight(Size::new(80.0, 40.0));
        let size = render.compute_dry_layout(constraints);
        assert_eq!(size, Size::new(80.0, 40.0));
    }
}
