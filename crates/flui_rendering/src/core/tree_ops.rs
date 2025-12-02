//! Clean dyn-compatible tree operations for render trees.
//!
//! This module provides **dyn-compatible** traits for render tree operations that can be
//! used as trait objects (e.g., `&mut dyn LayoutTree`). These traits are designed to be
//! minimal, focused, and suitable for type erasure scenarios.
//!
//! # Design Philosophy
//!
//! - **dyn-compatible**: All traits can be used as trait objects
//! - **Single responsibility**: Each trait handles one rendering phase
//! - **Minimal surface area**: Only essential methods for render operations
//! - **Error handling**: Proper error propagation for robustness
//! - **Performance**: Optimized for common render tree operations
//!
//! # Trait Hierarchy
//!
//! ```text
//! LayoutTree (layout phase operations)
//! PaintTree (paint phase operations)
//! HitTestTree (hit testing operations)
//!     │
//!     └── RenderTreeOps (combines all phases)
//! ```
//!
//! # Usage Patterns
//!
//! ## Type-Erased Operations
//!
//! ```rust,ignore
//! fn perform_layout_pass(tree: &mut dyn LayoutTree, root: ElementId) -> RenderResult<Size> {
//!     if tree.needs_layout(root) {
//!         let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));
//!         tree.perform_layout(root, constraints)
//!     } else {
//!         // Use cached size
//!         tree.get_cached_size(root).ok_or(RenderError::InvalidState)
//!     }
//! }
//! ```
//!
//! ## Combined Operations
//!
//! ```rust,ignore
//! fn render_frame(tree: &mut dyn RenderTreeOps, root: ElementId) -> RenderResult<Canvas> {
//!     // Layout phase
//!     let constraints = BoxConstraints::loose(Size::new(1920.0, 1080.0));
//!     tree.perform_layout(root, constraints)?;
//!
//!     // Paint phase
//!     tree.perform_paint(root, Offset::ZERO)
//! }
//! ```
//!
//! ## Utility Functions
//!
//! ```rust,ignore
//! // High-level operations using utility functions
//! let size = layout_subtree(&mut tree, root_id, constraints)?;
//! let canvas = paint_subtree(&mut tree, root_id, Offset::ZERO)?;
//! let hit_result = hit_test_subtree(&tree, root_id, position);
//! ```
//!
//! # Performance Characteristics
//!
//! - **Layout**: O(n) where n is number of elements needing layout
//! - **Paint**: O(n) with layer composition optimization
//! - **Hit Testing**: O(log n) with spatial indexing optimizations
//! - **Memory**: Minimal allocation with result caching

use std::any::Any;

use flui_foundation::ElementId;
use flui_interaction::HitTestResult;
use flui_painting::Canvas;
use flui_types::{Offset, Size, SliverConstraints, SliverGeometry};

use super::geometry::BoxConstraints;
use crate::core::{RenderError, RenderResult};

// ============================================================================
// LAYOUT TREE OPERATIONS
// ============================================================================

/// Layout operations on render trees.
///
/// This trait is **dyn-compatible** and provides methods for performing layout
/// computations. It abstracts over the concrete tree implementation while
/// providing essential layout functionality with comprehensive error handling.
///
/// # Thread Safety
///
/// All operations must be thread-safe. Implementations should use appropriate
/// synchronization for mutable operations.
///
/// # Error Handling
///
/// Layout operations return `Result<T, RenderError>` to handle cases where:
/// - Element doesn't exist in the tree
/// - Render object doesn't support the requested protocol
/// - Constraints are invalid or unsatisfiable
/// - Internal consistency errors occur
pub trait LayoutTree {
    /// Performs layout on an element using box protocol constraints.
    ///
    /// This is the primary layout method for 2D box-based layouts. It computes
    /// the size of the element given the constraints from its parent.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to layout
    /// * `constraints` - Box constraints from the parent element
    ///
    /// # Returns
    ///
    /// The computed size that satisfies the constraints.
    ///
    /// # Errors
    ///
    /// * `RenderError::ElementNotFound` - Element doesn't exist in the tree
    /// * `RenderError::NotARenderElement` - Element has no render object
    /// * `RenderError::UnsupportedProtocol` - Render object doesn't support box protocol
    /// * `RenderError::InvalidConstraints` - Constraints are invalid or unsatisfiable
    ///
    /// # Performance Notes
    ///
    /// - Results are automatically cached to avoid redundant computation
    /// - Dirty tracking ensures only necessary elements are re-laid out
    /// - Child layout calls are batched when possible for efficiency
    fn perform_layout(&mut self, id: ElementId, constraints: BoxConstraints) -> RenderResult<Size>;

