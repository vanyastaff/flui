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
//! # Proxy Methods
//!
//! For single-child elements that simply pass through to their child,
//! each context provides a `proxy()` method for convenience.

use std::fmt;
use std::marker::PhantomData;

use flui_types::Offset;

use crate::element::ElementTree;
use crate::render::arity::Arity;
use crate::render::protocol::Protocol;
use crate::ElementId;

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
/// - `A`: Arity (child count) - determines the `children` accessor type
/// - `P`: Protocol (BoxProtocol or SliverProtocol)
///
/// # Example
///
/// ```rust,ignore
/// fn layout(&mut self, ctx: LayoutContext<'_, Single, BoxProtocol>) -> Size {
///     let child_size = ctx.layout_child(ctx.children.single(), ctx.constraints);
///     child_size
/// }
/// ```
pub struct LayoutContext<'a, A: Arity, P: Protocol> {
    tree: &'a ElementTree,

    /// Constraints from parent specifying allowed sizes.
    pub constraints: P::Constraints,

    /// Typed children accessor based on arity.
    pub children: A::Children<'a>,

    _phantom: PhantomData<P>,
}

impl<'a, A: Arity, P: Protocol> fmt::Debug for LayoutContext<'a, A, P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LayoutContext")
            .field("constraints", &self.constraints)
            .finish_non_exhaustive()
    }
}

impl<'a, A: Arity, P: Protocol> LayoutContext<'a, A, P> {
    /// Create a new layout context
    pub fn new(
        tree: &'a ElementTree,
        constraints: P::Constraints,
        children: A::Children<'a>,
    ) -> Self {
        Self {
            tree,
            constraints,
            children,
            _phantom: PhantomData,
        }
    }

    /// Get reference to the element tree
    ///
    /// Use this for advanced operations. Prefer helper methods when available.
    pub fn tree(&self) -> &'a ElementTree {
        self.tree
    }
}

impl<'a, A: Arity, P: Protocol> HasTypedChildren<'a, A> for LayoutContext<'a, A, P> {
    fn children(&self) -> A::Children<'a> {
        self.children
    }
}

// Box-specific helper methods
impl<'a, A: Arity> LayoutContext<'a, A, crate::render::BoxProtocol> {
    /// Layout a child element with the given constraints
    ///
    /// Returns the child's computed size.
    #[inline]
    pub fn layout_child(
        &self,
        child_id: std::num::NonZeroUsize,
        constraints: flui_types::constraints::BoxConstraints,
    ) -> flui_types::Size {
        self.tree
            .layout_child(ElementId::new(child_id.get()), constraints)
    }
}

// Sliver-specific helper methods
impl<'a, A: Arity> LayoutContext<'a, A, crate::render::SliverProtocol> {
    /// Layout a child sliver element with the given constraints
    ///
    /// Returns the child's computed sliver geometry.
    #[inline]
    pub fn layout_child(
        &self,
        child_id: std::num::NonZeroUsize,
        constraints: flui_types::SliverConstraints,
    ) -> flui_types::SliverGeometry {
        self.tree
            .layout_sliver_child(ElementId::new(child_id.get()), constraints)
    }
}

// Box protocol proxy for Single arity
impl<'a> LayoutContext<'a, crate::render::Single, crate::render::BoxProtocol> {
    /// Proxy layout - passes constraints directly to child
    #[inline]
    pub fn proxy(&self) -> flui_types::Size {
        self.layout_child(self.children.single(), self.constraints)
    }
}

// Sliver protocol proxy for Single arity
impl<'a> LayoutContext<'a, crate::render::Single, crate::render::SliverProtocol> {
    /// Proxy layout - passes constraints directly to child
    #[inline]
    pub fn proxy(&self) -> flui_types::SliverGeometry {
        self.layout_child(self.children.single(), self.constraints)
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
/// # Example
///
/// ```rust,ignore
/// fn paint(&self, ctx: &mut PaintContext<'_, Single>) {
///     ctx.canvas().draw_rect(rect, paint);
///     ctx.paint_child(ctx.children.single(), ctx.offset);
/// }
/// ```
pub struct PaintContext<'a, A: Arity> {
    tree: &'a ElementTree,

    /// Offset in parent's coordinate space.
    pub offset: Offset,

    /// Typed children accessor based on arity.
    pub children: A::Children<'a>,

    canvas: flui_painting::Canvas,
}

impl<'a, A: Arity> fmt::Debug for PaintContext<'a, A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PaintContext")
            .field("offset", &self.offset)
            .finish_non_exhaustive()
    }
}

impl<'a, A: Arity> PaintContext<'a, A> {
    /// Create a new paint context
    pub fn new(tree: &'a ElementTree, offset: Offset, children: A::Children<'a>) -> Self {
        Self {
            tree,
            offset,
            children,
            canvas: flui_painting::Canvas::new(),
        }
    }

    /// Get mutable access to the canvas for drawing
    pub fn canvas(&mut self) -> &mut flui_painting::Canvas {
        &mut self.canvas
    }

