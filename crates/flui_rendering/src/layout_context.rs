//! Layout context for computing element sizes and positions.
//!
//! This module provides [`LayoutContext`] - a GAT-based context for layout operations
//! with compile-time arity validation and protocol-specific constraints.
//!
//! # Type Aliases
//!
//! - [`BoxLayoutContext`] - Layout context for Box protocol (Size, BoxConstraints)
//! - [`SliverLayoutContext`] - Layout context for Sliver protocol (SliverGeometry, SliverConstraints)

use std::fmt;
use std::marker::PhantomData;

use flui_foundation::ElementId;
use flui_types::{BoxConstraints, Offset, Size, SliverConstraints, SliverGeometry};
use tracing::{instrument, trace};

use crate::arity::{Arity, ChildrenAccess, Single};
use crate::layout_tree::LayoutTree;
use crate::protocol::{BoxProtocol, Protocol, SliverProtocol};
use crate::RenderResult;

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
    /// Children accessor for compile-time arity-checked access.
    ///
    /// Use methods like `.single()`, `.optional()`, or `.iter()` depending on arity.
    pub children: A::Accessor<'a, ElementId>,
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
            .field("children_count", &self.children.len())
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
    pub fn new(
        tree: &'a mut T,
        element_id: ElementId,
        constraints: P::Constraints,
        children: A::Accessor<'a, ElementId>,
    ) -> Self {
        Self {
            tree,
            element_id,
            constraints,
            children,
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
        self.children.iter().copied()
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
    #[instrument(level = "trace", skip(self, constraints), fields(child = %child_id.get()))]
    pub fn layout_child(
        &mut self,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> RenderResult<Size> {
        let result = self.tree.perform_layout(child_id, constraints);
        if let Ok(size) = &result {
            trace!(width = %size.width, height = %size.height, "child layout complete");
        }
        result
    }

    /// Layouts a child only if it needs layout.
    ///
    /// This is an optimization that skips layout if the child's dirty flag is not set.
    /// Returns `None` if the child doesn't need layout (caller should use cached size).
    #[instrument(level = "trace", skip(self, constraints), fields(child = %child_id.get()))]
    pub fn layout_child_if_needed(
        &mut self,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> RenderResult<Option<Size>> {
        if !self.tree.needs_layout(child_id) {
            trace!("child layout skipped (not dirty)");
            return Ok(None);
        }

        let size = self.tree.perform_layout(child_id, constraints)?;
        trace!(width = %size.width, height = %size.height, "child layout complete");
        Ok(Some(size))
    }

    /// Layouts all children with the same constraints.
    ///
    /// Returns a vector of (child_id, size) tuples on success.
    #[instrument(level = "trace", skip(self, constraints), fields(element = %self.element_id.get()))]
    pub fn layout_all_children(
        &mut self,
        constraints: BoxConstraints,
    ) -> RenderResult<Vec<(ElementId, Size)>> {
        let children: Vec<_> = self.children().collect();
        trace!(child_count = children.len(), "laying out all children");
        let mut results = Vec::with_capacity(children.len());

        for child_id in children {
            let size = self.layout_child(child_id, constraints)?;
            results.push((child_id, size));
        }

        Ok(results)
    }
}

// ============================================================================
// LAYOUT CONTEXT - SINGLE CHILD BOX PROTOCOL
// ============================================================================

impl<'a, T: LayoutTree> LayoutContext<'a, Single, BoxProtocol, T> {
    /// Gets the single child ID (convenience for Single arity).
    #[inline]
    pub fn single_child(&self) -> ElementId {
        *self.children.single()
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
    #[instrument(level = "trace", skip(self, constraints), fields(child = %child_id.get()))]
    pub fn layout_child(
        &mut self,
        child_id: ElementId,
        constraints: SliverConstraints,
    ) -> RenderResult<SliverGeometry> {
        let result = self.tree.perform_sliver_layout(child_id, constraints);
        if let Ok(geometry) = &result {
            trace!(
                scroll_extent = %geometry.scroll_extent,
                paint_extent = %geometry.paint_extent,
                "sliver child layout complete"
            );
        }
        result
    }

    /// Layouts a sliver child only if it needs layout.
    #[instrument(level = "trace", skip(self, constraints), fields(child = %child_id.get()))]
    pub fn layout_child_if_needed(
        &mut self,
        child_id: ElementId,
        constraints: SliverConstraints,
    ) -> RenderResult<Option<SliverGeometry>> {
        if !self.tree.needs_layout(child_id) {
            trace!("sliver child layout skipped (not dirty)");
            return Ok(None);
        }

        let geometry = self.tree.perform_sliver_layout(child_id, constraints)?;
        trace!(
            scroll_extent = %geometry.scroll_extent,
            paint_extent = %geometry.paint_extent,
            "sliver child layout complete"
        );
        Ok(Some(geometry))
    }

    /// Layouts all sliver children with the same constraints.
    #[instrument(level = "trace", skip(self, constraints), fields(element = %self.element_id.get()))]
    pub fn layout_all_children(
        &mut self,
        constraints: SliverConstraints,
    ) -> RenderResult<Vec<(ElementId, SliverGeometry)>> {
        let children: Vec<_> = self.children().collect();
        trace!(
            child_count = children.len(),
            "laying out all sliver children"
        );
        let mut results = Vec::with_capacity(children.len());

        for child_id in children {
            let geometry = self.layout_child(child_id, constraints)?;
            results.push((child_id, geometry));
        }

        Ok(results)
    }
}

// ============================================================================
// LAYOUT CONTEXT - SINGLE CHILD SLIVER PROTOCOL
// ============================================================================

impl<'a, T: LayoutTree> LayoutContext<'a, Single, SliverProtocol, T> {
    /// Gets the single child ID (convenience for Single arity).
    #[inline]
    pub fn single_child(&self) -> ElementId {
        *self.children.single()
    }

    /// Layouts the single child with the current constraints.
    pub fn layout_single_child(&mut self) -> RenderResult<SliverGeometry> {
        let child_id = self.single_child();
        self.layout_child(child_id, self.constraints)
    }

    /// Layouts the single child with transformed constraints.
    pub fn layout_single_child_with<F>(&mut self, transform: F) -> RenderResult<SliverGeometry>
    where
        F: FnOnce(SliverConstraints) -> SliverConstraints,
    {
        let child_id = self.single_child();
        let transformed_constraints = transform(self.constraints);
        self.layout_child(child_id, transformed_constraints)
    }
}