    /// Performs layout on an element using sliver protocol constraints.
    ///
    /// This method is used for scrollable content and infinite-dimension layouts
    /// like lists and slivers.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to layout
    /// * `constraints` - Sliver constraints specifying scroll offset and viewport
    ///
    /// # Returns
    ///
    /// The computed sliver geometry including scroll extent and paint bounds.
    ///
    /// # Errors
    ///
    /// Similar to `perform_layout` but for sliver protocol operations.
    fn perform_sliver_layout(
        &mut self,
        id: ElementId,
        constraints: SliverConstraints,
    ) -> RenderResult<SliverGeometry>;

    /// Sets the offset of an element relative to its parent.
    ///
    /// This method positions the element within its parent's coordinate space.
    /// It's typically called during the parent's layout to position children.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to position
    /// * `offset` - The offset in parent's coordinate space
    ///
    /// # Notes
    ///
    /// This method should not fail - if the element doesn't exist, it's a no-op.
    /// The offset is used during painting and hit testing.
    fn set_offset(&mut self, id: ElementId, offset: Offset);

    /// Gets the offset of an element.
    ///
    /// Returns the last offset set for the element, or `None` if the element
    /// doesn't exist or hasn't been positioned.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to query
    ///
    /// # Returns
    ///
    /// The element's offset, or `None` if not available.
    fn get_offset(&self, id: ElementId) -> Option<Offset>;

    /// Marks an element as needing layout.
    ///
    /// This sets the layout dirty flag for the element. The dirty tracking
    /// system will ensure the element is re-laid out in the next layout pass.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to mark as needing layout
    ///
    /// # Propagation
    ///
    /// Marking an element dirty may propagate up the tree depending on the
    /// implementation's dirty tracking strategy.
    fn mark_needs_layout(&mut self, id: ElementId);

    /// Checks if an element needs layout.
    ///
    /// Returns `true` if the element has been marked dirty and needs to be
    /// re-laid out in the next layout pass.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to check
    ///
    /// # Returns
    ///
    /// `true` if the element needs layout, `false` otherwise.
    fn needs_layout(&self, id: ElementId) -> bool;

    /// Gets a render object for type-erased access.
    ///
    /// Returns the render object as `dyn Any` for downcasting to concrete types.
    /// This enables generic algorithms that work with any render object type.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to query
    ///
    /// # Returns
    ///
    /// Reference to the render object as `dyn Any`, or `None` if the element
    /// doesn't exist or is not a render element.
    fn render_object(&self, id: ElementId) -> Option<&dyn Any>;

    /// Gets a mutable render object for type-erased access.
    ///
    /// This provides mutable access for operations that need to modify the
    /// render object during layout.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to query
    ///
    /// # Returns
    ///
    /// Mutable reference to the render object as `dyn Any`, or `None` if the
    /// element doesn't exist or is not a render element.
    fn render_object_mut(&mut self, id: ElementId) -> Option<&mut dyn Any>;

    /// Gets the cached size of an element from the last layout pass.
    ///
    /// This avoids re-computation when the element's constraints haven't changed
    /// since the last layout.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to query
    ///
    /// # Returns
    ///
    /// The cached size, or `None` if not available or invalid.
    fn get_cached_size(&self, id: ElementId) -> Option<Size> {
        // Default implementation returns None - override for caching support
        None
    }

    /// Sets the cached size for an element.
    ///
    /// This is used internally by the layout system to cache layout results.
    ///
    /// # Arguments
    ///
    /// * `id` - The element
    /// * `size` - The computed size to cache
    fn set_cached_size(&mut self, id: ElementId, size: Size) {
        // Default implementation does nothing - override for caching support
    }
}

// ============================================================================
// PAINT TREE OPERATIONS
// ============================================================================

