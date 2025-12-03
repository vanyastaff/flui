//! GAT-based contexts for layout, paint, and hit testing.
//!
//! - [`LayoutContext`] - Layout operations with constraint passing
//! - [`PaintContext`] - Canvas operations and child painting
//! - [`HitTestContext`] - Pointer event detection

use std::fmt;
use std::marker::PhantomData;

use flui_foundation::ElementId;
use flui_interaction::{HitTestEntry, HitTestResult};
use flui_painting::Canvas;
use flui_types::{BoxConstraints, Offset, Rect, Size, SliverConstraints, SliverGeometry};

use super::arity::{Arity, ChildrenAccess, Single};
use super::protocol::{BoxProtocol, Protocol, SliverProtocol};
use super::render_tree::{HitTestTree, LayoutTree, PaintTree};
use crate::core::RenderResult;

// ============================================================================
// TYPE ALIASES FOR ERGONOMICS
// ============================================================================

/// Box layout context with dynamic dispatch (convenience alias).
///
/// Equivalent to `LayoutContext<'a, A, BoxProtocol, Box<dyn LayoutTree + Send + Sync>>`.
///
/// # Example
///
/// ```rust,ignore
/// fn layout(&mut self, mut ctx: BoxLayoutContext<'_, Single>) -> Size {
///     let child_id = ctx.single_child();
///     ctx.layout_child(child_id, ctx.constraints)?
/// }
/// ```
pub type BoxLayoutContext<'a, A, T = Box<dyn LayoutTree + Send + Sync>> =
    LayoutContext<'a, A, BoxProtocol, T>;

/// Sliver layout context with dynamic dispatch (convenience alias).
///
/// Equivalent to `LayoutContext<'a, A, SliverProtocol, Box<dyn LayoutTree + Send + Sync>>`.
pub type SliverLayoutContext<'a, A, T = Box<dyn LayoutTree + Send + Sync>> =
    LayoutContext<'a, A, SliverProtocol, T>;

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

/// Box hit test context with dynamic dispatch (convenience alias).
///
/// Equivalent to `HitTestContext<'a, A, BoxProtocol, Box<dyn HitTestTree + Send + Sync>>`.
pub type BoxHitTestContext<'a, A, T = Box<dyn HitTestTree + Send + Sync>> =
    HitTestContext<'a, A, BoxProtocol, T>;

/// Sliver hit test context with dynamic dispatch (convenience alias).
///
/// Equivalent to `HitTestContext<'a, A, SliverProtocol, Box<dyn HitTestTree + Send + Sync>>`.
pub type SliverHitTestContext<'a, A, T = Box<dyn HitTestTree + Send + Sync>> =
    HitTestContext<'a, A, SliverProtocol, T>;

// ============================================================================
// LAYOUT CONTEXT
// ============================================================================

/// GAT-based layout context for computing element sizes and positions.
///
/// This context provides type-safe access to layout operations with:
/// - Compile-time arity validation
/// - Protocol-specific constraints and geometry
/// - Optional static dispatch for performance
/// - Zero-cost abstractions through generics
///
/// # Type Parameters
///
/// - `'a`: Lifetime of tree reference and children access
/// - `A`: Arity type constraining the number of children
/// - `P`: Layout protocol (defaults to `BoxProtocol`)
/// - `T`: Tree implementation (defaults to dynamic dispatch)
///
/// # Protocol-Specific Types
///
/// - `BoxProtocol`: constraints is `BoxConstraints`, returns `Size`
/// - `SliverProtocol`: constraints is `SliverConstraints`, returns `SliverGeometry`
///
/// # Examples
///
/// ## Minimal (uses defaults)
///
/// ```rust,ignore
/// fn layout(&mut self, mut ctx: LayoutContext<'_, Single>) -> Size {
///     // Uses BoxProtocol and dynamic dispatch by default
///     ctx.layout_single_child()?
/// }
/// ```
///
/// ## With type alias
///
/// ```rust,ignore
/// fn layout(&mut self, mut ctx: BoxLayoutContext<'_, Variable>) -> Size {
///     let mut total_width = 0.0;
///     for child_id in ctx.children() {
///         let size = ctx.layout_child(child_id, ctx.constraints)?;
///         total_width += size.width;
///     }
///     Size::new(total_width, ctx.constraints.max_height)
/// }
/// ```
///
/// ## With named constructor
///
/// ```rust,ignore
/// let ctx = LayoutContext::for_box(tree, id, constraints, accessor);
/// let ctx = LayoutContext::for_sliver(tree, id, constraints, accessor);
/// ```
pub struct LayoutContext<
    'a,
    A: Arity,
    P: Protocol = BoxProtocol,
    T: LayoutTree = Box<dyn LayoutTree + Send + Sync>,
