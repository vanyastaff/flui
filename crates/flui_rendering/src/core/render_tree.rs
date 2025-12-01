//! Render tree traits for layout and paint operations.
//!
//! These traits extend the basic tree traits from `flui-tree` with
//! render-specific functionality that requires concrete types.
//!
//! # Trait Hierarchy
//!
//! ```text
//! flui-tree (type-erased, abstract patterns):
//!     TreeNav
//!         └── RenderTreeAccess (dyn Any access to render objects)
//!             └── RenderTreeExt (iterator-based access)
//!             └── DirtyTracking (needs_layout/needs_paint)
//!                 └── DirtyTrackingExt (batch operations)
//!
//! flui-tree pipeline traits (abstract):
//!     LayoutVisitable, PaintVisitable, HitTestVisitable
//!
//! flui-rendering (this module, concrete types):
//!     RenderTreeAccess + RenderState
//!         ├── LayoutTree (Size, BoxConstraints, SliverConstraints)
//!         │   └── LayoutTreeExt (iterator-based layout)
//!         ├── PaintTree (Canvas, Offset)
//!         │   └── PaintTreeExt (iterator-based paint)
//!         └── HitTestTree (HitTestResult)
//!             └── HitTestTreeExt (iterator-based hit test)
//! ```
//!
//! # Re-exports from flui-tree
//!
//! This module re-exports the base traits from `flui-tree`:
//! - [`RenderTreeAccess`] - Type-erased access via `dyn Any`
//! - [`RenderTreeAccessExt`] - Typed access via downcasting
//! - [`RenderTreeExt`] - Iterator-based render tree operations
//! - [`DirtyTracking`] - Layout/paint dirty flag management
//! - [`DirtyTrackingExt`] - Extended dirty tracking operations
//! - [`AtomicDirtyFlags`] - Lock-free atomic dirty flags
//!
//! # Usage with flui-tree iterators
//!
//! ```rust,ignore
//! use flui_tree::{RenderChildren, RenderDescendants, find_render_ancestor};
//! use flui_rendering::{LayoutTree, LayoutTreeExt};
//!
//! fn layout_all_children(tree: &mut impl LayoutTree, parent: ElementId) {
//!     // Use the extension trait for iterator-based layout
//!     let sizes = tree.layout_render_children(parent, constraints);
//! }
//! ```

use flui_foundation::ElementId;
use flui_interaction::HitTestResult;
use flui_painting::Canvas;
use flui_types::{Offset, Size, SliverConstraints, SliverGeometry};

use crate::core::{BoxConstraints, Geometry, RenderState};
use crate::error::RenderError;

// Re-export base traits from flui-tree
pub use flui_tree::{
    // Utility functions
    collect_render_children,
    count_render_children,
    find_render_ancestor,
    find_render_root,
    first_render_child,
    has_render_children,
    is_render_descendant,
    is_render_leaf,
    render_depth,
    render_parent,
    // Dirty tracking
    AtomicDirtyFlags,
    DirtyTracking,
    DirtyTrackingExt,
    // Iterators (commonly used with render trees)
    RenderAncestors,
    RenderChildren,
    RenderDescendants,
    RenderLeaves,
    RenderPath,
    RenderSubtree,
    // Render access
    RenderTreeAccess,
    RenderTreeAccessExt,
    RenderTreeExt,
};

// ============================================================================
// LAYOUT TREE TRAIT
// ============================================================================

/// Layout operations on the render tree.
///
/// This trait extends [`DirtyTracking`] with concrete layout operations
/// that use FLUI's specific types (`Size`, `BoxConstraints`, etc.).
///
/// # Implementation Note
///
/// Implementors must also implement [`RenderTreeAccess`] from `flui-tree`.
/// The typed access methods use [`RenderTreeAccessExt`] for downcasting.
///
/// # Example
///
/// ```rust,ignore
/// impl LayoutTree for MyTree {
///     fn perform_layout(&mut self, id: ElementId, constraints: BoxConstraints) -> Result<Size, RenderError> {
///         // Get render object
///         let render = self.render_object_typed::<dyn RenderObject>(id)?;
///
///         // Perform layout
///         render.layout(constraints, self)
///     }
///     // ...
/// }
/// ```
pub trait LayoutTree: DirtyTracking + RenderTreeAccessExt {
    /// Perform layout on an element with box constraints.
    ///
    /// Returns the computed size.
    fn perform_layout(
        &mut self,
        id: ElementId,
        constraints: BoxConstraints,
    ) -> Result<Size, RenderError>;

