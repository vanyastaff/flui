//! Hit test tree trait for hit testing operations.
//!
//! This module provides the [`HitTestTree`] trait - a **dyn-compatible** trait for
//! performing hit testing on the render tree.

use std::any::Any;

use flui_foundation::ElementId;
use flui_interaction::HitTestResult;
use flui_types::Offset;

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

    /// Returns an iterator over child element IDs.
    ///
    /// Used by default hit_test_children implementation to iterate over children.
    fn children(&self, id: ElementId) -> Box<dyn Iterator<Item = ElementId> + '_>;

    /// Gets the offset of an element relative to its parent.
    ///
    /// Used by hit testing to transform positions to child coordinates.
    fn get_offset(&self, id: ElementId) -> Option<Offset>;
}

// ============================================================================
// BOX<DYN TRAIT> IMPLEMENTATION
// ============================================================================

impl HitTestTree for Box<dyn HitTestTree + Send + Sync> {
    fn hit_test(&self, id: ElementId, position: Offset, result: &mut HitTestResult) -> bool {
        (**self).hit_test(id, position, result)
    }

    fn render_object(&self, id: ElementId) -> Option<&dyn Any> {
        (**self).render_object(id)
    }

    fn children(&self, id: ElementId) -> Box<dyn Iterator<Item = ElementId> + '_> {
        (**self).children(id)
    }

    fn get_offset(&self, id: ElementId) -> Option<Offset> {
        (**self).get_offset(id)
    }
}

// ============================================================================
// EXTENSION TRAIT
// ============================================================================

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
            result.entries().first().map(|entry| entry.element_id)
        } else {
            None
        }
    }
}

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================

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
