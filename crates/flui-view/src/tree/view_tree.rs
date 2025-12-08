//! ViewTree - Slab-based storage for view nodes
//!
//! This module implements the first of FLUI's four trees (View, Element, RenderObject, Layer).
//! Following Flutter's architecture, ViewObjects are stored in a separate tree from Elements.
//!
//! # Architecture
//!
//! ```text
//! ViewTree
//!   ├─ nodes: Slab<ViewNode>
//!   └─ root: Option<ViewId>
//!
//! ViewNode
//!   ├─ view_object: Box<dyn ViewObject>
//!   ├─ mode: ViewMode
//!   ├─ lifecycle: ViewLifecycle
//!   └─ tree structure (parent, children)
//! ```
//!
//! # Flutter Analogy
//!
//! This corresponds to Flutter's Widget tree, but with mutable ViewObjects.
//! Unlike Flutter Widgets (which are immutable), FLUI ViewObjects can be mutable
//! for efficiency, but are still stored separately from Elements for architectural clarity.

use std::fmt;

use slab::Slab;

use flui_foundation::{Key, ViewId};
use flui_tree::iter::{AllSiblings, Ancestors, DescendantsWithDepth};
use flui_tree::traits::{TreeNav, TreeRead, TreeWrite};

use crate::view_mode::ViewMode;
use crate::view_object::ViewObject;
use crate::ViewLifecycle;

// ============================================================================
// VIEW NODE
// ============================================================================

/// A node in the ViewTree that wraps a ViewObject with tree structure metadata.
///
/// # Design
///
/// Similar to LayerNode which contains a Layer enum, ViewNode contains
/// a `Box<dyn ViewObject>` for type erasure. This allows storing different
/// view types (Stateless, Stateful, Provider, etc.) in the same tree.
pub struct ViewNode {
    // ========== Tree Structure ==========
    parent: Option<ViewId>,
    children: Vec<ViewId>,

    // ========== ViewObject ==========
    /// The view object (type-erased)
    view_object: Box<dyn ViewObject>,

    // ========== Metadata ==========
    /// View mode (Stateless, Stateful, Provider, etc.)
    mode: ViewMode,

    /// Current lifecycle state
    lifecycle: ViewLifecycle,

    /// Optional key for reconciliation
    key: Option<Key>,
}

impl ViewNode {
    /// Creates a new ViewNode with the given ViewObject and mode.
    pub fn new<V: ViewObject + fmt::Debug + 'static>(object: V, mode: ViewMode) -> Self {
        Self {
            parent: None,
            children: Vec::new(),
            view_object: Box::new(object),
            mode,
            lifecycle: ViewLifecycle::Initial,
            key: None,
        }
    }

    /// Creates a ViewNode from a boxed ViewObject.
    pub fn from_boxed(view_object: Box<dyn ViewObject>, mode: ViewMode) -> Self {
        Self {
            parent: None,
            children: Vec::new(),
            view_object,
            mode,
            lifecycle: ViewLifecycle::Initial,
            key: None,
        }
    }

    /// Creates a ViewNode with a key.
    pub fn with_key(mut self, key: Key) -> Self {
        self.key = Some(key);
        self
    }

    // ========== Tree Structure ==========

    /// Gets the parent ViewId.
    #[inline]
    pub fn parent(&self) -> Option<ViewId> {
        self.parent
    }

    /// Sets the parent ViewId.
    #[inline]
    pub fn set_parent(&mut self, parent: Option<ViewId>) {
        self.parent = parent;
    }

    /// Gets all children ViewIds.
    #[inline]
    pub fn children(&self) -> &[ViewId] {
        &self.children
    }

    /// Adds a child to this view node.
    #[inline]
    pub fn add_child(&mut self, child: ViewId) {
        self.children.push(child);
    }

    /// Removes a child from this view node.
    #[inline]
    pub fn remove_child(&mut self, child: ViewId) {
        self.children.retain(|&id| id != child);
    }

    /// Clears all children from this view node.
    #[inline]
    pub fn clear_children(&mut self) {
        self.children.clear();
    }

    // ========== ViewObject Access ==========

    /// Returns reference to the ViewObject.
    #[inline]
    pub fn view_object(&self) -> &dyn ViewObject {
        &*self.view_object
    }

    /// Returns mutable reference to the ViewObject.
    #[inline]
    pub fn view_object_mut(&mut self) -> &mut dyn ViewObject {
        &mut *self.view_object
    }

    // ========== Metadata ==========

    /// Returns the ViewMode.
    #[inline]
    pub fn mode(&self) -> ViewMode {
        self.mode
    }

    /// Returns the ViewLifecycle.
    #[inline]
    pub fn lifecycle(&self) -> ViewLifecycle {
        self.lifecycle
    }

    /// Sets the ViewLifecycle.
    #[inline]
    pub fn set_lifecycle(&mut self, lifecycle: ViewLifecycle) {
        self.lifecycle = lifecycle;
    }

    /// Returns the key.
    #[inline]
    pub fn key(&self) -> Option<&Key> {
        self.key.as_ref()
    }

    /// Sets the key.
    #[inline]
    pub fn set_key(&mut self, key: Option<Key>) {
        self.key = key;
    }
}

