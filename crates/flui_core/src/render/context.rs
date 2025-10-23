//! Render context for accessing children during layout and paint
//!
//! This module solves the fundamental problem: RenderObjects need access to
//! their children's RenderObjects, but children are owned by Elements in the
//! Element tree, not by the parent RenderObject.
//!
//! RenderContext provides a clean, explicit way to access the ElementTree
//! during layout and paint operations.

use crate::{BoxConstraints, ElementId, ElementTree};
use flui_types::{Offset, Size};

/// Context for rendering operations (layout, paint)
///
/// Provides access to the Element tree so RenderObjects can access their children.
/// This solves the architectural problem where RenderObjects need to layout/paint
/// children, but children are owned by Elements, not by the parent RenderObject.
///
/// # Architecture
///
/// ```text
/// RenderFlex::layout(constraints, ctx) {
///     for child_id in ctx.children() {
///         let size = ctx.layout_child(child_id, child_constraints);
///     }
/// }
/// ```
///
/// # Examples
///
/// ```rust,ignore
/// impl DynRenderObject for RenderFlex {
///     fn layout(&mut self, constraints: BoxConstraints, ctx: &RenderContext) -> Size {
///         let mut total_height = 0.0;
///
///         // Access children through context
///         for &child_id in ctx.children() {
///             let child_size = ctx.layout_child(child_id, child_constraints);
///             total_height += child_size.height;
///         }
///
///         Size::new(constraints.max_width, total_height)
///     }
/// }
/// ```
pub struct RenderContext<'a> {
    /// Reference to the element tree
    pub(crate) tree: &'a ElementTree,

    /// Current element index (Slab index)
    pub(crate) element_id: ElementId,
}

impl<'a> RenderContext<'a> {
    /// Create a new render context
    ///
    /// # Parameters
    ///
    /// - `tree`: Reference to the element tree
    /// - `element_id`: Current element's Slab index
    #[inline]
    pub fn new(tree: &'a ElementTree, element_id: ElementId) -> Self {
        Self { tree, element_id }
    }

    /// Get children of the current element
    ///
    /// Returns Slab indices of child elements.
    ///
    /// # Returns
    ///
    /// Slice of child element IDs (Slab indices)
    #[inline]
    pub fn children(&self) -> &[ElementId] {
        self.tree.children(self.element_id)
    }

    /// Get the current element ID
    #[inline]
    pub fn element_id(&self) -> ElementId {
        self.element_id
    }

    /// Layout a child element
    ///
    /// Finds the child's RenderObject and calls its layout method recursively.
    ///
    /// # Parameters
    ///
    /// - `child_id`: Child element's Slab index
    /// - `constraints`: Layout constraints for the child
    ///
    /// # Returns
    ///
    /// Size chosen by the child, or Size::ZERO if no RenderObject found
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// for &child_id in ctx.children() {
    ///     let child_size = ctx.layout_child(child_id, child_constraints);
    ///     total_height += child_size.height;
    /// }
    /// ```
    pub fn layout_child(&self, child_id: ElementId, constraints: BoxConstraints) -> Size {
        tracing::trace!("RenderContext::layout_child({}) called", child_id);

        // Get child element
        let child_elem = match self.tree.get(child_id) {
            Some(elem) => elem,
            None => {
                tracing::warn!("layout_child: child {} not found", child_id);
                return Size::ZERO;
            }
        };

        // Get child's RenderObject
        let child_ro = match child_elem.render_object() {
            Some(ro) => ro,
            None => {
                tracing::trace!("layout_child: child {} has no RenderObject, recursing", child_id);
                // Child has no RenderObject - walk down to find one
                return self.layout_child_recursive(child_id, constraints);
            }
        };

        // Create context for child
        let child_ctx = RenderContext::new(self.tree, child_id);

        // Get child's RenderState (ensure it exists)
        self.tree.ensure_render_state(child_id);
        let mut child_state = self.tree.render_state_mut(child_id)
            .expect("Child render_state should exist after ensure_render_state");

        // Layout child (passing state explicitly via &mut *RefMut)
        let size = child_ro.layout(&mut *child_state, constraints, &child_ctx);
        tracing::debug!("layout_child({}): size = {:?}", child_id, size);
        size
    }