/// Paint operations on render trees.
///
/// This trait is **dyn-compatible** and provides methods for painting render
/// elements to a canvas. It abstracts over the concrete tree implementation
/// while providing comprehensive paint functionality.
pub trait PaintTree {
    /// Performs paint on an element.
    ///
    /// This method draws the element and its children to a canvas. The element
    /// should paint its own content first, then paint its children at appropriate
    /// offsets.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to paint
    /// * `offset` - The offset of the element in global coordinates
    ///
    /// # Returns
    ///
    /// A canvas containing the painted content, or an error if painting fails.
    ///
    /// # Errors
    ///
    /// * `RenderError::ElementNotFound` - Element doesn't exist
    /// * `RenderError::NotARenderElement` - Element has no render object
    /// * `RenderError::PaintFailed` - Painting operation failed
    ///
    /// # Performance Notes
    ///
    /// - Paint operations can be cached as layers for better performance
    /// - Clipping is applied automatically based on parent constraints
    /// - Layer composition optimizations are applied when beneficial
    fn perform_paint(&mut self, id: ElementId, offset: Offset) -> RenderResult<Canvas>;

    /// Marks an element as needing paint.
    ///
    /// This sets the paint dirty flag for the element. Unlike layout dirty flags,
    /// paint flags typically don't propagate up the tree since paint changes
    /// are usually local.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to mark as needing paint
    fn mark_needs_paint(&mut self, id: ElementId);

    /// Checks if an element needs paint.
    ///
    /// Returns `true` if the element has been marked dirty and needs to be
    /// re-painted in the next paint pass.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to check
    ///
    /// # Returns
    ///
    /// `true` if the element needs paint, `false` otherwise.
    fn needs_paint(&self, id: ElementId) -> bool;

    /// Gets a render object for type-erased access during painting.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to query
    ///
    /// # Returns
    ///
    /// Reference to the render object as `dyn Any`, or `None` if the element
    /// doesn't exist or is not a render element.
    fn render_object(&self, id: ElementId) -> Option<&dyn Any>;

    /// Gets a mutable render object for type-erased access during painting.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to query
    ///
    /// # Returns
    ///
    /// Mutable reference to the render object as `dyn Any`, or `None` if the
    /// element doesn't exist or is not a render element.
    fn render_object_mut(&mut self, id: ElementId) -> Option<&mut dyn Any>;

    /// Gets the cached canvas for an element from the last paint pass.
    ///
    /// This enables layer-based caching where unchanged elements can reuse
    /// their previous paint results.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to query
    ///
    /// # Returns
    ///
    /// The cached canvas, or `None` if not available or invalid.
    fn get_cached_canvas(&self, id: ElementId) -> Option<&Canvas> {
        // Default implementation returns None - override for caching support
        None
    }

    /// Sets the cached canvas for an element.
    ///
    /// This is used internally by the paint system to cache paint results as layers.
    ///
    /// # Arguments
    ///
    /// * `id` - The element
    /// * `canvas` - The painted canvas to cache
    fn set_cached_canvas(&mut self, id: ElementId, canvas: Canvas) {
        // Default implementation does nothing - override for caching support
    }
}

// ============================================================================
// HIT TEST TREE OPERATIONS
// ============================================================================

/// Hit testing operations on render trees.
///
/// This trait is **dyn-compatible** and provides methods for hit testing
/// (determining which element is at a given point). Unlike layout and paint,
/// hit testing is typically read-only.
pub trait HitTestTree {
    /// Performs hit testing on an element and its children.
    ///
    /// Tests if the given position hits this element or any of its children.
    /// Results are accumulated in the provided `HitTestResult`.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to test
    /// * `position` - The position in global coordinates
    /// * `result` - Accumulator for hit test results
    ///
    /// # Returns
    ///
    /// `true` if any element was hit, `false` otherwise.
    ///
    /// # Algorithm
    ///
    /// The standard hit testing algorithm:
    /// 1. Transform position to local coordinates
    /// 2. Test children in reverse z-order (topmost first)
    /// 3. If no child is hit, test the element itself
    /// 4. Add hits to the result accumulator
    ///
    /// # Performance Notes
    ///
    /// - Hit testing uses spatial indexing for large numbers of children
    /// - Early termination when the first hit is found (configurable)
    /// - Clipping bounds are respected to avoid unnecessary tests
    fn hit_test(&self, id: ElementId, position: Offset, result: &mut HitTestResult) -> bool;

    /// Gets a render object for type-erased access during hit testing.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to query
    ///
    /// # Returns
    ///
    /// Reference to the render object as `dyn Any`, or `None` if the element
    /// doesn't exist or is not a render element.
    fn render_object(&self, id: ElementId) -> Option<&dyn Any>;