> where
    A::Accessor<'a, ElementId>: ChildrenAccess<'a, ElementId>,
{
    tree: &'a mut T,
    element_id: ElementId,
    /// Layout constraints from the parent element (protocol-specific).
    ///
    /// - For `BoxProtocol`: `BoxConstraints` (min/max width/height)
    /// - For `SliverProtocol`: `SliverConstraints` (scroll offset, viewport)
    pub constraints: P::Constraints,
    children_accessor: A::Accessor<'a, ElementId>,
    _phantom: PhantomData<P>,
}

impl<'a, A: Arity, P: Protocol, T: LayoutTree> fmt::Debug for LayoutContext<'a, A, P, T>
where
    A::Accessor<'a, ElementId>: ChildrenAccess<'a, ElementId>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LayoutContext")
            .field("element_id", &self.element_id)
            .field("constraints", &self.constraints)
            .field("children_count", &self.children_accessor.len())
            .finish_non_exhaustive()
    }
}

// ============================================================================
// LAYOUT CONTEXT - COMMON METHODS
// ============================================================================

impl<'a, A: Arity, P: Protocol, T: LayoutTree> LayoutContext<'a, A, P, T>
where
    A::Accessor<'a, ElementId>: ChildrenAccess<'a, ElementId>,
{
    /// Creates a new layout context.
    ///
    /// For more explicit construction, consider using:
    /// - [`LayoutContext::for_box`] for Box protocol
    /// - [`LayoutContext::for_sliver`] for Sliver protocol
    pub fn new(
        tree: &'a mut T,
        element_id: ElementId,
        constraints: P::Constraints,
        children_accessor: A::Accessor<'a, ElementId>,
    ) -> Self {
        Self {
            tree,
            element_id,
            constraints,
            children_accessor,
            _phantom: PhantomData,
        }
    }

    /// Gets the element ID this context is operating on.
    #[inline]
    pub fn element_id(&self) -> ElementId {
        self.element_id
    }

    /// Gets read-only access to the tree for navigation operations.
    #[inline]
    pub fn tree(&self) -> &T {
        self.tree
    }

    /// Gets mutable access to the tree for layout operations.
    #[inline]
    pub fn tree_mut(&mut self) -> &mut T {
        self.tree
    }

    /// Returns a GAT-based iterator over child ElementIds.
    ///
    /// This provides zero-cost iteration with proper lifetime management.
    #[inline]
    pub fn children(&self) -> impl Iterator<Item = ElementId> + 'a {
        self.children_accessor.iter().copied()
    }

    /// Returns children matching the given HRTB predicate.
    ///
    /// This method leverages Higher-Rank Trait Bounds for maximum flexibility
    /// while maintaining zero-cost abstractions.
    pub fn children_where<F>(&self, predicate: F) -> Vec<ElementId>
    where
        F: for<'b> Fn(&'b ElementId) -> bool,
    {
        self.children().filter(|id| predicate(id)).collect()
    }

    /// Sets the offset of a child element.
    ///
    /// Called during parent's layout to position children.
    pub fn set_child_offset(&mut self, child_id: ElementId, offset: Offset) {
        self.tree.set_offset(child_id, offset);
    }

    /// Gets the offset of a child element.
    pub fn get_child_offset(&self, child_id: ElementId) -> Option<Offset> {
        self.tree.get_offset(child_id)
    }

    /// Marks a child as needing layout.
    pub fn mark_child_needs_layout(&mut self, child_id: ElementId) {
        self.tree.mark_needs_layout(child_id);
    }

    /// Checks if a child needs layout.
    pub fn child_needs_layout(&self, child_id: ElementId) -> bool {
        self.tree.needs_layout(child_id)
    }
}

// ============================================================================
// LAYOUT CONTEXT - BOX PROTOCOL SPECIFIC
// ============================================================================