    /// Recursively find and layout first descendant with RenderObject
    fn layout_child_recursive(&self, start_id: ElementId, constraints: BoxConstraints) -> Size {
        // Get grandchildren
        let grandchildren = self.tree.children(start_id);

        for &grandchild_id in grandchildren {
            if let Some(grandchild_elem) = self.tree.get(grandchild_id) {
                if let Some(grandchild_ro) = grandchild_elem.render_object() {
                    // Layout grandchild (passing state explicitly)
                    let grandchild_ctx = RenderContext::new(self.tree, grandchild_id);
                    self.tree.ensure_render_state(grandchild_id);
                    let mut grandchild_state = self.tree.render_state_mut(grandchild_id)
                        .expect("Grandchild render_state should exist after ensure_render_state");
                    let size = grandchild_ro.layout(&mut *grandchild_state, constraints, &grandchild_ctx);
                    return size;
                } else {
                    // Continue searching deeper
                    let size = self.layout_child_recursive(grandchild_id, constraints);
                    if size != Size::ZERO {
                        return size;
                    }
                }
            }
        }

        Size::ZERO
    }

    /// Paint a child element
    ///
    /// Finds the child's RenderObject and calls its paint method recursively.
    ///
    /// # Parameters
    ///
    /// - `child_id`: Child element's Slab index
    /// - `painter`: egui Painter for drawing
    /// - `offset`: Position relative to parent
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// for &child_id in ctx.children() {
    ///     ctx.paint_child(child_id, painter, child_offset);
    ///     child_offset.y += child_height;
    /// }
    /// ```
    pub fn paint_child(&self, child_id: ElementId, painter: &egui::Painter, offset: Offset) {
        // Get child element
        let child_elem = match self.tree.get(child_id) {
            Some(elem) => elem,
            None => {
                tracing::warn!("paint_child: child {} not found", child_id);
                return;
            }
        };

        // Get child's RenderObject
        let child_ro = match child_elem.render_object() {
            Some(ro) => ro,
            None => {
                // Child has no RenderObject - walk down to find one
                self.paint_child_recursive(child_id, painter, offset);
                return;
            }
        };

        // Create context for child
        let child_ctx = RenderContext::new(self.tree, child_id);

        // Get child's RenderState (should already exist from layout)
        if let Some(child_state) = self.tree.render_state(child_id) {
            // Paint child (passing state explicitly via &*Ref)
            child_ro.paint(&*child_state, painter, offset, &child_ctx);
        } else {
            tracing::warn!("paint_child: child {} has no render_state (layout not called?)", child_id);
        }
    }

    /// Recursively find and paint first descendant with RenderObject
    fn paint_child_recursive(&self, start_id: ElementId, painter: &egui::Painter, offset: Offset) {
        // Get grandchildren
        for &grandchild_id in self.tree.children(start_id) {
            if let Some(grandchild_elem) = self.tree.get(grandchild_id) {
                if let Some(grandchild_ro) = grandchild_elem.render_object() {
                    let grandchild_ctx = RenderContext::new(self.tree, grandchild_id);
                    if let Some(grandchild_state) = self.tree.render_state(grandchild_id) {
                        grandchild_ro.paint(&*grandchild_state, painter, offset, &grandchild_ctx);
                    } else {
                        tracing::warn!("paint_child_recursive: grandchild {} has no render_state", grandchild_id);
                    }
                    return;
                } else {
                    // Continue searching deeper
                    self.paint_child_recursive(grandchild_id, painter, offset);
                }
            }
        }
    }

    /// Get the ElementTree reference
    ///
    /// For advanced use cases where direct tree access is needed.
    #[inline]
    pub fn tree(&self) -> &ElementTree {
        self.tree
    }

    /// Get size of child element (traversing to find RenderObject if needed)
    ///
    /// If the child doesn't have a RenderObject, traverses descendants to find one.
    ///
    /// # Parameters
    ///
    /// - `child_id`: Child element's Slab index
    ///
    /// # Returns
    ///
    /// Size of the child's RenderObject, or Size::ZERO if none found
    pub fn child_size(&self, child_id: ElementId) -> Size {
        // Try to get size from render_state directly (state is stored in ElementTree)
        if let Some(state) = self.tree.render_state(child_id) {
            if let Some(size) = *state.size.lock() {
                return size;
            }
        }

        // If child doesn't have render_state, search descendants
        self.child_size_recursive(child_id)
    }