impl std::fmt::Debug for ViewNode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewNode")
            .field("parent", &self.parent)
            .field("children", &self.children)
            .field("mode", &self.mode)
            .field("lifecycle", &self.lifecycle)
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

// ============================================================================
// VIEW TREE
// ============================================================================

/// ViewTree - Slab-based storage for view nodes.
///
/// This is the first of FLUI's four trees, corresponding to Flutter's Widget tree.
///
/// # Architecture
///
/// ```text
/// ViewTree
///   ├─ nodes: Slab<ViewNode>  (direct storage)
///   └─ root: Option<ViewId>
/// ```
///
/// # Thread Safety
///
/// ViewTree itself is not thread-safe. Use `Arc<RwLock<ViewTree>>`
/// for multi-threaded access.
///
/// # Example
///
/// ```rust,ignore
/// use flui_view::tree::{ViewTree, ViewNode};
/// use flui_tree::traits::{TreeRead, TreeNav};
///
/// let mut tree = ViewTree::new();
///
/// // Insert view
/// let id = tree.insert(ViewNode::new(my_view, ViewMode::Stateless));
///
/// // Access via TreeRead trait
/// let node = tree.get(id).unwrap();
/// assert_eq!(node.mode(), ViewMode::Stateless);
///
/// // Navigate via TreeNav trait
/// for child_id in tree.children(id) {
///     println!("Child: {:?}", child_id);
/// }
/// ```
#[derive(Debug)]
pub struct ViewTree {
    /// Slab storage for ViewNodes (0-based indexing internally)
    nodes: Slab<ViewNode>,

    /// Root ViewNode ID (None if tree is empty)
    root: Option<ViewId>,
}

impl ViewTree {
    /// Creates a new empty ViewTree.
    pub fn new() -> Self {
        Self {
            nodes: Slab::new(),
            root: None,
        }
    }

    /// Creates a ViewTree with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: Slab::with_capacity(capacity),
            root: None,
        }
    }

    // ========== Root Management ==========

    /// Get the root ViewNode ID.
    #[inline]
    pub fn root(&self) -> Option<ViewId> {
        self.root
    }

    /// Set the root ViewNode ID.
    #[inline]
    pub fn set_root(&mut self, root: Option<ViewId>) {
        self.root = root;
    }

    // ========== Basic Operations ==========

    /// Inserts a ViewNode into the tree.
    ///
    /// Returns the ViewId of the inserted node.
    pub fn insert(&mut self, node: ViewNode) -> ViewId {
        let slab_index = self.nodes.insert(node);
        ViewId::new(slab_index + 1) // +1 offset for NonZeroUsize
    }

    /// Returns a reference to a ViewNode.
    #[inline]
    pub fn get(&self, id: ViewId) -> Option<&ViewNode> {
        self.nodes.get(id.get() - 1)
    }

    /// Returns a mutable reference to a ViewNode.
    #[inline]
    pub fn get_mut(&mut self, id: ViewId) -> Option<&mut ViewNode> {
        self.nodes.get_mut(id.get() - 1)
    }

    /// Checks if a ViewNode exists in the tree.
    #[inline]
    pub fn contains(&self, id: ViewId) -> bool {
        self.nodes.contains(id.get() - 1)
    }

    /// Returns the number of ViewNodes in the tree.
    #[inline]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns true if the tree is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Removes a ViewNode from the tree.
    ///
    /// Returns the removed node, or None if it didn't exist.
    ///
    /// **Note:** This does NOT remove children. Caller must handle tree cleanup.
    pub fn remove(&mut self, id: ViewId) -> Option<ViewNode> {
        // Update root if removing root
        if self.root == Some(id) {
            self.root = None;
        }

        // Remove from parent's children list
        if let Some(node) = self.nodes.get(id.get() - 1) {
            if let Some(parent_id) = node.parent() {
                if let Some(parent) = self.nodes.get_mut(parent_id.get() - 1) {
                    parent.remove_child(id);
                }
            }
        }

        self.nodes.try_remove(id.get() - 1)
    }

    /// Clears all nodes from the tree.
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.root = None;
    }

    /// Reserves capacity for additional nodes.
    pub fn reserve(&mut self, additional: usize) {
        self.nodes.reserve(additional);
    }

    // ========== Tree Operations ==========

    /// Adds a child to a parent ViewNode.
    ///
    /// Updates both parent's children list and child's parent pointer.
    pub fn add_child(&mut self, parent_id: ViewId, child_id: ViewId) {
        // Update parent's children
        if let Some(parent) = self.get_mut(parent_id) {
            parent.add_child(child_id);
        }

        // Update child's parent
        if let Some(child) = self.get_mut(child_id) {
            child.set_parent(Some(parent_id));
        }
    }

    /// Removes a child from a parent ViewNode.
    pub fn remove_child(&mut self, parent_id: ViewId, child_id: ViewId) {
        // Update parent's children
        if let Some(parent) = self.get_mut(parent_id) {
            parent.remove_child(child_id);
        }

        // Update child's parent
        if let Some(child) = self.get_mut(child_id) {
            child.set_parent(None);
        }
    }

    /// Returns the parent of a node.
    #[inline]
    pub fn parent(&self, id: ViewId) -> Option<ViewId> {
        self.get(id)?.parent()
    }

    /// Returns an iterator over slab entries (for node_ids).
    pub(crate) fn iter_slab(&self) -> impl Iterator<Item = (usize, &ViewNode)> + '_ {
        self.nodes.iter()
    }
}