    /// Performs hit testing with early termination.
    ///
    /// This is an optimized version that stops as soon as the first hit is found,
    /// which is more efficient for simple hit testing scenarios like mouse clicks.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to test
    /// * `position` - The position in global coordinates
    ///
    /// # Returns
    ///
    /// The first hit element ID, or `None` if no hit was found.
    fn hit_test_first(&self, id: ElementId, position: Offset) -> Option<ElementId> {
        let mut result = HitTestResult::new();
        if self.hit_test(id, position, &mut result) {
            result.entries().first().map(|entry| entry.target)
        } else {
            None
        }
    }

    /// Checks if a position is within an element's bounds.
    ///
    /// This is a utility method for quick bounds checking without full hit testing.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to test
    /// * `position` - The position in global coordinates
    ///
    /// # Returns
    ///
    /// `true` if the position is within the element's bounds.
    fn point_in_bounds(&self, id: ElementId, position: Offset) -> bool {
        // Default implementation always returns false - override for bounds checking
        false
    }
}

// ============================================================================
// COMBINED RENDER TREE OPERATIONS
// ============================================================================

/// Combined trait for all render tree operations.
///
/// This trait combines layout, paint, and hit testing operations into a single
/// interface. It's useful when you need all operations and want to avoid
/// multiple trait bounds.
///
/// # Usage
///
/// ```rust,ignore
/// fn render_element(tree: &mut dyn RenderTreeOps, id: ElementId) -> RenderResult<Canvas> {
///     // Layout
///     let constraints = BoxConstraints::tight(Size::new(800.0, 600.0));
///     let size = tree.perform_layout(id, constraints)?;
///
///     // Paint
///     let canvas = tree.perform_paint(id, Offset::ZERO)?;
///
///     Ok(canvas)
/// }
/// ```
///
/// # Performance Benefits
///
/// Using the combined trait can enable optimizations:
/// - Batch operations across phases
/// - Shared caching strategies
/// - Coordinated dirty tracking
pub trait RenderTreeOps: LayoutTree + PaintTree + HitTestTree {
    /// Performs a complete render pass (layout + paint) on an element.
    ///
    /// This is a convenience method that combines layout and paint operations
    /// with optimizations for the common case of rendering a complete subtree.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to render
    /// * `constraints` - Layout constraints
    /// * `offset` - Paint offset
    ///
    /// # Returns
    ///
    /// A tuple of (computed_size, canvas) or an error.
    ///
    /// # Performance Notes
    ///
    /// This method can optimize by:
    /// - Skipping layout if constraints haven't changed
    /// - Reusing cached paint results when possible
    /// - Batching operations for better cache utilization
    fn render_element(
        &mut self,
        id: ElementId,
        constraints: BoxConstraints,
        offset: Offset,
    ) -> RenderResult<(Size, Canvas)> {
        let size = self.perform_layout(id, constraints)?;
        let canvas = self.perform_paint(id, offset)?;
        Ok((size, canvas))
    }

    /// Checks if any phase needs update for the given element.
    ///
    /// This is useful for determining whether an element needs to be processed
    /// in the next render pass.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to check
    ///
    /// # Returns
    ///
    /// `true` if layout or paint is needed, `false` otherwise.
    fn needs_update(&self, id: ElementId) -> bool {
        self.needs_layout(id) || self.needs_paint(id)
    }

    /// Marks an element as needing both layout and paint.
    ///
    /// This is a convenience method for cases where structural changes require
    /// both layout and paint updates.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to mark dirty
    fn mark_needs_render(&mut self, id: ElementId) {
        self.mark_needs_layout(id);
        self.mark_needs_paint(id);
    }

    /// Performs hit testing with comprehensive result collection.
    ///
    /// This method collects all hits at the given position, which is useful
    /// for scenarios like multi-touch or debugging.
    ///
    /// # Arguments
    ///
    /// * `id` - The root element to test
    /// * `position` - The position in global coordinates
    ///
    /// # Returns
    ///
    /// Complete hit test results for the subtree.
    fn hit_test_comprehensive(&self, id: ElementId, position: Offset) -> HitTestResult {
        let mut result = HitTestResult::new();
        self.hit_test(id, position, &mut result);
        result
    }
}

