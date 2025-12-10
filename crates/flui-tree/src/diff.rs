//! Tree diffing for reconciliation.
//!
//! This module provides types and algorithms for computing differences
//! between tree states, useful for efficient UI reconciliation.
//!
//! # Overview
//!
//! ```text
//! Old Tree:              New Tree:              Diff:
//!
//! A                      A                      Keep(A)
//! ├── B                  ├── B                  Keep(B)
//! │   └── D              │   └── E              Remove(D), Insert(E)
//! └── C                  ├── D                  Move(D, parent=A, idx=1)
//!                        └── C                  Keep(C)
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_tree::{TreeDiff, DiffOp, TreeNav};
//!
//! // Compare two tree states
//! let diff = TreeDiff::compute(&old_tree, &new_tree, root_id);
//!
//! // Apply operations
//! for op in diff.ops() {
//!     match op {
//!         DiffOp::Insert { id, parent, index } => { /* mount new */ }
//!         DiffOp::Remove { id } => { /* unmount */ }
//!         DiffOp::Move { id, new_parent, new_index } => { /* reposition */ }
//!         DiffOp::Update { id } => { /* rebuild */ }
//!         DiffOp::Keep { id } => { /* no-op */ }
//!     }
//! }
//! ```

use std::collections::{HashMap, HashSet};
use std::fmt;

use smallvec::SmallVec;

use flui_foundation::Identifier;

use crate::traits::TreeNav;

// ============================================================================
// DIFF OPERATION
// ============================================================================

/// A single diff operation representing a change between tree states.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DiffOp<I: Identifier> {
    /// Node was inserted (exists in new tree, not in old).
    Insert {
        /// ID of the inserted node.
        id: I,
        /// Parent in new tree.
        parent: I,
        /// Index within parent's children.
        index: usize,
    },

    /// Node was removed (exists in old tree, not in new).
    Remove {
        /// ID of the removed node.
        id: I,
        /// Former parent (for cleanup).
        old_parent: Option<I>,
    },

    /// Node was moved to different parent or position.
    Move {
        /// ID of the moved node.
        id: I,
        /// Old parent.
        old_parent: I,
        /// Old index.
        old_index: usize,
        /// New parent.
        new_parent: I,
        /// New index.
        new_index: usize,
    },

    /// Node exists in both trees but may have changed data.
    ///
    /// The actual comparison of node data is left to the caller,
    /// as diff only compares structure (IDs and positions).
    Update {
        /// ID of the potentially updated node.
        id: I,
    },

    /// Node is unchanged (same ID, same parent, same position).
    Keep {
        /// ID of the unchanged node.
        id: I,
    },
}

impl<I: Identifier> DiffOp<I> {
    /// Returns the node ID this operation affects.
    #[inline]
    #[must_use]
    pub fn id(&self) -> I {
        match self {
            Self::Insert { id, .. }
            | Self::Remove { id, .. }
            | Self::Move { id, .. }
            | Self::Update { id }
            | Self::Keep { id } => *id,
        }
    }

    /// Returns true if this is an Insert operation.
    #[inline]
    #[must_use]
    pub fn is_insert(&self) -> bool {
        matches!(self, Self::Insert { .. })
    }

    /// Returns true if this is a Remove operation.
    #[inline]
    #[must_use]
    pub fn is_remove(&self) -> bool {
        matches!(self, Self::Remove { .. })
    }

    /// Returns true if this is a Move operation.
    #[inline]
    #[must_use]
    pub fn is_move(&self) -> bool {
        matches!(self, Self::Move { .. })
    }

    /// Returns true if this is an Update operation.
    #[inline]
    #[must_use]
    pub fn is_update(&self) -> bool {
        matches!(self, Self::Update { .. })
    }

    /// Returns true if this is a Keep operation.
    #[inline]
    #[must_use]
    pub fn is_keep(&self) -> bool {
        matches!(self, Self::Keep { .. })
    }

    /// Returns true if this operation represents a structural change.
    ///
    /// Structural changes are Insert, Remove, and Move.
    #[inline]
    #[must_use]
    pub fn is_structural(&self) -> bool {
        matches!(
            self,
            Self::Insert { .. } | Self::Remove { .. } | Self::Move { .. }
        )
    }
}

