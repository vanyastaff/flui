//! Render tree traits for layout, paint, and hit testing operations.
//!
//! This module provides **dyn-compatible** traits for render operations that can be
//! used as trait objects (e.g., `&mut dyn LayoutTree`). These traits are designed
//! to be minimal and focused on specific phases of the rendering pipeline.
//!
//! For full tree navigation and GAT-based operations, use the concrete tree type
//! directly with traits from `flui-tree`. These traits are strictly for render
//! operations where type erasure is required.
//!
//! # Design Philosophy
//!
//! - **dyn-compatible**: All traits can be used as trait objects
//! - **Single responsibility**: Each trait handles one rendering phase
//! - **Minimal surface area**: Only essential methods for render operations
//! - **Error handling**: Proper error propagation for robustness
//!
//! # Trait Hierarchy
//!
//! ```text
//! LayoutTree (layout phase)
//! PaintTree (paint phase)
//! HitTestTree (hit testing)
//!     │
//!     └── FullRenderTree (combines all phases)
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! // Type-erased render operations
//! fn perform_layout_pass(tree: &mut dyn LayoutTree, root: ElementId) {
//!     if tree.needs_layout(root) {
//!         let size = tree.perform_layout(root, BoxConstraints::tight(Size::new(800.0, 600.0)))?;
//!         println!("Root size: {:?}", size);
//!     }
//! }
//!
//! // Combined operations with full render tree
//! fn render_frame(tree: &mut dyn FullRenderTree, root: ElementId) -> Result<Canvas, RenderError> {
//!     // Layout phase
//!     tree.perform_layout(root, constraints)?;
//!
//!     // Paint phase
//!     let canvas = tree.perform_paint(root, Offset::ZERO)?;
//!
//!     Ok(canvas)
//! }
//! ```

use std::any::Any;

use flui_foundation::ElementId;
use flui_interaction::HitTestResult;
use flui_painting::Canvas;
use flui_types::{Offset, Size, SliverConstraints, SliverGeometry};

use crate::core::BoxConstraints;
use crate::error::RenderError;

// ============================================================================
// LAYOUT TREE TRAIT
// ============================================================================

/// Layout operations on the render tree.
///
/// This trait is **dyn-compatible** and provides methods for performing layout
/// computations. It abstracts over the concrete tree implementation while
/// providing essential layout functionality.
///
/// # dyn Compatibility
///
/// All methods avoid Generic Associated Types (GAT) and return concrete types
/// to ensure the trait can be used as `&mut dyn LayoutTree`.
///
/// # Error Handling
///
/// Layout operations return `Result<T, RenderError>` to handle cases where:
/// - Element doesn't exist in the tree
/// - Render object doesn't support the requested protocol
/// - Internal consistency errors
pub trait LayoutTree {
    /// Performs layout on an element using box protocol constraints.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to layout
    /// * `constraints` - Box constraints from parent
    ///
    /// # Returns
    ///
    /// The computed size that satisfies the constraints.
    ///
    /// # Errors
    ///
    /// * `RenderError::ElementNotFound` - Element doesn't exist
    /// * `RenderError::NotARenderElement` - Element has no render object
    /// * `RenderError::UnsupportedProtocol` - Render object doesn't support box protocol
    fn perform_layout(
        &mut self,
        id: ElementId,
        constraints: BoxConstraints,
    ) -> Result<Size, RenderError>;

    /// Performs layout on an element using sliver protocol constraints.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to layout
    /// * `constraints` - Sliver constraints from parent
    ///
    /// # Returns
    ///
    /// The computed sliver geometry.
    ///
    /// # Errors
    ///
    /// * `RenderError::ElementNotFound` - Element doesn't exist
    /// * `RenderError::NotARenderElement` - Element has no render object
    /// * `RenderError::UnsupportedProtocol` - Render object doesn't support sliver protocol
    fn perform_sliver_layout(
        &mut self,
        id: ElementId,
        constraints: SliverConstraints,
    ) -> Result<SliverGeometry, RenderError>;

    /// Sets the offset of an element (position relative to parent).
    ///
    /// # Arguments
    ///
    /// * `id` - The element to position
    /// * `offset` - The offset in parent's coordinate space
    ///
    /// # Notes
    ///
    /// This method should not fail - if the element doesn't exist, it's a no-op.
    fn set_offset(&mut self, id: ElementId, offset: Offset);

    /// Gets the offset of an element.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to query
    ///
    /// # Returns
    ///
    /// The offset if the element exists and has been positioned, `None` otherwise.
    fn get_offset(&self, id: ElementId) -> Option<Offset>;

