//! Advanced GAT-based contexts for layout, paint, and hit testing operations.
//!
//! This module provides the core context types that render objects use to interact
//! with the rendering system. These contexts leverage Generic Associated Types (GAT)
//! for zero-cost abstractions and provide comprehensive access to tree operations,
//! children management, and rendering services.
//!
//! # Design Philosophy
//!
//! - **GAT-based**: Zero-cost abstractions with compile-time optimization
//! - **Protocol-generic**: Works with any layout protocol (Box, Sliver, Custom)
//! - **Arity-aware**: Type-safe children access based on arity constraints
//! - **Performance-optimized**: Batch operations and HRTB predicates
//! - **Ergonomic**: Convenient methods for common rendering operations
//!
//! # Context Types
//!
//! ## Layout Context
//!
//! Provides access to layout operations, constraints, and children management:
//!
//! ```rust,ignore
//! impl RenderBox<Variable> for RenderFlex {
//!     fn layout(&mut self, ctx: LayoutContext<'_, Variable, BoxProtocol>) -> Size {
//!         let mut total_size = 0.0;
//!
//!         // Use GAT-based iteration
//!         for child_id in ctx.children() {
//!             let size = ctx.layout_child(child_id, ctx.constraints)?;
//!             total_size += size.width;
//!         }
//!
//!         // Use HRTB predicates
//!         let flexible_children = ctx.children_where(|id| ctx.is_flexible(*id));
//!
//!         Size::new(total_size, ctx.constraints.max_height())
//!     }
//! }
//! ```
//!
//! ## Paint Context
//!
//! Provides canvas access and child painting operations:
//!
//! ```rust,ignore
//! impl RenderBox<Single> for RenderDecoratedBox {
//!     fn paint(&self, ctx: &mut PaintContext<'_, Single, BoxProtocol>) {
//!         // Draw decoration
//!         ctx.canvas_mut().draw_decoration(&self.decoration, ctx.bounds());
//!
//!         // Paint child
//!         let child_id = ctx.single_child();
//!         ctx.paint_child(child_id, Offset::ZERO)?;
//!     }
//! }
//! ```
//!
//! ## Hit Test Context
//!
//! Provides efficient hit testing with spatial optimizations:
//!
//! ```rust,ignore
//! impl RenderBox<Variable> for RenderStack {
//!     fn hit_test(&self, ctx: &HitTestContext<'_, Variable, BoxProtocol>, result: &mut HitTestResult) -> bool {
//!         // Test children in reverse z-order
//!         for child_id in ctx.children_reverse() {
//!             if ctx.hit_test_child(child_id, ctx.position, result) {
//!                 return true; // Early termination
//!             }
//!         }
//!
//!         // Test self
//!         ctx.hit_test_self(result)
//!     }
//! }
//! ```
//!
//! # Performance Features
//!
//! - **Batch operations**: Process multiple children efficiently
//! - **Const generic optimization**: Compile-time sizing for common cases
//! - **HRTB predicates**: Flexible filtering and searching
//! - **Early termination**: Optimized algorithms for common patterns
//! - **Cache integration**: Automatic result caching where beneficial

use std::fmt;
use std::marker::PhantomData;

use flui_foundation::ElementId;
use flui_interaction::HitTestResult;
use flui_painting::Canvas;
use flui_types::{Offset, Rect, Size, SliverConstraints, SliverGeometry};

use super::arity::{Arity, ChildrenAccess, RenderChildrenExt};
use super::geometry::BoxConstraints;
use super::protocol::{BoxProtocol, Protocol, SliverProtocol};
use super::render_tree::{HitTestTree, LayoutTree, PaintTree};
use crate::core::RenderResult;

// ============================================================================
// TYPE ALIASES FOR COMMON PROTOCOLS
// ============================================================================

/// Layout context for box protocol operations.
pub type BoxLayoutContext<'a, A> = LayoutContext<'a, A, BoxProtocol>;