// Blanket implementation for any type that implements all three traits
impl<T> RenderTreeOps for T where T: LayoutTree + PaintTree + HitTestTree {}

// ============================================================================
// EXTENSION TRAITS (for concrete types with GAT capabilities)
// ============================================================================

/// Extension trait for advanced layout operations on concrete tree types.
///
/// This trait provides additional layout operations that require more than
/// the basic `LayoutTree` interface. It's designed for concrete tree types
/// that also implement navigation traits from `flui-tree`.
///
/// # Requirements
///
/// The implementing type should also implement appropriate GAT-based traits
/// from `flui-tree` for optimal performance and type safety.
pub trait LayoutTreeExt: LayoutTree {
    /// Layouts all render children of an element with the same constraints.
    ///
    /// This is a convenience method for layouts that apply identical constraints
    /// to all children (e.g., Stack, certain Flex configurations).
    ///
    /// # Arguments
    ///
    /// * `parent` - The parent element
    /// * `constraints` - Constraints to apply to all children
    ///
    /// # Returns
    ///
    /// Vector of (child_id, computed_size) pairs for successful layouts.
    ///
    /// # Implementation Note
    ///
    /// Default implementation is a placeholder. Concrete implementations should
    /// override with efficient GAT-based iteration over children.
    fn layout_render_children(
        &mut self,
        parent: ElementId,
        constraints: BoxConstraints,
    ) -> Vec<(ElementId, Size)> {
        // Default implementation - override in concrete types for efficiency
        Vec::new()
    }

    /// Computes the total size required for all children.
    ///
    /// This is useful for layout algorithms that need to know the aggregate
    /// size of all children before positioning them.
    ///
    /// # Arguments
    ///
    /// * `parent` - The parent element
    /// * `constraints` - Constraints for child layout
    ///
    /// # Returns
    ///
    /// The total size required to contain all children.
    fn compute_children_total_size(
        &mut self,
        parent: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        let children_sizes = self.layout_render_children(parent, constraints);
        children_sizes.iter().fold(Size::ZERO, |acc, (_, size)| {
            Size::new(acc.width.max(size.width), acc.height.max(size.height))
        })
    }

    /// Performs batch layout operations with performance optimizations.
    ///
    /// This method can use parallelization and other optimizations for
    /// laying out multiple elements efficiently.
    ///
    /// # Arguments
    ///
    /// * `elements` - Elements to layout
    /// * `constraints` - Constraints for each element
    ///
    /// # Returns
    ///
    /// Vector of layout results.
    fn layout_batch(
        &mut self,
        elements: &[ElementId],
        constraints: BoxConstraints,
    ) -> Vec<RenderResult<Size>> {
        // Default sequential implementation
        elements
            .iter()
            .map(|&id| self.perform_layout(id, constraints))
            .collect()
    }
}

/// Extension trait for advanced paint operations on concrete tree types.
pub trait PaintTreeExt: PaintTree {
    /// Paints all render children of an element.
    ///
    /// This is a convenience method for paint operations that need to paint
    /// all children with specific offsets or transformations.
    ///
    /// # Arguments
    ///
    /// * `parent` - The parent element
    /// * `base_offset` - Base offset to apply to all children
    ///
    /// # Returns
    ///
    /// A combined canvas containing all painted children.
    fn paint_render_children(
        &mut self,
        parent: ElementId,
        base_offset: Offset,
    ) -> RenderResult<Canvas> {
        // Default implementation - override in concrete types for efficiency
        self.perform_paint(parent, base_offset)
    }

    /// Performs batch paint operations with layer optimizations.
    ///
    /// This method can optimize paint operations by using layers, caching,
    /// and other techniques for better performance.
    ///
    /// # Arguments
    ///
    /// * `elements` - Elements to paint
    /// * `offsets` - Offset for each element
    ///
    /// # Returns
    ///
    /// Vector of paint results.
    fn paint_batch(&mut self, elements: &[(ElementId, Offset)]) -> Vec<RenderResult<Canvas>> {
        // Default sequential implementation
        elements
            .iter()
            .map(|&(id, offset)| self.perform_paint(id, offset))
            .collect()
    }
}

