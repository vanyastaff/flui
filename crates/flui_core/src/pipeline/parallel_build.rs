//! Parallel build implementation
//!
//! This module provides parallel execution of widget builds across independent subtrees.

#[cfg(feature = "parallel")]
use rayon::prelude::*;

use crate::element::ElementId;
use crate::element::ElementTree;
use parking_lot::RwLock;
use std::sync::Arc;

/// Minimum number of dirty elements to consider parallel execution
///
/// Below this threshold, sequential execution is faster due to lower overhead.
#[cfg(feature = "parallel")]
const MIN_PARALLEL_ELEMENTS: usize = 50;

/// Represents an independent subtree that can be built in parallel
#[derive(Debug, Clone)]
pub struct Subtree {
    /// Elements in this subtree, sorted by depth (parent before children)
    pub elements: Vec<(ElementId, usize)>,
}

impl Subtree {
    /// Create a new subtree with a root element
    fn new(root: (ElementId, usize)) -> Self {
        Self {
            elements: vec![root],
        }
    }

    /// Add an element to this subtree
    fn push(&mut self, element: (ElementId, usize)) {
        self.elements.push(element);
    }

    /// Get the root element (first element, lowest depth)
    fn root(&self) -> (ElementId, usize) {
        self.elements[0]
    }
}

/// Partition dirty elements into independent subtrees
///
/// # Algorithm
///
/// 1. Sort elements by depth (already done by caller)
/// 2. For each element, check if it's a descendant of any existing subtree root
/// 3. If descendant → add to that subtree
/// 4. If not descendant → start new independent subtree
///
/// # Parameters
///
/// - `dirty`: Sorted list of (ElementId, depth) pairs
/// - `tree`: Element tree for checking parent relationships
///
/// # Returns
///
/// Vec of subtrees, each containing elements that must be built sequentially
/// within the subtree, but different subtrees can be built in parallel.
///
/// # Example
///
/// ```text
/// Input:  [(1,0), (2,1), (3,1), (4,2), (5,2)]
/// Tree:   1 → 2 → 4
///         1 → 3 → 5
///
/// Output: [Subtree([1,2,4]), Subtree([3,5])]  ← Subtree starting from 3 is independent
/// ```
pub fn partition_subtrees(
    dirty: &[(ElementId, usize)],
    tree: &Arc<RwLock<ElementTree>>,
) -> Vec<Subtree> {
    if dirty.is_empty() {
        return Vec::new();
    }

    let tree_guard = tree.read();
    let mut subtrees: Vec<Subtree> = Vec::new();

    for &(element_id, depth) in dirty {
        let mut added = false;

        // Check if this element belongs to an existing subtree
        for subtree in &mut subtrees {
            let (root_id, _root_depth) = subtree.root();

            // Check if element_id is a descendant of root_id
            if is_descendant(&tree_guard, element_id, root_id) {
                subtree.push((element_id, depth));
                added = true;
                break;
            }
        }

        // Start new independent subtree if not added to existing one
        if !added {
            subtrees.push(Subtree::new((element_id, depth)));
        }
    }

    drop(tree_guard);
    subtrees
}

/// Check if `element_id` is a descendant of `ancestor_id`
///
/// Walks up the tree from element_id to root, checking if we encounter ancestor_id.
fn is_descendant(tree: &ElementTree, element_id: ElementId, ancestor_id: ElementId) -> bool {
    let mut current = element_id;

    // Walk up the tree
    while let Some(parent) = tree.parent(current) {
        if parent == ancestor_id {
            return true;
        }
        current = parent;
    }

    false
}