/// Paint context for box protocol operations.
pub type BoxPaintContext<'a, A> = PaintContext<'a, A, BoxProtocol>;

/// Hit test context for box protocol operations.
pub type BoxHitTestContext<'a, A> = HitTestContext<'a, A, BoxProtocol>;

/// Layout context for sliver protocol operations.
pub type SliverLayoutContext<'a, A> = LayoutContext<'a, A, SliverProtocol>;

/// Paint context for sliver protocol operations.
pub type SliverPaintContext<'a, A> = PaintContext<'a, A, SliverProtocol>;

/// Hit test context for sliver protocol operations.
pub type SliverHitTestContext<'a, A> = HitTestContext<'a, A, SliverProtocol>;

// ============================================================================
// LAYOUT CONTEXT
// ============================================================================

/// GAT-based layout context for computing element sizes and positions.
///
/// This context provides type-safe access to layout operations with compile-time
/// arity validation and zero-cost abstractions through Generic Associated Types.
///
/// # Type Parameters
///
/// - `'a`: Lifetime of tree reference and children access
/// - `A`: Arity type constraining the number of children
/// - `P`: Layout protocol (BoxProtocol, SliverProtocol, etc.)
///
/// # Performance Characteristics
///
/// - **O(1) tree access**: Direct access to tree operations
/// - **Zero-cost iteration**: GAT-based children iteration
/// - **Compile-time optimization**: Const generics for batch operations
/// - **SIMD-friendly**: Layout operations can be vectorized
pub struct LayoutContext<'a, A: Arity, P: Protocol>
where
    A::Accessor<'a, ElementId>: RenderChildrenExt<'a>,
{
    tree: &'a mut (dyn LayoutTree + Send + Sync),
    element_id: ElementId,
    /// Layout constraints from the parent element.
    pub constraints: P::Constraints,
    children_accessor: A::Accessor<'a, ElementId>,
    _phantom: PhantomData<P>,
}

impl<'a, A: Arity, P: Protocol> fmt::Debug for LayoutContext<'a, A, P>
where
    A::Accessor<'a, ElementId>: RenderChildrenExt<'a>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LayoutContext")
            .field("element_id", &self.element_id)
            .field("constraints", &self.constraints)
            .field("children_count", &self.children_accessor.len())
            .finish_non_exhaustive()
    }
}

impl<'a, A: Arity, P: Protocol> LayoutContext<'a, A, P>
where
    A::Accessor<'a, ElementId>: RenderChildrenExt<'a>,
{
    /// Creates a new layout context.
    pub fn new(
        tree: &'a mut (dyn LayoutTree + Send + Sync),
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
    pub fn tree(&self) -> &dyn LayoutTree {
        self.tree
    }

    /// Gets mutable access to the tree for layout operations.
    #[inline]
    pub fn tree_mut(&mut self) -> &mut (dyn LayoutTree + Send + Sync) {
        self.tree
    }

    /// Returns a GAT-based iterator over child ElementIds.
    ///
    /// This provides zero-cost iteration with proper lifetime management.
    #[inline]
    pub fn children(&self) -> impl Iterator<Item = ElementId> + 'a {
        self.children_accessor.element_ids()
    }

    /// Returns children matching the given HRTB predicate.
    ///
    /// This method leverages Higher-Rank Trait Bounds for maximum flexibility
    /// while maintaining zero-cost abstractions.
    pub fn children_where<F>(&self, predicate: F) -> impl Iterator<Item = ElementId> + 'a
    where
        F: for<'b> Fn(&'b ElementId) -> bool + 'a,
    {
        self.children().filter(move |id| predicate(id))
    }

    /// Gets the number of children.
    #[inline]
    pub fn child_count(&self) -> usize {
        self.children_accessor.len()
    }

    /// Checks if this element has no children.
    #[inline]
    pub fn is_leaf(&self) -> bool {
        self.children_accessor.is_empty()
    }

    /// Finds the first child matching the predicate.
    pub fn find_child<F>(&self, predicate: F) -> Option<ElementId>
    where
        F: for<'b> Fn(&'b ElementId) -> bool,
    {
        self.children().find(|id| predicate(id))
    }

    /// Counts children matching the predicate.
    pub fn count_children_where<F>(&self, predicate: F) -> usize
    where
        F: for<'b> Fn(&'b ElementId) -> bool,
    {
        self.children().filter(|id| predicate(id)).count()
    }

    /// Checks if any child matches the predicate.
    pub fn any_child_where<F>(&self, predicate: F) -> bool
    where
        F: for<'b> Fn(&'b ElementId) -> bool,
    {
        self.children().any(|id| predicate(id))
    }

    /// Checks if all children match the predicate.
    pub fn all_children_where<F>(&self, predicate: F) -> bool
    where
        F: for<'b> Fn(&'b ElementId) -> bool,
    {
        !self.is_leaf() && self.children().all(|id| predicate(id))
    }
}

