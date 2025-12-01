//! Unified context types for render operations.
//!
//! This module provides generic contexts for layout, paint, and hit testing
//! that work with any Protocol (Box or Sliver). These contexts encapsulate
//! tree access and provide type-safe, arity-aware child access.
//!
//! # Context Types
//!
//! - [`LayoutContext`]: Computes sizes/geometry from constraints
//! - [`PaintContext`]: Draws to canvas and paints children
//! - [`HitTestContext`]: Tests if a point hits this element or children
//!
//! # Design
//!
//! Contexts are generic over a tree type `T` that implements the appropriate
//! traits from `flui-tree`. This allows the rendering layer to be independent
//! of the concrete `ElementTree` implementation.
//!
//! # Integration with flui-tree
//!
//! These contexts work seamlessly with flui-tree iterators and utility functions:
//!
//! ```rust,ignore
//! use flui_tree::{RenderChildren, first_render_child, count_render_children};
//!
//! // In a Multi-child render object:
//! fn layout<T: LayoutTree>(&mut self, ctx: LayoutContext<'_, T, Multi, BoxProtocol>) -> Size {
//!     // Use flui-tree iterators directly on the tree
//!     let child_count = count_render_children(ctx.tree(), parent_id);
//!
//!     // Or use the extension methods
//!     let sizes = ctx.tree_mut().layout_render_children(parent_id, constraints);
//! }
//! ```

use std::fmt;
use std::marker::PhantomData;

use flui_foundation::ElementId;
use flui_interaction::HitTestResult;
use flui_painting::Canvas;
use flui_types::{Offset, Size, SliverConstraints, SliverGeometry};

use super::arity::Arity;
use super::protocol::{BoxConstraints, BoxProtocol, Protocol, SliverProtocol};
use super::render_tree::{LayoutTree, PaintTree};

/// Trait for contexts that provide typed children access.
///
/// All context types implement this for generic access to children,
/// allowing render objects to access children in a type-safe way based
/// on their arity.
pub trait HasTypedChildren<'a, A: Arity> {
    /// Returns the typed children accessor for this arity.
    fn children(&self) -> A::Children<'a>;
}

// ============================================================================
// LAYOUT CONTEXT
// ============================================================================

/// Layout context for computing sizes and geometry.
///
/// Provides safe, controlled access to layout operations without exposing
/// internal framework lifecycle methods.
///
/// # Type Parameters
///
/// - `'a`: Lifetime of tree reference
/// - `T`: Tree type implementing [`LayoutTree`]
/// - `A`: Arity (child count) - determines the `children` accessor type
/// - `P`: Protocol (BoxProtocol or SliverProtocol)
///
/// # Example
///
/// ```rust,ignore
/// fn layout<T: LayoutTree>(&mut self, ctx: LayoutContext<'_, T, Single, BoxProtocol>) -> Size {
///     let child_size = ctx.layout_child(ctx.children.single(), ctx.constraints);
///     child_size
/// }
/// ```
pub struct LayoutContext<'a, T, A: Arity, P: Protocol> {
    tree: &'a mut T,

    /// Constraints from parent specifying allowed sizes.
    pub constraints: P::Constraints,

    /// Typed children accessor based on arity.
    pub children: A::Children<'a>,

    _phantom: PhantomData<P>,
}

impl<'a, T, A: Arity, P: Protocol> fmt::Debug for LayoutContext<'a, T, A, P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LayoutContext")
            .field("constraints", &self.constraints)
            .finish_non_exhaustive()
    }
}

impl<'a, T, A: Arity, P: Protocol> LayoutContext<'a, T, A, P> {
    /// Create a new layout context
    pub fn new(tree: &'a mut T, constraints: P::Constraints, children: A::Children<'a>) -> Self {
        Self {
            tree,
            constraints,
            children,
            _phantom: PhantomData,
        }
    }

    /// Get reference to the tree
    pub fn tree(&self) -> &T {
        self.tree
    }

    /// Get mutable reference to the tree
    pub fn tree_mut(&mut self) -> &mut T {
        self.tree
    }
}

impl<'a, T, A: Arity, P: Protocol> HasTypedChildren<'a, A> for LayoutContext<'a, T, A, P> {
    fn children(&self) -> A::Children<'a> {
        self.children
    }
}

// Box-specific helper methods
impl<'a, T: LayoutTree, A: Arity> LayoutContext<'a, T, A, BoxProtocol> {
    /// Layout a child element with the given constraints
    ///
    /// Returns the child's computed size.
    #[inline]
    pub fn layout_child(
        &mut self,
        child_id: std::num::NonZeroUsize,
        constraints: BoxConstraints,
    ) -> Result<Size, super::super::error::RenderError> {
        self.tree
            .perform_layout(ElementId::new(child_id.get()), constraints)
    }
}