impl<I: Identifier> fmt::Display for DiffOp<I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Insert { id, parent, index } => {
                write!(f, "Insert({id} into {parent} at {index})")
            }
            Self::Remove { id, old_parent } => match old_parent {
                Some(p) => write!(f, "Remove({id} from {p})"),
                None => write!(f, "Remove({id})"),
            },
            Self::Move {
                id,
                new_parent,
                new_index,
                ..
            } => {
                write!(f, "Move({id} to {new_parent} at {new_index})")
            }
            Self::Update { id } => write!(f, "Update({id})"),
            Self::Keep { id } => write!(f, "Keep({id})"),
        }
    }
}

// ============================================================================
// TREE DIFF
// ============================================================================

/// Result of comparing two tree states.
///
/// Contains a sequence of operations that transform the old tree into the new tree.
///
/// # Operation Order
///
/// Operations are ordered for safe application:
/// 1. Removes (deepest first - post-order)
/// 2. Moves
/// 3. Inserts (shallowest first - pre-order)
/// 4. Updates/Keeps (any order)
#[derive(Debug, Clone)]
pub struct TreeDiff<I: Identifier> {
    /// Diff operations in application order.
    ops: Vec<DiffOp<I>>,
    /// Statistics about the diff.
    stats: DiffStats,
}

/// Statistics about a tree diff.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiffStats {
    /// Number of inserted nodes.
    pub inserts: usize,
    /// Number of removed nodes.
    pub removes: usize,
    /// Number of moved nodes.
    pub moves: usize,
    /// Number of updated nodes.
    pub updates: usize,
    /// Number of kept nodes.
    pub keeps: usize,
}

impl DiffStats {
    /// Returns total number of operations.
    #[inline]
    #[must_use]
    pub fn total(&self) -> usize {
        self.inserts + self.removes + self.moves + self.updates + self.keeps
    }

    /// Returns true if there are no changes.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inserts == 0 && self.removes == 0 && self.moves == 0 && self.updates == 0
    }

    /// Returns number of structural changes (insert + remove + move).
    #[inline]
    #[must_use]
    pub fn structural_changes(&self) -> usize {
        self.inserts + self.removes + self.moves
    }
}

impl<I: Identifier> TreeDiff<I> {
    /// Creates an empty diff.
    #[inline]
    #[must_use]
    pub fn empty() -> Self {
        Self {
            ops: Vec::new(),
            stats: DiffStats::default(),
        }
    }

    /// Computes diff between two trees starting from given roots.
    ///
    /// # Arguments
    ///
    /// * `old_tree` - The old (current) tree state
    /// * `new_tree` - The new (target) tree state
    /// * `old_root` - Root node in old tree
    /// * `new_root` - Root node in new tree
    ///
    /// # Returns
    ///
    /// A `TreeDiff` containing operations to transform old into new.
    pub fn compute<T>(old_tree: &T, new_tree: &T, old_root: I, new_root: I) -> Self
    where
        T: TreeNav<I>,
    {
        let mut differ = TreeDiffer::new(old_tree, new_tree);
        differ.diff(old_root, new_root);
        differ.into_diff()
    }

    /// Computes diff assuming same root ID in both trees.
    #[inline]
    pub fn compute_same_root<T>(old_tree: &T, new_tree: &T, root: I) -> Self
    where
        T: TreeNav<I>,
    {
        Self::compute(old_tree, new_tree, root, root)
    }

    /// Returns the diff operations.
    #[inline]
    #[must_use]
    pub fn ops(&self) -> &[DiffOp<I>] {
        &self.ops
    }

    /// Returns diff statistics.
    #[inline]
    #[must_use]
    pub fn stats(&self) -> DiffStats {
        self.stats
    }

