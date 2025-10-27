//! ElementTree - Slab-based tree for managing Element instances
//!
//! Provides efficient O(1) access to elements via slab allocation.

use slab::Slab;
use flui_types::constraints::BoxConstraints;

use crate::element::{Element, ElementId};
use crate::render::RenderState;

/// Element tree managing Element instances with efficient slab allocation
///
/// # New Architecture
///
/// ElementTree now stores heterogeneous Elements (ComponentElement, StatefulElement,
/// RenderObjectElement) instead of RenderObjects directly. This provides:
/// - Unified tree structure for all element types
/// - Widget lifecycle management (build, rebuild, mount, unmount)
/// - State management for StatefulElements
/// - RenderState is now inside RenderObjectElement
///
/// # Memory Layout
///
/// ```text
/// ElementTree {
///     nodes: Slab<ElementNode>  ← Contiguous memory for cache efficiency
/// }
///
/// ElementNode {
///     element: Element  ← Enum-based heterogeneous storage (3-4x faster!)
///         ├─ Element::Component(ComponentElement)
///         ├─ Element::Stateful(StatefulElement)
///         ├─ Element::Inherited(InheritedElement)
///         ├─ Element::Render(RenderElement)
///         └─ Element::ParentData(ParentDataElement)
/// }
/// ```
///
/// # Usage
///
/// ```rust,ignore
/// use flui_core::{ElementTree, RenderObjectElement};
///
/// let mut tree = ElementTree::new();
///
/// // Insert root element (now stores Element, not RenderObject)
/// let root_element = RenderObjectElement::new(FlexWidget::column());
/// let root_id = tree.insert(Box::new(root_element));
///
/// // Access element
/// let element = tree.get(root_id).unwrap();
/// ```
#[derive(Debug)]
pub struct ElementTree {
    /// Slab-based arena for element nodes
    ///
    /// Each ElementNode contains:
    /// - RenderObject (boxed trait object)
    /// - RenderState (size, constraints, flags)
    /// - Parent/children relationships
    pub(super) nodes: Slab<ElementNode>,
}

/// Internal node in the element tree
///
/// Contains an Element enum variant (Component, Stateful, Inherited, Render, ParentData).
/// The Element enum contains all necessary data including:
/// - Widget configuration
/// - State (for StatefulElement)
/// - RenderObject + RenderState (for RenderElement)
/// - Lifecycle state
/// - Children management
#[derive(Debug)]
pub(super) struct ElementNode {
    /// The Element for this node
    ///
    /// Stored as `Element` enum for heterogeneous storage with compile-time dispatch.
    /// This is 3-4x faster than Box<dyn> thanks to match-based dispatch.
    pub(super) element: Element,
}

impl ElementTree {
    /// Create a new empty element tree
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let tree = ElementTree::new();
    /// ```
    pub fn new() -> Self {
        Self {
            nodes: Slab::new(),
        }
    }

