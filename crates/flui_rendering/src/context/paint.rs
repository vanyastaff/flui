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
//!     ├── paint_child(index) → recursively paints child
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
//!     // Draw background
//!     let rect = Rect::from_size(self.size).translate(ctx.offset());
//!     ctx.canvas().draw_rect(rect, &paint);
//!
//!     // Paint child (pipeline handles the recursion)
//!     ctx.paint_child();
//!
//!     // Draw foreground overlay
//!     ctx.canvas().draw_rect(overlay_rect, &overlay_paint);
//! }
//! ```

use flui_types::Offset;

use crate::arity::{Arity, Leaf, Optional, Single, Variable};
use crate::parent_data::ParentData;
use crate::protocol::Protocol;

use super::canvas::CanvasContext;
use super::Canvas;

// ============================================================================
// PaintChildCallback - Function type for painting children
// ============================================================================

/// Callback type for painting a child at a given index with offset.
///
/// The pipeline provides this callback which has access to the render tree
/// and can recursively paint child render objects.
pub type PaintChildCallback<'a> = &'a mut dyn FnMut(usize, Offset);

// ============================================================================
// ChildPaintInfo - Information about a child for painting
// ============================================================================

/// Information about a child needed for painting.
#[derive(Debug, Clone, Copy)]
pub struct ChildPaintInfo {
    /// The offset of this child from parent's origin.
    pub offset: Offset,
}

// ============================================================================
// PaintContext
// ============================================================================

/// High-level painting context with offset and children access.
///
/// `PaintContext` wraps a [`CanvasContext`] and provides:
/// - Current paint offset
/// - Methods to paint children at their positions
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
///         // Draw background at current offset
///         let rect = Rect::from_size(self.size).translate(ctx.offset());
///         ctx.canvas().draw_rect(rect, &Paint::fill(Color::WHITE));
///
///         // Paint the single child
///         ctx.paint_child();
///     }
/// }
/// ```
pub struct PaintContext<'ctx, P: Protocol, A: Arity, PD: ParentData + Default> {
    /// The underlying canvas context for low-level operations.
    inner: &'ctx mut CanvasContext,

    /// Current paint offset from parent.
    offset: Offset,

    /// Information about children (offsets, etc.)
    children_info: &'ctx [ChildPaintInfo],

    /// Callback to paint a child (provided by pipeline).
    paint_child_callback: PaintChildCallback<'ctx>,

    /// Phantom for protocol and parent data types.
    _phantom: std::marker::PhantomData<(P, A, PD)>,
}

impl<'ctx, P: Protocol, A: Arity, PD: ParentData + Default> PaintContext<'ctx, P, A, PD> {
    /// Creates a new paint context.
    pub fn new(
        inner: &'ctx mut CanvasContext,
        offset: Offset,
        children_info: &'ctx [ChildPaintInfo],
        paint_child_callback: PaintChildCallback<'ctx>,
    ) -> Self {
        Self {
            inner,
            offset,
            children_info,
            paint_child_callback,
            _phantom: std::marker::PhantomData,
        }
    }

    // ========================================================================
    // Offset Access
    // ========================================================================

    /// Returns the current paint offset.
    ///
    /// This is the offset from the root to this object's origin.
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
    // Children Info
    // ========================================================================

    /// Returns the number of children.
    #[inline]
    pub fn child_count(&self) -> usize {
        self.children_info.len()
    }

    /// Returns whether there are any children.
    #[inline]
    pub fn has_children(&self) -> bool {
        !self.children_info.is_empty()
    }

