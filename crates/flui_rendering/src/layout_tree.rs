//! Layout tree trait for layout operations.
//!
//! This module provides the [`LayoutTree`] trait - a **dyn-compatible** trait for
//! performing layout computations on the render tree.

use std::any::Any;

use flui_foundation::ElementId;
use flui_types::{BoxConstraints, Offset, Size, SliverConstraints, SliverGeometry};

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

    /// Sets up ParentData for a child element.
    ///
    /// Called when a child is added to a parent. The parent's render object
    /// creates appropriate ParentData and attaches it to the child.
    ///
    /// # Arguments
    ///
    /// * `parent_id` - The parent element
    /// * `child_id` - The child element to set up
    fn setup_child_parent_data(&mut self, parent_id: ElementId, child_id: ElementId);
}

// ============================================================================
// BOX<DYN TRAIT> IMPLEMENTATION
// ============================================================================

impl LayoutTree for Box<dyn LayoutTree + Send + Sync> {
    fn perform_layout(
        &mut self,
        id: ElementId,
        constraints: BoxConstraints,
    ) -> Result<Size, RenderError> {
        (**self).perform_layout(id, constraints)
    }

    fn perform_sliver_layout(
        &mut self,
        id: ElementId,
        constraints: SliverConstraints,
    ) -> Result<SliverGeometry, RenderError> {
        (**self).perform_sliver_layout(id, constraints)
    }

    fn set_offset(&mut self, id: ElementId, offset: Offset) {
        (**self).set_offset(id, offset)
    }

    fn get_offset(&self, id: ElementId) -> Option<Offset> {
        (**self).get_offset(id)
    }

    fn mark_needs_layout(&mut self, id: ElementId) {
        (**self).mark_needs_layout(id)
    }

    fn needs_layout(&self, id: ElementId) -> bool {
        (**self).needs_layout(id)
    }

    fn render_object(&self, id: ElementId) -> Option<&dyn Any> {
        (**self).render_object(id)
    }

    fn render_object_mut(&mut self, id: ElementId) -> Option<&mut dyn Any> {
        (**self).render_object_mut(id)
    }

    fn setup_child_parent_data(&mut self, parent_id: ElementId, child_id: ElementId) {
        (**self).setup_child_parent_data(parent_id, child_id)
    }
}

// ============================================================================
// EXTENSION TRAIT
// ============================================================================

/// Extension trait for advanced layout operations.
///
/// This trait provides additional layout operations that require more than
/// the basic `LayoutTree` interface.
pub trait LayoutTreeExt: LayoutTree {
    /// Layouts all render children of an element with the same constraints.
    ///
    /// # Arguments
    ///
    /// * `parent` - The parent element
    /// * `constraints` - Constraints to apply to all children
    ///
    /// # Returns
    ///
    /// Vector of (child_id, computed_size) pairs.
    fn layout_render_children(
        &mut self,
        _parent: ElementId,
        _constraints: BoxConstraints,
    ) -> Vec<(ElementId, Size)> {
        // Default implementation - override in concrete types for efficiency
        Vec::new()
    }

    /// Computes the total size of all children given constraints.
    fn total_children_size(&mut self, parent: ElementId, constraints: BoxConstraints) -> Size {
        let children_sizes = self.layout_render_children(parent, constraints);
        children_sizes.iter().fold(Size::ZERO, |acc, (_, size)| {
            Size::new(acc.width + size.width, acc.height.max(size.height))
        })
    }
}

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================

/// Performs a depth-first layout pass on a render tree.
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
    tree.perform_layout(root, constraints)
}

/// Performs batch layout operations on multiple elements.
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
) -> Vec<Result<Size, RenderError>> {
    tracing::trace!("Batch layout for {} elements", elements.len());

    elements
        .iter()
        .map(|&(id, constraints)| tree.perform_layout(id, constraints))
        .collect()
}