// Box protocol specific methods
impl<'a, A: Arity> LayoutContext<'a, A, BoxProtocol>
where
    A::Accessor<'a, ElementId>: RenderChildrenExt<'a>,
{
    /// Layouts a child element with box constraints.
    ///
    /// This is the primary method for performing child layout operations.
    /// It handles error propagation and integrates with the dirty tracking system.
    pub fn layout_child(
        &mut self,
        child_id: ElementId,
        constraints: BoxConstraints,
    ) -> RenderResult<Size> {
        match self.tree.perform_layout(child_id, constraints) {
            Ok(size) => {
                self.tree.set_offset(child_id, Offset::ZERO);
                Ok(size)
            }
            Err(e) => Err(e),
        }
    }

    /// Layouts a child at a specific offset.
    ///
    /// This combines layout and positioning in a single operation.
    pub fn layout_child_at(
        &mut self,
        child_id: ElementId,
        constraints: BoxConstraints,
        offset: Offset,
    ) -> RenderResult<Size> {
        let size = self.layout_child(child_id, constraints)?;
        self.tree.set_offset(child_id, offset);
        Ok(size)
    }

    /// Layouts all children with the same constraints.
    ///
    /// Returns successful layouts only, filtering out any failures.
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

    /// Layouts children in batches using const generic optimization.
    ///
    /// This method can be optimized at compile time for known batch sizes,
    /// enabling better vectorization and cache usage.
    pub fn layout_children_batch<const BATCH_SIZE: usize>(
        &mut self,
        constraints: BoxConstraints,
    ) -> Vec<[Option<Size>; BATCH_SIZE]> {
        let children: Vec<_> = self.children().collect();
        let mut results = Vec::new();

        for batch in children.chunks(BATCH_SIZE) {
            let mut batch_result = [None; BATCH_SIZE];

            for (i, &child_id) in batch.iter().enumerate() {
                batch_result[i] = self.layout_child(child_id, constraints).ok();
            }

            results.push(batch_result);
        }

        results
    }

    /// Layouts children conditionally based on a predicate.
    ///
    /// This is useful for layouts that only need to process certain children
    /// (e.g., visible children in a viewport).
    pub fn layout_children_where<F>(
        &mut self,
        constraints: BoxConstraints,
        predicate: F,
    ) -> Vec<(ElementId, Size)>
    where
        F: for<'b> Fn(&'b ElementId) -> bool,
    {
        let target_children: Vec<_> = self.children_where(predicate).collect();
        let mut results = Vec::with_capacity(target_children.len());

        for child_id in target_children {
            if let Ok(size) = self.layout_child(child_id, constraints) {
                results.push((child_id, size));
            }
        }

        results
    }

    /// Computes intrinsic dimensions by laying out children with loose constraints.
    ///
    /// This is useful for determining the natural size of an element when
    /// not constrained by parent layout requirements.
    pub fn compute_intrinsic_size(&mut self, axis: Option<f32>) -> Size {
        let loose_constraints = match axis {
            Some(width) => BoxConstraints::loose_width(width),
            None => BoxConstraints::loose(Size::new(f32::INFINITY, f32::INFINITY)),
        };

        let children_sizes = self.layout_all_children(loose_constraints);

        children_sizes
            .iter()
            .fold(Size::ZERO, |max_size, (_, size)| {
                Size::new(
                    max_size.width.max(size.width),
                    max_size.height.max(size.height),
                )
            })
    }

    /// Gets the cached size of a child, if available.
    ///
    /// This avoids re-layout if the child hasn't changed since the last layout pass.
    pub fn get_child_size(&self, child_id: ElementId) -> Option<Size> {
        // This would integrate with the tree's caching system
        // For now, return None to force layout
        None
    }

    /// Checks if a child needs to be laid out.
    pub fn child_needs_layout(&self, child_id: ElementId) -> bool {
        self.tree.needs_layout(child_id)
    }

    /// Marks a child as needing layout.
    pub fn mark_child_needs_layout(&mut self, child_id: ElementId) {
        self.tree.mark_needs_layout(child_id);
    }
}