    /// Returns the offset of a child.
    #[inline]
    pub fn child_offset(&self, index: usize) -> Option<Offset> {
        self.children_info.get(index).map(|info| info.offset)
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
// Leaf Arity - No paint_child methods
// ============================================================================

impl<'ctx, P: Protocol, PD: ParentData + Default> PaintContext<'ctx, P, Leaf, PD> {
    // Leaf has no children, so no paint_child methods
}

// ============================================================================
// Optional Arity - paint_child for 0 or 1 child
// ============================================================================

impl<'ctx, P: Protocol, PD: ParentData + Default> PaintContext<'ctx, P, Optional, PD> {
    /// Paints the optional child if present.
    ///
    /// # Example
    ///
    /// ```ignore
    /// fn paint(&self, ctx: &mut BoxPaintContext<'_, Optional, BoxParentData>) {
    ///     // Draw background
    ///     ctx.canvas().draw_rect(...);
    ///     // Paint child if exists
    ///     ctx.paint_child_if_present();
    /// }
    /// ```
    pub fn paint_child_if_present(&mut self) {
        if !self.children_info.is_empty() {
            let offset = self.offset + self.children_info[0].offset;
            (self.paint_child_callback)(0, offset);
        }
    }

    /// Returns whether a child is present.
    #[inline]
    pub fn has_child(&self) -> bool {
        !self.children_info.is_empty()
    }
}

// ============================================================================
// Single Arity - paint_child for exactly 1 child
// ============================================================================

impl<'ctx, P: Protocol, PD: ParentData + Default> PaintContext<'ctx, P, Single, PD> {
    /// Paints the single child.
    ///
    /// # Example
    ///
    /// ```ignore
    /// fn paint(&self, ctx: &mut BoxPaintContext<'_, Single, BoxParentData>) {
    ///     ctx.canvas().draw_rect(...);  // Background
    ///     ctx.paint_child();             // Child
    ///     ctx.canvas().draw_rect(...);  // Foreground
    /// }
    /// ```
    pub fn paint_child(&mut self) {
        if !self.children_info.is_empty() {
            let offset = self.offset + self.children_info[0].offset;
            (self.paint_child_callback)(0, offset);
        }
    }

    /// Paints the single child with a custom offset.
    ///
    /// This ignores the child's stored offset and uses the provided one instead.
    pub fn paint_child_at(&mut self, offset: Offset) {
        let absolute_offset = self.offset + offset;
        (self.paint_child_callback)(0, absolute_offset);
    }
}

// ============================================================================
// Variable Arity - paint_child for N children
// ============================================================================

impl<'ctx, P: Protocol, PD: ParentData + Default> PaintContext<'ctx, P, Variable, PD> {
    /// Paints a specific child by index.
    ///
    /// # Example
    ///
    /// ```ignore
    /// fn paint(&self, ctx: &mut BoxPaintContext<'_, Variable, BoxParentData>) {
    ///     // Paint children in reverse order (back to front)
    ///     for i in (0..ctx.child_count()).rev() {
    ///         ctx.paint_child(i);
    ///     }
    /// }
    /// ```
    pub fn paint_child(&mut self, index: usize) {
        if let Some(info) = self.children_info.get(index) {
            let offset = self.offset + info.offset;
            (self.paint_child_callback)(index, offset);
        }
    }

    /// Paints a specific child at a custom offset.
    pub fn paint_child_at(&mut self, index: usize, offset: Offset) {
        if index < self.children_info.len() {
            let absolute_offset = self.offset + offset;
            (self.paint_child_callback)(index, absolute_offset);
        }
    }

    /// Paints all children in order (first to last).
    ///
    /// # Example
    ///
    /// ```ignore
    /// fn paint(&self, ctx: &mut BoxPaintContext<'_, Variable, BoxParentData>) {
    ///     ctx.canvas().draw_rect(...);  // Background
    ///     ctx.paint_children();          // All children
    /// }
    /// ```
    pub fn paint_children(&mut self) {
        for i in 0..self.children_info.len() {
            self.paint_child(i);
        }
    }

    /// Paints all children in reverse order (last to first).
    ///
    /// Useful when children are stacked and you want back-to-front painting.
    pub fn paint_children_reverse(&mut self) {
        for i in (0..self.children_info.len()).rev() {
            self.paint_child(i);
        }
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

    #[test]
    fn test_child_paint_info() {
        let info = ChildPaintInfo {
            offset: Offset::new(10.0, 20.0),
        };
        assert_eq!(info.offset.dx, 10.0);
        assert_eq!(info.offset.dy, 20.0);
    }
}
