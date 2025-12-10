//! RenderTree - Slab-based storage for RenderObjects
//!
//! This module implements the third tree of FLUI's four-tree architecture
//! (View, Element, Render, Layer). Following Flutter's pattern but simplified.
//!
//! # Architecture
//!
//! ```text
//! RenderTree
//!   ├─ nodes: Slab<RenderNode>
//!   └─ root: Option<RenderId>
//!
//! RenderNode (concrete struct, like LayerNode)
//!   ├─ parent: Option<RenderId>
//!   ├─ children: Vec<RenderId>
//!   ├─ render_object: Box<dyn RenderObject>
//!   ├─ lifecycle: RenderLifecycle
//!   ├─ cached_size: Option<Size>
//!   └─ element_id: Option<ElementId>
//! ```
//!
//! # flui-tree Integration
//!
//! RenderTree implements `TreeRead`, `TreeWrite`, and `TreeNav` from flui-tree,
//! enabling generic tree algorithms and visitors to work with RenderTree.

use slab::Slab;

use flui_tree::iter::{AllSiblings, Ancestors, DescendantsWithDepth};
use flui_tree::{TreeNav, TreeRead, TreeWrite};

use flui_foundation::{ElementId, RenderId};
use flui_types::Size;

use crate::{RenderLifecycle, RenderObject};

// ============================================================================
// RENDER NODE (Concrete struct, like LayerNode)
// ============================================================================

/// RenderNode - stores RenderObject with tree structure.
///
/// This is a concrete struct (not a trait) following the same pattern as LayerNode.
/// The RenderObject is stored as `Box<dyn RenderObject>` for type erasure.
#[derive(Debug)]
pub struct RenderNode {
    /// Parent in the render tree
    parent: Option<RenderId>,

    /// Children in the render tree
    children: Vec<RenderId>,

    /// The type-erased RenderObject
    render_object: Box<dyn RenderObject>,

    /// Current lifecycle state
    lifecycle: RenderLifecycle,

    /// Cached size from last layout
    cached_size: Option<Size>,

    /// Associated ElementId (for cross-tree references)
    element_id: Option<ElementId>,
}

impl RenderNode {
    /// Creates a new RenderNode with the given RenderObject.
    pub fn new<R: RenderObject + 'static>(object: R) -> Self {
        Self {
            parent: None,
            children: Vec::new(),
            render_object: Box::new(object),
            lifecycle: RenderLifecycle::Detached,
            cached_size: None,
            element_id: None,
        }
    }

    /// Creates a RenderNode from a boxed RenderObject.
    pub fn from_boxed(render_object: Box<dyn RenderObject>) -> Self {
        Self {
            parent: None,
            children: Vec::new(),
            render_object,
            lifecycle: RenderLifecycle::Detached,
            cached_size: None,
            element_id: None,
        }
    }

    /// Creates a RenderNode with an associated ElementId.
    pub fn with_element_id(mut self, element_id: ElementId) -> Self {
        self.element_id = Some(element_id);
        self
    }

    // ========== Tree Structure ==========

    /// Gets the parent RenderId.
    #[inline]
    pub fn parent(&self) -> Option<RenderId> {
        self.parent
    }

    /// Sets the parent RenderId.
    #[inline]
    pub fn set_parent(&mut self, parent: Option<RenderId>) {
        self.parent = parent;
    }

    /// Gets all children RenderIds.
    #[inline]
    pub fn children(&self) -> &[RenderId] {
        &self.children
    }

    /// Adds a child to this render node.
    #[inline]
    pub fn add_child(&mut self, child: RenderId) {
        self.children.push(child);
    }

    /// Removes a child from this render node.
    #[inline]
    pub fn remove_child(&mut self, child: RenderId) {
        self.children.retain(|&id| id != child);
    }

    // ========== RenderObject Access ==========

    /// Returns reference to the RenderObject.
    #[inline]
    pub fn render_object(&self) -> &dyn RenderObject {
        &*self.render_object
    }

    /// Returns mutable reference to the RenderObject.
    #[inline]
    pub fn render_object_mut(&mut self) -> &mut dyn RenderObject {
        &mut *self.render_object
    }

    // ========== Metadata ==========

    /// Gets the current lifecycle state.
    #[inline]
    pub fn lifecycle(&self) -> RenderLifecycle {
        self.lifecycle
    }

    /// Sets the lifecycle state.
    #[inline]
    pub fn set_lifecycle(&mut self, lifecycle: RenderLifecycle) {
        self.lifecycle = lifecycle;
    }

    /// Gets the cached size from last layout.
    #[inline]
    pub fn cached_size(&self) -> Option<Size> {
        self.cached_size
    }

    /// Sets the cached size.
    #[inline]
    pub fn set_cached_size(&mut self, size: Option<Size>) {
        self.cached_size = size;
    }

    /// Gets the associated ElementId.
    #[inline]
    pub fn element_id(&self) -> Option<ElementId> {
        self.element_id
    }

    /// Sets the associated ElementId.
    #[inline]
    pub fn set_element_id(&mut self, element_id: Option<ElementId>) {
        self.element_id = element_id;
    }
}