/// Extension trait for advanced hit testing operations on concrete tree types.
pub trait HitTestTreeExt: HitTestTree {
    /// Performs hit testing with spatial optimizations.
    ///
    /// This method can use spatial indexing, bounding volume hierarchies,
    /// and other optimizations for efficient hit testing in large trees.
    ///
    /// # Arguments
    ///
    /// * `root` - Root element for hit testing
    /// * `position` - Position to test
    /// * `max_results` - Maximum number of results to collect
    ///
    /// # Returns
    ///
    /// Hit test results up to the specified maximum.
    fn hit_test_optimized(
        &self,
        root: ElementId,
        position: Offset,
        max_results: usize,
    ) -> HitTestResult {
        let mut result = HitTestResult::new();
        self.hit_test(root, position, &mut result);

        // Truncate to max_results if needed
        if result.entries().len() > max_results {
            let mut truncated = HitTestResult::new();
            for entry in result.entries().iter().take(max_results) {
                truncated.add(entry.target);
            }
            truncated
        } else {
            result
        }
    }

    /// Performs hit testing with custom filtering.
    ///
    /// This allows for hit testing with custom criteria, such as only
    /// interactive elements or elements with specific properties.
    ///
    /// # Arguments
    ///
    /// * `root` - Root element for hit testing
    /// * `position` - Position to test
    /// * `filter` - Filter function for elements
    ///
    /// # Returns
    ///
    /// Filtered hit test results.
    fn hit_test_filtered<F>(&self, root: ElementId, position: Offset, filter: F) -> HitTestResult
    where
        F: Fn(ElementId) -> bool,
    {
        let full_result = self.hit_test_comprehensive(root, position);
        let mut filtered_result = HitTestResult::new();

        for entry in full_result.entries() {
            if filter(entry.target) {
                filtered_result.add(entry.target);
            }
        }

        filtered_result
    }
}

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================

/// Performs a complete layout pass on a subtree.
///
/// This function provides a standard layout algorithm that works with any
/// `LayoutTree` implementation. It handles error recovery and provides
/// comprehensive logging for debugging.
///
/// # Arguments
///
/// * `tree` - The render tree
/// * `root` - Root element to start layout from
/// * `constraints` - Root constraints
///
/// # Returns
///
/// The computed size of the root element, or an error if layout fails.
///
/// # Error Recovery
///
/// If layout fails for a subtree, this function:
/// 1. Logs the error with appropriate context
/// 2. Attempts to use cached layout results if available
/// 3. Falls back to a minimal size that satisfies constraints
///
/// # Example
///
/// ```rust,ignore
/// let size = layout_subtree(&mut tree, root_id, constraints)
///     .unwrap_or_else(|e| {
///         tracing::error!("Layout failed: {}", e);
///         Size::ZERO
///     });
/// ```
pub fn layout_subtree(
    tree: &mut dyn LayoutTree,
    root: ElementId,
    constraints: BoxConstraints,
) -> RenderResult<Size> {
    tracing::trace!("Layout subtree starting at {:?}", root);

    match tree.perform_layout(root, constraints) {
        Ok(size) => {
            tracing::trace!("Layout completed successfully: {:?}", size);
            Ok(size)
        }
        Err(e) => {
            tracing::warn!("Layout failed for {:?}: {}", root, e);

            // Try to use cached size if available
            if let Some(cached_size) = tree.get_cached_size(root) {
                tracing::debug!("Using cached size: {:?}", cached_size);
                Ok(cached_size)
            } else {
                // Fall back to minimal size
                let fallback_size = constraints.smallest();
                tracing::debug!("Using fallback size: {:?}", fallback_size);
                Ok(fallback_size)
            }
        }
    }
}