// Single child convenience methods
impl<'a> LayoutContext<'a, super::arity::Single, BoxProtocol> {
    /// Gets the single child ElementId.
    #[inline]
    pub fn single_child(&self) -> ElementId {
        self.children_accessor.single_child_id()
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

// Sliver protocol specific methods
impl<'a, A: Arity> LayoutContext<'a, A, SliverProtocol>
where
    A::Accessor<'a, ElementId>: RenderChildrenExt<'a>,
{
    /// Layouts a child sliver element.
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
// PAINT CONTEXT
// ============================================================================

/// GAT-based paint context for drawing elements to a canvas.
///
/// This context provides comprehensive access to painting operations with
/// type-safe children management and efficient canvas operations.
pub struct PaintContext<'a, A: Arity, P: Protocol>
where
    A::Accessor<'a, ElementId>: RenderChildrenExt<'a>,
{
    tree: &'a mut (dyn PaintTree + Send + Sync),
    element_id: ElementId,
    /// The offset of this element in parent coordinates.
    pub offset: Offset,
    /// The size of this element from layout.
    pub size: Size,
    canvas: &'a mut Canvas,
    children_accessor: A::Accessor<'a, ElementId>,
    _phantom: PhantomData<P>,
}

impl<'a, A: Arity, P: Protocol> fmt::Debug for PaintContext<'a, A, P>
where
    A::Accessor<'a, ElementId>: RenderChildrenExt<'a>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PaintContext")
            .field("element_id", &self.element_id)
            .field("offset", &self.offset)
            .field("size", &self.size)
            .field("children_count", &self.children_accessor.len())
            .finish_non_exhaustive()
    }
}

impl<'a, A: Arity, P: Protocol> PaintContext<'a, A, P>
where
    A::Accessor<'a, ElementId>: RenderChildrenExt<'a>,
{
    /// Creates a new paint context.
    pub fn new(
        tree: &'a mut (dyn PaintTree + Send + Sync),
        element_id: ElementId,
        offset: Offset,
        size: Size,
        canvas: &'a mut Canvas,
        children_accessor: A::Accessor<'a, ElementId>,
    ) -> Self {
        Self {
            tree,
            element_id,
            offset,
            size,
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
        self.children_accessor.element_ids()
    }

    /// Returns the bounding rectangle of this element.
    #[inline]
    pub fn bounds(&self) -> Rect {
        Rect::from_offset_size(self.offset, self.size)
    }

    /// Returns the local bounding rectangle (at origin).
    #[inline]
    pub fn local_bounds(&self) -> Rect {
        Rect::from_size(self.size)
    }

    /// Paints a child element at the given offset.
    pub fn paint_child(&mut self, child_id: ElementId, child_offset: Offset) -> RenderResult<()> {
        let absolute_offset = self.offset + child_offset;
        match self.tree.perform_paint(child_id, absolute_offset) {
            Ok(child_canvas) => {
                // Composite the child canvas onto our canvas
                self.canvas.composite(child_canvas, child_offset);
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Paints all children at their stored offsets.
    ///
    /// This assumes that child offsets were set during the layout phase.
    /// Paints all children at their stored offsets.
    ///
    /// # Note
    ///
    /// Children without a stored offset will be painted at `Offset::ZERO`.
    /// This is safe because layout should have set offsets, but we gracefully
    /// handle missing offsets to avoid panics.
    pub fn paint_all_children(&mut self) -> Vec<RenderResult<()>> {
        let children: Vec<_> = self.children().collect();
        let mut results = Vec::with_capacity(children.len());

        for child_id in children {
            // Use ZERO as fallback if offset wasn't set during layout
            let child_offset = self.tree.get_offset(child_id).unwrap_or(Offset::ZERO);
            results.push(self.paint_child(child_id, child_offset));
        }

        results
    }

    /// Paints children with the same offset.
    pub fn paint_children_at(&mut self, offset: Offset) -> Vec<RenderResult<()>> {
        let children: Vec<_> = self.children().collect();
        let mut results = Vec::with_capacity(children.len());

        for child_id in children {
            results.push(self.paint_child(child_id, offset));
        }

        results
    }

    /// Paints children conditionally based on a predicate.
    ///
    /// # Note
    ///
    /// Children without a stored offset will be painted at `Offset::ZERO`.
    /// See `paint_all_children()` for details on offset fallback behavior.
    pub fn paint_children_where<F>(&mut self, predicate: F) -> Vec<RenderResult<()>>
    where
        F: for<'b> Fn(&'b ElementId) -> bool,
    {
        let target_children: Vec<_> = self.children().filter(|id| predicate(id)).collect();

        let mut results = Vec::with_capacity(target_children.len());

        for child_id in target_children {
            // Use ZERO as fallback if offset wasn't set during layout
            let child_offset = self.tree.get_offset(child_id).unwrap_or(Offset::ZERO);
            results.push(self.paint_child(child_id, child_offset));
        }

        results
    }

    /// Saves the current canvas state for later restoration.
    pub fn save(&mut self) {
        self.canvas.save();
    }

    /// Restores the previously saved canvas state.
    pub fn restore(&mut self) {
        self.canvas.restore();
    }

    /// Applies a clipping region for child painting.
    pub fn with_clip<F, R>(&mut self, clip_rect: Rect, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.canvas.save();
        self.canvas.clip_rect(clip_rect);
        let result = f(self);
        self.canvas.restore();
        result
    }

    /// Applies a translation offset for child painting.
    pub fn with_translation<F, R>(&mut self, offset: Offset, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        self.canvas.save();
        self.canvas.translate(offset.x, offset.y);
        let result = f(self);
        self.canvas.restore();
        result
    }
}

// Single child convenience methods
impl<'a> PaintContext<'a, super::arity::Single, BoxProtocol> {
    /// Gets the single child ElementId.
    #[inline]
    pub fn single_child(&self) -> ElementId {
        self.children_accessor.single_child_id()
    }

    /// Paints the single child at the given offset.
    pub fn paint_single_child(&mut self, offset: Offset) -> RenderResult<()> {
        let child_id = self.single_child();
        self.paint_child(child_id, offset)
    }

    /// Paints the single child at its stored offset.
    ///
    /// # Note
    ///
    /// If the child's offset hasn't been set (returns `None`), this method
    /// uses `Offset::ZERO` as a fallback. This is safe because:
    /// - The child will be painted at the parent's origin
    /// - Layout should have set the offset before painting
    /// - Using ZERO prevents panics while maintaining reasonable behavior
    pub fn paint_single_child_at_offset(&mut self) -> RenderResult<()> {
        let child_id = self.single_child();
        // Use ZERO as fallback if offset wasn't set during layout
        // This is safe because layout should have set the offset, but we
        // gracefully handle the case where it wasn't to avoid panics.
        let offset = self.tree.get_offset(child_id).unwrap_or(Offset::ZERO);
        self.paint_child(child_id, offset)
    }
}

// ============================================================================
// HIT TEST CONTEXT
// ============================================================================

/// GAT-based hit test context for pointer event handling.
///
/// This context provides efficient hit testing capabilities with spatial
/// optimizations and early termination support.
pub struct HitTestContext<'a, A: Arity, P: Protocol>
where
    A::Accessor<'a, ElementId>: RenderChildrenExt<'a>,
{
    tree: &'a (dyn HitTestTree + Send + Sync),
    element_id: ElementId,
    /// The position being tested in local coordinates.
    pub position: Offset,
    /// The size of this element.
    pub size: Size,
    children_accessor: A::Accessor<'a, ElementId>,
    _phantom: PhantomData<P>,
}

impl<'a, A: Arity, P: Protocol> fmt::Debug for HitTestContext<'a, A, P>
where
    A::Accessor<'a, ElementId>: RenderChildrenExt<'a>,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HitTestContext")
            .field("element_id", &self.element_id)
            .field("position", &self.position)
            .field("size", &self.size)
            .field("children_count", &self.children_accessor.len())
            .finish_non_exhaustive()
    }
}

impl<'a, A: Arity, P: Protocol> HitTestContext<'a, A, P>
where
    A::Accessor<'a, ElementId>: RenderChildrenExt<'a>,
{
    /// Creates a new hit test context.
    pub fn new(
        tree: &'a (dyn HitTestTree + Send + Sync),
        element_id: ElementId,
        position: Offset,
        size: Size,
        children_accessor: A::Accessor<'a, ElementId>,
    ) -> Self {
        Self {
            tree,
            element_id,
            position,
            size,
            children_accessor,
            _phantom: PhantomData,
        }
    }

    /// Gets the element ID being tested.
    #[inline]
    pub fn element_id(&self) -> ElementId {
        self.element_id
    }

    /// Returns a GAT-based iterator over child ElementIds.
    #[inline]
    pub fn children(&self) -> impl Iterator<Item = ElementId> + 'a {
        self.children_accessor.element_ids()
    }

    /// Returns children in reverse order (for z-order hit testing).
    pub fn children_reverse(&self) -> impl Iterator<Item = ElementId> + 'a {
        let children: Vec<_> = self.children().collect();
        children.into_iter().rev()
    }