    /// Returns true if there are no changes.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.stats.is_empty()
    }

    /// Returns number of operations.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    /// Consumes the diff and returns the operations.
    #[inline]
    #[must_use]
    pub fn into_ops(self) -> Vec<DiffOp<I>> {
        self.ops
    }

    /// Returns iterator over operations.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &DiffOp<I>> {
        self.ops.iter()
    }

    /// Returns only structural operations (Insert, Remove, Move).
    #[inline]
    pub fn structural_ops(&self) -> impl Iterator<Item = &DiffOp<I>> {
        self.ops.iter().filter(|op| op.is_structural())
    }

    /// Returns only Insert operations.
    #[inline]
    pub fn inserts(&self) -> impl Iterator<Item = &DiffOp<I>> {
        self.ops.iter().filter(|op| op.is_insert())
    }

    /// Returns only Remove operations.
    #[inline]
    pub fn removes(&self) -> impl Iterator<Item = &DiffOp<I>> {
        self.ops.iter().filter(|op| op.is_remove())
    }

    /// Returns only Move operations.
    #[inline]
    pub fn moves(&self) -> impl Iterator<Item = &DiffOp<I>> {
        self.ops.iter().filter(|op| op.is_move())
    }
}

impl<I: Identifier> Default for TreeDiff<I> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<I: Identifier> IntoIterator for TreeDiff<I> {
    type Item = DiffOp<I>;
    type IntoIter = std::vec::IntoIter<DiffOp<I>>;

    fn into_iter(self) -> Self::IntoIter {
        self.ops.into_iter()
    }
}

impl<'a, I: Identifier> IntoIterator for &'a TreeDiff<I> {
    type Item = &'a DiffOp<I>;
    type IntoIter = std::slice::Iter<'a, DiffOp<I>>;

    fn into_iter(self) -> Self::IntoIter {
        self.ops.iter()
    }
}

// ============================================================================
// NODE INFO (internal)
// ============================================================================

/// Information about a node's position in the tree.
#[derive(Debug, Clone, Copy)]
struct NodeInfo<I: Identifier> {
    parent: Option<I>,
    index: usize,
}

// ============================================================================
// TREE DIFFER (internal algorithm)
// ============================================================================

/// Internal struct that performs the actual diffing.
struct TreeDiffer<'a, I: Identifier, T: TreeNav<I>> {
    old_tree: &'a T,
    new_tree: &'a T,

    /// Old tree: id -> (parent, index)
    old_positions: HashMap<I, NodeInfo<I>>,
    /// New tree: id -> (parent, index)
    new_positions: HashMap<I, NodeInfo<I>>,

    /// IDs that exist in old tree
    old_ids: HashSet<I>,
    /// IDs that exist in new tree
    new_ids: HashSet<I>,

    /// Collected operations
    ops: Vec<DiffOp<I>>,
    stats: DiffStats,
}

impl<'a, I: Identifier, T: TreeNav<I>> TreeDiffer<'a, I, T> {
    fn new(old_tree: &'a T, new_tree: &'a T) -> Self {
        Self {
            old_tree,
            new_tree,
            old_positions: HashMap::new(),
            new_positions: HashMap::new(),
            old_ids: HashSet::new(),
            new_ids: HashSet::new(),
            ops: Vec::new(),
            stats: DiffStats::default(),
        }
    }

    fn diff(&mut self, old_root: I, new_root: I) {
        // Phase 1: Collect all node positions from both trees
        self.collect_positions(old_root, new_root);

        // Phase 2: Classify each node
        self.classify_nodes();

        // Phase 3: Sort operations for safe application
        self.sort_operations();
    }

    fn collect_positions(&mut self, old_root: I, new_root: I) {
        // Collect from old tree
        Self::collect_tree_positions(
            self.old_tree,
            old_root,
            &mut self.old_positions,
            &mut self.old_ids,
        );

        // Collect from new tree
        Self::collect_tree_positions(
            self.new_tree,
            new_root,
            &mut self.new_positions,
            &mut self.new_ids,
        );
    }

    fn collect_tree_positions(
        tree: &T,
        root: I,
        positions: &mut HashMap<I, NodeInfo<I>>,
        ids: &mut HashSet<I>,
    ) {
        // Use stack for DFS traversal
        let mut stack: SmallVec<[(I, Option<I>); 32]> = SmallVec::new();
        stack.push((root, None));

        while let Some((id, parent)) = stack.pop() {
            if !tree.contains(id) {
                continue;
            }

            ids.insert(id);

            // Find index within parent
            let index = if let Some(p) = parent {
                tree.children(p).position(|c| c == id).unwrap_or(0)
            } else {
                0
            };

            positions.insert(id, NodeInfo { parent, index });

            // Push children
            for child in tree.children(id) {
                stack.push((child, Some(id)));
            }
        }
    }