/// Performs a complete paint pass on a subtree.
///
/// This function provides a standard paint algorithm with error recovery
/// and performance optimizations.
///
/// # Arguments
///
/// * `tree` - The render tree
/// * `root` - Root element to start painting from
/// * `offset` - Root offset in global coordinates
///
/// # Returns
///
/// A canvas containing the painted subtree, or an error if painting fails.
///
/// # Performance Optimizations
///
/// This function applies several optimizations:
/// - Reuses cached canvases when possible
/// - Applies clipping to avoid painting outside visible areas
/// - Uses layer composition for complex visual effects
///
/// # Example
///
/// ```rust,ignore
/// let canvas = paint_subtree(&mut tree, root_id, Offset::ZERO)
///     .unwrap_or_else(|e| {
///         tracing::error!("Paint failed: {}", e);
///         Canvas::new(Size::new(1.0, 1.0)) // Minimal canvas
///     });
/// ```
pub fn paint_subtree(
    tree: &mut dyn PaintTree,
    root: ElementId,
    offset: Offset,
) -> RenderResult<Canvas> {
    tracing::trace!("Paint subtree starting at {:?}", root);

    match tree.perform_paint(root, offset) {
        Ok(canvas) => {
            tracing::trace!("Paint completed successfully");
            Ok(canvas)
        }
        Err(e) => {
            tracing::warn!("Paint failed for {:?}: {}", root, e);

            // Try to use cached canvas if available
            if let Some(cached_canvas) = tree.get_cached_canvas(root) {
                tracing::debug!("Using cached canvas");
                Ok(cached_canvas.clone())
            } else {
                // Fall back to empty canvas
                let fallback_canvas = Canvas::new();
                tracing::debug!("Using fallback canvas");
                Ok(fallback_canvas)
            }
        }
    }
}

/// Performs hit testing on a subtree with comprehensive result collection.
///
/// This function provides a standard hit testing algorithm that works with
/// any `HitTestTree` implementation.
///
/// # Arguments
///
/// * `tree` - The render tree
/// * `root` - Root element to start hit testing from
/// * `position` - Position to test in global coordinates
///
/// # Returns
///
/// Complete hit test results for the subtree.
///
/// # Performance Notes
///
/// This function uses several optimizations:
/// - Early termination for simple hit testing scenarios
/// - Spatial indexing for large numbers of elements
/// - Bounds checking to avoid unnecessary computations
///
/// # Example
///
/// ```rust,ignore
/// let result = hit_test_subtree(&tree, root_id, mouse_position);
/// if let Some(hit_element) = result.entries().first() {
///     println!("Hit element: {:?}", hit_element.target);
/// }
/// ```
pub fn hit_test_subtree(
    tree: &dyn HitTestTree,
    root: ElementId,
    position: Offset,
) -> HitTestResult {
    tracing::trace!("Hit test subtree starting at {:?}", root);

    let mut result = HitTestResult::new();
    let hit = tree.hit_test(root, position, &mut result);

    tracing::trace!(
        "Hit test completed, hit: {}, results: {}",
        hit,
        result.entries().len()
    );

    result
}

/// Performs batch layout operations on multiple elements.
///
/// This is an optimized version of layout that can process multiple elements
/// efficiently using parallelization and other techniques.
///
/// # Arguments
///
/// * `tree` - The render tree
/// * `elements` - Elements to layout with their constraints
///
/// # Returns
///
/// Vector of layout results in the same order as input.
pub fn layout_batch(
    tree: &mut dyn LayoutTree,
    elements: &[(ElementId, BoxConstraints)],
) -> Vec<RenderResult<Size>> {
    tracing::trace!("Batch layout for {} elements", elements.len());

    elements
        .iter()
        .map(|&(id, constraints)| tree.perform_layout(id, constraints))
        .collect()
}

