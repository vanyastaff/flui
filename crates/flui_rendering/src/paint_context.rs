//! Paint context for drawing elements to a canvas.
//!
//! This module provides [`PaintContext`] - a GAT-based context for painting operations
//! with compile-time arity validation and protocol-specific geometry.
//!
//! # Type Aliases
//!
//! - [`BoxPaintContext`] - Paint context for Box protocol (Size geometry)
//! - [`SliverPaintContext`] - Paint context for Sliver protocol (SliverGeometry)

use std::fmt;
use std::marker::PhantomData;

use flui_foundation::ElementId;
use flui_painting::Canvas;
use flui_types::{Offset, Rect, Size, SliverGeometry};
use tracing::instrument;

use crate::arity::{Arity, ChildrenAccess, Single};
use crate::paint_tree::PaintTree;
use crate::protocol::{BoxProtocol, Protocol, SliverProtocol};

// ============================================================================
// TYPE ALIASES FOR ERGONOMICS
// ============================================================================

/// Box paint context with dynamic dispatch (convenience alias).
///
/// Equivalent to `PaintContext<'a, A, BoxProtocol, Box<dyn PaintTree + Send + Sync>>`.
pub type BoxPaintContext<'a, A, T = Box<dyn PaintTree + Send + Sync>> =
    PaintContext<'a, A, BoxProtocol, T>;

/// Sliver paint context with dynamic dispatch (convenience alias).
///
/// Equivalent to `PaintContext<'a, A, SliverProtocol, Box<dyn PaintTree + Send + Sync>>`.
pub type SliverPaintContext<'a, A, T = Box<dyn PaintTree + Send + Sync>> =
    PaintContext<'a, A, SliverProtocol, T>;

// ============================================================================
// PAINT CONTEXT
// ============================================================================

/// GAT-based paint context for drawing elements to a canvas.
///
/// This context provides comprehensive access to painting operations with:
/// - Type-safe children management
/// - Protocol-specific geometry
/// - Efficient canvas operations
/// - Optional static dispatch
///
/// # Type Parameters
///
/// - `'a`: Lifetime of tree reference and children access
/// - `A`: Arity type constraining the number of children
/// - `P`: Layout protocol (defaults to `BoxProtocol`)
/// - `T`: Tree implementation (defaults to dynamic dispatch)
///
/// # Protocol-Specific Geometry
///
/// The `geometry` field type depends on the protocol:
/// - `BoxProtocol`: `Size` (width, height)
/// - `SliverProtocol`: `SliverGeometry` (paint/scroll/layout extents)
///
/// # Examples
///
/// ## Minimal
///
/// ```rust,ignore
/// fn paint(&self, ctx: &mut PaintContext<'_, Single>) {
///     // Uses BoxProtocol by default, geometry is Size
///     ctx.canvas_mut().draw_rect(Rect::from_min_size(Offset::ZERO, ctx.geometry));
/// }
/// ```
///
/// ## With type alias
///
/// ```rust,ignore
/// fn paint(&self, ctx: &mut BoxPaintContext<'_, Single>) {
///     let bounds = ctx.bounds();  // Box-specific method
///     ctx.canvas_mut().draw_rect(bounds);
/// }
/// ```
pub struct PaintContext<
    'a,
    A: Arity,
    P: Protocol = BoxProtocol,
    T: PaintTree = Box<dyn PaintTree + Send + Sync>,
> where
    A::Accessor<'a, ElementId>: ChildrenAccess<'a, ElementId>,
{
    tree: &'a mut T,
    element_id: ElementId,
    /// The offset of this element in parent coordinates.
    pub offset: Offset,
    /// The computed geometry from layout (protocol-specific).
    ///
    /// - For `BoxProtocol`: `Size` (width, height)
    /// - For `SliverProtocol`: `SliverGeometry` (paint/scroll extents)
    pub geometry: P::Geometry,
    canvas: &'a mut Canvas,
    /// Children accessor for compile-time arity-checked access.
    pub children: A::Accessor<'a, ElementId>,
    _phantom: PhantomData<P>,
}

impl<'a, A: Arity, P: Protocol, T: PaintTree> fmt::Debug for PaintContext<'a, A, P, T>
where
    A::Accessor<'a, ElementId>: ChildrenAccess<'a, ElementId>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PaintContext")
            .field("element_id", &self.element_id)
            .field("offset", &self.offset)
            .field("geometry", &self.geometry)
            .field("children_count", &self.children.len())
            .finish_non_exhaustive()
    }
}

// ============================================================================
// PAINT CONTEXT - COMMON METHODS
// ============================================================================