impl<'a, A: Arity, T: LayoutTree> LayoutContext<'a, A, BoxProtocol, T>
where
    A::Accessor<'a, ElementId>: ChildrenAccess<'a, ElementId>,
{
    /// Layouts a child box element.
    ///
    /// Returns the computed size that satisfies the given constraints.
    pub fn layout_child(
        &mut self,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> RenderResult<Size> {
        self.tree.perform_layout(child_id, constraints)
    }

    /// Layouts all children with the same constraints.
    ///
    /// Returns a vector of (child_id, size) tuples.
    pub fn layout_all_children(&mut self, constraints: BoxConstraints) -> Vec<(ElementId, Size)> {
        let children: Vec<_> = self.children().collect();
        let mut results = Vec::with_capacity(children.len());

        for child_id in children {
            if let Ok(size) = self.layout_child(child_id, constraints) {
                results.push((child_id, size));
            }
        }

        results
    }
}

// ============================================================================
// LAYOUT CONTEXT - SINGLE CHILD BOX PROTOCOL
// ============================================================================

impl<'a, T: LayoutTree> LayoutContext<'a, Single, BoxProtocol, T> {
    /// Gets the single child ID (convenience for Single arity).
    #[inline]
    pub fn single_child(&self) -> ElementId {
        *self.children_accessor.single()
    }

    /// Layouts the single child with the current constraints.
    ///
    /// This is a convenience method for simple wrapper render objects.
    pub fn layout_single_child(&mut self) -> RenderResult<Size> {
        let child_id = self.single_child();
        self.layout_child(child_id, self.constraints)
    }

    /// Layouts the single child with transformed constraints.
    pub fn layout_single_child_with<F>(&mut self, transform: F) -> RenderResult<Size>
    where
        F: FnOnce(BoxConstraints) -> BoxConstraints,
    {
        let child_id = self.single_child();
        let transformed_constraints = transform(self.constraints);
        self.layout_child(child_id, transformed_constraints)
    }
}

// ============================================================================
// LAYOUT CONTEXT - SLIVER PROTOCOL SPECIFIC
// ============================================================================

impl<'a, A: Arity, T: LayoutTree> LayoutContext<'a, A, SliverProtocol, T>
where
    A::Accessor<'a, ElementId>: ChildrenAccess<'a, ElementId>,
{
    /// Layouts a child sliver element.
    ///
    /// Returns the computed geometry with scroll/paint extents.
    pub fn layout_child(
        &mut self,
        child_id: ElementId,
        constraints: SliverConstraints,
    ) -> RenderResult<SliverGeometry> {
        self.tree.perform_sliver_layout(child_id, constraints)
    }

    /// Layouts all sliver children with the same constraints.
    pub fn layout_all_children(
        &mut self,
        constraints: SliverConstraints,
    ) -> Vec<(ElementId, SliverGeometry)> {
        let children: Vec<_> = self.children().collect();
        let mut results = Vec::with_capacity(children.len());

        for child_id in children {
            if let Ok(geometry) = self.layout_child(child_id, constraints) {
                results.push((child_id, geometry));
            }
        }

        results
    }
}

// ============================================================================
// LAYOUT CONTEXT - SINGLE CHILD SLIVER PROTOCOL
// ============================================================================

impl<'a, T: LayoutTree> LayoutContext<'a, Single, SliverProtocol, T> {
    /// Gets the single child ID (convenience for Single arity).
    #[inline]
    pub fn single_child(&self) -> ElementId {
        *self.children_accessor.single()
    }

    /// Layouts the single child with the current constraints.
    pub fn layout_single_child(&mut self) -> RenderResult<SliverGeometry> {
        let child_id = self.single_child();
        self.layout_child(child_id, self.constraints.clone())
    }

    /// Layouts the single child with transformed constraints.
    pub fn layout_single_child_with<F>(&mut self, transform: F) -> RenderResult<SliverGeometry>
    where
        F: FnOnce(SliverConstraints) -> SliverConstraints,
    {
        let child_id = self.single_child();
        let transformed_constraints = transform(self.constraints.clone());
        self.layout_child(child_id, transformed_constraints)
    }
}

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
///
/// ## With named constructor
///
/// ```rust,ignore
/// let ctx = PaintContext::for_box(tree, id, offset, size, canvas, accessor);
/// let ctx = PaintContext::for_sliver(tree, id, offset, geometry, canvas, accessor);
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
    children_accessor: A::Accessor<'a, ElementId>,
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
            .field("children_count", &self.children_accessor.len())
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
    ///
    /// For more explicit construction, consider using:
    /// - [`PaintContext::for_box`] for Box protocol
    /// - [`PaintContext::for_sliver`] for Sliver protocol
    pub fn new(
        tree: &'a mut T,
        element_id: ElementId,
        offset: Offset,
        geometry: P::Geometry,
        canvas: &'a mut Canvas,
        children_accessor: A::Accessor<'a, ElementId>,
    ) -> Self {
        Self {
            tree,
            element_id,
            offset,
            geometry,
            canvas,
            children_accessor,
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
        self.children_accessor.iter().copied()
    }

    /// Paints a child element at the given offset.
    pub fn paint_child(&mut self, child_id: ElementId, offset: Offset) -> RenderResult<()> {
        let _canvas = self.tree.perform_paint(child_id, offset)?;
        Ok(())
    }

    /// Paints all children using their stored offsets from layout.
    ///
    /// This method retrieves each child's offset that was set during the layout
    /// phase via `set_child_offset` and paints the child at that position.
    /// If a child has no stored offset, it defaults to `Offset::ZERO`.
    pub fn paint_all_children(&mut self) -> RenderResult<()> {
        let children: Vec<_> = self.children().collect();

        for child_id in children {
            let offset = self.tree.get_offset(child_id).unwrap_or(Offset::ZERO);
            self.paint_child(child_id, offset)?;
        }

        Ok(())
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
        *self.children_accessor.single()
    }

    /// Paints the single child at the given offset.
    pub fn paint_single_child(&mut self, offset: Offset) -> RenderResult<()> {
        let child_id = self.single_child();
        self.paint_child(child_id, offset)
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
        self.geometry.clone()
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
        *self.children_accessor.single()
    }

    /// Paints the single child at the given offset.
    pub fn paint_single_child(&mut self, offset: Offset) -> RenderResult<()> {
        let child_id = self.single_child();
        self.paint_child(child_id, offset)
    }
}

// ============================================================================
// HIT TEST CONTEXT
// ============================================================================

/// GAT-based hit test context for pointer event detection.
///
/// This context provides efficient hit testing with:
/// - Spatial optimizations
/// - Protocol-specific geometry
/// - Type-safe children access
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
/// - `BoxProtocol`: `Size` for rectangular hit testing
/// - `SliverProtocol`: `SliverGeometry` for scroll-aware hit testing
///
/// # Examples
///
/// ## Minimal
///
/// ```rust,ignore
/// fn hit_test(&self, ctx: &HitTestContext<'_, Variable>, result: &mut HitTestResult) -> bool {
///     // Test children in reverse z-order
///     for child_id in ctx.children_reverse() {
///         if ctx.hit_test_child(child_id, ctx.position, result) {
///             return true;
///         }
///     }
///     ctx.hit_test_self(result)
/// }
/// ```
pub struct HitTestContext<
    'a,
    A: Arity,
    P: Protocol = BoxProtocol,
    T: HitTestTree = Box<dyn HitTestTree + Send + Sync>,
> where
    A::Accessor<'a, ElementId>: ChildrenAccess<'a, ElementId>,
{
    tree: &'a T,
    element_id: ElementId,
    /// The position to test (in parent coordinates).
    pub position: Offset,
    /// The computed geometry from layout (protocol-specific).
    pub geometry: P::Geometry,
    children_accessor: A::Accessor<'a, ElementId>,
    _phantom: PhantomData<P>,
}

impl<'a, A: Arity, P: Protocol, T: HitTestTree> fmt::Debug for HitTestContext<'a, A, P, T>
where
    A::Accessor<'a, ElementId>: ChildrenAccess<'a, ElementId>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HitTestContext")
            .field("element_id", &self.element_id)
            .field("position", &self.position)
            .field("geometry", &self.geometry)
            .field("children_count", &self.children_accessor.len())
            .finish_non_exhaustive()
    }
}

