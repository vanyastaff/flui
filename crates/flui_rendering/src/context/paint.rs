//! PaintContext - high-level painting context with offset and children access.
//!
//! This module provides [`PaintContext`], a wrapper around [`CanvasContext`] that
//! adds offset tracking and type-safe children access for painting operations.
//!
//! # Architecture
//!
//! ```text
//! RenderBox::paint() / RenderSliver::paint()
//!     │
//!     ▼
//! PaintContext (high-level: offset + children)
//!     │
//!     ▼
//! CanvasContext (low-level: canvas + layers)
//!     │
//!     ▼
//! LayerTree + Picture layers
//! ```
//!
//! # Usage
//!
//! ```ignore
//! fn paint(&self, ctx: &mut BoxPaintContext<'_, Single, BoxParentData>) {
//!     // Access offset
//!     let offset = ctx.offset();
//!
//!     // Draw using canvas
//!     ctx.canvas().draw_rect(rect.translate(offset), &paint);
//!
//!     // Paint children
//!     ctx.paint_children();
//! }
//! ```

use flui_types::Offset;

use crate::arity::Arity;
use crate::children_access::ChildrenAccess;
use crate::parent_data::ParentData;
use crate::protocol::Protocol;

use super::canvas::CanvasContext;
use super::Canvas;

// ============================================================================
// PaintContext
// ============================================================================

/// High-level painting context with offset and children access.
///
/// `PaintContext` wraps a [`CanvasContext`] and provides:
/// - Current paint offset
/// - Type-safe access to children for painting
/// - Convenience methods for common paint operations
///
/// # Type Parameters
///
/// - `P`: The protocol (`BoxProtocol` or `SliverProtocol`)
/// - `A`: The arity (number of children)
/// - `PD`: The parent data type
///
/// # Example
///
/// ```ignore
/// impl RenderBox for MyWidget {
///     type Arity = Single;
///     type ParentData = BoxParentData;
///
///     fn paint(&self, ctx: &mut BoxPaintContext<'_, Single, BoxParentData>) {
///         // Get offset
///         let offset = ctx.offset();
///
///         // Draw background
///         ctx.canvas().draw_rect(
///             Rect::from_size(self.size).translate(offset),
///             &Paint::fill(Color::WHITE)
///         );
///
///         // Paint child at its position
///         ctx.paint_child(0, child_offset);
///     }
/// }
/// ```
pub struct PaintContext<'ctx, P: Protocol, A: Arity, PD: ParentData + Default> {
    /// The underlying canvas context for low-level operations.
    inner: &'ctx mut CanvasContext,

    /// Current paint offset from parent.
    offset: Offset,

    /// Access to children for painting.
    children: &'ctx mut ChildrenAccess<'ctx, A, PD>,

    /// Phantom for protocol type.
    _protocol: std::marker::PhantomData<P>,
}

impl<'ctx, P: Protocol, A: Arity, PD: ParentData + Default> PaintContext<'ctx, P, A, PD> {
    /// Creates a new paint context.
    pub fn new(
        inner: &'ctx mut CanvasContext,
        offset: Offset,
        children: &'ctx mut ChildrenAccess<'ctx, A, PD>,
    ) -> Self {
        Self {
            inner,
            offset,
            children,
            _protocol: std::marker::PhantomData,
        }
    }

    // ========================================================================
    // Offset Access
    // ========================================================================

    /// Returns the current paint offset.
    ///
    /// This is the offset from the parent's origin to this object's origin.
    #[inline]
    pub fn offset(&self) -> Offset {
        self.offset
    }

    /// Returns the X component of the paint offset.
    #[inline]
    pub fn offset_x(&self) -> f32 {
        self.offset.dx
    }

    /// Returns the Y component of the paint offset.
    #[inline]
    pub fn offset_y(&self) -> f32 {
        self.offset.dy
    }

    // ========================================================================
    // Canvas Access
    // ========================================================================

    /// Returns the canvas for drawing operations.
    ///
    /// Use this to draw shapes, text, images, etc.
    ///
    /// # Example
    ///
    /// ```ignore
    /// ctx.canvas().draw_rect(rect, &paint);
    /// ctx.canvas().draw_circle(center, radius, &paint);
    /// ```
    #[inline]
    pub fn canvas(&mut self) -> &mut Canvas {
        self.inner.canvas()
    }

    /// Returns a reference to the underlying canvas context.
    #[inline]
    pub fn canvas_context(&self) -> &CanvasContext {
        self.inner
    }

    /// Returns a mutable reference to the underlying canvas context.
    ///
    /// Use this for advanced layer operations like `push_opacity`, `push_transform`, etc.
    #[inline]
    pub fn canvas_context_mut(&mut self) -> &mut CanvasContext {
        self.inner
    }

    // ========================================================================
    // Children Access
    // ========================================================================

    /// Returns mutable access to children.
    #[inline]
    pub fn children_mut(&mut self) -> &mut ChildrenAccess<'ctx, A, PD> {
        self.children
    }

    /// Returns the number of children.
    #[inline]
    pub fn child_count(&self) -> usize {
        self.children.len()
    }

    /// Returns whether there are any children.
    #[inline]
    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    // ========================================================================
    // Scoped Operations
    // ========================================================================

    /// Executes a closure with a translated offset.
    ///
    /// The canvas state is saved before and restored after the closure.
    pub fn with_offset<F, R>(&mut self, additional_offset: Offset, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        let original_offset = self.offset;
        self.offset = original_offset + additional_offset;
        self.inner.canvas().save();
        self.inner
            .canvas()
            .translate(additional_offset.dx, additional_offset.dy);
        let result = f(self);
        self.inner.canvas().restore();
        self.offset = original_offset;
        result
    }

    /// Executes a closure with a saved canvas state.
    pub fn with_save<F, R>(&mut self, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.inner.canvas().save();
        let result = f(self);
        self.inner.canvas().restore();
        result
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::Rect;

    #[test]
    fn test_canvas_context_new() {
        let bounds = Rect::from_ltrb(0.0, 0.0, 100.0, 100.0);
        let ctx = CanvasContext::new(bounds);
        assert_eq!(ctx.estimated_bounds(), bounds);
    }
}