// ============================================================================
// RENDER TREE
// ============================================================================

/// RenderTree - Slab-based storage for render nodes.
///
/// This is the third of FLUI's four trees, storing RenderObjects.
///
/// # Thread Safety
///
/// RenderTree itself is not thread-safe. Use `Arc<RwLock<RenderTree>>`
/// for multi-threaded access.
#[derive(Debug)]
pub struct RenderTree {
    /// Slab storage for RenderNodes (0-based indexing internally)
    nodes: Slab<RenderNode>,

    /// Root RenderNode ID (None if tree is empty)
    root: Option<RenderId>,
}

impl RenderTree {
    /// Creates a new empty RenderTree.
    pub fn new() -> Self {
        Self {
            nodes: Slab::new(),
            root: None,
        }
    }

    /// Creates a RenderTree with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            nodes: Slab::with_capacity(capacity),
            root: None,
        }
    }

    // ========== Root Management ==========

    /// Get the root RenderNode ID.
    #[inline]
    pub fn root(&self) -> Option<RenderId> {
        self.root
    }

    /// Set the root RenderNode ID.
    #[inline]
    pub fn set_root(&mut self, root: Option<RenderId>) {
        self.root = root;
    }

    // ========== Basic Operations ==========

    /// Checks if a RenderNode exists in the tree.
    #[inline]
    pub fn contains(&self, id: RenderId) -> bool {
        self.nodes.contains(id.get() - 1)
    }

    /// Returns the number of RenderNodes in the tree.
    #[inline]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns true if the tree is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Returns the parent of a node.
    #[inline]
    pub fn parent(&self, id: RenderId) -> Option<RenderId> {
        self.get(id).and_then(|node| node.parent())
    }

    /// Inserts a RenderNode into the tree.
    ///
    /// # Slab Offset Pattern
    ///
    /// Applies +1 offset: `nodes.insert()` returns 0 → `RenderId(1)`
    pub fn insert(&mut self, node: RenderNode) -> RenderId {
        let slab_index = self.nodes.insert(node);
        RenderId::new(slab_index + 1)
    }

    /// Inserts a RenderObject into the tree, creating a RenderNode.
    pub fn insert_object<R: RenderObject + 'static>(&mut self, object: R) -> RenderId {
        self.insert(RenderNode::new(object))
    }

    /// Returns a reference to a RenderNode.
    ///
    /// # Slab Offset Pattern
    ///
    /// Applies -1 offset: `RenderId(1)` → `nodes[0]`
    #[inline]
    pub fn get(&self, id: RenderId) -> Option<&RenderNode> {
        self.nodes.get(id.get() - 1)
    }

    /// Returns a mutable reference to a RenderNode.
    #[inline]
    pub fn get_mut(&mut self, id: RenderId) -> Option<&mut RenderNode> {
        self.nodes.get_mut(id.get() - 1)
    }

    /// Removes a RenderNode from the tree.
    ///
    /// **Note:** This does NOT remove children. Caller must handle tree cleanup.
    pub fn remove(&mut self, id: RenderId) -> Option<RenderNode> {
        if self.root == Some(id) {
            self.root = None;
        }
        self.nodes.try_remove(id.get() - 1)
    }

    /// Clears all nodes from the tree.
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.root = None;
    }

    /// Reserves capacity for at least `additional` more nodes.
    pub fn reserve(&mut self, additional: usize) {
        self.nodes.reserve(additional);
    }

    // ========== Tree Operations ==========

    /// Adds a child to a parent RenderNode.
    pub fn add_child(&mut self, parent_id: RenderId, child_id: RenderId) {
        if let Some(parent) = self.get_mut(parent_id) {
            parent.add_child(child_id);
        }
        if let Some(child) = self.get_mut(child_id) {
            child.set_parent(Some(parent_id));
        }
    }

    /// Removes a child from a parent RenderNode.
    pub fn remove_child(&mut self, parent_id: RenderId, child_id: RenderId) {
        if let Some(parent) = self.get_mut(parent_id) {
            parent.remove_child(child_id);
        }
        if let Some(child) = self.get_mut(child_id) {
            child.set_parent(None);
        }
    }

    /// Returns an iterator over slab entries (index, node).
    #[inline]
    pub fn iter_slab(&self) -> slab::Iter<'_, RenderNode> {
        self.nodes.iter()
    }

    /// Returns a mutable iterator over slab entries.
    #[inline]
    pub fn iter_slab_mut(&mut self) -> slab::IterMut<'_, RenderNode> {
        self.nodes.iter_mut()
    }
}