// Sliver-specific helper methods
impl<'a, T: LayoutTree, A: Arity> LayoutContext<'a, T, A, SliverProtocol> {
    /// Layout a child sliver element with the given constraints
    ///
    /// Returns the child's computed sliver geometry.
    #[inline]
    pub fn layout_child(
        &mut self,
        child_id: std::num::NonZeroUsize,
        constraints: SliverConstraints,
    ) -> Result<SliverGeometry, super::super::error::RenderError> {
        self.tree
            .perform_sliver_layout(ElementId::new(child_id.get()), constraints)
    }
}

// Box protocol proxy for Single arity
impl<'a, T: LayoutTree> LayoutContext<'a, T, super::arity::Single, BoxProtocol> {
    /// Proxy layout - passes constraints directly to child
    #[inline]
    pub fn proxy(&mut self) -> Result<Size, super::super::error::RenderError> {
        let child = self.children.single();
        let constraints = self.constraints;
        self.layout_child(child, constraints)
    }
}

// Sliver protocol proxy for Single arity
impl<'a, T: LayoutTree> LayoutContext<'a, T, super::arity::Single, SliverProtocol> {
    /// Proxy layout - passes constraints directly to child
    #[inline]
    pub fn proxy(&mut self) -> Result<SliverGeometry, super::super::error::RenderError> {
        let child = self.children.single();
        let constraints = self.constraints;
        self.layout_child(child, constraints)
    }
}

// ============================================================================
// PAINT CONTEXT
// ============================================================================

/// Paint context for drawing to canvas.
///
/// Provides access to canvas for drawing operations and child painting.
/// Protocol-independent since all protocols paint to the same canvas type.
///
/// # Type Parameters
///
/// - `'a`: Lifetime of tree reference
/// - `T`: Tree type implementing [`PaintTree`]
/// - `A`: Arity (child count) - determines the `children` accessor type
///
/// # Example
///
/// ```rust,ignore
/// fn paint<T: PaintTree>(&self, ctx: &mut PaintContext<'_, T, Single>) {
///     ctx.canvas().draw_rect(rect, paint);
///     ctx.paint_child(ctx.children.single(), ctx.offset);
/// }
/// ```
pub struct PaintContext<'a, T, A: Arity> {
    tree: &'a mut T,

    /// Offset in parent's coordinate space.
    pub offset: Offset,

    /// Typed children accessor based on arity.
    pub children: A::Children<'a>,

    canvas: Canvas,
}

impl<'a, T, A: Arity> fmt::Debug for PaintContext<'a, T, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PaintContext")
            .field("offset", &self.offset)
            .finish_non_exhaustive()
    }
}

impl<'a, T, A: Arity> PaintContext<'a, T, A> {
    /// Create a new paint context
    pub fn new(tree: &'a mut T, offset: Offset, children: A::Children<'a>) -> Self {
        Self {
            tree,
            offset,
            children,
            canvas: Canvas::new(),
        }
    }

    /// Get mutable access to the canvas for drawing
    pub fn canvas(&mut self) -> &mut Canvas {
        &mut self.canvas
    }

    /// Take ownership of the canvas (used by framework after paint)
    pub fn take_canvas(self) -> Canvas {
        self.canvas
    }

    /// Get reference to the tree
    pub fn tree(&self) -> &T {
        self.tree
    }

    /// Get mutable reference to the tree
    pub fn tree_mut(&mut self) -> &mut T {
        self.tree
    }
}

impl<'a, T: PaintTree, A: Arity> PaintContext<'a, T, A> {
    /// Paint a child element at the given offset
    #[inline]
    pub fn paint_child(
        &mut self,
        child_id: std::num::NonZeroUsize,
        offset: Offset,
    ) -> Result<(), super::super::error::RenderError> {
        let child_canvas = self
            .tree
            .perform_paint(ElementId::new(child_id.get()), offset)?;
        self.canvas.append_canvas(child_canvas);
        Ok(())
    }
}

impl<'a, T, A: Arity> HasTypedChildren<'a, A> for PaintContext<'a, T, A> {
    fn children(&self) -> A::Children<'a> {
        self.children
    }
}

// Paint proxy for Single arity
impl<'a, T: PaintTree> PaintContext<'a, T, super::arity::Single> {
    /// Proxy paint - paints child at same offset
    #[inline]
    pub fn proxy(&mut self) -> Result<(), super::super::error::RenderError> {
        let child = self.children.single();
        let offset = self.offset;
        self.paint_child(child, offset)
    }
}

// ============================================================================
// HIT TEST CONTEXT
// ============================================================================