/// Rebuild dirty elements using parallel execution
///
/// # Strategy
///
/// 1. Partition elements into independent subtrees
/// 2. If few elements → fall back to sequential
/// 3. Otherwise → parallel execution with rayon
///
/// # Parameters
///
/// - `tree`: Element tree (thread-safe via Arc<RwLock>)
/// - `dirty`: Sorted list of (ElementId, depth) to rebuild
///
/// # Returns
///
/// Number of elements rebuilt
#[cfg(feature = "parallel")]
pub fn rebuild_dirty_parallel(
    tree: &Arc<RwLock<ElementTree>>,
    dirty: Vec<(ElementId, usize)>,
) -> usize {
    let element_count = dirty.len();

    // Fall back to sequential for small trees (overhead > benefit)
    if element_count < MIN_PARALLEL_ELEMENTS {
        #[cfg(debug_assertions)]
        tracing::debug!(
            "parallel_build: {} elements < threshold {}, using sequential",
            element_count,
            MIN_PARALLEL_ELEMENTS
        );

        return rebuild_sequential(tree, dirty);
    }

    // Partition into independent subtrees
    let subtrees = partition_subtrees(&dirty, tree);

    #[cfg(debug_assertions)]
    tracing::debug!(
        "parallel_build: partitioned {} elements into {} subtrees",
        element_count,
        subtrees.len()
    );

    // Parallel execution: each subtree on its own thread
    subtrees.par_iter().for_each(|subtree| {
        rebuild_subtree(tree, &subtree.elements);
    });

    element_count
}

/// Fallback for when parallel feature is disabled
#[cfg(not(feature = "parallel"))]
pub fn rebuild_dirty_parallel(
    tree: &Arc<RwLock<ElementTree>>,
    dirty: Vec<(ElementId, usize)>,
) -> usize {
    rebuild_sequential(tree, dirty)
}

/// Rebuild elements sequentially
///
/// Used as fallback when:
/// - parallel-build feature is disabled
/// - Element count < MIN_PARALLEL_ELEMENTS
fn rebuild_sequential(tree: &Arc<RwLock<ElementTree>>, dirty: Vec<(ElementId, usize)>) -> usize {
    let count = dirty.len();

    for (element_id, depth) in dirty {
        rebuild_element(tree, element_id, depth);
    }

    count
}

/// Rebuild a single subtree sequentially
///
/// Called from parallel context - each subtree runs on its own thread.
#[cfg(feature = "parallel")]
fn rebuild_subtree(tree: &Arc<RwLock<ElementTree>>, elements: &[(ElementId, usize)]) {
    for &(element_id, depth) in elements {
        rebuild_element(tree, element_id, depth);
    }
}