impl Default for RenderTree {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// TREE READ IMPLEMENTATION
// ============================================================================

impl TreeRead<RenderId> for RenderTree {
    type Node = RenderNode;

    const DEFAULT_CAPACITY: usize = 64;
    const INLINE_THRESHOLD: usize = 16;

    #[inline]
    fn get(&self, id: RenderId) -> Option<&Self::Node> {
        RenderTree::get(self, id)
    }

    #[inline]
    fn contains(&self, id: RenderId) -> bool {
        RenderTree::contains(self, id)
    }

    #[inline]
    fn len(&self) -> usize {
        RenderTree::len(self)
    }

    #[inline]
    fn node_ids(&self) -> impl Iterator<Item = RenderId> + '_ {
        self.iter_slab().map(|(index, _)| RenderId::new(index + 1))
    }
}

// ============================================================================
// TREE WRITE IMPLEMENTATION
// ============================================================================

impl TreeWrite<RenderId> for RenderTree {
    #[inline]
    fn get_mut(&mut self, id: RenderId) -> Option<&mut Self::Node> {
        RenderTree::get_mut(self, id)
    }

    #[inline]
    fn insert(&mut self, node: Self::Node) -> RenderId {
        RenderTree::insert(self, node)
    }

    #[inline]
    fn remove(&mut self, id: RenderId) -> Option<Self::Node> {
        RenderTree::remove(self, id)
    }

    #[inline]
    fn clear(&mut self) {
        RenderTree::clear(self)
    }

    #[inline]
    fn reserve(&mut self, additional: usize) {
        RenderTree::reserve(self, additional)
    }
}

// ============================================================================
// TREE NAV IMPLEMENTATION
// ============================================================================

impl TreeNav<RenderId> for RenderTree {
    const MAX_DEPTH: usize = 32;
    const AVG_CHILDREN: usize = 4;

    #[inline]
    fn parent(&self, id: RenderId) -> Option<RenderId> {
        RenderTree::parent(self, id)
    }