    /// Recursively find size of first descendant with RenderObject
    fn child_size_recursive(&self, start_id: ElementId) -> Size {
        for &grandchild_id in self.tree.children(start_id) {
            // Try to get size from render_state
            if let Some(state) = self.tree.render_state(grandchild_id) {
                if let Some(size) = *state.size.lock() {
                    return size;
                }
            }

            // Continue searching deeper
            let size = self.child_size_recursive(grandchild_id);
            if size != Size::ZERO {
                return size;
            }
        }

        Size::ZERO
    }

    // ========== RenderState Access ==========
    //
    // These methods provide convenient access to RenderState stored in ElementTree.
    // Since RenderState uses interior mutability (Mutex), we can modify it through &self.

    /// Get reference to the RenderState for current element
    ///
    /// Returns None if element doesn't have a RenderObject.
    ///
    /// # Panics
    ///
    /// Panics if the current element doesn't exist in the tree.
    // Note: state() method removed - state is now passed explicitly to layout/paint

    /// Set size for current element's RenderObject
    ///
    /// Updates the size in the ElementTree's render_state.
    #[inline]
    pub fn set_size(&self, size: Size) {
        if let Some(state) = self.tree.render_state(self.element_id) {
            *state.size.lock() = Some(size);
        }
    }

    /// Get size of current element's RenderObject
    ///
    /// Returns Size::ZERO if not laid out yet.
    #[inline]
    pub fn get_size(&self) -> Size {
        self.tree.render_state(self.element_id)
            .and_then(|state| *state.size.lock())
            .unwrap_or(Size::ZERO)
    }

    /// Set constraints for current element's RenderObject
    #[inline]
    pub fn set_constraints(&self, constraints: BoxConstraints) {
        if let Some(state) = self.tree.render_state(self.element_id) {
            *state.constraints.lock() = Some(constraints);
        }
    }

    /// Get constraints of current element's RenderObject
    #[inline]
    pub fn get_constraints(&self) -> Option<BoxConstraints> {
        self.tree.render_state(self.element_id)
            .and_then(|state| *state.constraints.lock())
    }

    /// Mark current element's RenderObject as needing layout
    #[inline]
    pub fn mark_needs_layout(&self) {
        if let Some(state) = self.tree.render_state(self.element_id) {
            state.mark_needs_layout();
        }
    }

    /// Mark current element's RenderObject as needing paint
    #[inline]
    pub fn mark_needs_paint(&self) {
        if let Some(state) = self.tree.render_state(self.element_id) {
            state.mark_needs_paint();
        }
    }

    /// Clear needs_layout flag for current element
    #[inline]
    pub fn clear_needs_layout(&self) {
        if let Some(state) = self.tree.render_state(self.element_id) {
            state.clear_needs_layout();
        }
    }

    /// Clear needs_paint flag for current element
    #[inline]
    pub fn clear_needs_paint(&self) {
        if let Some(state) = self.tree.render_state(self.element_id) {
            state.clear_needs_paint();
        }
    }

    /// Check if current element's RenderObject needs layout
    #[inline]
    pub fn needs_layout(&self) -> bool {
        self.tree.render_state(self.element_id)
            .map(|state| state.needs_layout())
            .unwrap_or(false)
    }

    /// Check if current element's RenderObject needs paint
    #[inline]
    pub fn needs_paint(&self) -> bool {
        self.tree.render_state(self.element_id)
            .map(|state| state.needs_paint())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_context_new() {
        let tree = ElementTree::new();
        let ctx = RenderContext::new(&tree, 0);

        assert_eq!(ctx.element_id(), 0);
        assert_eq!(ctx.children().len(), 0);
    }

    #[test]
    fn test_render_context_children() {
        let mut tree = ElementTree::new();
        // Would need proper setup with widgets/elements to test fully
        let ctx = RenderContext::new(&tree, 0);

        assert_eq!(ctx.children(), &[]);
    }
}
