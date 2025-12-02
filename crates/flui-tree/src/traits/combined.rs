//! Combined traits for convenience.
//!
//! This module provides combined traits that bundle multiple
//! capabilities for common use cases.

use super::{DirtyTracking, RenderTreeAccess, TreeNav, TreeRead, TreeWrite};

/// Full mutable tree access (read + write + navigate).
///
/// This trait is automatically implemented for any type that
/// implements [`TreeRead`], [`TreeWrite`], and [`TreeNav`].
///
/// # Example
///
/// ```rust,ignore
/// use flui_tree::TreeMut;
///
/// fn reparent_subtree<T: TreeMut>(
///     tree: &mut T,
///     subtree_root: ElementId,
///     new_parent: ElementId,
/// ) {
///     // Can use navigation...
///     let old_parent = tree.parent(subtree_root);
///
///     // ...and mutation
///     tree.set_parent(subtree_root, Some(new_parent));
/// }
/// ```
pub trait TreeMut: TreeRead + TreeWrite + TreeNav {}

// Blanket implementation
impl<T: TreeRead + TreeWrite + TreeNav> TreeMut for T {}

/// Full tree access including render operations.
///
/// This trait combines all tree capabilities:
/// - [`TreeRead`] - Read access
/// - [`TreeWrite`] - Write access
/// - [`TreeNav`] - Navigation
/// - [`RenderTreeAccess`] - Render data access
/// - [`DirtyTracking`] - Dirty flag management
///
/// This is the most complete interface, typically implemented
/// by `ElementTree` in `flui-pipeline`.
///
/// # Example
///
/// ```rust,ignore
/// use flui_tree::FullTreeAccess;
///
/// fn perform_layout<T: FullTreeAccess>(
///     tree: &mut T,
///     root: ElementId,
///     constraints: Constraints,
/// ) {
///     // Access render objects
///     if tree.is_render_element(root) {
///         // Do layout...
///         tree.clear_needs_layout(root);
///     }
///
///     // Recurse to children
///     for child in tree.children(root).to_vec() {
///         perform_layout(tree, child, child_constraints);
///     }
/// }
/// ```
pub trait FullTreeAccess: TreeMut + RenderTreeAccess + DirtyTracking {}

// Blanket implementation
impl<T: TreeMut + RenderTreeAccess + DirtyTracking> FullTreeAccess for T {}

// ============================================================================
// OBJECT-SAFE VARIANTS
// ============================================================================

/// Object-safe version of `TreeRead`.
///
/// This trait provides a subset of `TreeRead` that can be used as
/// a trait object (`dyn TreeReadDyn`).
pub trait TreeReadDyn: Send + Sync {
    /// Returns `true` if the tree contains a node with the given ID.
    fn contains_dyn(&self, id: flui_foundation::ElementId) -> bool;

    /// Returns the number of nodes in the tree.
    fn len_dyn(&self) -> usize;

    /// Returns `true` if the tree is empty.
    fn is_empty_dyn(&self) -> bool {
        self.len_dyn() == 0
    }
}

/// Object-safe version of `TreeNav`.
///
/// This trait provides a subset of `TreeNav` that can be used as
/// a trait object (`dyn TreeNavDyn`).
pub trait TreeNavDyn: TreeReadDyn {
    /// Returns the parent of the given node.
    fn parent_dyn(&self, id: flui_foundation::ElementId) -> Option<flui_foundation::ElementId>;

    /// Returns the number of children of the given node.
    fn child_count_dyn(&self, id: flui_foundation::ElementId) -> usize;

    /// Returns the child at the given index.
    fn child_at_dyn(
        &self,
        id: flui_foundation::ElementId,
        index: usize,
    ) -> Option<flui_foundation::ElementId>;

    /// Returns `true` if the node is a root.
    fn is_root_dyn(&self, id: flui_foundation::ElementId) -> bool {
        self.parent_dyn(id).is_none() && self.contains_dyn(id)
    }