// ============================================================================
// HIT TEST CONTEXT - COMMON METHODS
// ============================================================================

impl<'a, A: Arity, P: Protocol, T: HitTestTree> HitTestContext<'a, A, P, T>
where
    A::Accessor<'a, ElementId>: ChildrenAccess<'a, ElementId>,
{
    /// Creates a new hit test context.
    ///
    /// For more explicit construction, consider using:
    /// - [`HitTestContext::for_box`] for Box protocol
    /// - [`HitTestContext::for_sliver`] for Sliver protocol
    pub fn new(
        tree: &'a T,
        element_id: ElementId,
        position: Offset,
        geometry: P::Geometry,
        children_accessor: A::Accessor<'a, ElementId>,
    ) -> Self {
        Self {
            tree,
            element_id,
            position,
            geometry,
            children_accessor,
            _phantom: PhantomData,
        }
    }

    /// Gets the element ID this context is testing.
    #[inline]
    pub fn element_id(&self) -> ElementId {
        self.element_id
    }

    /// Returns a GAT-based iterator over child ElementIds.
    #[inline]
    pub fn children(&self) -> impl Iterator<Item = ElementId> + 'a {
        self.children_accessor.iter().copied()
    }

    /// Returns children in reverse order (for z-order hit testing).
    pub fn children_reverse(&self) -> impl DoubleEndedIterator<Item = ElementId> + 'a {
        self.children_accessor.as_slice().iter().copied().rev()
    }

    /// Hit tests a child element.
    pub fn hit_test_child(
        &self,
        child_id: ElementId,
        position: Offset,
        result: &mut HitTestResult,
    ) -> bool {
        self.tree.hit_test(child_id, position, result)
    }
}

