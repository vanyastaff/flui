//! RenderContext - helper for accessing ElementTree during rendering
//!
//! Provides convenient access to the ElementTree and common operations
//! during layout and paint. Most typed operations are in LayoutCx/PaintCx,
//! but RenderContext provides untyped helpers for advanced use cases.

use crate::element::{ElementId, ElementTree};
use crate::render::RenderState;
use parking_lot::{RwLockReadGuard, RwLockWriteGuard};

/// Context for rendering operations
///
/// Provides access to the ElementTree so RenderObjects can query tree structure,
/// access RenderState, and manage ParentData.
///
/// # Design
///
/// Most layout/paint operations use the typed `LayoutCx<Arity>` and `PaintCx<Arity>`.
/// RenderContext is for:
/// - Untyped tree queries
/// - RenderState access
/// - ParentData management
/// - Advanced/internal operations
///
/// # Example
///
/// ```rust,ignore
/// fn layout(&mut self, cx: &mut LayoutCx<MultiArity>) -> Size {
///     // Typed operations use LayoutCx
///     for &child_id in cx.children() {
///         let child_size = cx.layout_child(child_id, constraints);
///     }
///
///     // Advanced operations use RenderContext
///     let render_ctx = RenderContext::new(cx.tree(), cx.element_id());
///     if let Some(parent_data) = render_ctx.parent_data(child_id) {
///         // Access parent data
///     }
/// }
/// ```
pub struct RenderContext<'a> {
    /// Reference to the element tree
    tree: &'a ElementTree,

    /// Current element ID
    element_id: ElementId,
}

impl<'a> RenderContext<'a> {
    /// Create a new render context
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let ctx = RenderContext::new(&tree, element_id);
    /// ```
    #[inline]
    pub fn new(tree: &'a ElementTree, element_id: ElementId) -> Self {
        Self { tree, element_id }
    }

    // ========== Tree Access ==========

    /// Get the ElementTree reference
    #[inline]
    pub fn tree(&self) -> &ElementTree {
        self.tree
    }

    /// Get the current element ID
    #[inline]
    pub fn element_id(&self) -> ElementId {
        self.element_id
    }

    /// Get children of the current element
    ///
    /// Returns a Vec of child ElementIds.
    #[inline]
    pub fn children(&self) -> Vec<ElementId> {
        self.tree.children(self.element_id)
    }

    /// Get child count
    #[inline]
    pub fn child_count(&self) -> usize {
        self.tree.child_count(self.element_id)
    }

    /// Get parent of the current element
    #[inline]
    pub fn parent(&self) -> Option<ElementId> {
        self.tree.parent(self.element_id)
    }

    // ========== RenderState Access ==========