    /// Returns `true` if the node is a leaf.
    fn is_leaf_dyn(&self, id: flui_foundation::ElementId) -> bool {
        self.child_count_dyn(id) == 0 && self.contains_dyn(id)
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{TreeError, TreeResult};
    use crate::traits::write::TreeWriteNav;
    use flui_foundation::{ElementId, Slot};
    use std::any::Any;

    // Complete test implementation
    #[derive(Default)]
    struct TestNode {
        parent: Option<ElementId>,
        children: Vec<ElementId>,
        is_render: bool,
    }

    struct CompleteTree {
        nodes: Vec<Option<TestNode>>,
        layout_dirty: std::collections::HashSet<ElementId>,
        paint_dirty: std::collections::HashSet<ElementId>,
    }

    impl CompleteTree {
        fn new() -> Self {
            Self {
                nodes: Vec::new(),
                layout_dirty: std::collections::HashSet::new(),
                paint_dirty: std::collections::HashSet::new(),
            }
        }
    }

    impl TreeRead for CompleteTree {
        type Node = TestNode;

        fn get(&self, id: ElementId) -> Option<&TestNode> {
            self.nodes.get(id.get() as usize - 1)?.as_ref()
        }

        fn len(&self) -> usize {
            self.nodes.iter().filter(|n| n.is_some()).count()
        }
    }

    impl TreeNav for CompleteTree {
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

    impl TreeWrite for CompleteTree {
        fn get_mut(&mut self, id: ElementId) -> Option<&mut TestNode> {
            self.nodes.get_mut(id.get() as usize - 1)?.as_mut()
        }

        fn insert(&mut self, node: TestNode) -> ElementId {
            let id = ElementId::new(self.nodes.len() + 1);
            self.nodes.push(Some(node));
            id
        }

        fn remove(&mut self, id: ElementId) -> Option<TestNode> {
            let index = id.get() as usize - 1;
            self.nodes.get_mut(index)?.take()
        }
    }

    impl TreeWriteNav for CompleteTree {
        fn set_parent(
            &mut self,
            child: ElementId,
            new_parent: Option<ElementId>,
        ) -> TreeResult<()> {
            if !self.contains(child) {
                return Err(TreeError::not_found(child));
            }

            if let Some(parent_id) = new_parent {
                if !self.contains(parent_id) {
                    return Err(TreeError::not_found(parent_id));
                }
                // Check for cycles: new_parent must not be a descendant of child
                if self.is_ancestor_of(child, parent_id) || parent_id == child {
                    return Err(TreeError::cycle_detected(child));
                }
            }

            // Remove from old parent
            if let Some(old_parent) = self.parent(child) {
                if let Some(Some(p)) = self.nodes.get_mut(old_parent.get() as usize - 1) {
                    p.children.retain(|&c| c != child);
                }
            }

            // Update child's parent
            if let Some(Some(c)) = self.nodes.get_mut(child.get() as usize - 1) {
                c.parent = new_parent;
            }

            // Add to new parent
            if let Some(parent_id) = new_parent {
                if let Some(Some(p)) = self.nodes.get_mut(parent_id.get() as usize - 1) {
                    p.children.push(child);
                }
            }

            Ok(())
        }
    }

    impl RenderTreeAccess for CompleteTree {
        fn render_object(&self, id: ElementId) -> Option<&dyn Any> {
            if self.get(id)?.is_render {
                Some(&() as &dyn Any)
            } else {
                None
            }
        }

        fn render_object_mut(&mut self, id: ElementId) -> Option<&mut dyn Any> {
            None // Simplified
        }

        fn render_state(&self, id: ElementId) -> Option<&dyn Any> {
            if self.get(id)?.is_render {
                Some(&() as &dyn Any)
            } else {
                None
            }
        }

        fn render_state_mut(&mut self, id: ElementId) -> Option<&mut dyn Any> {
            None // Simplified
        }
    }

    impl DirtyTracking for CompleteTree {
        fn mark_needs_layout(&self, id: ElementId) {
            // Would need interior mutability in real impl
        }

        fn mark_needs_paint(&self, id: ElementId) {
            // Would need interior mutability in real impl
        }

        fn clear_needs_layout(&self, id: ElementId) {}
        fn clear_needs_paint(&self, id: ElementId) {}

        fn needs_layout(&self, _id: ElementId) -> bool {
            false
        }

        fn needs_paint(&self, _id: ElementId) -> bool {
            false
        }
    }

    #[test]
    fn test_tree_mut_trait() {
        fn use_tree_mut<T: TreeMut>(_tree: &mut T) {
            // Compiles = trait bounds work
        }

        let mut tree = CompleteTree::new();
        use_tree_mut(&mut tree);
    }

    #[test]
    fn test_full_tree_access_trait() {
        fn use_full_access<T: FullTreeAccess>(_tree: &mut T) {
            // Compiles = trait bounds work
        }

        let mut tree = CompleteTree::new();
        use_full_access(&mut tree);
    }
}