    fn classify_nodes(&mut self) {
        // Nodes only in old tree -> Remove
        for &id in &self.old_ids {
            if !self.new_ids.contains(&id) {
                let old_info = self.old_positions.get(&id);
                self.ops.push(DiffOp::Remove {
                    id,
                    old_parent: old_info.and_then(|info| info.parent),
                });
                self.stats.removes += 1;
            }
        }

        // Nodes only in new tree -> Insert
        for &id in &self.new_ids {
            if !self.old_ids.contains(&id) {
                if let Some(new_info) = self.new_positions.get(&id) {
                    if let Some(parent) = new_info.parent {
                        self.ops.push(DiffOp::Insert {
                            id,
                            parent,
                            index: new_info.index,
                        });
                        self.stats.inserts += 1;
                    }
                    // Root nodes without parent are handled specially
                }
            }
        }

        // Nodes in both trees -> Keep, Move, or Update
        for &id in &self.old_ids {
            if self.new_ids.contains(&id) {
                let old_info = self.old_positions.get(&id).copied();
                let new_info = self.new_positions.get(&id).copied();

                if let (Some(old), Some(new)) = (old_info, new_info) {
                    let parent_changed = old.parent != new.parent;
                    let index_changed = old.index != new.index;

                    if parent_changed || index_changed {
                        // Node moved
                        if let (Some(old_parent), Some(new_parent)) = (old.parent, new.parent) {
                            self.ops.push(DiffOp::Move {
                                id,
                                old_parent,
                                old_index: old.index,
                                new_parent,
                                new_index: new.index,
                            });
                            self.stats.moves += 1;
                        } else {
                            // Root node position changed - treat as update
                            self.ops.push(DiffOp::Update { id });
                            self.stats.updates += 1;
                        }
                    } else {
                        // Same position - keep (but might need data update)
                        self.ops.push(DiffOp::Keep { id });
                        self.stats.keeps += 1;
                    }
                } else {
                    // Shouldn't happen, but treat as update
                    self.ops.push(DiffOp::Update { id });
                    self.stats.updates += 1;
                }
            }
        }
    }

    fn sort_operations(&mut self) {
        // Sort for safe application order:
        // 1. Removes (to free up positions)
        // 2. Moves
        // 3. Inserts
        // 4. Updates/Keeps
        self.ops.sort_by(|a, b| {
            let order_a = match a {
                DiffOp::Remove { .. } => 0,
                DiffOp::Move { .. } => 1,
                DiffOp::Insert { .. } => 2,
                DiffOp::Update { .. } | DiffOp::Keep { .. } => 3,
            };
            let order_b = match b {
                DiffOp::Remove { .. } => 0,
                DiffOp::Move { .. } => 1,
                DiffOp::Insert { .. } => 2,
                DiffOp::Update { .. } | DiffOp::Keep { .. } => 3,
            };
            order_a.cmp(&order_b)
        });
    }

    fn into_diff(self) -> TreeDiff<I> {
        TreeDiff {
            ops: self.ops,
            stats: self.stats,
        }
    }
}

// ============================================================================
// CHILD DIFF (for incremental updates)
// ============================================================================

/// Diff result for a single parent's children.
///
/// This is a simpler, more focused diff for when you only need
/// to compare children of a specific node (common in reconciliation).
#[derive(Debug, Clone)]
pub struct ChildDiff<I: Identifier> {
    /// Parent node ID.
    parent: I,
    /// Operations on children.
    ops: SmallVec<[ChildOp<I>; 8]>,
}

/// Operation on a child during reconciliation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChildOp<I: Identifier> {
    /// Keep child at same position.
    Keep { id: I, index: usize },
    /// Insert new child.
    Insert { id: I, index: usize },
    /// Remove child.
    Remove { id: I },
    /// Move child to new index.
    Reorder {
        id: I,
        old_index: usize,
        new_index: usize,
    },
}