    /// Create an element tree with pre-allocated capacity
    ///
    /// # Arguments
    ///
    /// - `capacity`: Initial capacity for the slab
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Pre-allocate for 1000 elements
    /// let tree = ElementTree::with_capacity(1000);
    /// ```
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: Slab::with_capacity(capacity),
        }
    }

    // ========== Element Insertion/Removal ==========

    /// Insert a new element into the tree
    ///
    /// # Arguments
    ///
    /// - `element`: The Element enum (Component, Stateful, Inherited, Render, or ParentData)
    ///
    /// # Returns
    ///
    /// The ElementId for the newly inserted element
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_core::{Element, RenderElement, FlexWidget};
    ///
    /// let render_elem = RenderElement::new(FlexWidget::column());
    /// let root_id = tree.insert(Element::Render(render_elem));
    /// ```
    pub fn insert(&mut self, element: Element) -> ElementId {
        // Create the node
        let node = ElementNode { element };

        // Insert into slab and get ID
        let element_id = self.nodes.insert(node);

        element_id
    }

    /// Remove an element and all its descendants from the tree
    ///
    /// # Arguments
    ///
    /// - `element_id`: The element to remove
    ///
    /// # Returns
    ///
    /// `true` if the element was removed, `false` if it didn't exist
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// tree.remove(element_id);
    /// ```
    pub fn remove(&mut self, element_id: ElementId) -> bool {
        // Get element to call unmount
        if let Some(node) = self.nodes.get_mut(element_id) {
            // Call unmount lifecycle
            node.element.unmount();
        }

        // Get children from element (before removing)
        let children: Vec<ElementId> = if let Some(node) = self.nodes.get(element_id) {
            node.element.children().collect()
        } else {
            Vec::new()
        };

        // Remove all children recursively
        for child_id in children {
            self.remove(child_id);
        }

        // Remove from parent's children list
        if let Some(parent_id) = self.get(element_id).and_then(|e| e.parent()) {
            if let Some(parent_node) = self.nodes.get_mut(parent_id) {
                parent_node.element.forget_child(element_id);
            }
        }

        // Remove the node itself
        self.nodes.try_remove(element_id).is_some()
    }

    // ========== Tree Traversal ==========

    /// Check if an element exists in the tree
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if tree.contains(element_id) {
    ///     // Element exists
    /// }
    /// ```
    #[inline]
    pub fn contains(&self, element_id: ElementId) -> bool {
        self.nodes.contains(element_id)
    }

    /// Get a reference to an element
    ///
    /// # Returns
    ///
    /// `Some(&Element)` if the element exists, `None` otherwise
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(element) = tree.get(element_id) {
    ///     println!("Element has {} children", element.children().count());
    /// }
    /// ```
    #[inline]
    pub fn get(&self, element_id: ElementId) -> Option<&Element> {
        self.nodes.get(element_id).map(|node| &node.element)
    }

    /// Get a mutable reference to an element
    ///
    /// # Returns
    ///
    /// `Some(&mut Element)` if the element exists, `None` otherwise
    #[inline]
    pub fn get_mut(&mut self, element_id: ElementId) -> Option<&mut Element> {
        self.nodes.get_mut(element_id).map(|node| &mut node.element)
    }

    /// Get the parent of an element
    ///
    /// # Returns
    ///
    /// `Some(parent_id)` if the element has a parent, `None` if it's root or doesn't exist
    #[inline]
    pub fn parent(&self, element_id: ElementId) -> Option<ElementId> {
        self.get(element_id).and_then(|element| element.parent())
    }

    /// Get the children of an element as a Vec
    ///
    /// # Returns
    ///
    /// A Vec of child ElementIds, or empty Vec if element has no children or doesn't exist
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// for child_id in tree.children(parent_id) {
    ///     println!("Child: {}", child_id);
    /// }
    /// ```
    #[inline]
    pub fn children(&self, element_id: ElementId) -> Vec<ElementId> {
        self.get(element_id)
            .map(|element| element.children().collect())
            .unwrap_or_else(Vec::new)
    }

    /// Get the number of children for an element
    #[inline]
    pub fn child_count(&self, element_id: ElementId) -> usize {
        self.get(element_id)
            .map(|element| element.children().count())
            .unwrap_or(0)
    }

    // ========== RenderObject Access ==========

    /// Get a reference to the RenderObject for an element
    ///
    /// # Returns
    ///
    /// `Some(&dyn DynRenderObject)` if the element is a RenderObjectElement, `None` otherwise
    ///
    /// # Note
    ///
    /// Only RenderObjectElements have RenderObjects. ComponentElements and StatefulElements
    /// will return None.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(render_obj) = tree.render_object(element_id) {
    ///     println!("Arity: {:?}", render_obj.arity());
    /// }
    /// ```
    #[inline]
    pub fn render_object(&self, element_id: ElementId) -> Option<&dyn crate::DynRenderObject> {
        self.get(element_id).and_then(|element| element.render_object())
    }

    /// Get a mutable reference to the RenderObject for an element
    ///
    /// # Returns
    ///
    /// `Some(&mut dyn DynRenderObject)` if the element is a RenderObjectElement, `None` otherwise
    #[inline]
    pub fn render_object_mut(&mut self, element_id: ElementId) -> Option<&mut dyn crate::DynRenderObject> {
        self.get_mut(element_id).and_then(|element| element.render_object_mut())
    }

    // ========== RenderState Access ==========

    /// Get a read guard to the RenderState for an element
    ///
    /// # Returns
    ///
    /// `Some(RwLockReadGuard<RenderState>)` if the element is a RenderObjectElement
    ///
    /// # Note
    ///
    /// Only RenderObjectElements have RenderState. ComponentElements and StatefulElements
    /// will return None.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(state) = tree.render_state(element_id) {
    ///     if state.needs_layout() {  // Lock-free atomic check!
    ///         // Layout needed
    ///     }
    /// }
    /// ```
    #[inline]
    pub fn render_state(&self, element_id: ElementId) -> Option<parking_lot::RwLockReadGuard<RenderState>> {
        self.get(element_id).and_then(|element| {
            element.render_state_ptr().map(|ptr| unsafe {
                // SAFETY: The pointer is valid for the lifetime of the element
                (*ptr).read()
            })
        })
    }

    /// Get a write guard to the RenderState for an element
    ///
    /// # Returns
    ///
    /// `Some(RwLockWriteGuard<RenderState>)` if the element is a RenderObjectElement
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// if let Some(mut state) = tree.render_state_mut(element_id) {
    ///     state.set_size(Size::new(100.0, 50.0));
    ///     state.clear_needs_layout();
    /// }
    /// ```
    #[inline]
    pub fn render_state_mut(&self, element_id: ElementId) -> Option<parking_lot::RwLockWriteGuard<RenderState>> {
        self.get(element_id).and_then(|element| {
            element.render_state_ptr().map(|ptr| unsafe {
                // SAFETY: The pointer is valid for the lifetime of the element
                (*ptr).write()
            })
        })
    }

    // ========== Layout & Paint Helpers ==========

    /// Perform layout on a RenderObject
    ///
    /// This is a helper method that safely handles the split borrow between
    /// the render object (mutable) and the tree (immutable for children access).
    ///
    /// # Arguments
    ///
    /// - `element_id`: The element to layout
    /// - `constraints`: Layout constraints
    ///
    /// # Returns
    ///
    /// The size computed by the RenderObject, or None if element is not a RenderObjectElement
    pub fn layout_render_object(&mut self, element_id: ElementId, constraints: BoxConstraints) -> Option<flui_types::Size> {
        // Check element exists and has a render object
        if !self.contains(element_id) {
            return None;
        }

        // SAFETY: We're doing a split borrow using raw pointers:
        // - Get mutable pointer to render_object for calling layout()
        // - Get const pointer to self for tree access in LayoutCx
        //
        // This is safe because:
        // 1. The render_object being laid out is at element_id
        // 2. LayoutCx only reads tree for children (not element_id's render_object)
        // 3. We've verified element_id exists above
        // 4. No aliasing occurs - we're modifying render_object, reading tree
        unsafe {
            let self_ptr = self as *const Self;
            let self_mut_ptr = self as *mut Self;

            // Get mutable access to element
            let element = (*self_mut_ptr).get_mut(element_id)?;

            // Get mutable access to render object
            let render_object = element.render_object_mut()?;

            let size = render_object.dyn_layout(&*self_ptr, element_id, constraints);
            Some(size)
        }
    }

    /// Perform paint on a RenderObject
    ///
    /// This is a helper method that safely handles access to the render object
    /// and tree for painting.
    ///
    /// # Arguments
    ///
    /// - `element_id`: The element to paint
    /// - `offset`: Painting offset
    ///
    /// # Returns
    ///
    /// The layer tree, or None if element is not a RenderObjectElement
    pub fn paint_render_object(&self, element_id: ElementId, offset: crate::Offset) -> Option<crate::BoxedLayer> {
        let element = self.get(element_id)?;
        let render_object = element.render_object()?;
        let layer = render_object.dyn_paint(self, element_id, offset);
        Some(layer)
    }

    // ========== Tree Information ==========

    /// Get the total number of elements in the tree
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// println!("Tree has {} elements", tree.len());
    /// ```
    #[inline]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Check if the tree is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Get the current capacity of the slab
    #[inline]
    pub fn capacity(&self) -> usize {
        self.nodes.capacity()
    }

    /// Clear the entire tree
    ///
    /// Removes all elements and frees memory.
    pub fn clear(&mut self) {
        self.nodes.clear();
    }

    // ========== Iteration ==========

    /// Visit all RenderObjectElements in the tree
    ///
    /// This only visits elements that have RenderObjects (RenderObjectElement).
    /// ComponentElements and StatefulElements are skipped.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// tree.visit_all_render_objects(|element_id, render_obj, state| {
    ///     println!("Element {}: arity = {:?}", element_id, render_obj.arity());
    /// });
    /// ```
    pub fn visit_all_render_objects<F>(&self, mut visitor: F)
    where
        F: FnMut(ElementId, &dyn crate::DynRenderObject, parking_lot::RwLockReadGuard<RenderState>),
    {
        for (element_id, node) in &self.nodes {
            // Only visit elements with RenderObjects
            if let Some(render_obj) = node.element.render_object() {
                if let Some(render_elem) = node.element.as_render() {
                    let state_ptr: *const parking_lot::RwLock<RenderState> = render_elem.render_state();
                    let state = unsafe { (*state_ptr).read() };
                    visitor(element_id, render_obj, state);
                }
            }
        }
    }

    /// Visit all elements in the tree
    ///
    /// This visits all elements (Component, Stateful, Inherited, Render, and ParentData).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// tree.visit_all_elements(|element_id, element| {
    ///     println!("Element {} has {} children", element_id, element.children().count());
    /// });
    /// ```
    pub fn visit_all_elements<F>(&self, mut visitor: F)
    where
        F: FnMut(ElementId, &Element),
    {
        for (element_id, node) in &self.nodes {
            visitor(element_id, &node.element);
        }
    }

    /// Visit all elements in a subtree (depth-first, pre-order)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// tree.visit_subtree(root_id, |element_id, element| {
    ///     println!("Visiting {}", element_id);
    /// });
    /// ```
    pub fn visit_subtree<F>(&self, element_id: ElementId, visitor: &mut F)
    where
        F: FnMut(ElementId, &Element),
    {
        if let Some(element) = self.get(element_id) {
            visitor(element_id, element);

            // Visit children
            let children: Vec<ElementId> = element.children().collect();
            for child_id in children {
                self.visit_subtree(child_id, visitor);
            }
        }
    }
}

impl Default for ElementTree {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RenderObject, RenderObjectElement, Widget, DynWidget, RenderObjectWidget};
    use crate::{LeafArity, SingleArity, LayoutCx, PaintCx};
    use flui_types::Size;
    use flui_engine::{BoxedLayer, ContainerLayer};

    // Test Widgets and RenderObjects
    #[derive(Debug, Clone)]
    struct TestLeafWidget;

    impl Widget for TestLeafWidget {}
    impl DynWidget for TestLeafWidget {
        fn as_any(&self) -> &dyn std::any::Any { self }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
    }

    impl RenderObjectWidget for TestLeafWidget {
        type Arity = LeafArity;
        type Render = TestLeafRender;

        fn create_render_object(&self) -> Self::Render {
            TestLeafRender
        }

        fn update_render_object(&self, _render: &mut Self::Render) {}
    }

    #[derive(Debug)]
    struct TestLeafRender;

    impl RenderObject for TestLeafRender {
        type Arity = LeafArity;

        fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
            cx.constraints().constrain(Size::new(100.0, 50.0))
        }

        fn paint(&self, _cx: &PaintCx<Self::Arity>) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }
    }

    #[derive(Debug, Clone)]
    struct TestSingleWidget;

    impl Widget for TestSingleWidget {}
    impl DynWidget for TestSingleWidget {
        fn as_any(&self) -> &dyn std::any::Any { self }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
    }

    impl RenderObjectWidget for TestSingleWidget {
        type Arity = SingleArity;
        type Render = TestSingleRender;

        fn create_render_object(&self) -> Self::Render {
            TestSingleRender
        }

        fn update_render_object(&self, _render: &mut Self::Render) {}
    }

    #[derive(Debug)]
    struct TestSingleRender;

    impl RenderObject for TestSingleRender {
        type Arity = SingleArity;

        fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
            cx.constraints().constrain(Size::new(200.0, 100.0))
        }

        fn paint(&self, _cx: &PaintCx<Self::Arity>) -> BoxedLayer {
            Box::new(ContainerLayer::new())
        }
    }

    #[test]
    fn test_element_tree_creation() {
        let tree = ElementTree::new();
        assert_eq!(tree.len(), 0);
        assert!(tree.is_empty());
    }

    #[test]
    fn test_element_tree_with_capacity() {
        let tree = ElementTree::with_capacity(100);
        assert!(tree.capacity() >= 100);
    }

    #[test]
    fn test_insert_root() {
        let mut tree = ElementTree::new();
        let element = RenderObjectElement::new(TestLeafWidget);
        let root_id = tree.insert(Box::new(element));

        assert_eq!(tree.len(), 1);
        assert!(tree.contains(root_id));
        assert_eq!(tree.parent(root_id), None);
    }

    #[test]
    fn test_remove_element() {
        let mut tree = ElementTree::new();
        let element = RenderObjectElement::new(TestLeafWidget);
        let root_id = tree.insert(Box::new(element));

        assert!(tree.remove(root_id));
        assert!(!tree.contains(root_id));
        assert_eq!(tree.len(), 0);
    }

    #[test]
    fn test_render_object_access() {
        let mut tree = ElementTree::new();
        let element = RenderObjectElement::new(TestLeafWidget);
        let element_id = tree.insert(Box::new(element));

        // Immutable access
        let render_obj = tree.render_object(element_id).unwrap();
        assert_eq!(render_obj.arity(), Some(0));

        // Mutable access
        let render_obj_mut = tree.render_object_mut(element_id).unwrap();
        assert_eq!(render_obj_mut.arity(), Some(0));
    }

    #[test]
    fn test_render_state_access() {
        let mut tree = ElementTree::new();
        let element = RenderObjectElement::new(TestLeafWidget);
        let element_id = tree.insert(Box::new(element));

        // Read access
        {
            let state = tree.render_state(element_id).unwrap();
            assert!(!state.has_size());
        }

        // Write access
        {
            let state = tree.render_state_mut(element_id).unwrap();
            state.set_size(Size::new(100.0, 50.0));
        }

        // Verify
        {
            let state = tree.render_state(element_id).unwrap();
            assert!(state.has_size());
            assert_eq!(state.get_size(), Some(Size::new(100.0, 50.0)));
        }
    }

    #[test]
    fn test_visit_all_elements() {
        let mut tree = ElementTree::new();
        tree.insert(Box::new(RenderObjectElement::new(TestLeafWidget)));
        tree.insert(Box::new(RenderObjectElement::new(TestLeafWidget)));

        let mut count = 0;
        tree.visit_all_elements(|_id, _element| {
            count += 1;
        });

        assert_eq!(count, 2);
    }

    #[test]
    fn test_clear() {
        let mut tree = ElementTree::new();
        tree.insert(Box::new(RenderObjectElement::new(TestLeafWidget)));
        tree.insert(Box::new(RenderObjectElement::new(TestLeafWidget)));

        assert_eq!(tree.len(), 2);

        tree.clear();

        assert_eq!(tree.len(), 0);
        assert!(tree.is_empty());
    }
}