    /// Get read access to RenderState for current element
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(state) = ctx.render_state() {
    ///     if state.needs_layout() {  // Lock-free atomic check
    ///         // Layout needed
    ///     }
    /// }
    /// ```
    #[inline]
    pub fn render_state(&self) -> Option<RwLockReadGuard<RenderState>> {
        self.tree.render_state(self.element_id)
    }

    /// Get write access to RenderState for current element
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(mut state) = ctx.render_state_mut() {
    ///     state.set_size(Size::new(100.0, 50.0));
    ///     state.clear_needs_layout();
    /// }
    /// ```
    #[inline]
    pub fn render_state_mut(&self) -> Option<RwLockWriteGuard<RenderState>> {
        self.tree.render_state_mut(self.element_id)
    }

    /// Get read access to RenderState for a child element
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// for &child_id in ctx.children() {
    ///     if let Some(state) = ctx.child_render_state(child_id) {
    ///         let size = state.get_size();
    ///     }
    /// }
    /// ```
    #[inline]
    pub fn child_render_state(&self, child_id: ElementId) -> Option<RwLockReadGuard<RenderState>> {
        self.tree.render_state(child_id)
    }

    /// Get write access to RenderState for a child element
    #[inline]
    pub fn child_render_state_mut(&self, child_id: ElementId) -> Option<RwLockWriteGuard<RenderState>> {
        self.tree.render_state_mut(child_id)
    }

    // ========== RenderObject Access ==========

    // NOTE: These methods removed due to RefCell guard lifetime issues.
    // Use tree.get(element_id)?.render_object()? directly instead.
    //
    // /// Get the RenderObject for current element
    // pub fn render_object(&self) -> Option<std::cell::Ref<'_, dyn crate::DynRenderObject>> {
    //     self.tree.get(self.element_id)?.render_object()
    // }
    //
    // /// Get the RenderObject for a child element
    // pub fn child_render_object(&self, child_id: ElementId) -> Option<std::cell::Ref<'_, dyn crate::DynRenderObject>> {
    //     self.tree.get(child_id)?.render_object()
    // }

    // ========== Helper Methods ==========

    /// Check if current element exists in the tree
    #[inline]
    pub fn exists(&self) -> bool {
        self.tree.contains(self.element_id)
    }

    /// Check if a child element exists
    #[inline]
    pub fn child_exists(&self, child_id: ElementId) -> bool {
        self.tree.contains(child_id)
    }

    /// Check if current element has children
    #[inline]
    pub fn has_children(&self) -> bool {
        !self.children().is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RenderObject, LeafArity, LayoutCx, PaintCx};
    use flui_types::Size;
    use flui_engine::{BoxedLayer, ContainerLayer};

    #[derive(Debug)]
    struct TestRender;

    impl RenderObject for TestRender {
        type Arity = LeafArity;

        fn layout(&mut self, _cx: &mut LayoutCx<Self::Arity>) -> Size {
            Size::new(10.0, 10.0)
        }

        fn paint(&self, _cx: &PaintCx<Self::Arity>) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }
    }

    #[test]
    #[cfg(disabled)] // TODO: Update for new Element-based ElementTree
    fn test_render_context_new() {
        let tree = ElementTree::new();
        let ctx = RenderContext::new(&tree, 0);

        assert_eq!(ctx.element_id(), 0);
    }

    #[test]
    #[cfg(disabled)] // TODO: Update for new Element-based ElementTree
    fn test_render_context_children() {
        let mut tree = ElementTree::new();
        let parent_id = tree.insert(None, Box::new(TestRender));
        let child_id = tree.insert(Some(parent_id), Box::new(TestRender));

        let ctx = RenderContext::new(&tree, parent_id);

        assert_eq!(ctx.children(), &[child_id]);
        assert_eq!(ctx.child_count(), 1);
        assert!(ctx.has_children());
    }

    #[test]
    #[cfg(disabled)] // TODO: Update for new Element-based ElementTree
    fn test_render_context_parent() {
        let mut tree = ElementTree::new();
        let parent_id = tree.insert(None, Box::new(TestRender));
        let child_id = tree.insert(Some(parent_id), Box::new(TestRender));

        let ctx = RenderContext::new(&tree, child_id);

        assert_eq!(ctx.parent(), Some(parent_id));
    }

    #[test]
    #[cfg(disabled)] // TODO: Update for new Element-based ElementTree
    fn test_render_context_render_state() {
        let mut tree = ElementTree::new();
        let element_id = tree.insert(None, Box::new(TestRender));

        let ctx = RenderContext::new(&tree, element_id);

        // Read access
        {
            let state = ctx.render_state().unwrap();
            assert!(!state.has_size());
        }

        // Write access
        {
            let mut state = ctx.render_state_mut().unwrap();
            state.set_size(Size::new(100.0, 50.0));
        }

        // Verify
        {
            let state = ctx.render_state().unwrap();
            assert!(state.has_size());
            assert_eq!(state.get_size(), Some(Size::new(100.0, 50.0)));
        }
    }

    #[test]
    #[cfg(disabled)] // TODO: Update for new Element-based ElementTree
    fn test_render_context_child_render_state() {
        let mut tree = ElementTree::new();
        let parent_id = tree.insert(None, Box::new(TestRender));
        let child_id = tree.insert(Some(parent_id), Box::new(TestRender));

        let ctx = RenderContext::new(&tree, parent_id);

        // Access child's state
        {
            let mut state = ctx.child_render_state_mut(child_id).unwrap();
            state.set_size(Size::new(50.0, 25.0));
        }

        // Verify
        {
            let state = ctx.child_render_state(child_id).unwrap();
            assert_eq!(state.get_size(), Some(Size::new(50.0, 25.0)));
        }
    }

    #[test]
    #[cfg(disabled)] // TODO: Update for new Element-based ElementTree
    fn test_render_context_render_object() {
        let mut tree = ElementTree::new();
        let element_id = tree.insert(None, Box::new(TestRender));

        let ctx = RenderContext::new(&tree, element_id);

        let render_obj = ctx.render_object().unwrap();
        assert_eq!(render_obj.arity(), Some(0)); // LeafArity
    }

    #[test]
    #[cfg(disabled)] // TODO: Update for new Element-based ElementTree
    fn test_render_context_exists() {
        let mut tree = ElementTree::new();
        let element_id = tree.insert(None, Box::new(TestRender));

        let ctx = RenderContext::new(&tree, element_id);

        assert!(ctx.exists());
        assert!(!ctx.child_exists(999)); // Non-existent child
    }
}
