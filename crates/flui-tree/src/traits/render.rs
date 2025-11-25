//! Render tree access trait.
//!
//! This module provides the [`RenderTreeAccess`] trait for accessing
//! render-specific data (RenderObject, RenderState) from tree nodes.
//!
//! # Design Rationale
//!
//! This trait allows `flui-rendering` to work with abstract tree
//! interfaces without depending on `flui-element`. The concrete
//! implementation is provided by `flui-pipeline`.

use flui_foundation::ElementId;
use std::any::Any;

use super::TreeNav;

/// Access to render-specific data in tree nodes.
///
/// This trait extends [`TreeNav`] with methods to access RenderObject
/// and RenderState. It uses `dyn Any` for type erasure, allowing
/// the trait to be defined without depending on concrete render types.
///
/// # Type Erasure Strategy
///
/// RenderObject and RenderState are accessed as `dyn Any` because:
/// 1. Keeps `flui-tree` independent of `flui-rendering` types
/// 2. Implementations downcast to concrete types as needed
/// 3. Enables generic algorithms over any render tree
///
/// # Example
///
/// ```rust,ignore
/// use flui_tree::RenderTreeAccess;
/// use flui_foundation::ElementId;
///
/// fn collect_render_elements<T: RenderTreeAccess>(
///     tree: &T,
///     root: ElementId,
/// ) -> Vec<ElementId> {
///     tree.descendants(root)
///         .filter(|&id| tree.is_render_element(id))
///         .collect()
/// }
/// ```
pub trait RenderTreeAccess: TreeNav {
    /// Returns the RenderObject for an element, if it's a render element.
    ///
    /// # Arguments
    ///
    /// * `id` - The element ID
    ///
    /// # Returns
    ///
    /// Reference to the RenderObject as `dyn Any`, or `None` if the
    /// element is not a render element.
    ///
    /// # Type Safety
    ///
    /// Callers should downcast using `downcast_ref::<ConcreteType>()`.
    fn render_object(&self, id: ElementId) -> Option<&dyn Any>;

    /// Returns a mutable reference to the RenderObject.
    ///
    /// # Arguments
    ///
    /// * `id` - The element ID
    ///
    /// # Returns
    ///
    /// Mutable reference to the RenderObject as `dyn Any`, or `None`
    /// if the element is not a render element.
    fn render_object_mut(&mut self, id: ElementId) -> Option<&mut dyn Any>;

    /// Returns the RenderState for an element, if it's a render element.
    ///
    /// # Arguments
    ///
    /// * `id` - The element ID
    ///
    /// # Returns
    ///
    /// Reference to the RenderState as `dyn Any`, or `None` if the
    /// element is not a render element.
    fn render_state(&self, id: ElementId) -> Option<&dyn Any>;

    /// Returns a mutable reference to the RenderState.
    ///
    /// # Arguments
    ///
    /// * `id` - The element ID
    fn render_state_mut(&mut self, id: ElementId) -> Option<&mut dyn Any>;

    /// Returns `true` if the element is a render element.
    ///
    /// A render element is one that participates in layout and paint.
    /// Non-render elements (like StatelessView) just compose other elements.
    ///
    /// # Arguments
    ///
    /// * `id` - The element ID
    #[inline]
    fn is_render_element(&self, id: ElementId) -> bool {
        self.render_object(id).is_some()
    }

    /// Finds the nearest render ancestor of an element.
    ///
    /// This is useful for finding the render parent when the element
    /// tree contains non-render elements (like StatelessView wrappers).
    ///
    /// # Arguments
    ///
    /// * `id` - The starting element
    ///
    /// # Returns
    ///
    /// The nearest ancestor that is a render element, or `None` if
    /// there are no render ancestors.
    fn render_ancestor(&self, id: ElementId) -> Option<ElementId>
    where
        Self: Sized,
    {
        self.ancestors(id)
            .skip(1) // Skip self
            .find(|&ancestor| self.is_render_element(ancestor))
    }

    /// Finds all render children of an element.
    ///
    /// This skips non-render elements and finds the actual render
    /// children that should participate in layout.
    ///
    /// # Arguments
    ///
    /// * `id` - The parent element
    ///
    /// # Returns
    ///
    /// A vector of render element IDs that are descendants of `id`
    /// and have `id` as their render ancestor.
    fn render_children(&self, id: ElementId) -> Vec<ElementId>
    where
        Self: Sized,
    {
        self.children(id)
            .iter()
            .copied()
            .flat_map(|child| {
                if self.is_render_element(child) {
                    vec![child]
                } else {
                    // Recursively find render children
                    self.render_children(child)
                }
            })
            .collect()
    }