    /// Perform layout on an element with sliver constraints.
    ///
    /// Returns the computed sliver geometry.
    fn perform_sliver_layout(
        &mut self,
        id: ElementId,
        constraints: SliverConstraints,
    ) -> Result<SliverGeometry, RenderError>;

    /// Layout a child element (called from RenderObject::layout).
    ///
    /// This is the method render objects use to layout their children.
    fn layout_child(
        &mut self,
        child: ElementId,
        constraints: BoxConstraints,
    ) -> Result<Size, RenderError> {
        self.perform_layout(child, constraints)
    }

    /// Layout a sliver child element.
    fn layout_sliver_child(
        &mut self,
        child: ElementId,
        constraints: SliverConstraints,
    ) -> Result<SliverGeometry, RenderError> {
        self.perform_sliver_layout(child, constraints)
    }

    /// Get the cached size of an element.
    ///
    /// Uses [`RenderTreeAccessExt::render_state_typed`] to downcast to [`RenderState`].
    fn get_size(&self, id: ElementId) -> Option<Size> {
        self.render_state_typed::<RenderState>(id)?
            .geometry()
            .and_then(|g| g.try_as_box())
    }

    /// Get the cached geometry of an element.
    ///
    /// Uses [`RenderTreeAccessExt::render_state_typed`] to downcast to [`RenderState`].
    fn get_geometry(&self, id: ElementId) -> Option<Geometry> {
        self.render_state_typed::<RenderState>(id)?.geometry()
    }

    /// Set the offset of an element (position relative to parent).
    fn set_offset(&mut self, id: ElementId, offset: Offset);

    /// Get the offset of an element.
    fn get_offset(&self, id: ElementId) -> Option<Offset>;
}

/// Extension trait for iterator-based layout operations.
///
/// This trait provides convenience methods that use flui-tree iterators
/// for layout operations on multiple elements.
pub trait LayoutTreeExt: LayoutTree {
    /// Layout all render children of an element with the same constraints.
    ///
    /// Returns a vector of (child_id, size) pairs.
    ///
    /// # Note
    ///
    /// Children are collected first to avoid borrow conflicts during layout.
    fn layout_render_children(
        &mut self,
        parent: ElementId,
        constraints: BoxConstraints,
    ) -> Vec<(ElementId, Result<Size, RenderError>)> {
        let children: Vec<_> = RenderChildren::new(self, parent).collect();
        children
            .into_iter()
            .map(|child| (child, self.perform_layout(child, constraints)))
            .collect()
    }

    /// Layout all render children with individual constraints from a closure.
    ///
    /// The closure receives (index, child_id) and returns constraints for that child.
    fn layout_render_children_with<F>(
        &mut self,
        parent: ElementId,
        mut constraints_fn: F,
    ) -> Vec<(ElementId, Result<Size, RenderError>)>
    where
        F: FnMut(usize, ElementId) -> BoxConstraints,
    {
        let children: Vec<_> = RenderChildren::new(self, parent).collect();
        children
            .into_iter()
            .enumerate()
            .map(|(idx, child)| {
                let constraints = constraints_fn(idx, child);
                (child, self.perform_layout(child, constraints))
            })
            .collect()
    }

    /// Layout a single render child (for SingleRender elements).
    ///
    /// Returns None if no render child exists.
    fn layout_single_child(
        &mut self,
        parent: ElementId,
        constraints: BoxConstraints,
    ) -> Option<(ElementId, Result<Size, RenderError>)> {
        let child = first_render_child(self, parent)?;
        Some((child, self.perform_layout(child, constraints)))
    }

    /// Count render children of an element.
    #[inline]
    fn render_child_count(&self, parent: ElementId) -> usize {
        count_render_children(self, parent)
    }