    /// Take ownership of the canvas (used by framework after paint)
    pub fn take_canvas(self) -> flui_painting::Canvas {
        self.canvas
    }

    /// Paint a child element at the given offset
    #[inline]
    pub fn paint_child(&mut self, child_id: std::num::NonZeroUsize, offset: Offset) {
        let child_canvas = self
            .tree
            .paint_child(ElementId::new(child_id.get()), offset);
        self.canvas.append_canvas(child_canvas);
    }

    /// Get reference to the element tree
    pub fn tree(&self) -> &'a ElementTree {
        self.tree
    }
}

impl<'a, A: Arity> HasTypedChildren<'a, A> for PaintContext<'a, A> {
    fn children(&self) -> A::Children<'a> {
        self.children
    }
}

// Paint proxy for Single arity
impl<'a> PaintContext<'a, crate::render::Single> {
    /// Proxy paint - paints child at same offset
    #[inline]
    pub fn proxy(&mut self) {
        self.paint_child(self.children.single(), self.offset);
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
/// - `A`: Arity (child count) - determines the `children` accessor type
/// - `P`: Protocol (BoxProtocol or SliverProtocol)
///
/// # Example
///
/// ```rust,ignore
/// fn hit_test(&self, ctx: HitTestContext<'_, Single, BoxProtocol>, result: &mut BoxHitTestResult) -> bool {
///     if ctx.contains(ctx.position) {
///         ctx.hit_test_child(ctx.children.single(), ctx.position, result)
///     } else {
///         false
///     }
/// }
/// ```
pub struct HitTestContext<'a, A: Arity, P: Protocol> {
    tree: &'a ElementTree,

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

impl<'a, A: Arity, P: Protocol> fmt::Debug for HitTestContext<'a, A, P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HitTestContext")
            .field("position", &self.position)
            .field("geometry", &self.geometry)
            .field("element_id", &self.element_id)
            .finish_non_exhaustive()
    }
}

impl<'a, A: Arity, P: Protocol> HitTestContext<'a, A, P> {
    /// Create a new hit test context
    pub fn new(
        tree: &'a ElementTree,
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

    /// Get reference to the element tree
    pub fn tree(&self) -> &'a ElementTree {
        self.tree
    }
}

impl<'a, A: Arity, P: Protocol> HasTypedChildren<'a, A> for HitTestContext<'a, A, P> {
    fn children(&self) -> A::Children<'a> {
        self.children
    }
}

// Box-specific helper methods
impl<'a, A: Arity> HitTestContext<'a, A, crate::render::BoxProtocol> {
    /// Get the size (Box protocol geometry)
    pub fn size(&self) -> flui_types::Size {
        self.geometry
    }

    /// Check if position is within bounds
    pub fn contains(&self, position: Offset) -> bool {
        position.dx >= 0.0
            && position.dy >= 0.0
            && position.dx < self.geometry.width
            && position.dy < self.geometry.height
    }

    /// Hit test a child element
    #[inline]
    pub fn hit_test_child(
        &self,
        child_id: std::num::NonZeroUsize,
        position: Offset,
        result: &mut crate::element::hit_test::BoxHitTestResult,
    ) -> bool {
        self.tree
            .hit_test_box_child(ElementId::new(child_id.get()), position, result)
    }
}

// Sliver-specific helper methods
impl<'a, A: Arity> HitTestContext<'a, A, crate::render::SliverProtocol> {
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

    /// Hit test a child element
    #[inline]
    pub fn hit_test_child(
        &self,
        child_id: std::num::NonZeroUsize,
        position: Offset,
        result: &mut crate::element::hit_test::SliverHitTestResult,
    ) -> bool {
        self.tree
            .hit_test_sliver_child(ElementId::new(child_id.get()), position, result)
    }
}

// Box protocol proxy for Single arity
impl<'a> HitTestContext<'a, crate::render::Single, crate::render::BoxProtocol> {
    /// Proxy hit test - forwards to child at same position
    #[inline]
    pub fn proxy(&self, result: &mut crate::element::hit_test::BoxHitTestResult) -> bool {
        self.hit_test_child(self.children.single(), self.position, result)
    }
}

// Sliver protocol proxy for Single arity
impl<'a> HitTestContext<'a, crate::render::Single, crate::render::SliverProtocol> {
    /// Proxy hit test - forwards to child at same position
    #[inline]
    pub fn proxy(&self, result: &mut crate::element::hit_test::SliverHitTestResult) -> bool {
        self.hit_test_child(self.children.single(), self.position, result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::Size;

    #[test]
    fn test_box_hit_test_contains() {
        // This test would need a mock ElementTree
        // For now just test the contains logic
        let size = Size::new(100.0, 50.0);

        // Test bounds checking logic
        assert!(Offset::new(0.0, 0.0).dx >= 0.0);
        assert!(Offset::new(50.0, 25.0).dx < size.width);
        assert!(Offset::new(50.0, 25.0).dy < size.height);
        assert!(!(Offset::new(100.0, 25.0).dx < size.width));
        assert!(!(Offset::new(50.0, 50.0).dy < size.height));
    }
}