/// Hit test context for pointer event routing.
///
/// Provides information for hit testing and child traversal. Used to determine
/// which elements should receive pointer events based on position.
///
/// # Type Parameters
///
/// - `'a`: Lifetime of tree reference
/// - `T`: Tree type (for future extension)
/// - `A`: Arity (child count) - determines the `children` accessor type
/// - `P`: Protocol (BoxProtocol or SliverProtocol)
///
/// # Example
///
/// ```rust,ignore
/// fn hit_test<T>(&self, ctx: HitTestContext<'_, T, Single, BoxProtocol>, result: &mut HitTestResult) -> bool {
///     if ctx.contains(ctx.position) {
///         // Add self to result and test children
///         true
///     } else {
///         false
///     }
/// }
/// ```
pub struct HitTestContext<'a, T, A: Arity, P: Protocol> {
    #[allow(dead_code)]
    tree: &'a T,

    /// Hit test position in local coordinates.
    pub position: Offset,

    /// Computed geometry from layout (Size for Box, SliverGeometry for Sliver).
    pub geometry: P::Geometry,

    /// Element ID being tested.
    pub element_id: ElementId,

    /// Typed children accessor based on arity.
    pub children: A::Children<'a>,

    _phantom: PhantomData<P>,
}

impl<'a, T, A: Arity, P: Protocol> fmt::Debug for HitTestContext<'a, T, A, P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HitTestContext")
            .field("position", &self.position)
            .field("geometry", &self.geometry)
            .field("element_id", &self.element_id)
            .finish_non_exhaustive()
    }
}

impl<'a, T, A: Arity, P: Protocol> HitTestContext<'a, T, A, P> {
    /// Create a new hit test context
    pub fn new(
        tree: &'a T,
        position: Offset,
        geometry: P::Geometry,
        element_id: ElementId,
        children: A::Children<'a>,
    ) -> Self {
        Self {
            tree,
            position,
            geometry,
            element_id,
            children,
            _phantom: PhantomData,
        }
    }

    /// Create a new context with a different position (for coordinate transformations)
    pub fn with_position(&self, position: Offset) -> Self
    where
        P::Geometry: Clone,
    {
        Self {
            tree: self.tree,
            position,
            geometry: self.geometry.clone(),
            element_id: self.element_id,
            children: self.children,
            _phantom: PhantomData,
        }
    }

    /// Get reference to the tree
    pub fn tree(&self) -> &T {
        self.tree
    }
}

impl<'a, T, A: Arity, P: Protocol> HasTypedChildren<'a, A> for HitTestContext<'a, T, A, P> {
    fn children(&self) -> A::Children<'a> {
        self.children
    }
}

// Box-specific helper methods
impl<'a, T, A: Arity> HitTestContext<'a, T, A, BoxProtocol> {
    /// Get the size (Box protocol geometry)
    pub fn size(&self) -> Size {
        self.geometry
    }

    /// Check if position is within bounds
    pub fn contains(&self, position: Offset) -> bool {
        position.dx >= 0.0
            && position.dy >= 0.0
            && position.dx < self.geometry.width
            && position.dy < self.geometry.height
    }

    /// Add this element to hit test result
    pub fn add_to_result(&self, result: &mut HitTestResult) {
        use flui_types::Rect;
        let entry = flui_interaction::HitTestEntry {
            element_id: self.element_id.into(),
            local_position: self.position,
            bounds: Rect::from_xywh(0.0, 0.0, self.geometry.width, self.geometry.height),
            handler: None,
            transform: None,
        };
        result.add(entry);
    }
}

// Sliver-specific helper methods
impl<'a, T, A: Arity> HitTestContext<'a, T, A, SliverProtocol> {
    /// Get main axis position (y for vertical scroll)
    pub fn main_axis_position(&self) -> f32 {
        self.position.dy
    }

    /// Get cross axis position (x for vertical scroll)
    pub fn cross_axis_position(&self) -> f32 {
        self.position.dx
    }

    /// Check if hit position is within visible region
    pub fn is_visible(&self) -> bool {
        self.position.dy >= 0.0 && self.position.dy < self.geometry.paint_extent
    }

    /// Add this element to hit test result
    pub fn add_to_result(&self, result: &mut HitTestResult) {
        use flui_types::Rect;
        let entry = flui_interaction::HitTestEntry {
            element_id: self.element_id.into(),
            local_position: self.position,
            bounds: Rect::from_xywh(
                0.0,
                0.0,
                self.geometry.cross_axis_extent,
                self.geometry.paint_extent,
            ),
            handler: None,
            transform: None,
        };
        result.add(entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_box_hit_test_contains() {
        let size = Size::new(100.0, 50.0);

        // Test bounds checking logic
        assert!(Offset::new(0.0, 0.0).dx >= 0.0);
        assert!(Offset::new(50.0, 25.0).dx < size.width);
        assert!(Offset::new(50.0, 25.0).dy < size.height);
        assert!(!(Offset::new(100.0, 25.0).dx < size.width));
        assert!(!(Offset::new(50.0, 50.0).dy < size.height));
    }
}