    /// Check if element has any render children.
    #[inline]
    fn has_render_children(&self, parent: ElementId) -> bool {
        has_render_children(self, parent)
    }
}

// Blanket implementation
impl<T: LayoutTree> LayoutTreeExt for T {}

// ============================================================================
// PAINT TREE TRAIT
// ============================================================================

/// Paint operations on the render tree.
///
/// This trait provides methods for painting render elements to a canvas.
pub trait PaintTree: RenderTreeAccess {
    /// Perform paint on an element.
    ///
    /// Returns the canvas with all drawing operations.
    fn perform_paint(&mut self, id: ElementId, offset: Offset) -> Result<Canvas, RenderError>;

    /// Paint a child element (called from RenderObject::paint).
    ///
    /// This appends the child's canvas to the parent's canvas.
    fn paint_child(&mut self, child: ElementId, offset: Offset) -> Result<Canvas, RenderError> {
        self.perform_paint(child, offset)
    }
}

/// Extension trait for iterator-based paint operations.
pub trait PaintTreeExt: PaintTree {
    /// Paint all render children of an element.
    ///
    /// Returns a vector of (child_id, canvas) pairs.
    fn paint_render_children(
        &mut self,
        parent: ElementId,
        base_offset: Offset,
    ) -> Vec<(ElementId, Result<Canvas, RenderError>)> {
        let children: Vec<_> = RenderChildren::new(self, parent).collect();
        children
            .into_iter()
            .map(|child| (child, self.perform_paint(child, base_offset)))
            .collect()
    }

    /// Paint all render children with individual offsets from a closure.
    fn paint_render_children_with<F>(
        &mut self,
        parent: ElementId,
        mut offset_fn: F,
    ) -> Vec<(ElementId, Result<Canvas, RenderError>)>
    where
        F: FnMut(usize, ElementId) -> Offset,
    {
        let children: Vec<_> = RenderChildren::new(self, parent).collect();
        children
            .into_iter()
            .enumerate()
            .map(|(idx, child)| {
                let offset = offset_fn(idx, child);
                (child, self.perform_paint(child, offset))
            })
            .collect()
    }

    /// Paint a single render child.
    fn paint_single_child(
        &mut self,
        parent: ElementId,
        offset: Offset,
    ) -> Option<(ElementId, Result<Canvas, RenderError>)> {
        let child = first_render_child(self, parent)?;
        Some((child, self.perform_paint(child, offset)))
    }
}

// Blanket implementation
impl<T: PaintTree> PaintTreeExt for T {}

// ============================================================================
// HIT TEST TREE TRAIT
// ============================================================================

/// Hit testing operations on the render tree.
///
/// This trait provides methods for determining which render element
/// is at a given position.
pub trait HitTestTree: RenderTreeAccess {
    /// Perform hit test on an element.
    ///
    /// Returns true if the element or any child was hit.
    fn hit_test(&self, id: ElementId, position: Offset, result: &mut HitTestResult) -> bool;

    /// Hit test a child element.
    fn hit_test_child(
        &self,
        child: ElementId,
        position: Offset,
        result: &mut HitTestResult,
    ) -> bool {
        self.hit_test(child, position, result)
    }
}

/// Extension trait for iterator-based hit test operations.
pub trait HitTestTreeExt: HitTestTree {
    /// Hit test all render children of an element (in reverse z-order).
    ///
    /// Returns the first child that was hit, if any.
    fn hit_test_render_children(
        &self,
        parent: ElementId,
        position: Offset,
        result: &mut HitTestResult,
    ) -> Option<ElementId> {
        // Collect children and iterate in reverse for proper z-ordering
        let children: Vec<_> = RenderChildren::new(self, parent).collect();
        for child in children.into_iter().rev() {
            if self.hit_test(child, position, result) {
                return Some(child);
            }
        }
        None
    }

