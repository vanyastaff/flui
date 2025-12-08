//! Paint tree trait for paint operations.
//!
//! This module provides the [`PaintTree`] trait - a **dyn-compatible** trait for
//! performing paint operations on the render tree.

use std::any::Any;

use flui_foundation::ElementId;
use flui_painting::Canvas;
use flui_types::Offset;

use crate::error::RenderError;

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

    /// Gets the offset of an element (position relative to parent).
    ///
    /// This is used during paint to retrieve offsets that were set during layout.
    ///
    /// # Arguments
    ///
    /// * `id` - The element to query
    ///
    /// # Returns
    ///
    /// The offset if the element exists and has been positioned, `None` otherwise.
    fn get_offset(&self, id: ElementId) -> Option<Offset>;
}

// ============================================================================
// BOX<DYN TRAIT> IMPLEMENTATION
// ============================================================================

impl PaintTree for Box<dyn PaintTree + Send + Sync> {
    fn perform_paint(&mut self, id: ElementId, offset: Offset) -> Result<Canvas, RenderError> {
        (**self).perform_paint(id, offset)
    }

    fn mark_needs_paint(&mut self, id: ElementId) {
        (**self).mark_needs_paint(id)
    }

    fn needs_paint(&self, id: ElementId) -> bool {
        (**self).needs_paint(id)
    }

    fn render_object(&self, id: ElementId) -> Option<&dyn Any> {
        (**self).render_object(id)
    }

    fn render_object_mut(&mut self, id: ElementId) -> Option<&mut dyn Any> {
        (**self).render_object_mut(id)
    }

    fn get_offset(&self, id: ElementId) -> Option<Offset> {
        (**self).get_offset(id)
    }
}

// ============================================================================
// EXTENSION TRAIT
// ============================================================================

/// Extension trait for advanced paint operations.
pub trait PaintTreeExt: PaintTree {
    /// Paints all render children of an element.
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

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================

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
    tree.perform_paint(root, offset)
}

/// Performs batch paint operations on multiple elements.
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
) -> Vec<Result<Canvas, RenderError>> {
    tracing::trace!("Batch paint for {} elements", elements.len());

    elements
        .iter()
        .map(|&(id, offset)| tree.perform_paint(id, offset))
        .collect()
}