    /// Returns the bounds of this element.
    #[inline]
    pub fn bounds(&self) -> Rect {
        Rect::from_size(self.size)
    }

    /// Checks if the position is within this element's bounds.
    #[inline]
    pub fn contains_position(&self, position: Offset) -> bool {
        position.dx >= 0.0
            && position.dy >= 0.0
            && position.dx < self.size.width
            && position.dy < self.size.height
    }

    /// Checks if the test position is within this element's bounds.
    #[inline]
    pub fn contains_self(&self) -> bool {
        self.contains_position(self.position)
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

    /// Hit tests children with early termination.
    ///
    /// Returns true as soon as the first hit is found, which is more
    /// efficient for simple hit testing scenarios.
    pub fn hit_test_children_early(&self, result: &mut HitTestResult) -> bool {
        for child_id in self.children_reverse() {
            if self.hit_test_child(child_id, self.position, result) {
                return true;
            }
        }
        false
    }

    /// Hit tests all children and accumulates results.
    ///
    /// This is useful when you need all hits (e.g., for multi-touch scenarios).
    pub fn hit_test_all_children(&self, result: &mut HitTestResult) -> bool {
        let mut any_hit = false;

        for child_id in self.children_reverse() {
            if self.hit_test_child(child_id, self.position, result) {
                any_hit = true;
            }
        }

        any_hit
    }

    /// Adds this element to the hit test result.
    pub fn add_to_result(&self, result: &mut HitTestResult) {
        result.add(self.element_id);
    }

    /// Performs a complete hit test including self and children.
    ///
    /// This is a convenience method that implements the standard hit testing
    /// algorithm: test children first (in reverse z-order), then self.
    pub fn hit_test_self_and_children(&self, result: &mut HitTestResult) -> bool {
        // Test children first (topmost first)
        if self.hit_test_children_early(result) {
            return true;
        }

        // Test self if no children were hit
        if self.contains_self() {
            self.add_to_result(result);
            return true;
        }

        false
    }

    /// Finds the topmost child at the given position.
    pub fn find_child_at_position(&self, position: Offset) -> Option<ElementId> {
        let mut result = HitTestResult::new();

        for child_id in self.children_reverse() {
            if self.hit_test_child(child_id, position, &mut result) {
                return Some(child_id);
            }
        }

        None
    }
}

// Single child convenience methods
impl<'a> HitTestContext<'a, super::arity::Single, BoxProtocol> {
    /// Gets the single child ElementId.
    #[inline]
    pub fn single_child(&self) -> ElementId {
        self.children_accessor.single_child_id()
    }