/// Rebuild a single element
///
/// This is the core rebuild logic, extracted for reuse by both
/// sequential and parallel implementations.
fn rebuild_element(tree: &Arc<RwLock<ElementTree>>, element_id: ElementId, depth: usize) {
    #[cfg(debug_assertions)]
    tracing::trace!(
        "rebuild_element: Processing element {:?} at depth {}",
        element_id,
        depth
    );

    // Get write lock for this specific element
    let tree_guard = tree.write();

    // Verify element still exists in tree
    let element = match tree_guard.get(element_id) {
        Some(elem) => elem,
        None => {
            tracing::error!(
                element_id = ?element_id,
                "Element marked dirty but not found in tree during parallel rebuild"
            );
            return;
        }
    };

    // Dispatch rebuild based on element type
    match element {
        crate::element::Element::Component(_comp) => {
            // TODO(Issue #2): Full View-based component rebuild
            #[cfg(debug_assertions)]
            tracing::debug!(
                "Component element {:?} rebuild deferred to parent (View integration pending)",
                element_id
            );
        }

        crate::element::Element::Render(_render) => {
            // RenderElements don't rebuild - they only relayout
            #[cfg(debug_assertions)]
            tracing::trace!("Render element {:?} skipped (rebuilds via layout)", element_id);
        }

        crate::element::Element::Provider(_provider) => {
            // TODO(Issue #2): Provider rebuild
            #[cfg(debug_assertions)]
            tracing::debug!(
                "Provider element {:?} rebuild (change propagation pending)",
                element_id
            );
        }
    }

    drop(tree_guard);

    #[cfg(debug_assertions)]
    tracing::trace!("Processed element {:?}", element_id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::{Element, RenderElement};
    use crate::render::LeafRender;

    fn create_test_tree() -> Arc<RwLock<ElementTree>> {
        let tree = Arc::new(RwLock::new(ElementTree::new()));

        // Create simple tree:
        //     1
        //    / \
        //   2   3
        //  /     \
        // 4       5
        //
        // Note: For testing subtree partitioning, we just need a tree structure.
        // We'll create RenderElements since they're simpler (no view/state requirements).

        let mut tree_guard = tree.write();

        // Helper to create a simple render element
        fn create_render_elem() -> Element {
            Element::Render(RenderElement::new(Box::new(LeafRender)))
        }

        // Root
        let mut root = create_render_elem();
        root.mount(None, Some(crate::foundation::Slot::new(0)));
        let id1 = tree_guard.insert(root);

        // Left subtree
        let mut child2 = create_render_elem();
        child2.mount(Some(id1), Some(crate::foundation::Slot::new(0)));
        let id2 = tree_guard.insert(child2);

        let mut child4 = create_render_elem();
        child4.mount(Some(id2), Some(crate::foundation::Slot::new(0)));
        let _id4 = tree_guard.insert(child4);

        // Right subtree
        let mut child3 = create_render_elem();
        child3.mount(Some(id1), Some(crate::foundation::Slot::new(1)));
        let id3 = tree_guard.insert(child3);

        let mut child5 = create_render_elem();
        child5.mount(Some(id3), Some(crate::foundation::Slot::new(0)));
        let _id5 = tree_guard.insert(child5);

        drop(tree_guard);
        tree
    }

    #[test]
    fn test_is_descendant() {
        let tree = create_test_tree();
        let tree_guard = tree.read();

        // Tree: 1 → 2 → 4
        //       1 → 3 → 5
        // IDs: 0, 1, 2, 3, 4

        let id1 = 0;
        let id2 = 1;
        let id3 = 3;
        let id4 = 2;
        let id5 = 4;

        // id4 is descendant of id2 and id1
        assert!(is_descendant(&tree_guard, id4, id2));
        assert!(is_descendant(&tree_guard, id4, id1));

        // id4 is NOT descendant of id3
        assert!(!is_descendant(&tree_guard, id4, id3));

        // id5 is descendant of id3 and id1
        assert!(is_descendant(&tree_guard, id5, id3));
        assert!(is_descendant(&tree_guard, id5, id1));

        // id5 is NOT descendant of id2
        assert!(!is_descendant(&tree_guard, id5, id2));
    }

    #[test]
    fn test_partition_subtrees_single() {
        let tree = create_test_tree();

        // All elements in one subtree (from root)
        let dirty = vec![(0, 0), (1, 1), (2, 2)];

        let subtrees = partition_subtrees(&dirty, &tree);

        assert_eq!(subtrees.len(), 1);
        assert_eq!(subtrees[0].elements.len(), 3);
    }

    #[test]
    fn test_partition_subtrees_independent() {
        let tree = create_test_tree();

        // Two independent subtrees
        let dirty = vec![(1, 1), (2, 2), (3, 1), (4, 2)];

        let subtrees = partition_subtrees(&dirty, &tree);

        // Should have 2 subtrees: one starting from id1 (with id2), one from id3 (with id4)
        assert_eq!(subtrees.len(), 2);
    }

    #[test]
    fn test_rebuild_sequential() {
        let tree = create_test_tree();

        let dirty = vec![(0, 0), (1, 1), (2, 2)];

        let count = rebuild_sequential(&tree, dirty);

        assert_eq!(count, 3);
    }

    #[cfg(feature = "parallel")]
    #[test]
    fn test_rebuild_parallel_small_tree() {
        let tree = create_test_tree();

        // Small tree should fall back to sequential
        let dirty = vec![(0, 0), (1, 1)];

        let count = rebuild_dirty_parallel(&tree, dirty);

        assert_eq!(count, 2);
    }
}
