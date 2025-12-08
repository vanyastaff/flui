//! Tree trait implementations for LayerTree
//!
//! This module implements `TreeRead<LayerId>` and `TreeNav<LayerId>` from flui-tree,
//! enabling generic tree algorithms and visitors to work with LayerTree.

use flui_foundation::LayerId;
use flui_tree::iter::{Ancestors, DescendantsWithDepth};
use flui_tree::{TreeNav, TreeRead};

use super::layer_tree::{ConcreteLayerNode, LayerNode, LayerTree};

// ============================================================================
// TREE READ IMPLEMENTATION
// ============================================================================

impl TreeRead<LayerId> for LayerTree {
    type Node = ConcreteLayerNode;
    type NodeIter<'a> = LayerIdIter<'a>;

    const DEFAULT_CAPACITY: usize = 64;
    const INLINE_THRESHOLD: usize = 16;

    #[inline]
    fn get(&self, id: LayerId) -> Option<&Self::Node> {
        LayerTree::get(self, id)
    }

    #[inline]
    fn contains(&self, id: LayerId) -> bool {
        LayerTree::contains(self, id)
    }

    #[inline]
    fn len(&self) -> usize {
        LayerTree::len(self)
    }

    #[inline]
    fn node_ids(&self) -> Self::NodeIter<'_> {
        LayerIdIter::new(self)
    }
}

// ============================================================================
// TREE NAV IMPLEMENTATION
// ============================================================================

impl TreeNav<LayerId> for LayerTree {
    type ChildrenIter<'a> = ChildrenIter<'a>;
    type AncestorsIter<'a> = Ancestors<'a, LayerId, Self>;
    type DescendantsIter<'a> = DescendantsWithDepth<'a, LayerId, Self>;
    type SiblingsIter<'a> = SiblingsIter<'a>;

    const MAX_DEPTH: usize = 32;
    const AVG_CHILDREN: usize = 4;

    #[inline]
    fn parent(&self, id: LayerId) -> Option<LayerId> {
        LayerTree::parent(self, id)
    }

    #[inline]
    fn children(&self, id: LayerId) -> Self::ChildrenIter<'_> {
        ChildrenIter::new(self, id)
    }

    #[inline]
    fn ancestors(&self, start: LayerId) -> Self::AncestorsIter<'_> {
        Ancestors::new(self, start)
    }

    #[inline]
    fn descendants(&self, root: LayerId) -> Self::DescendantsIter<'_> {
        DescendantsWithDepth::new(self, root)
    }

    #[inline]
    fn siblings(&self, id: LayerId) -> Self::SiblingsIter<'_> {
        SiblingsIter::new(self, id)
    }

    #[inline]
    fn child_count(&self, id: LayerId) -> usize {
        self.get(id).map(|node| node.children().len()).unwrap_or(0)
    }

    #[inline]
    fn has_children(&self, id: LayerId) -> bool {
        self.get(id)
            .map(|node| !node.children().is_empty())
            .unwrap_or(false)
    }
}

// ============================================================================
// CUSTOM ITERATORS
// ============================================================================