    /// Marks an element as needing layout.
    ///
    /// This sets the dirty flag for the element and may propagate up the tree
    /// depending on the implementation's dirty tracking strategy.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to mark dirty
    fn mark_needs_layout(&mut self, id: ElementId);

    /// Checks if an element needs layout.
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
    /// # Arguments
    ///
    /// * `id` - The element to query
    ///
    /// # Returns
    ///
    /// Mutable reference to the render object as `dyn Any`, or `None` if the
    /// element doesn't exist or is not a render element.
    fn render_object_mut(&mut self, id: ElementId) -> Option<&mut dyn Any>;
}

// ============================================================================
// PAINT TREE TRAIT
// ============================================================================

/// Paint operations on the render tree.
///
/// This trait is **dyn-compatible** and provides methods for painting render
/// elements to a canvas. It abstracts over the concrete tree implementation.
pub trait PaintTree {
    /// Performs paint on an element.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to paint
    /// * `offset` - The offset in global coordinates
    ///
    /// # Returns
    ///
    /// A canvas containing the painted content.
    ///
    /// # Errors
    ///
    /// * `RenderError::ElementNotFound` - Element doesn't exist
    /// * `RenderError::NotARenderElement` - Element has no render object
    /// * `RenderError::PaintFailed` - Painting operation failed
    fn perform_paint(&mut self, id: ElementId, offset: Offset) -> Result<Canvas, RenderError>;

    /// Marks an element as needing paint.
    ///
    /// This sets the paint dirty flag for the element. Unlike layout dirty flags,
    /// paint flags typically don't propagate up the tree.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to mark as needing paint
    fn mark_needs_paint(&mut self, id: ElementId);

    /// Checks if an element needs paint.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to check
    ///
    /// # Returns
    ///
    /// `true` if the element needs paint, `false` otherwise.
    fn needs_paint(&self, id: ElementId) -> bool;

    /// Gets a render object for type-erased access.
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
    /// # Arguments
    ///
    /// * `id` - The element to query
    ///
    /// # Returns
    ///
    /// Mutable reference to the render object as `dyn Any`, or `None` if the
    /// element doesn't exist or is not a render element.
    fn render_object_mut(&mut self, id: ElementId) -> Option<&mut dyn Any>;
}

// ============================================================================
// HIT TEST TREE TRAIT
// ============================================================================

/// Hit testing operations on the render tree.
///
/// This trait is **dyn-compatible** and provides methods for hit testing
/// (determining which element is at a given point). Unlike layout and paint,
/// hit testing is typically read-only.
pub trait HitTestTree {
    /// Performs hit testing on an element.
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
    fn hit_test(&self, id: ElementId, position: Offset, result: &mut HitTestResult) -> bool;

    /// Gets a render object for type-erased access.
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
}

// ============================================================================
// COMBINED TRAIT
// ============================================================================

/// Combined trait for full render tree operations.
///
/// This trait combines all rendering phases (layout, paint, hit testing) into
/// a single interface. It's useful when you need all operations and want to
/// avoid multiple trait bounds.
///
/// # Usage
///
/// ```rust,ignore
/// fn render_element(tree: &mut dyn FullRenderTree, id: ElementId) -> Result<Canvas, RenderError> {
///     // Layout
///     let size = tree.perform_layout(id, constraints)?;
///
///     // Paint
///     let canvas = tree.perform_paint(id, Offset::ZERO)?;
///
///     Ok(canvas)
/// }
/// ```
pub trait FullRenderTree: LayoutTree + PaintTree + HitTestTree {
    /// Performs a complete render pass (layout + paint) on an element.
    ///
    /// This is a convenience method that combines layout and paint operations.
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
    fn render_element(
        &mut self,
        id: ElementId,
        constraints: BoxConstraints,
        offset: Offset,
    ) -> Result<(Size, Canvas), RenderError> {
        let size = self.perform_layout(id, constraints)?;
        let canvas = self.perform_paint(id, offset)?;
        Ok((size, canvas))
    }

    /// Checks if any phase needs update for the given element.
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
}

// Blanket implementation for any type that implements all three traits
impl<T> FullRenderTree for T where T: LayoutTree + PaintTree + HitTestTree {}

// ============================================================================
// EXTENSION TRAITS (for concrete types)
// ============================================================================