impl Default for ViewTree {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// flui-tree TRAIT IMPLEMENTATIONS
// ============================================================================

impl TreeRead<ViewId> for ViewTree {
    type Node = ViewNode;

    const DEFAULT_CAPACITY: usize = 64;
    const INLINE_THRESHOLD: usize = 16;

    #[inline]
    fn get(&self, id: ViewId) -> Option<&Self::Node> {
        ViewTree::get(self, id)
    }

    #[inline]
    fn contains(&self, id: ViewId) -> bool {
        ViewTree::contains(self, id)
    }

    #[inline]
    fn len(&self) -> usize {
        ViewTree::len(self)
    }

    #[inline]
    fn node_ids(&self) -> impl Iterator<Item = ViewId> + '_ {
        self.iter_slab().map(|(idx, _)| ViewId::new(idx + 1))
    }
}

impl TreeWrite<ViewId> for ViewTree {
    #[inline]
    fn get_mut(&mut self, id: ViewId) -> Option<&mut Self::Node> {
        ViewTree::get_mut(self, id)
    }

    #[inline]
    fn insert(&mut self, node: Self::Node) -> ViewId {
        ViewTree::insert(self, node)
    }

    #[inline]
    fn remove(&mut self, id: ViewId) -> Option<Self::Node> {
        ViewTree::remove(self, id)
    }

    #[inline]
    fn clear(&mut self) {
        ViewTree::clear(self);
    }

    #[inline]
    fn reserve(&mut self, additional: usize) {
        ViewTree::reserve(self, additional);
    }
}

impl TreeNav<ViewId> for ViewTree {
    const MAX_DEPTH: usize = 32;
    const AVG_CHILDREN: usize = 4;

    #[inline]
    fn parent(&self, id: ViewId) -> Option<ViewId> {
        ViewTree::parent(self, id)
    }