    /// Returns an iterator over render ancestors.
    ///
    /// Similar to `ancestors()` but only yields render elements.
    ///
    /// # Arguments
    ///
    /// * `id` - The starting element
    #[inline]
    fn render_ancestors(&self, id: ElementId) -> crate::iter::RenderAncestors<'_, Self>
    where
        Self: Sized,
    {
        crate::iter::RenderAncestors::new(self, id)
    }

    /// Returns an iterator over render descendants.
    ///
    /// Similar to `descendants()` but only yields render elements.
    ///
    /// # Arguments
    ///
    /// * `id` - The starting element
    #[inline]
    fn render_descendants(&self, id: ElementId) -> crate::iter::RenderDescendants<'_, Self>
    where
        Self: Sized,
    {
        crate::iter::RenderDescendants::new(self, id)
    }

    /// Gets the size from RenderState, if available.
    ///
    /// This is a convenience method that accesses the cached size
    /// from the RenderState. Returns `None` if:
    /// - Element doesn't exist
    /// - Element is not a render element
    /// - Size hasn't been computed yet
    ///
    /// # Arguments
    ///
    /// * `id` - The element ID
    ///
    /// # Note
    ///
    /// Default implementation returns `None`. Concrete implementations
    /// should override to extract size from RenderState.
    #[inline]
    fn get_size(&self, id: ElementId) -> Option<(f32, f32)> {
        let _ = id;
        None
    }

    /// Gets the cached constraints from RenderState, if available.
    ///
    /// # Arguments
    ///
    /// * `id` - The element ID
    ///
    /// # Note
    ///
    /// Default implementation returns `None`. Concrete implementations
    /// should override to extract constraints from RenderState.
    #[inline]
    fn get_constraints(&self, id: ElementId) -> Option<&dyn Any> {
        let _ = id;
        None
    }

    /// Gets the offset from RenderState, if available.
    ///
    /// The offset is the position relative to the parent.
    ///
    /// # Arguments
    ///
    /// * `id` - The element ID
    #[inline]
    fn get_offset(&self, id: ElementId) -> Option<(f32, f32)> {
        let _ = id;
        None
    }
}

// ============================================================================
// TYPED ACCESS EXTENSION
// ============================================================================

/// Extension trait for typed access to render data.
///
/// This trait provides type-safe access when the caller knows the
/// concrete types.
pub trait RenderTreeAccessExt: RenderTreeAccess {
    /// Gets the RenderObject with a specific type.
    ///
    /// # Type Parameters
    ///
    /// * `R` - The expected RenderObject type
    ///
    /// # Returns
    ///
    /// Reference to the RenderObject if it exists and has the correct type.
    #[inline]
    fn render_object_typed<R: 'static>(&self, id: ElementId) -> Option<&R> {
        self.render_object(id)?.downcast_ref::<R>()
    }

    /// Gets the RenderObject mutably with a specific type.
    #[inline]
    fn render_object_typed_mut<R: 'static>(&mut self, id: ElementId) -> Option<&mut R> {
        self.render_object_mut(id)?.downcast_mut::<R>()
    }

    /// Gets the RenderState with a specific type.
    #[inline]
    fn render_state_typed<S: 'static>(&self, id: ElementId) -> Option<&S> {
        self.render_state(id)?.downcast_ref::<S>()
    }

    /// Gets the RenderState mutably with a specific type.
    #[inline]
    fn render_state_typed_mut<S: 'static>(&mut self, id: ElementId) -> Option<&mut S> {
        self.render_state_mut(id)?.downcast_mut::<S>()
    }
}