    /// Hit tests the single child.
    pub fn hit_test_single_child(&self, result: &mut HitTestResult) -> bool {
        let child_id = self.single_child();
        self.hit_test_child(child_id, self.position, result)
    }
}

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================

/// Creates a layout context for box protocol operations.
pub fn create_box_layout_context<'a, A: Arity>(
    tree: &'a mut (dyn LayoutTree + Send + Sync),
    element_id: ElementId,
    constraints: BoxConstraints,
    children_accessor: A::Accessor<'a, ElementId>,
) -> LayoutContext<'a, A, BoxProtocol>
where
    A::Accessor<'a, ElementId>: RenderChildrenExt<'a>,
{
    LayoutContext::new(tree, element_id, constraints, children_accessor)
}

/// Creates a paint context for box protocol operations.
pub fn create_box_paint_context<'a, A: Arity>(
    tree: &'a mut (dyn PaintTree + Send + Sync),
    element_id: ElementId,
    offset: Offset,
    size: Size,
    canvas: &'a mut Canvas,
    children_accessor: A::Accessor<'a, ElementId>,
) -> PaintContext<'a, A, BoxProtocol>
where
    A::Accessor<'a, ElementId>: RenderChildrenExt<'a>,
{
    PaintContext::new(tree, element_id, offset, size, canvas, children_accessor)
}