    /// Hit test all render children with position transform.
    ///
    /// The transform function adjusts the position for each child
    /// (e.g., subtracting the child's offset).
    fn hit_test_render_children_with<F>(
        &self,
        parent: ElementId,
        position: Offset,
        result: &mut HitTestResult,
        mut transform_fn: F,
    ) -> Option<ElementId>
    where
        F: FnMut(ElementId, Offset) -> Offset,
    {
        let children: Vec<_> = RenderChildren::new(self, parent).collect();
        for child in children.into_iter().rev() {
            let child_position = transform_fn(child, position);
            if self.hit_test(child, child_position, result) {
                return Some(child);
            }
        }
        None
    }

    /// Hit test a single render child.
    fn hit_test_single_child(
        &self,
        parent: ElementId,
        position: Offset,
        result: &mut HitTestResult,
    ) -> bool {
        if let Some(child) = first_render_child(self, parent) {
            self.hit_test(child, position, result)
        } else {
            false
        }
    }

    /// Find the render path to the hit element.
    ///
    /// Returns the path from root to the deepest hit element.
    fn hit_test_path(
        &self,
        root: ElementId,
        position: Offset,
        result: &mut HitTestResult,
    ) -> Vec<ElementId> {
        let mut path = Vec::new();
        self.hit_test_path_recursive(root, position, result, &mut path);
        path
    }

    /// Helper for recursive path building.
    #[doc(hidden)]
    fn hit_test_path_recursive(
        &self,
        id: ElementId,
        position: Offset,
        result: &mut HitTestResult,
        path: &mut Vec<ElementId>,
    ) -> bool {
        if !self.hit_test(id, position, result) {
            return false;
        }

        path.push(id);

        // Check children in reverse order
        let children: Vec<_> = RenderChildren::new(self, id).collect();
        for child in children.into_iter().rev() {
            if self.hit_test_path_recursive(child, position, result, path) {
                return true;
            }
        }

        true
    }
}

// Blanket implementation
impl<T: HitTestTree> HitTestTreeExt for T {}

// ============================================================================
// COMBINED TRAIT
// ============================================================================

/// Combined trait for full render tree functionality.
///
/// This is a convenience trait that combines all render tree operations.
/// Use this when you need layout, paint, and hit test capabilities.
pub trait FullRenderTree: LayoutTree + PaintTree + HitTestTree {}

// Blanket implementation
impl<T> FullRenderTree for T where T: LayoutTree + PaintTree + HitTestTree {}

/// Extension trait combining all render tree extensions.
pub trait FullRenderTreeExt: LayoutTreeExt + PaintTreeExt + HitTestTreeExt {}

// Blanket implementation
impl<T> FullRenderTreeExt for T where T: LayoutTreeExt + PaintTreeExt + HitTestTreeExt {}

// ============================================================================
// RENDER TREE UTILITIES
// ============================================================================

/// Find the nearest layout boundary ancestor.
///
/// A layout boundary is an element where layout changes don't propagate
/// to ancestors (e.g., elements with tight constraints).
pub fn find_layout_boundary<T: LayoutTree>(tree: &T, id: ElementId) -> Option<ElementId> {
    for ancestor in RenderAncestors::new(tree, id) {
        if let Some(state) = tree.render_state_typed::<RenderState>(ancestor) {
            if state.is_relayout_boundary() {
                return Some(ancestor);
            }
        }
    }
    None
}

/// Find the nearest repaint boundary ancestor.
///
/// A repaint boundary is an element that creates a separate layer,
/// isolating paint operations.
pub fn find_repaint_boundary<T: PaintTree>(tree: &T, id: ElementId) -> Option<ElementId> {
    use flui_tree::RenderAncestors;

    for ancestor in RenderAncestors::new(tree, id) {
        if let Some(state) = tree.render_state(ancestor) {
            if let Some(render_state) = state.downcast_ref::<RenderState>() {
                if render_state.is_repaint_boundary() {
                    return Some(ancestor);
                }
            }
        }
    }
    None
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Re-export tests from flui-tree to ensure compatibility
    #[test]
    fn test_trait_bounds() {
        // Ensure our traits have the expected bounds
        fn assert_layout_tree<T: LayoutTree>() {}
        fn assert_paint_tree<T: PaintTree>() {}
        fn assert_hit_test_tree<T: HitTestTree>() {}
        fn assert_full_render_tree<T: FullRenderTree>() {}

        // These are compile-time checks
    }
}