// Blanket implementation for all RenderTreeAccess implementors
impl<T: RenderTreeAccess + ?Sized> RenderTreeAccessExt for T {}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::traits::{TreeNav, TreeRead};
    use flui_foundation::Slot;

    // Mock render object
    #[derive(Debug)]
    struct MockRenderObject {
        width: f32,
        height: f32,
    }

    // Mock render state
    #[derive(Debug)]
    struct MockRenderState {
        size: (f32, f32),
        needs_layout: bool,
    }

    // Test node
    struct TestNode {
        parent: Option<ElementId>,
        children: Vec<ElementId>,
        render_object: Option<MockRenderObject>,
        render_state: Option<MockRenderState>,
    }

    impl TestNode {
        fn new_render(width: f32, height: f32) -> Self {
            Self {
                parent: None,
                children: Vec::new(),
                render_object: Some(MockRenderObject { width, height }),
                render_state: Some(MockRenderState {
                    size: (width, height),
                    needs_layout: false,
                }),
            }
        }

        fn new_component() -> Self {
            Self {
                parent: None,
                children: Vec::new(),
                render_object: None,
                render_state: None,
            }
        }
    }

    struct TestTree {
        nodes: Vec<Option<TestNode>>,
    }

    impl TestTree {
        fn new() -> Self {
            Self { nodes: Vec::new() }
        }

        fn insert(&mut self, mut node: TestNode, parent: Option<ElementId>) -> ElementId {
            let id = ElementId::new(self.nodes.len() as u64 + 1);
            node.parent = parent;
            self.nodes.push(Some(node));

            if let Some(parent_id) = parent {
                if let Some(Some(parent_node)) = self.nodes.get_mut(parent_id.get() as usize - 1) {
                    parent_node.children.push(id);
                }
            }

            id
        }
    }

    impl TreeRead for TestTree {
        type Node = TestNode;

        fn get(&self, id: ElementId) -> Option<&TestNode> {
            self.nodes.get(id.get() as usize - 1)?.as_ref()
        }

        fn len(&self) -> usize {
            self.nodes.iter().filter(|n| n.is_some()).count()
        }
    }

    impl TreeNav for TestTree {
        fn parent(&self, id: ElementId) -> Option<ElementId> {
            self.get(id)?.parent
        }

        fn children(&self, id: ElementId) -> &[ElementId] {
            self.get(id).map(|n| n.children.as_slice()).unwrap_or(&[])
        }

        fn slot(&self, _id: ElementId) -> Option<Slot> {
            None
        }
    }

    impl RenderTreeAccess for TestTree {
        fn render_object(&self, id: ElementId) -> Option<&dyn Any> {
            self.get(id)?.render_object.as_ref().map(|r| r as &dyn Any)
        }

        fn render_object_mut(&mut self, id: ElementId) -> Option<&mut dyn Any> {
            self.nodes
                .get_mut(id.get() as usize - 1)?
                .as_mut()?
                .render_object
                .as_mut()
                .map(|r| r as &mut dyn Any)
        }

        fn render_state(&self, id: ElementId) -> Option<&dyn Any> {
            self.get(id)?.render_state.as_ref().map(|s| s as &dyn Any)
        }

        fn render_state_mut(&mut self, id: ElementId) -> Option<&mut dyn Any> {
            self.nodes
                .get_mut(id.get() as usize - 1)?
                .as_mut()?
                .render_state
                .as_mut()
                .map(|s| s as &mut dyn Any)
        }

        fn get_size(&self, id: ElementId) -> Option<(f32, f32)> {
            self.get(id)?.render_state.as_ref().map(|s| s.size)
        }
    }

    #[test]
    fn test_is_render_element() {
        let mut tree = TestTree::new();
        let render = tree.insert(TestNode::new_render(100.0, 50.0), None);
        let component = tree.insert(TestNode::new_component(), None);

        assert!(tree.is_render_element(render));
        assert!(!tree.is_render_element(component));
    }

    #[test]
    fn test_render_object_typed() {
        let mut tree = TestTree::new();
        let id = tree.insert(TestNode::new_render(100.0, 50.0), None);

        let obj = tree.render_object_typed::<MockRenderObject>(id).unwrap();
        assert_eq!(obj.width, 100.0);
        assert_eq!(obj.height, 50.0);
    }

    #[test]
    fn test_render_state_typed() {
        let mut tree = TestTree::new();
        let id = tree.insert(TestNode::new_render(100.0, 50.0), None);

        let state = tree.render_state_typed::<MockRenderState>(id).unwrap();
        assert_eq!(state.size, (100.0, 50.0));
        assert!(!state.needs_layout);
    }

    #[test]
    fn test_render_ancestor() {
        let mut tree = TestTree::new();

        // Build tree: render -> component -> render2
        let render1 = tree.insert(TestNode::new_render(100.0, 100.0), None);
        let component = tree.insert(TestNode::new_component(), Some(render1));
        let render2 = tree.insert(TestNode::new_render(50.0, 50.0), Some(component));

        // render2's render ancestor should be render1 (skipping component)
        assert_eq!(tree.render_ancestor(render2), Some(render1));

        // component's render ancestor should be render1
        assert_eq!(tree.render_ancestor(component), Some(render1));

        // render1 has no render ancestor
        assert_eq!(tree.render_ancestor(render1), None);
    }

    #[test]
    fn test_render_children() {
        let mut tree = TestTree::new();

        // Build tree: render1 -> component -> [render2, render3]
        let render1 = tree.insert(TestNode::new_render(100.0, 100.0), None);
        let component = tree.insert(TestNode::new_component(), Some(render1));
        let render2 = tree.insert(TestNode::new_render(50.0, 50.0), Some(component));
        let render3 = tree.insert(TestNode::new_render(25.0, 25.0), Some(component));

        // render1's render children should be [render2, render3]
        // (skipping component, finding its render children)
        let children = tree.render_children(render1);
        assert_eq!(children.len(), 2);
        assert!(children.contains(&render2));
        assert!(children.contains(&render3));
    }

    #[test]
    fn test_get_size() {
        let mut tree = TestTree::new();
        let id = tree.insert(TestNode::new_render(100.0, 50.0), None);

        assert_eq!(tree.get_size(id), Some((100.0, 50.0)));
    }
}