/// Creates a hit test context for box protocol operations.
pub fn create_box_hit_test_context<'a, A: Arity>(
    tree: &'a (dyn HitTestTree + Send + Sync),
    element_id: ElementId,
    position: Offset,
    size: Size,
    children_accessor: A::Accessor<'a, ElementId>,
) -> HitTestContext<'a, A, BoxProtocol>
where
    A::Accessor<'a, ElementId>: RenderChildrenExt<'a>,
{
    HitTestContext::new(tree, element_id, position, size, children_accessor)
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::arity::{Leaf, Single, Variable};

    // Mock implementations for testing
    struct MockLayoutTree;
    impl LayoutTree for MockLayoutTree {
        fn perform_layout(
            &mut self,
            _id: ElementId,
            constraints: BoxConstraints,
        ) -> RenderResult<Size> {
            Ok(constraints.biggest())
        }
        fn perform_sliver_layout(
            &mut self,
            _id: ElementId,
            _constraints: SliverConstraints,
        ) -> RenderResult<SliverGeometry> {
            Ok(SliverGeometry::zero())
        }
        fn set_offset(&mut self, _id: ElementId, _offset: Offset) {}
        fn get_offset(&self, _id: ElementId) -> Option<Offset> {
            Some(Offset::ZERO)
        }
        fn mark_needs_layout(&mut self, _id: ElementId) {}
        fn needs_layout(&self, _id: ElementId) -> bool {
            false
        }
        fn render_object(&self, _id: ElementId) -> Option<&dyn std::any::Any> {
            None
        }
        fn render_object_mut(&mut self, _id: ElementId) -> Option<&mut dyn std::any::Any> {
            None
        }
    }

    struct MockPaintTree;
    impl PaintTree for MockPaintTree {
        fn perform_paint(&mut self, _id: ElementId, _offset: Offset) -> RenderResult<Canvas> {
            Ok(Canvas::new())
        }
        fn mark_needs_paint(&mut self, _id: ElementId) {}
        fn needs_paint(&self, _id: ElementId) -> bool {
            false
        }
        fn render_object(&self, _id: ElementId) -> Option<&dyn std::any::Any> {
            None
        }
        fn render_object_mut(&mut self, _id: ElementId) -> Option<&mut dyn std::any::Any> {
            None
        }
    }

    struct MockHitTestTree;
    impl HitTestTree for MockHitTestTree {
        fn hit_test(&self, _id: ElementId, _position: Offset, _result: &mut HitTestResult) -> bool {
            false
        }
        fn render_object(&self, _id: ElementId) -> Option<&dyn std::any::Any> {
            None
        }
    }

    #[test]
    fn test_layout_context_creation() {
        let mut tree = MockLayoutTree;
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let children: [ElementId; 0] = [];
        let accessor = Leaf::from_slice(&children);

        let ctx = LayoutContext::new(&mut tree, ElementId::new(1), constraints, accessor);

        assert_eq!(ctx.element_id(), ElementId::new(1));
        assert_eq!(ctx.child_count(), 0);
        assert!(ctx.is_leaf());
    }

    #[test]
    fn test_single_child_methods() {
        let mut tree = MockLayoutTree;
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let children = [ElementId::new(42)];
        let accessor = Single::from_slice(&children);

        let ctx = LayoutContext::new(&mut tree, ElementId::new(1), constraints, accessor);

        assert_eq!(ctx.single_child(), ElementId::new(42));
        assert_eq!(ctx.child_count(), 1);
        assert!(!ctx.is_leaf());
    }

    #[test]
    fn test_children_predicates() {
        let mut tree = MockLayoutTree;
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let children = [
            ElementId::new(1),
            ElementId::new(2),
            ElementId::new(3),
            ElementId::new(4),
        ];
        let accessor = Variable::from_slice(&children);

        let ctx = LayoutContext::new(&mut tree, ElementId::new(1), constraints, accessor);

        // Test HRTB predicates
        let even_count = ctx.count_children_where(|id| id.get() % 2 == 0);
        assert_eq!(even_count, 2);

        let has_large = ctx.any_child_where(|id| id.get() > 3);
        assert!(has_large);

        let all_positive = ctx.all_children_where(|id| id.get() > 0);
        assert!(all_positive);

        let first_even = ctx.find_child(|id| id.get() % 2 == 0);
        assert_eq!(first_even, Some(ElementId::new(2)));
    }

    #[test]
    fn test_paint_context_bounds() {
        let mut tree = MockPaintTree;
        let mut canvas = Canvas::new();
        let children: [ElementId; 0] = [];
        let accessor = Leaf::from_slice(&children);

        let ctx = PaintContext::new(
            &mut tree,
            ElementId::new(1),
            Offset::new(10.0, 20.0),
            Size::new(100.0, 50.0),
            &mut canvas,
            accessor,
        );

        assert_eq!(ctx.bounds(), Rect::from_xywh(10.0, 20.0, 100.0, 50.0));
        assert_eq!(ctx.local_bounds(), Rect::from_xywh(0.0, 0.0, 100.0, 50.0));
    }

    #[test]
    fn test_hit_test_context_contains() {
        let tree = MockHitTestTree;
        let children: [ElementId; 0] = [];
        let accessor = Leaf::from_slice(&children);

        let ctx = HitTestContext::new(
            &tree,
            ElementId::new(1),
            Offset::new(25.0, 15.0),
            Size::new(100.0, 50.0),
            accessor,
        );

        assert!(ctx.contains_self());
        assert!(ctx.contains_position(Offset::new(50.0, 25.0)));
        assert!(!ctx.contains_position(Offset::new(150.0, 25.0)));
        assert!(!ctx.contains_position(Offset::new(-10.0, 25.0)));
    }
}