impl<'a, A: Arity, P: Protocol, T: PaintTree> PaintContext<'a, A, P, T>
where
    A::Accessor<'a, ElementId>: ChildrenAccess<'a, ElementId>,
{
    /// Creates a new paint context.
    pub fn new(
        tree: &'a mut T,
        element_id: ElementId,
        offset: Offset,
        geometry: P::Geometry,
        canvas: &'a mut Canvas,
        children: A::Accessor<'a, ElementId>,
    ) -> Self {
        Self {
            tree,
            element_id,
            offset,
            geometry,
            canvas,
            children,
            _phantom: PhantomData,
        }
    }

    /// Gets the element ID this context is painting.
    #[inline]
    pub fn element_id(&self) -> ElementId {
        self.element_id
    }

    /// Gets immutable access to the canvas.
    #[inline]
    pub fn canvas(&self) -> &Canvas {
        self.canvas
    }

    /// Gets mutable access to the canvas.
    #[inline]
    pub fn canvas_mut(&mut self) -> &mut Canvas {
        self.canvas
    }

    /// Returns a GAT-based iterator over child ElementIds.
    #[inline]
    pub fn children(&self) -> impl Iterator<Item = ElementId> + 'a {
        self.children.iter().copied()
    }

    /// Paints a child element at the given offset.
    ///
    /// # Error Handling
    ///
    /// Following Flutter's paint protocol, this method never returns errors.
    /// Any paint failures are logged via tracing::error and execution continues.
    #[instrument(level = "trace", skip(self), fields(child = %child_id.get(), x = %offset.dx, y = %offset.dy))]
    pub fn paint_child(&mut self, child_id: ElementId, offset: Offset) {
        if let Err(e) = self.tree.perform_paint(child_id, offset) {
            tracing::error!(
                child = %child_id.get(),
                offset = ?offset,
                error = %e,
                "paint_child failed"
            );
        }
    }

    /// Paints all children using their stored offsets from layout.
    ///
    /// This method retrieves each child's offset that was set during the layout
    /// phase via `set_child_offset` and paints the child at that position.
    #[instrument(level = "trace", skip(self), fields(element = %self.element_id.get()))]
    pub fn paint_all_children(&mut self) {
        let children: Vec<_> = self.children().collect();
        tracing::trace!(child_count = children.len(), "painting all children");

        for child_id in children {
            let offset = self.tree.get_offset(child_id).unwrap_or(Offset::ZERO);
            self.paint_child(child_id, offset);
        }
    }
}

// ============================================================================
// PAINT CONTEXT - BOX PROTOCOL SPECIFIC
// ============================================================================

impl<'a, A: Arity, T: PaintTree> PaintContext<'a, A, BoxProtocol, T>
where
    A::Accessor<'a, ElementId>: ChildrenAccess<'a, ElementId>,
{
    /// Returns the size of this element (convenience for Box protocol).
    ///
    /// This is equivalent to `ctx.geometry` but more ergonomic.
    #[inline]
    pub fn size(&self) -> Size {
        self.geometry
    }

    /// Returns the bounding rectangle of this element in parent coordinates.
    ///
    /// This combines offset and size into a Rect.
    #[inline]
    pub fn bounds(&self) -> Rect {
        Rect::from_min_size(self.offset, self.geometry)
    }

    /// Returns the local bounding rectangle (at origin).
    ///
    /// This is useful for drawing within the element's own coordinate space.
    #[inline]
    pub fn local_bounds(&self) -> Rect {
        Rect::from_min_size(Offset::ZERO, self.geometry)
    }
}

// ============================================================================
// PAINT CONTEXT - SINGLE CHILD BOX PROTOCOL
// ============================================================================

impl<'a, T: PaintTree> PaintContext<'a, Single, BoxProtocol, T> {
    /// Gets the single child ID (convenience for Single arity).
    #[inline]
    pub fn single_child(&self) -> ElementId {
        *self.children.single()
    }

    /// Paints the single child at the given offset.
    pub fn paint_single_child(&mut self, offset: Offset) {
        let child_id = self.single_child();
        self.paint_child(child_id, offset);
    }
}

// ============================================================================
// PAINT CONTEXT - SLIVER PROTOCOL SPECIFIC
// ============================================================================

impl<'a, A: Arity, T: PaintTree> PaintContext<'a, A, SliverProtocol, T>
where
    A::Accessor<'a, ElementId>: ChildrenAccess<'a, ElementId>,
{
    /// Returns the sliver geometry (convenience for Sliver protocol).
    ///
    /// This is equivalent to `ctx.geometry` but more ergonomic.
    #[inline]
    pub fn sliver_geometry(&self) -> SliverGeometry {
        self.geometry
    }

    /// Returns the paint extent (how much space is visible).
    #[inline]
    pub fn paint_extent(&self) -> f32 {
        self.geometry.paint_extent
    }

    /// Returns the scroll extent (how much scrollable content).
    #[inline]
    pub fn scroll_extent(&self) -> f32 {
        self.geometry.scroll_extent
    }
}

// ============================================================================
// PAINT CONTEXT - SINGLE CHILD SLIVER PROTOCOL
// ============================================================================

impl<'a, T: PaintTree> PaintContext<'a, Single, SliverProtocol, T> {
    /// Gets the single child ID (convenience for Single arity).
    #[inline]
    pub fn single_child(&self) -> ElementId {
        *self.children.single()
    }

    /// Paints the single child at the given offset.
    pub fn paint_single_child(&mut self, offset: Offset) {
        let child_id = self.single_child();
        self.paint_child(child_id, offset);
    }
}