impl<I: Identifier> ChildDiff<I> {
    /// Creates a new child diff for the given parent.
    #[inline]
    #[must_use]
    pub fn new(parent: I) -> Self {
        Self {
            parent,
            ops: SmallVec::new(),
        }
    }

    /// Computes diff between old and new children lists.
    ///
    /// # Arguments
    ///
    /// * `parent` - Parent node ID
    /// * `old_children` - Iterator over old child IDs (in order)
    /// * `new_children` - Iterator over new child IDs (in order)
    pub fn compute(
        parent: I,
        old_children: impl IntoIterator<Item = I>,
        new_children: impl IntoIterator<Item = I>,
    ) -> Self {
        let old: Vec<I> = old_children.into_iter().collect();
        let new: Vec<I> = new_children.into_iter().collect();

        let old_set: HashSet<I> = old.iter().copied().collect();
        let new_set: HashSet<I> = new.iter().copied().collect();

        let mut ops = SmallVec::new();

        // Find removes (in old but not new)
        for &id in &old {
            if !new_set.contains(&id) {
                ops.push(ChildOp::Remove { id });
            }
        }

        // Build old index map
        let old_indices: HashMap<I, usize> = old
            .iter()
            .copied()
            .enumerate()
            .map(|(i, id)| (id, i))
            .collect();

        // Process new children
        for (new_index, &id) in new.iter().enumerate() {
            if !old_set.contains(&id) {
                // New child
                ops.push(ChildOp::Insert {
                    id,
                    index: new_index,
                });
            } else if let Some(&old_index) = old_indices.get(&id) {
                if old_index == new_index {
                    // Same position
                    ops.push(ChildOp::Keep {
                        id,
                        index: new_index,
                    });
                } else {
                    // Moved
                    ops.push(ChildOp::Reorder {
                        id,
                        old_index,
                        new_index,
                    });
                }
            }
        }

        Self { parent, ops }
    }

    /// Returns the parent node ID.
    #[inline]
    #[must_use]
    pub fn parent(&self) -> I {
        self.parent
    }

    /// Returns the operations.
    #[inline]
    #[must_use]
    pub fn ops(&self) -> &[ChildOp<I>] {
        &self.ops
    }

    /// Returns true if no changes.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ops.iter().all(|op| matches!(op, ChildOp::Keep { .. }))
    }

    /// Returns number of operations.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    /// Returns number of structural changes.
    #[inline]
    #[must_use]
    pub fn changes(&self) -> usize {
        self.ops
            .iter()
            .filter(|op| !matches!(op, ChildOp::Keep { .. }))
            .count()
    }
}

impl<I: Identifier> ChildOp<I> {
    /// Returns the node ID.
    #[inline]
    #[must_use]
    pub fn id(&self) -> I {
        match self {
            Self::Keep { id, .. }
            | Self::Insert { id, .. }
            | Self::Remove { id }
            | Self::Reorder { id, .. } => *id,
        }
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iter::{Ancestors, DescendantsWithDepth};
    use flui_foundation::ElementId;

    // Simple test tree implementation
    struct TestTree {
        nodes: HashMap<ElementId, (Option<ElementId>, Vec<ElementId>)>,
    }

    impl TestTree {
        fn new() -> Self {
            Self {
                nodes: HashMap::new(),
            }
        }

        fn add(&mut self, id: ElementId, parent: Option<ElementId>) {
            self.nodes.insert(id, (parent, Vec::new()));
            if let Some(p) = parent {
                if let Some((_, children)) = self.nodes.get_mut(&p) {
                    children.push(id);
                }
            }
        }
    }

    impl crate::traits::TreeRead<ElementId> for TestTree {
        type Node = ();

        fn get(&self, _id: ElementId) -> Option<&Self::Node> {
            Some(&())
        }

        fn contains(&self, id: ElementId) -> bool {
            self.nodes.contains_key(&id)
        }

        fn len(&self) -> usize {
            self.nodes.len()
        }

        fn node_ids(&self) -> impl Iterator<Item = ElementId> + '_ {
            self.nodes.keys().copied()
        }
    }