// ============================================================================
// HIT TEST CONTEXT - BOX PROTOCOL SPECIFIC
// ============================================================================

impl<'a, A: Arity, T: HitTestTree> HitTestContext<'a, A, BoxProtocol, T>
where
    A::Accessor<'a, ElementId>: ChildrenAccess<'a, ElementId>,
{
    /// Returns the size of this element (convenience for Box protocol).
    #[inline]
    pub fn size(&self) -> Size {
        self.geometry
    }

    /// Adds this element to the hit test result.
    pub fn hit_test_self(&self, result: &mut HitTestResult) -> bool {
        let bounds = Rect::from_min_size(Offset::ZERO, self.geometry);
        let entry = HitTestEntry::new(self.element_id, self.position, bounds);
        result.add(entry);
        true
    }

    /// Checks if the position is within this element's bounds.
    pub fn contains_position(&self) -> bool {
        let local_bounds = Rect::from_min_size(Offset::ZERO, self.geometry);
        local_bounds.contains(self.position)
    }

    /// Checks if the position is within a specific rectangle.
    pub fn position_in_rect(&self, rect: Rect) -> bool {
        rect.contains(self.position)
    }
}

// ============================================================================
// HIT TEST CONTEXT - SINGLE CHILD BOX PROTOCOL
// ============================================================================

impl<'a, T: HitTestTree> HitTestContext<'a, Single, BoxProtocol, T> {
    /// Gets the single child ID (convenience for Single arity).
    #[inline]
    pub fn single_child(&self) -> ElementId {
        *self.children_accessor.single()
    }

    /// Hit tests the single child at the given position.
    pub fn hit_test_single_child(&self, position: Offset, result: &mut HitTestResult) -> bool {
        let child_id = self.single_child();
        self.hit_test_child(child_id, position, result)
    }
}

// ============================================================================
// HIT TEST CONTEXT - SLIVER PROTOCOL SPECIFIC
// ============================================================================

impl<'a, A: Arity, T: HitTestTree> HitTestContext<'a, A, SliverProtocol, T>
where
    A::Accessor<'a, ElementId>: ChildrenAccess<'a, ElementId>,
{
    /// Returns the sliver geometry (convenience for Sliver protocol).
    #[inline]
    pub fn sliver_geometry(&self) -> SliverGeometry {
        self.geometry.clone()
    }

    /// Adds this element to the hit test result.
    ///
    /// For slivers, uses the paint extent for bounds.
    pub fn hit_test_self(&self, result: &mut HitTestResult) -> bool {
        // For slivers, use paint extent for bounds calculation
        let bounds = Rect::from_ltwh(0.0, 0.0, 0.0, self.geometry.paint_extent);
        let entry = HitTestEntry::new(self.element_id, self.position, bounds);
        result.add(entry);
        true
    }
}

// ============================================================================
// HIT TEST CONTEXT - SINGLE CHILD SLIVER PROTOCOL
// ============================================================================

impl<'a, T: HitTestTree> HitTestContext<'a, Single, SliverProtocol, T> {
    /// Gets the single child ID (convenience for Single arity).
    #[inline]
    pub fn single_child(&self) -> ElementId {
        *self.children_accessor.single()
    }

    /// Hit tests the single child at the given position.
    pub fn hit_test_single_child(&self, position: Offset, result: &mut HitTestResult) -> bool {
        let child_id = self.single_child();
        self.hit_test_child(child_id, position, result)
    }
}