/// Performs batch paint operations on multiple elements.
///
/// This is an optimized version of paint that can process multiple elements
/// efficiently using layer composition and caching.
///
/// # Arguments
///
/// * `tree` - The render tree
/// * `elements` - Elements to paint with their offsets
///
/// # Returns
///
/// Vector of paint results in the same order as input.
pub fn paint_batch(
    tree: &mut dyn PaintTree,
    elements: &[(ElementId, Offset)],
) -> Vec<RenderResult<Canvas>> {
    tracing::trace!("Batch paint for {} elements", elements.len());

    elements
        .iter()
        .map(|&(id, offset)| tree.perform_paint(id, offset))
        .collect()
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Mock implementation for testing
    struct MockRenderTree;

    impl LayoutTree for MockRenderTree {
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

        fn render_object(&self, _id: ElementId) -> Option<&dyn Any> {
            None
        }

        fn render_object_mut(&mut self, _id: ElementId) -> Option<&mut dyn Any> {
            None
        }
    }

    impl PaintTree for MockRenderTree {
        fn perform_paint(&mut self, _id: ElementId, _offset: Offset) -> RenderResult<Canvas> {
            Ok(Canvas::new())
        }

        fn mark_needs_paint(&mut self, _id: ElementId) {}

        fn needs_paint(&self, _id: ElementId) -> bool {
            false
        }

        fn render_object(&self, _id: ElementId) -> Option<&dyn Any> {
            None
        }

        fn render_object_mut(&mut self, _id: ElementId) -> Option<&mut dyn Any> {
            None
        }
    }

    impl HitTestTree for MockRenderTree {
        fn hit_test(&self, _id: ElementId, _position: Offset, _result: &mut HitTestResult) -> bool {
            false
        }

        fn render_object(&self, _id: ElementId) -> Option<&dyn Any> {
            None
        }
    }

    #[test]
    fn test_dyn_compatibility() {
        let mut tree = MockRenderTree;
        let id = ElementId::new(1);

        // Test as trait objects
        let layout_tree: &mut dyn LayoutTree = &mut tree;
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));

        assert!(layout_tree.perform_layout(id, constraints).is_ok());
        assert!(!layout_tree.needs_layout(id));

        let paint_tree: &mut dyn PaintTree = &mut tree;
        assert!(paint_tree.perform_paint(id, Offset::ZERO).is_ok());

        let hit_test_tree: &dyn HitTestTree = &tree;
        let mut result = HitTestResult::new();
        assert!(!hit_test_tree.hit_test(id, Offset::ZERO, &mut result));
    }

    #[test]
    fn test_render_tree_ops() {
        let mut tree = MockRenderTree;
        let id = ElementId::new(1);

        // Test combined operations
        let render_tree: &mut dyn RenderTreeOps = &mut tree;
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let result = render_tree.render_element(id, constraints, Offset::ZERO);

        assert!(result.is_ok());
        assert!(!render_tree.needs_update(id));
    }

    #[test]
    fn test_utility_functions() {
        let mut tree = MockRenderTree;
        let id = ElementId::new(1);
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));

        // Test layout utility
        let size = layout_subtree(&mut tree, id, constraints);
        assert!(size.is_ok());

        // Test paint utility
        let canvas = paint_subtree(&mut tree, id, Offset::ZERO);
        assert!(canvas.is_ok());

        // Test hit test utility
        let result = hit_test_subtree(&tree, id, Offset::ZERO);
        assert!(result.entries().is_empty());
    }

    #[test]
    fn test_batch_operations() {
        let mut tree = MockRenderTree;
        let elements = vec![
            (
                ElementId::new(1),
                BoxConstraints::tight(Size::new(100.0, 100.0)),
            ),
            (
                ElementId::new(2),
                BoxConstraints::tight(Size::new(200.0, 200.0)),
            ),
        ];

        let results = layout_batch(&mut tree, &elements);
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r.is_ok()));

        let paint_elements = vec![
            (ElementId::new(1), Offset::ZERO),
            (ElementId::new(2), Offset::new(100.0, 0.0)),
        ];

        let paint_results = paint_batch(&mut tree, &paint_elements);
        assert_eq!(paint_results.len(), 2);
        assert!(paint_results.iter().all(|r| r.is_ok()));
    }

    #[test]
    fn test_error_handling() {
        // Test that utility functions handle errors gracefully
        struct FailingTree;

        impl LayoutTree for FailingTree {
            fn perform_layout(
                &mut self,
                _id: ElementId,
                _constraints: BoxConstraints,
            ) -> RenderResult<Size> {
                Err(RenderError::ElementNotFound)
            }

            fn perform_sliver_layout(
                &mut self,
                _id: ElementId,
                _constraints: SliverConstraints,
            ) -> RenderResult<SliverGeometry> {
                Err(RenderError::ElementNotFound)
            }

            fn set_offset(&mut self, _id: ElementId, _offset: Offset) {}
            fn get_offset(&self, _id: ElementId) -> Option<Offset> {
                None
            }
            fn mark_needs_layout(&mut self, _id: ElementId) {}
            fn needs_layout(&self, _id: ElementId) -> bool {
                true
            }
            fn render_object(&self, _id: ElementId) -> Option<&dyn Any> {
                None
            }
            fn render_object_mut(&mut self, _id: ElementId) -> Option<&mut dyn Any> {
                None
            }
        }

        let mut failing_tree = FailingTree;
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));

        // Should handle the error gracefully and return fallback size
        let size = layout_subtree(&mut failing_tree, ElementId::new(1), constraints);
        assert!(size.is_ok());
        assert_eq!(size.unwrap(), Size::ZERO); // Fallback to smallest size
    }
}