    impl TreeNav<ElementId> for TestTree {
        fn parent(&self, id: ElementId) -> Option<ElementId> {
            self.nodes.get(&id).and_then(|(p, _)| *p)
        }

        fn children(&self, id: ElementId) -> impl Iterator<Item = ElementId> + '_ {
            self.nodes
                .get(&id)
                .map(|(_, c)| c.iter().copied())
                .into_iter()
                .flatten()
        }

        fn ancestors(&self, start: ElementId) -> impl Iterator<Item = ElementId> + '_ {
            Ancestors::new(self, start)
        }

        fn descendants(&self, root: ElementId) -> impl Iterator<Item = (ElementId, usize)> + '_ {
            DescendantsWithDepth::new(self, root)
        }

        fn siblings(&self, id: ElementId) -> impl Iterator<Item = ElementId> + '_ {
            let parent_id = self.parent(id);
            parent_id
                .into_iter()
                .flat_map(move |pid| self.children(pid).filter(move |&cid| cid != id))
        }
    }

    // === DIFF OP TESTS ===

    #[test]
    fn test_diff_op_id() {
        let op: DiffOp<ElementId> = DiffOp::Insert {
            id: ElementId::new(1),
            parent: ElementId::new(2),
            index: 0,
        };
        assert_eq!(op.id(), ElementId::new(1));
    }

    #[test]
    fn test_diff_op_predicates() {
        let insert: DiffOp<ElementId> = DiffOp::Insert {
            id: ElementId::new(1),
            parent: ElementId::new(2),
            index: 0,
        };
        assert!(insert.is_insert());
        assert!(insert.is_structural());

        let keep: DiffOp<ElementId> = DiffOp::Keep {
            id: ElementId::new(1),
        };
        assert!(keep.is_keep());
        assert!(!keep.is_structural());
    }

    #[test]
    fn test_diff_op_display() {
        let op: DiffOp<ElementId> = DiffOp::Insert {
            id: ElementId::new(1),
            parent: ElementId::new(2),
            index: 3,
        };
        let s = format!("{}", op);
        assert!(s.contains("Insert"));
    }

    // === TREE DIFF TESTS ===

    #[test]
    fn test_empty_diff() {
        let diff = TreeDiff::<ElementId>::empty();
        assert!(diff.is_empty());
        assert_eq!(diff.len(), 0);
    }

    #[test]
    fn test_identical_trees() {
        let mut tree = TestTree::new();
        tree.add(ElementId::new(1), None);
        tree.add(ElementId::new(2), Some(ElementId::new(1)));
        tree.add(ElementId::new(3), Some(ElementId::new(1)));

        let diff = TreeDiff::compute_same_root(&tree, &tree, ElementId::new(1));

        assert_eq!(diff.stats().inserts, 0);
        assert_eq!(diff.stats().removes, 0);
        assert_eq!(diff.stats().moves, 0);
        assert_eq!(diff.stats().keeps, 3);
    }

    #[test]
    fn test_insert_node() {
        let mut old_tree = TestTree::new();
        old_tree.add(ElementId::new(1), None);

        let mut new_tree = TestTree::new();
        new_tree.add(ElementId::new(1), None);
        new_tree.add(ElementId::new(2), Some(ElementId::new(1)));

        let diff = TreeDiff::compute_same_root(&old_tree, &new_tree, ElementId::new(1));

        assert_eq!(diff.stats().inserts, 1);
        assert_eq!(diff.stats().removes, 0);
        assert_eq!(diff.stats().keeps, 1);
    }

    #[test]
    fn test_remove_node() {
        let mut old_tree = TestTree::new();
        old_tree.add(ElementId::new(1), None);
        old_tree.add(ElementId::new(2), Some(ElementId::new(1)));

        let mut new_tree = TestTree::new();
        new_tree.add(ElementId::new(1), None);

        let diff = TreeDiff::compute_same_root(&old_tree, &new_tree, ElementId::new(1));

        assert_eq!(diff.stats().inserts, 0);
        assert_eq!(diff.stats().removes, 1);
        assert_eq!(diff.stats().keeps, 1);
    }

    #[test]
    fn test_move_node() {
        // Old: 1 -> 2 -> 3
        let mut old_tree = TestTree::new();
        old_tree.add(ElementId::new(1), None);
        old_tree.add(ElementId::new(2), Some(ElementId::new(1)));
        old_tree.add(ElementId::new(3), Some(ElementId::new(2)));

        // New: 1 -> 2, 1 -> 3 (3 moved from under 2 to under 1)
        let mut new_tree = TestTree::new();
        new_tree.add(ElementId::new(1), None);
        new_tree.add(ElementId::new(2), Some(ElementId::new(1)));
        new_tree.add(ElementId::new(3), Some(ElementId::new(1)));

        let diff = TreeDiff::compute_same_root(&old_tree, &new_tree, ElementId::new(1));

        assert_eq!(diff.stats().moves, 1);
        assert!(diff.moves().any(|op| {
            matches!(op, DiffOp::Move { id, new_parent, .. }
                if *id == ElementId::new(3) && *new_parent == ElementId::new(1))
        }));
    }

    #[test]
    fn test_diff_stats() {
        let stats = DiffStats {
            inserts: 2,
            removes: 1,
            moves: 1,
            updates: 0,
            keeps: 5,
        };

        assert_eq!(stats.total(), 9);
        assert_eq!(stats.structural_changes(), 4);
        assert!(!stats.is_empty());
    }

    #[test]
    fn test_diff_iteration() {
        let mut old_tree = TestTree::new();
        old_tree.add(ElementId::new(1), None);

        let mut new_tree = TestTree::new();
        new_tree.add(ElementId::new(1), None);
        new_tree.add(ElementId::new(2), Some(ElementId::new(1)));

        let diff = TreeDiff::compute_same_root(&old_tree, &new_tree, ElementId::new(1));

        let ops: Vec<_> = diff.iter().collect();
        assert!(!ops.is_empty());

        let inserts: Vec<_> = diff.inserts().collect();
        assert_eq!(inserts.len(), 1);
    }

    // === CHILD DIFF TESTS ===

    #[test]
    fn test_child_diff_no_changes() {
        let old = vec![ElementId::new(1), ElementId::new(2), ElementId::new(3)];
        let new = vec![ElementId::new(1), ElementId::new(2), ElementId::new(3)];

        let diff = ChildDiff::compute(ElementId::new(100), old, new);

        assert!(diff.is_empty());
        assert_eq!(diff.changes(), 0);
    }

    #[test]
    fn test_child_diff_insert() {
        let old = vec![ElementId::new(1), ElementId::new(2)];
        let new = vec![ElementId::new(1), ElementId::new(2), ElementId::new(3)];

        let diff = ChildDiff::compute(ElementId::new(100), old, new);

        assert_eq!(diff.changes(), 1);
        assert!(diff
            .ops()
            .iter()
            .any(|op| matches!(op, ChildOp::Insert { id, .. } if *id == ElementId::new(3))));
    }

    #[test]
    fn test_child_diff_remove() {
        let old = vec![ElementId::new(1), ElementId::new(2), ElementId::new(3)];
        let new = vec![ElementId::new(1), ElementId::new(2)];

        let diff = ChildDiff::compute(ElementId::new(100), old, new);

        assert!(diff
            .ops()
            .iter()
            .any(|op| matches!(op, ChildOp::Remove { id } if *id == ElementId::new(3))));
    }

    #[test]
    fn test_child_diff_reorder() {
        let old = vec![ElementId::new(1), ElementId::new(2), ElementId::new(3)];
        let new = vec![ElementId::new(3), ElementId::new(1), ElementId::new(2)];

        let diff = ChildDiff::compute(ElementId::new(100), old, new);

        // All items reordered
        let reorders: Vec<_> = diff
            .ops()
            .iter()
            .filter(|op| matches!(op, ChildOp::Reorder { .. }))
            .collect();
        assert!(!reorders.is_empty());
    }

    #[test]
    fn test_child_op_id() {
        let op = ChildOp::Keep {
            id: ElementId::new(5),
            index: 0,
        };
        assert_eq!(op.id(), ElementId::new(5));
    }
}