/// Iterator over all LayerIds in the tree.
pub struct LayerIdIter<'a> {
    ids: Vec<LayerId>,
    index: usize,
    _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> LayerIdIter<'a> {
    fn new(tree: &'a LayerTree) -> Self {
        // Collect all IDs upfront - simple and safe
        let ids: Vec<LayerId> = tree.layer_ids().collect();
        Self {
            ids,
            index: 0,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<'a> Iterator for LayerIdIter<'a> {
    type Item = LayerId;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.ids.len() {
            let id = self.ids[self.index];
            self.index += 1;
            Some(id)
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.ids.len().saturating_sub(self.index);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for LayerIdIter<'_> {}

/// Iterator over children of a layer node.
pub struct ChildrenIter<'a> {
    children: Option<&'a [LayerId]>,
    index: usize,
}

impl<'a> ChildrenIter<'a> {
    fn new(tree: &'a LayerTree, id: LayerId) -> Self {
        Self {
            children: tree.children(id),
            index: 0,
        }
    }
}

impl<'a> Iterator for ChildrenIter<'a> {
    type Item = LayerId;

    fn next(&mut self) -> Option<Self::Item> {
        let children = self.children?;
        if self.index < children.len() {
            let id = children[self.index];
            self.index += 1;
            Some(id)
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self
            .children
            .map(|c| c.len().saturating_sub(self.index))
            .unwrap_or(0);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for ChildrenIter<'_> {}

/// Iterator over siblings of a layer node.
pub struct SiblingsIter<'a> {
    tree: &'a LayerTree,
    children: Option<&'a [LayerId]>,
    index: usize,
    exclude_id: LayerId,
}

impl<'a> SiblingsIter<'a> {
    fn new(tree: &'a LayerTree, id: LayerId) -> Self {
        let children = tree
            .parent(id)
            .and_then(|parent_id| tree.children(parent_id));

        Self {
            tree,
            children,
            index: 0,
            exclude_id: id,
        }
    }
}

impl<'a> Iterator for SiblingsIter<'a> {
    type Item = LayerId;

    fn next(&mut self) -> Option<Self::Item> {
        let children = self.children?;
        while self.index < children.len() {
            let id = children[self.index];
            self.index += 1;
            if id != self.exclude_id {
                return Some(id);
            }
        }
        None
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer::{CanvasLayer, Layer};

    #[test]
    fn test_tree_read_get() {
        let mut tree = LayerTree::new();
        let id = tree.insert(Layer::Canvas(CanvasLayer::new()));

        // Use TreeRead trait method
        let node: Option<&ConcreteLayerNode> = TreeRead::get(&tree, id);
        assert!(node.is_some());
    }

    #[test]
    fn test_tree_read_contains() {
        let mut tree = LayerTree::new();
        let id = tree.insert(Layer::Canvas(CanvasLayer::new()));

        assert!(TreeRead::contains(&tree, id));
        assert!(!TreeRead::contains(&tree, LayerId::new(999)));
    }

    #[test]
    fn test_tree_read_len() {
        let mut tree = LayerTree::new();
        assert_eq!(TreeRead::<LayerId>::len(&tree), 0);

        let _ = tree.insert(Layer::Canvas(CanvasLayer::new()));
        assert_eq!(TreeRead::<LayerId>::len(&tree), 1);

        let _ = tree.insert(Layer::Canvas(CanvasLayer::new()));
        assert_eq!(TreeRead::<LayerId>::len(&tree), 2);
    }

    #[test]
    fn test_tree_nav_parent() {
        let mut tree = LayerTree::new();
        let parent_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let child_id = tree.insert(Layer::Canvas(CanvasLayer::new()));

        tree.add_child(parent_id, child_id);

        assert_eq!(TreeNav::parent(&tree, child_id), Some(parent_id));
        assert_eq!(TreeNav::parent(&tree, parent_id), None);
    }

    #[test]
    fn test_tree_nav_children() {
        let mut tree = LayerTree::new();
        let parent_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let child1_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let child2_id = tree.insert(Layer::Canvas(CanvasLayer::new()));

        tree.add_child(parent_id, child1_id);
        tree.add_child(parent_id, child2_id);

        let children: Vec<_> = TreeNav::children(&tree, parent_id).collect();
        assert_eq!(children.len(), 2);
        assert!(children.contains(&child1_id));
        assert!(children.contains(&child2_id));
    }

    #[test]
    fn test_tree_nav_ancestors() {
        let mut tree = LayerTree::new();
        let root_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let child_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let grandchild_id = tree.insert(Layer::Canvas(CanvasLayer::new()));

        tree.add_child(root_id, child_id);
        tree.add_child(child_id, grandchild_id);

        let ancestors: Vec<_> = TreeNav::ancestors(&tree, grandchild_id).collect();
        assert_eq!(ancestors, vec![grandchild_id, child_id, root_id]);
    }

    #[test]
    fn test_tree_nav_descendants() {
        let mut tree = LayerTree::new();
        let root_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let child_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let grandchild_id = tree.insert(Layer::Canvas(CanvasLayer::new()));

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
        let mut tree = LayerTree::new();
        let parent_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let child1_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let child2_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let child3_id = tree.insert(Layer::Canvas(CanvasLayer::new()));

        tree.add_child(parent_id, child1_id);
        tree.add_child(parent_id, child2_id);
        tree.add_child(parent_id, child3_id);

        let siblings: Vec<_> = TreeNav::siblings(&tree, child2_id).collect();
        assert_eq!(siblings.len(), 2);
        assert!(siblings.contains(&child1_id));
        assert!(siblings.contains(&child3_id));
        assert!(!siblings.contains(&child2_id));
    }

    #[test]
    fn test_tree_nav_child_count() {
        let mut tree = LayerTree::new();
        let parent_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let child1_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let child2_id = tree.insert(Layer::Canvas(CanvasLayer::new()));

        assert_eq!(TreeNav::child_count(&tree, parent_id), 0);

        tree.add_child(parent_id, child1_id);
        assert_eq!(TreeNav::child_count(&tree, parent_id), 1);

        tree.add_child(parent_id, child2_id);
        assert_eq!(TreeNav::child_count(&tree, parent_id), 2);
    }

    #[test]
    fn test_tree_nav_has_children() {
        let mut tree = LayerTree::new();
        let parent_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let child_id = tree.insert(Layer::Canvas(CanvasLayer::new()));

        assert!(!TreeNav::has_children(&tree, parent_id));

        tree.add_child(parent_id, child_id);
        assert!(TreeNav::has_children(&tree, parent_id));
    }

    #[test]
    fn test_tree_nav_is_leaf() {
        let mut tree = LayerTree::new();
        let parent_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let child_id = tree.insert(Layer::Canvas(CanvasLayer::new()));

        assert!(TreeNav::is_leaf(&tree, parent_id));

        tree.add_child(parent_id, child_id);
        assert!(!TreeNav::is_leaf(&tree, parent_id));
        assert!(TreeNav::is_leaf(&tree, child_id));
    }

    #[test]
    fn test_tree_nav_is_root() {
        let mut tree = LayerTree::new();
        let parent_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let child_id = tree.insert(Layer::Canvas(CanvasLayer::new()));

        tree.add_child(parent_id, child_id);

        assert!(TreeNav::is_root(&tree, parent_id));
        assert!(!TreeNav::is_root(&tree, child_id));
    }

    #[test]
    fn test_tree_nav_find_root() {
        let mut tree = LayerTree::new();
        let root_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let child_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let grandchild_id = tree.insert(Layer::Canvas(CanvasLayer::new()));

        tree.add_child(root_id, child_id);
        tree.add_child(child_id, grandchild_id);

        assert_eq!(TreeNav::find_root(&tree, grandchild_id), root_id);
        assert_eq!(TreeNav::find_root(&tree, child_id), root_id);
        assert_eq!(TreeNav::find_root(&tree, root_id), root_id);
    }

    #[test]
    fn test_tree_nav_depth() {
        let mut tree = LayerTree::new();
        let root_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let child_id = tree.insert(Layer::Canvas(CanvasLayer::new()));
        let grandchild_id = tree.insert(Layer::Canvas(CanvasLayer::new()));

        tree.add_child(root_id, child_id);
        tree.add_child(child_id, grandchild_id);

        assert_eq!(TreeNav::depth(&tree, root_id), 0);
        assert_eq!(TreeNav::depth(&tree, child_id), 1);
        assert_eq!(TreeNav::depth(&tree, grandchild_id), 2);
    }
}