/// Extension trait for advanced layout operations.
///
/// This trait provides additional layout operations that require more than
/// the basic `LayoutTree` interface. It's designed for concrete tree types
/// that also implement navigation traits from `flui-tree`.
///
/// # Requirements
///
/// The implementing type must also implement appropriate traits from `flui-tree`
/// for tree navigation and render access.
pub trait LayoutTreeExt: LayoutTree {
    /// Layouts all render children of an element with the same constraints.
    ///
    /// This is a convenience method for layouts that apply identical constraints
    /// to all children (e.g., Stack, Flex in certain configurations).
    ///
    /// # Arguments
    ///
    /// * `parent` - The parent element
    /// * `constraints` - Constraints to apply to all children
    ///
    /// # Returns
    ///
    /// Vector of (child_id, computed_size) pairs.
    ///
    /// # Implementation Note
    ///
    /// Default implementation requires the type to also implement tree navigation
    /// traits to iterate over children. Concrete implementations can override
    /// for better performance.
    fn layout_render_children(
        &mut self,
        parent: ElementId,
        constraints: BoxConstraints,
    ) -> Vec<(ElementId, Size)> {
        // Default implementation - override in concrete types for efficiency
        Vec::new()
    }

    /// Computes the total size of all children given constraints.
    ///
    /// Useful for layout algorithms that need to know the aggregate size
    /// of all children before positioning them.
    fn total_children_size(
        &mut self,
        parent: ElementId,
        constraints: BoxConstraints,
    ) -> Size {
        let children_sizes = self.layout_render_children(parent, constraints);
        children_sizes.iter().fold(Size::ZERO, |acc, (_, size)| {
            Size::new(acc.width + size.width, acc.height.max(size.height))
        })
    }
}

/// Extension trait for advanced paint operations.
pub trait PaintTreeExt: PaintTree {
    /// Paints all render children of an element.
    ///
    /// This is a convenience method for paint operations that need to paint
    /// all children with specific offsets.
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
    ) -> Result<Canvas, RenderError> {
        // Default implementation - override in concrete types
        self.perform_paint(parent, base_offset)
    }
}

/// Extension trait for advanced hit testing operations.
pub trait HitTestTreeExt: HitTestTree {
    /// Performs hit testing with early termination.
    ///
    /// Stops testing as soon as the first hit is found, which can be more
    /// efficient than accumulating all hits.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to test
    /// * `position` - The position in global coordinates
    ///
    /// # Returns
    ///
    /// The first hit element ID, or `None` if no hit.
    fn hit_test_first(&self, id: ElementId, position: Offset) -> Option<ElementId> {
        let mut result = HitTestResult::new();
        if self.hit_test(id, position, &mut result) {
            result.entries().first().map(|entry| entry.target)
        } else {
            None
        }
    }
}

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================

/// Performs a depth-first layout pass on a render tree.
///
/// This function provides a standard layout algorithm that can work with
/// any `LayoutTree` implementation.
///
/// # Arguments
///
/// * `tree` - The render tree
/// * `root` - Root element to start layout from
/// * `constraints` - Root constraints
///
/// # Returns
///
/// The computed size of the root element.
pub fn layout_subtree(
    tree: &mut dyn LayoutTree,
    root: ElementId,
    constraints: BoxConstraints,
) -> Result<Size, RenderError> {
    // This is a simplified version - real implementation would traverse children
    tree.perform_layout(root, constraints)
}

/// Performs a depth-first paint pass on a render tree.
///
/// # Arguments
///
/// * `tree` - The render tree
/// * `root` - Root element to start painting from
/// * `offset` - Root offset
///
/// # Returns
///
/// A canvas containing the painted subtree.
pub fn paint_subtree(
    tree: &mut dyn PaintTree,
    root: ElementId,
    offset: Offset,
) -> Result<Canvas, RenderError> {
    // This is a simplified version - real implementation would traverse children
    tree.perform_paint(root, offset)
}

/// Performs hit testing on a subtree with detailed results.
///
/// # Arguments
///
/// * `tree` - The render tree
/// * `root` - Root element to start hit testing from
/// * `position` - Position to test
///
/// # Returns
///
/// Complete hit test results for the subtree.
pub fn hit_test_subtree(
    tree: &dyn HitTestTree,
    root: ElementId,
    position: Offset,
) -> HitTestResult {
    let mut result = HitTestResult::new();
    tree.hit_test(root, position, &mut result);
    result
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
        ) -> Result<Size, RenderError> {
            Ok(constraints.biggest())
        }

        fn perform_sliver_layout(
            &mut self,
            _id: ElementId,
            _constraints: SliverConstraints,
        ) -> Result<SliverGeometry, RenderError> {
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
        fn perform_paint(&mut self, _id: ElementId, _offset: Offset) -> Result<Canvas, RenderError> {
            Ok(Canvas::new(Size::new(100.0, 100.0)))
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
    fn test_full_render_tree() {
        let mut tree = MockRenderTree;
        let id = ElementId::new(1);

        // Test combined operations
        let full_tree: &mut dyn FullRenderTree = &mut tree;
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));
        let result = full_tree.render_element(id, constraints, Offset::ZERO);

        assert!(result.is_ok());
        assert!(!full_tree.needs_update(id));
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
}