    #[inline]
    fn children(&self, id: ViewId) -> impl Iterator<Item = ViewId> + '_ {
        self.get(id)
            .map(|node| node.children().iter().copied())
            .into_iter()
            .flatten()
    }

    #[inline]
    fn ancestors(&self, start: ViewId) -> impl Iterator<Item = ViewId> + '_ {
        Ancestors::new(self, start)
    }

    #[inline]
    fn descendants(&self, root: ViewId) -> impl Iterator<Item = (ViewId, usize)> + '_ {
        DescendantsWithDepth::new(self, root)
    }

    #[inline]
    fn siblings(&self, id: ViewId) -> impl Iterator<Item = ViewId> + '_ {
        AllSiblings::new(self, id)
    }

    #[inline]
    fn child_count(&self, id: ViewId) -> usize {
        self.get(id).map(|node| node.children().len()).unwrap_or(0)
    }

    #[inline]
    fn has_children(&self, id: ViewId) -> bool {
        self.get(id)
            .map(|node| !node.children().is_empty())
            .unwrap_or(false)
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view_object::ViewObject;
    use crate::BuildContext;

    // Simple test ViewObject implementation
    #[derive(Debug)]
    struct TestView {
        #[allow(dead_code)]
        name: String,
    }

    impl ViewObject for TestView {
        fn mode(&self) -> ViewMode {
            ViewMode::Stateless
        }

        fn build(&mut self, _ctx: &dyn BuildContext) -> Option<Box<dyn ViewObject>> {
            None
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }

    #[test]
    fn test_insert_and_get() {
        let mut tree = ViewTree::new();

        let view = TestView {
            name: "test".to_string(),
        };
        let id = tree.insert(ViewNode::new(view, ViewMode::Stateless));

        assert!(tree.contains(id));
        assert_eq!(tree.len(), 1);

        let node = tree.get(id).unwrap();
        assert_eq!(node.mode(), ViewMode::Stateless);
    }

    #[test]
    fn test_tree_nav_parent_children() {
        let mut tree = ViewTree::new();

        let root_id = tree.insert(ViewNode::new(
            TestView {
                name: "root".to_string(),
            },
            ViewMode::Stateless,
        ));
        let child_id = tree.insert(ViewNode::new(
            TestView {
                name: "child".to_string(),
            },
            ViewMode::Stateless,
        ));

        tree.add_child(root_id, child_id);

        // Test parent navigation
        assert_eq!(tree.parent(child_id), Some(root_id));
        assert_eq!(tree.parent(root_id), None);

        // Test children navigation
        let children: Vec<_> = TreeNav::children(&tree, root_id).collect();
        assert_eq!(children, vec![child_id]);
    }

    #[test]
    fn test_tree_nav_ancestors() {
        let mut tree = ViewTree::new();

        let root_id = tree.insert(ViewNode::new(
            TestView {
                name: "root".to_string(),
            },
            ViewMode::Stateless,
        ));
        let child_id = tree.insert(ViewNode::new(
            TestView {
                name: "child".to_string(),
            },
            ViewMode::Stateless,
        ));
        let grandchild_id = tree.insert(ViewNode::new(
            TestView {
                name: "grandchild".to_string(),
            },
            ViewMode::Stateless,
        ));

        tree.add_child(root_id, child_id);
        tree.add_child(child_id, grandchild_id);

        // Test ancestors from grandchild
        let ancestors: Vec<_> = TreeNav::ancestors(&tree, grandchild_id).collect();
        assert_eq!(ancestors, vec![grandchild_id, child_id, root_id]);
    }

    #[test]
    fn test_tree_nav_descendants() {
        let mut tree = ViewTree::new();

        let root_id = tree.insert(ViewNode::new(
            TestView {
                name: "root".to_string(),
            },
            ViewMode::Stateless,
        ));
        let child1_id = tree.insert(ViewNode::new(
            TestView {
                name: "child1".to_string(),
            },
            ViewMode::Stateless,
        ));
        let child2_id = tree.insert(ViewNode::new(
            TestView {
                name: "child2".to_string(),
            },
            ViewMode::Stateless,
        ));

        tree.add_child(root_id, child1_id);
        tree.add_child(root_id, child2_id);

        // Test descendants from root
        let descendants: Vec<_> = TreeNav::descendants(&tree, root_id).collect();
        assert_eq!(descendants.len(), 3);
        assert_eq!(descendants[0], (root_id, 0));
        assert!(descendants.iter().any(|&(id, d)| id == child1_id && d == 1));
        assert!(descendants.iter().any(|&(id, d)| id == child2_id && d == 1));
    }

    #[test]
    fn test_node_ids_iterator() {
        let mut tree = ViewTree::new();

        let id1 = tree.insert(ViewNode::new(
            TestView {
                name: "one".to_string(),
            },
            ViewMode::Stateless,
        ));
        let id2 = tree.insert(ViewNode::new(
            TestView {
                name: "two".to_string(),
            },
            ViewMode::Stateless,
        ));

        let ids: Vec<_> = TreeRead::node_ids(&tree).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&id1));
        assert!(ids.contains(&id2));
    }

    #[test]
    fn test_remove_node() {
        let mut tree = ViewTree::new();

        let id = tree.insert(ViewNode::new(
            TestView {
                name: "test".to_string(),
            },
            ViewMode::Stateless,
        ));

        assert!(tree.contains(id));
        tree.remove(id);
        assert!(!tree.contains(id));
    }

    #[test]
    fn test_clear() {
        let mut tree = ViewTree::new();

        tree.insert(ViewNode::new(
            TestView {
                name: "one".to_string(),
            },
            ViewMode::Stateless,
        ));
        tree.insert(ViewNode::new(
            TestView {
                name: "two".to_string(),
            },
            ViewMode::Stateless,
        ));

        assert_eq!(tree.len(), 2);
        tree.clear();
        assert_eq!(tree.len(), 0);
        assert!(tree.is_empty());
    }
}