    #[inline]
    fn children(&self, id: RenderId) -> impl Iterator<Item = RenderId> + '_ {
        self.get(id)
            .map(|node| node.children().iter().copied())
            .into_iter()
            .flatten()
    }

    #[inline]
    fn ancestors(&self, start: RenderId) -> impl Iterator<Item = RenderId> + '_ {
        Ancestors::new(self, start)
    }

    #[inline]
    fn descendants(&self, root: RenderId) -> impl Iterator<Item = (RenderId, usize)> + '_ {
        DescendantsWithDepth::new(self, root)
    }

    #[inline]
    fn siblings(&self, id: RenderId) -> impl Iterator<Item = RenderId> + '_ {
        AllSiblings::new(self, id)
    }

    #[inline]
    fn child_count(&self, id: RenderId) -> usize {
        self.get(id).map(|node| node.children().len()).unwrap_or(0)
    }

    #[inline]
    fn has_children(&self, id: RenderId) -> bool {
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
    use crate::RenderObject;

    // Simple test RenderObject (minimal - no layout/paint methods needed)
    #[derive(Debug)]
    struct TestRenderObject {
        name: String,
    }

    impl RenderObject for TestRenderObject {
        fn debug_name(&self) -> &'static str {
            "TestRenderObject"
        }
    }

    #[test]
    fn test_render_node_new() {
        let obj = TestRenderObject {
            name: "test".into(),
        };
        let node = RenderNode::new(obj);

        assert!(node.parent().is_none());
        assert!(node.children().is_empty());
        assert_eq!(node.lifecycle(), RenderLifecycle::Detached);
        assert!(node.cached_size().is_none());
        assert!(node.element_id().is_none());
    }

    #[test]
    fn test_render_tree_insert() {
        let mut tree = RenderTree::new();

        let obj = TestRenderObject {
            name: "root".into(),
        };
        let id = tree.insert(RenderNode::new(obj));

        assert_eq!(id.get(), 1);
        assert!(tree.contains(id));
        assert_eq!(tree.len(), 1);
    }

    #[test]
    fn test_render_tree_parent_child() {
        let mut tree = RenderTree::new();

        let parent_id = tree.insert_object(TestRenderObject {
            name: "parent".into(),
        });
        let child_id = tree.insert_object(TestRenderObject {
            name: "child".into(),
        });

        tree.add_child(parent_id, child_id);

        assert_eq!(tree.get(child_id).unwrap().parent(), Some(parent_id));
        assert_eq!(tree.get(parent_id).unwrap().children(), &[child_id]);
    }

    #[test]
    fn test_render_tree_remove() {
        let mut tree = RenderTree::new();

        let id = tree.insert_object(TestRenderObject {
            name: "test".into(),
        });
        assert!(tree.contains(id));

        let removed = tree.remove(id);
        assert!(removed.is_some());
        assert!(!tree.contains(id));
    }

    // ========== TreeRead/TreeWrite/TreeNav Tests ==========

    fn make_node() -> RenderNode {
        RenderNode::new(TestRenderObject {
            name: "test".into(),
        })
    }

    #[test]
    fn test_tree_read_get() {
        let mut tree = RenderTree::new();
        let id = tree.insert(make_node());

        let node: Option<&RenderNode> = TreeRead::get(&tree, id);
        assert!(node.is_some());
    }

    #[test]
    fn test_tree_read_contains() {
        let mut tree = RenderTree::new();
        let id = tree.insert(make_node());

        assert!(TreeRead::contains(&tree, id));
        assert!(!TreeRead::contains(&tree, RenderId::new(999)));
    }

    #[test]
    fn test_tree_read_len() {
        let mut tree = RenderTree::new();
        assert_eq!(TreeRead::<RenderId>::len(&tree), 0);

        tree.insert(make_node());
        assert_eq!(TreeRead::<RenderId>::len(&tree), 1);
    }

    #[test]
    fn test_tree_write_insert_remove() {
        let mut tree = RenderTree::new();

        let id: RenderId = TreeWrite::insert(&mut tree, make_node());
        assert!(TreeRead::contains(&tree, id));

        let removed: Option<RenderNode> = TreeWrite::remove(&mut tree, id);
        assert!(removed.is_some());
        assert!(!TreeRead::contains(&tree, id));
    }

    #[test]
    fn test_tree_nav_parent() {
        let mut tree = RenderTree::new();
        let parent_id = tree.insert(make_node());
        let child_id = tree.insert(make_node());

        tree.add_child(parent_id, child_id);

        assert_eq!(TreeNav::parent(&tree, child_id), Some(parent_id));
        assert_eq!(TreeNav::parent(&tree, parent_id), None);
    }

    #[test]
    fn test_tree_nav_children() {
        let mut tree = RenderTree::new();
        let parent_id = tree.insert(make_node());
        let child1_id = tree.insert(make_node());
        let child2_id = tree.insert(make_node());

        tree.add_child(parent_id, child1_id);
        tree.add_child(parent_id, child2_id);

        let children: Vec<_> = TreeNav::children(&tree, parent_id).collect();
        assert_eq!(children.len(), 2);
        assert!(children.contains(&child1_id));
        assert!(children.contains(&child2_id));
    }

    #[test]
    fn test_tree_nav_ancestors() {
        let mut tree = RenderTree::new();
        let root_id = tree.insert(make_node());
        let child_id = tree.insert(make_node());
        let grandchild_id = tree.insert(make_node());

        tree.add_child(root_id, child_id);
        tree.add_child(child_id, grandchild_id);

        let ancestors: Vec<_> = TreeNav::ancestors(&tree, grandchild_id).collect();
        assert_eq!(ancestors, vec![grandchild_id, child_id, root_id]);
    }

    #[test]
    fn test_tree_nav_descendants() {
        let mut tree = RenderTree::new();
        let root_id = tree.insert(make_node());
        let child_id = tree.insert(make_node());
        let grandchild_id = tree.insert(make_node());

        tree.add_child(root_id, child_id);
        tree.add_child(child_id, grandchild_id);

        let descendants: Vec<_> = TreeNav::descendants(&tree, root_id).collect();
        assert_eq!(descendants.len(), 3);
        assert_eq!(descendants[0], (root_id, 0));
        assert_eq!(descendants[1], (child_id, 1));
        assert_eq!(descendants[2], (grandchild_id, 2));
    }

    #[test]
    fn test_tree_nav_siblings() {
        let mut tree = RenderTree::new();
        let parent_id = tree.insert(make_node());
        let child1_id = tree.insert(make_node());
        let child2_id = tree.insert(make_node());
        let child3_id = tree.insert(make_node());

        tree.add_child(parent_id, child1_id);
        tree.add_child(parent_id, child2_id);
        tree.add_child(parent_id, child3_id);

        let siblings: Vec<_> = TreeNav::siblings(&tree, child2_id).collect();
        assert_eq!(siblings.len(), 2);
        assert!(siblings.contains(&child1_id));
        assert!(siblings.contains(&child3_id));
    }

    #[test]
    fn test_tree_nav_child_count() {
        let mut tree = RenderTree::new();
        let parent_id = tree.insert(make_node());
        let child1_id = tree.insert(make_node());
        let child2_id = tree.insert(make_node());

        assert_eq!(TreeNav::child_count(&tree, parent_id), 0);

        tree.add_child(parent_id, child1_id);
        assert_eq!(TreeNav::child_count(&tree, parent_id), 1);

        tree.add_child(parent_id, child2_id);
        assert_eq!(TreeNav::child_count(&tree, parent_id), 2);
    }

    #[test]
    fn test_tree_nav_depth() {
        let mut tree = RenderTree::new();
        let root_id = tree.insert(make_node());
        let child_id = tree.insert(make_node());
        let grandchild_id = tree.insert(make_node());

        tree.add_child(root_id, child_id);
        tree.add_child(child_id, grandchild_id);

        assert_eq!(TreeNav::depth(&tree, root_id), 0);
        assert_eq!(TreeNav::depth(&tree, child_id), 1);
        assert_eq!(TreeNav::depth(&tree, grandchild_id), 2);
    }
}
