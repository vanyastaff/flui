//! Stateful cursor for interactive tree navigation.
//!
//! `TreeCursor` provides a "current position" abstraction over tree navigation,
//! with optional history for backtracking. Unlike iterators which consume
//! elements sequentially, cursors can move freely in any direction.
//!
//! # Overview
//!
//! ```text
//! Cursor Operations:
//!
//!          go_parent()
//!               ↑
//!               │
//! go_prev() ← [Current] → go_next()
//!               │
//!               ↓
//!          go_child(n)
//! ```
//!
//! # When to Use
//!
//! Use cursors when:
//! - Building tree editors (`DevTools`, IDEs)
//! - Implementing focus/keyboard navigation
//! - Interactive tree exploration
//! - Algorithms that backtrack
//!
//! Use iterators when:
//! - Sequential, one-way traversal
//! - Functional transformations
//! - Collecting/filtering nodes
//!
//! # Example
//!
//! ```
//! # use flui_tree::{Ancestors, TreeCursor, TreeNav, TreeRead};
//! # use flui_foundation::ElementId;
//! # struct N { parent: Option<ElementId>, children: Vec<ElementId> }
//! # struct T(Vec<Option<N>>);
//! # impl T { fn ins(&mut self, p: Option<ElementId>) -> ElementId {
//! #     let id = ElementId::new(self.0.len()+1);
//! #     self.0.push(Some(N { parent: p, children: vec![] }));
//! #     if let Some(pid) = p { self.0[pid.get()-1].as_mut().unwrap().children.push(id); }
//! #     id
//! # }}
//! # impl TreeRead<ElementId> for T {
//! #     type Node = N;
//! #     fn get(&self, id: ElementId) -> Option<&N> { self.0.get(id.get()-1)?.as_ref() }
//! #     fn len(&self) -> usize { self.0.iter().flatten().count() }
//! #     fn node_ids(&self) -> impl Iterator<Item = ElementId> + '_ {
//! #         (0..self.0.len()).filter_map(|i| if self.0[i].is_some() { Some(ElementId::new(i+1)) } else { None })
//! #     }
//! # }
//! # impl TreeNav<ElementId> for T {
//! #     fn parent(&self, id: ElementId) -> Option<ElementId> { self.get(id)?.parent }
//! #     fn children(&self, id: ElementId) -> impl Iterator<Item = ElementId> + '_ {
//! #         self.get(id).into_iter().flat_map(|n| n.children.iter().copied())
//! #     }
//! #     fn ancestors(&self, s: ElementId) -> impl Iterator<Item = ElementId> + '_ { Ancestors::new(self, s) }
//! #     fn descendants(&self, r: ElementId) -> impl Iterator<Item = (ElementId, usize)> + '_ {
//! #         flui_tree::DescendantsWithDepth::new(self, r)
//! #     }
//! #     fn siblings(&self, id: ElementId) -> impl Iterator<Item = ElementId> + '_ {
//! #         self.parent(id).into_iter().flat_map(move |p| self.children(p).filter(move |&c| c != id))
//! #     }
//! # }
//! # let mut tree = T(vec![]);
//! # let root_id = tree.ins(None);
//! # let child = tree.ins(Some(root_id));
//! # let grandchild0 = tree.ins(Some(child));
//! # let grandchild1 = tree.ins(Some(child));
//! let mut cursor = TreeCursor::new(&tree, root_id);
//!
//! // Navigate down
//! while cursor.go_first_child() {
//!     // Descended deeper into the tree
//! }
//!
//! // Navigate back up
//! while cursor.go_parent() {
//!     // Ascended toward the root
//! }
//! assert_eq!(cursor.current(), root_id);
//!
//! // With history for undo
//! let mut cursor = TreeCursor::with_history(&tree, root_id, 10);
//! cursor.go_child(0);
//! cursor.go_child(1);
//! cursor.go_back();  // Returns to previous position
//! ```

use std::fmt;

use smallvec::SmallVec;

use flui_foundation::Identifier;

use super::path::{IndexPath, TreePath};
use crate::traits::TreeNav;

// ============================================================================
// CURSOR HISTORY
// ============================================================================

/// Internal history stack for cursor positions.
#[derive(Debug, Clone)]
struct CursorHistory<I: Identifier> {
    positions: SmallVec<[I; 16]>,
    max_size: usize,
}

impl<I: Identifier> CursorHistory<I> {
    /// Creates a new history with the given maximum size.
    fn new(max_size: usize) -> Self {
        Self {
            positions: SmallVec::new(),
            max_size,
        }
    }

    /// Pushes a position to history, dropping oldest if at capacity.
    fn push(&mut self, position: I) {
        if self.positions.len() >= self.max_size {
            self.positions.remove(0);
        }
        self.positions.push(position);
    }

    /// Pops the most recent position.
    fn pop(&mut self) -> Option<I> {
        self.positions.pop()
    }

    /// Peeks at the most recent position without removing.
    fn peek(&self) -> Option<I> {
        self.positions.last().copied()
    }

    /// Clears all history.
    fn clear(&mut self) {
        self.positions.clear();
    }

    /// Returns the number of positions in history.
    fn len(&self) -> usize {
        self.positions.len()
    }
}

// ============================================================================
// TREE CURSOR
// ============================================================================

/// A stateful cursor for navigating through a tree.
///
/// `TreeCursor` provides a "current position" abstraction over tree navigation,
/// with optional history for backtracking.
///
/// # Type Parameters
///
/// - `'a`: Lifetime of the tree reference
/// - `T`: Tree type implementing `TreeNav<I>`
/// - `I`: Identifier type for nodes
///
/// # History
///
/// Cursors optionally maintain a position history stack:
///
/// ```
/// # use flui_tree::{Ancestors, TreeCursor, TreeNav, TreeRead};
/// # use flui_foundation::ElementId;
/// # struct N { parent: Option<ElementId>, children: Vec<ElementId> }
/// # struct T(Vec<Option<N>>);
/// # impl T { fn ins(&mut self, p: Option<ElementId>) -> ElementId {
/// #     let id = ElementId::new(self.0.len()+1);
/// #     self.0.push(Some(N { parent: p, children: vec![] }));
/// #     if let Some(pid) = p { self.0[pid.get()-1].as_mut().unwrap().children.push(id); }
/// #     id
/// # }}
/// # impl TreeRead<ElementId> for T {
/// #     type Node = N;
/// #     fn get(&self, id: ElementId) -> Option<&N> { self.0.get(id.get()-1)?.as_ref() }
/// #     fn len(&self) -> usize { self.0.iter().flatten().count() }
/// #     fn node_ids(&self) -> impl Iterator<Item = ElementId> + '_ {
/// #         (0..self.0.len()).filter_map(|i| if self.0[i].is_some() { Some(ElementId::new(i+1)) } else { None })
/// #     }
/// # }
/// # impl TreeNav<ElementId> for T {
/// #     fn parent(&self, id: ElementId) -> Option<ElementId> { self.get(id)?.parent }
/// #     fn children(&self, id: ElementId) -> impl Iterator<Item = ElementId> + '_ {
/// #         self.get(id).into_iter().flat_map(|n| n.children.iter().copied())
/// #     }
/// #     fn ancestors(&self, s: ElementId) -> impl Iterator<Item = ElementId> + '_ { Ancestors::new(self, s) }
/// #     fn descendants(&self, r: ElementId) -> impl Iterator<Item = (ElementId, usize)> + '_ {
/// #         flui_tree::DescendantsWithDepth::new(self, r)
/// #     }
/// #     fn siblings(&self, id: ElementId) -> impl Iterator<Item = ElementId> + '_ {
/// #         self.parent(id).into_iter().flat_map(move |p| self.children(p).filter(move |&c| c != id))
/// #     }
/// # }
/// # let mut tree = T(vec![]);
/// # let root = tree.ins(None);
/// # let child0 = tree.ins(Some(root));
/// # let gc0 = tree.ins(Some(child0));
/// # let gc1 = tree.ins(Some(child0));
/// let mut cursor = TreeCursor::with_history(&tree, root, 10);
/// cursor.go_child(0);  // root -> child0
/// cursor.go_child(1);  // child0 -> grandchild1
/// cursor.go_back();    // grandchild1 -> child0
/// cursor.go_back();    // child0 -> root
/// assert_eq!(cursor.current(), root);
/// ```
///
/// # Thread Safety
///
/// `TreeCursor` borrows the tree immutably, allowing concurrent read access.
/// The cursor itself is not `Sync` but can be `Send` if the tree is.
pub struct TreeCursor<'a, T, I>
where
    T: TreeNav<I>,
    I: Identifier,
{
    /// Reference to the tree.
    tree: &'a T,
    /// Current position in the tree.
    current: I,
    /// Cached depth (updated on navigation).
    depth: usize,
    /// Optional history for backtracking.
    history: Option<CursorHistory<I>>,
}

impl<'a, T, I> TreeCursor<'a, T, I>
where
    T: TreeNav<I>,
    I: Identifier,
{
    // === CONSTRUCTORS ===

    /// Creates a new cursor at the given position.
    ///
    /// The cursor starts without history. Use `with_history` for backtracking support.
    #[must_use]
    pub fn new(tree: &'a T, position: I) -> Self {
        let depth = Self::compute_depth(tree, position);
        Self {
            tree,
            current: position,
            depth,
            history: None,
        }
    }

    /// Creates a cursor with position history (for backtracking).
    ///
    /// `max_history` limits the history stack size (older entries are dropped).
    #[must_use]
    pub fn with_history(tree: &'a T, position: I, max_history: usize) -> Self {
        let depth = Self::compute_depth(tree, position);
        Self {
            tree,
            current: position,
            depth,
            history: Some(CursorHistory::new(max_history)),
        }
    }

    /// Creates a cursor at the root of the subtree containing `node`.
    #[must_use]
    pub fn at_root(tree: &'a T, node: I) -> Self {
        let mut current = node;
        while let Some(parent) = tree.parent(current) {
            current = parent;
        }
        Self::new(tree, current)
    }

    // === INTERNAL HELPERS ===

    /// Computes depth by walking to root.
    fn compute_depth(tree: &T, position: I) -> usize {
        let mut depth = 0;
        let mut current = position;
        while let Some(parent) = tree.parent(current) {
            depth += 1;
            current = parent;
        }
        depth
    }

    /// Saves current position to history before moving.
    fn save_to_history(&mut self) {
        if let Some(ref mut history) = self.history {
            history.push(self.current);
        }
    }

    // === STATE ACCESSORS ===

    /// Returns the current node ID.
    #[inline]
    #[must_use]
    pub fn current(&self) -> I {
        self.current
    }

    /// Returns the current depth (0 = root of navigation start).
    #[inline]
    #[must_use]
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Returns a reference to the underlying tree.
    #[inline]
    #[must_use]
    pub fn tree(&self) -> &'a T {
        self.tree
    }

    /// Returns the path from root to current position.
    #[must_use]
    pub fn path(&self) -> TreePath<I> {
        TreePath::from_node(self.tree, self.current)
    }

    /// Returns the index path from root to current position.
    #[must_use]
    pub fn index_path(&self) -> IndexPath {
        IndexPath::from_node(self.tree, self.current)
    }

    /// Returns true if cursor has history enabled.
    #[inline]
    #[must_use]
    pub fn has_history(&self) -> bool {
        self.history.is_some()
    }

    /// Returns the number of positions in history.
    #[inline]
    #[must_use]
    pub fn history_len(&self) -> usize {
        self.history.as_ref().map_or(0, CursorHistory::len)
    }

    // === POSITION QUERIES ===

    /// Returns true if current position is a root (no parent).
    #[inline]
    #[must_use]
    pub fn is_at_root(&self) -> bool {
        self.tree.parent(self.current).is_none()
    }

    /// Returns true if current position is a leaf (no children).
    #[must_use]
    pub fn is_at_leaf(&self) -> bool {
        self.tree.children(self.current).next().is_none()
    }

    /// Returns true if current has a next sibling.
    #[must_use]
    pub fn has_next_sibling(&self) -> bool {
        self.get_next_sibling().is_some()
    }

    /// Returns true if current has a previous sibling.
    #[must_use]
    pub fn has_prev_sibling(&self) -> bool {
        self.get_prev_sibling().is_some()
    }

    /// Returns the number of children at current position.
    #[must_use]
    pub fn child_count(&self) -> usize {
        self.tree.children(self.current).count()
    }

    /// Returns the index of current node among its siblings.
    #[must_use]
    pub fn sibling_index(&self) -> Option<usize> {
        let parent = self.tree.parent(self.current)?;
        self.tree.children(parent).position(|c| c == self.current)
    }

    // === INTERNAL NAVIGATION HELPERS ===

    /// Gets the next sibling ID without moving.
    fn get_next_sibling(&self) -> Option<I> {
        let parent = self.tree.parent(self.current)?;
        let mut found = false;
        for child in self.tree.children(parent) {
            if found {
                return Some(child);
            }
            if child == self.current {
                found = true;
            }
        }
        None
    }

    /// Gets the previous sibling ID without moving.
    fn get_prev_sibling(&self) -> Option<I> {
        let parent = self.tree.parent(self.current)?;
        let mut prev = None;
        for child in self.tree.children(parent) {
            if child == self.current {
                return prev;
            }
            prev = Some(child);
        }
        None
    }

    // === NAVIGATION ===

    /// Moves to the parent node.
    ///
    /// Returns `true` if move succeeded, `false` if already at root.
    #[must_use]
    pub fn go_parent(&mut self) -> bool {
        if let Some(parent) = self.tree.parent(self.current) {
            self.save_to_history();
            self.current = parent;
            self.depth = self.depth.saturating_sub(1);
            true
        } else {
            false
        }
    }

    /// Moves to the nth child (0-indexed).
    ///
    /// Returns `true` if move succeeded, `false` if index out of bounds.
    #[must_use]
    pub fn go_child(&mut self, index: usize) -> bool {
        if let Some(child) = self.tree.children(self.current).nth(index) {
            self.save_to_history();
            self.current = child;
            self.depth += 1;
            true
        } else {
            false
        }
    }

    /// Moves to the first child.
    ///
    /// Returns `true` if move succeeded, `false` if no children.
    #[inline]
    #[must_use]
    pub fn go_first_child(&mut self) -> bool {
        self.go_child(0)
    }

    /// Moves to the last child.
    ///
    /// Returns `true` if move succeeded, `false` if no children.
    #[must_use]
    pub fn go_last_child(&mut self) -> bool {
        let children: Vec<_> = self.tree.children(self.current).collect();
        if children.is_empty() {
            return false;
        }

        self.save_to_history();
        self.current = children[children.len() - 1];
        self.depth += 1;
        true
    }

    /// Moves to the next sibling.
    ///
    /// Returns `true` if move succeeded, `false` if no next sibling.
    #[must_use]
    pub fn go_next_sibling(&mut self) -> bool {
        if let Some(next) = self.get_next_sibling() {
            self.save_to_history();
            self.current = next;
            // Depth stays the same for siblings
            true
        } else {
            false
        }
    }

    /// Moves to the previous sibling.
    ///
    /// Returns `true` if move succeeded, `false` if no previous sibling.
    #[must_use]
    pub fn go_prev_sibling(&mut self) -> bool {
        if let Some(prev) = self.get_prev_sibling() {
            self.save_to_history();
            self.current = prev;
            true
        } else {
            false
        }
    }

    /// Moves to a specific node by ID.
    ///
    /// Returns `true` if the node exists in the tree.
    /// Note: This doesn't validate that the node is reachable from current position.
    #[must_use]
    pub fn go_to(&mut self, target: I) -> bool {
        if !self.tree.contains(target) {
            return false;
        }

        self.save_to_history();
        self.current = target;
        self.depth = Self::compute_depth(self.tree, target);
        true
    }

    /// Moves to a node specified by path.
    ///
    /// Returns `true` if the entire path could be resolved.
    #[must_use]
    pub fn go_to_path(&mut self, path: &TreePath<I>) -> bool {
        if let Some(target) = path.resolve(self.tree) {
            self.go_to(target)
        } else {
            false
        }
    }

    /// Moves to a node specified by index path.
    ///
    /// Requires a root ID to start resolution from.
    #[must_use]
    pub fn go_to_index_path(&mut self, path: &IndexPath, root: I) -> bool {
        if let Some(target) = path.resolve(self.tree, root) {
            self.go_to(target)
        } else {
            false
        }
    }

    /// Moves to the root of the tree containing current node.
    pub fn go_root(&mut self) {
        while self.go_parent() {}
    }

    // === HISTORY OPERATIONS ===

    /// Goes back to the previous position in history.
    ///
    /// Returns `true` if there was a previous position.
    /// Does nothing if history is disabled or empty.
    #[must_use]
    pub fn go_back(&mut self) -> bool {
        if let Some(ref mut history) = self.history {
            if let Some(prev) = history.pop() {
                self.current = prev;
                self.depth = Self::compute_depth(self.tree, prev);
                return true;
            }
        }
        false
    }

    /// Pushes current position to history without moving.
    ///
    /// Useful for marking positions before exploratory navigation.
    pub fn push_position(&mut self) {
        self.save_to_history();
    }

    /// Pops and returns the last position from history without moving.
    pub fn pop_position(&mut self) -> Option<I> {
        self.history.as_mut()?.pop()
    }

    /// Clears all history.
    pub fn clear_history(&mut self) {
        if let Some(ref mut history) = self.history {
            history.clear();
        }
    }

    /// Peeks at the last position in history without removing it.
    pub fn peek_history(&self) -> Option<I> {
        self.history.as_ref()?.peek()
    }

    // === DFS TRAVERSAL ===

    /// Moves to the next node in pre-order DFS traversal.
    ///
    /// Order: self, then children depth-first, then siblings.
    /// Returns `false` when traversal is complete (back at or past start).
    #[must_use]
    pub fn go_next_dfs(&mut self) -> bool {
        // Try first child
        if self.go_first_child() {
            return true;
        }

        // Try next sibling
        if self.go_next_sibling() {
            return true;
        }

        // Go up and try siblings of ancestors
        loop {
            if !self.go_parent() {
                return false; // Reached root, traversal complete
            }
            if self.go_next_sibling() {
                return true;
            }
        }
    }

    /// Moves to the previous node in pre-order DFS traversal.
    ///
    /// Returns `false` when at the start of traversal.
    #[must_use]
    pub fn go_prev_dfs(&mut self) -> bool {
        // Try previous sibling's deepest last descendant
        if self.go_prev_sibling() {
            while self.go_last_child() {}
            return true;
        }

        // Otherwise go to parent
        self.go_parent()
    }

    // === SEARCH ===

    /// Finds first descendant matching predicate, moving cursor there.
    ///
    /// Returns `true` if found (cursor now at matching node).
    /// Uses DFS traversal.
    #[must_use]
    pub fn find_descendant<P>(&mut self, mut predicate: P) -> bool
    where
        P: FnMut(I) -> bool,
    {
        let start = self.current;

        // DFS search
        while self.go_next_dfs() {
            if predicate(self.current) {
                return true;
            }

            // If we've gone back up past our start, we're done
            if self.tree.parent(self.current).is_none() || self.current == start {
                break;
            }
        }

        // Not found - restore to start
        self.current = start;
        self.depth = Self::compute_depth(self.tree, start);
        false
    }

    /// Finds first ancestor matching predicate, moving cursor there.
    ///
    /// Returns `true` if found (cursor now at matching node).
    #[must_use]
    pub fn find_ancestor<P>(&mut self, mut predicate: P) -> bool
    where
        P: FnMut(I) -> bool,
    {
        let start = self.current;

        while self.go_parent() {
            if predicate(self.current) {
                return true;
            }
        }

        // Not found - restore to start
        self.current = start;
        self.depth = Self::compute_depth(self.tree, start);
        false
    }

    // === ITERATION HELPERS ===

    /// Iterates over children from current position.
    #[inline]
    pub fn children(&self) -> impl Iterator<Item = I> + '_ {
        self.tree.children(self.current)
    }

    /// Iterates over ancestors from current position (excluding self).
    pub fn ancestors(&self) -> impl Iterator<Item = I> + '_ {
        let mut current = self.tree.parent(self.current);
        std::iter::from_fn(move || {
            let id = current?;
            current = self.tree.parent(id);
            Some(id)
        })
    }

    /// Iterates over siblings (excluding current).
    pub fn siblings(&self) -> impl Iterator<Item = I> + '_ {
        let current = self.current;
        let parent = self.tree.parent(self.current);

        parent
            .into_iter()
            .flat_map(move |p| self.tree.children(p))
            .filter(move |&c| c != current)
    }
}

// === TRAIT IMPLEMENTATIONS ===

impl<T, I> Clone for TreeCursor<'_, T, I>
where
    T: TreeNav<I>,
    I: Identifier,
{
    fn clone(&self) -> Self {
        Self {
            tree: self.tree,
            current: self.current,
            depth: self.depth,
            history: self.history.clone(),
        }
    }
}

impl<T, I> fmt::Debug for TreeCursor<'_, T, I>
where
    T: TreeNav<I>,
    I: Identifier,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TreeCursor")
            .field("current", &self.current)
            .field("depth", &self.depth)
            .field("has_history", &self.history.is_some())
            .field("history_len", &self.history_len())
            .finish()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_foundation::ElementId;

    // Simple mock tree for testing
    struct MockTree {
        // Node -> (Parent, Children)
        nodes: std::collections::HashMap<ElementId, (Option<ElementId>, Vec<ElementId>)>,
    }

    impl MockTree {
        fn new() -> Self {
            Self {
                nodes: std::collections::HashMap::new(),
            }
        }

        fn add_node(&mut self, id: ElementId, parent: Option<ElementId>) {
            self.nodes.insert(id, (parent, Vec::new()));
            if let Some(p) = parent {
                if let Some((_, children)) = self.nodes.get_mut(&p) {
                    children.push(id);
                }
            }
        }
    }

    impl TreeNav<ElementId> for MockTree {
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
            crate::iter::Ancestors::new(self, start)
        }

        fn descendants(&self, root: ElementId) -> impl Iterator<Item = (ElementId, usize)> + '_ {
            crate::iter::DescendantsWithDepth::new(self, root)
        }

        fn siblings(&self, id: ElementId) -> impl Iterator<Item = ElementId> + '_ {
            let parent_id = self.parent(id);
            parent_id
                .into_iter()
                .flat_map(move |pid| self.children(pid).filter(move |&cid| cid != id))
        }
    }

    // TreeRead is required by TreeNav
    impl crate::traits::TreeRead<ElementId> for MockTree {
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

    fn create_test_tree() -> MockTree {
        // Tree structure:
        // 1 (root)
        //   2
        //     5
        //     6
        //   3
        //     7
        //   4
        let mut tree = MockTree::new();
        tree.add_node(ElementId::new(1), None);
        tree.add_node(ElementId::new(2), Some(ElementId::new(1)));
        tree.add_node(ElementId::new(3), Some(ElementId::new(1)));
        tree.add_node(ElementId::new(4), Some(ElementId::new(1)));
        tree.add_node(ElementId::new(5), Some(ElementId::new(2)));
        tree.add_node(ElementId::new(6), Some(ElementId::new(2)));
        tree.add_node(ElementId::new(7), Some(ElementId::new(3)));
        tree
    }

    #[test]
    fn test_cursor_new() {
        let tree = create_test_tree();
        let cursor = TreeCursor::new(&tree, ElementId::new(1));

        assert_eq!(cursor.current(), ElementId::new(1));
        assert_eq!(cursor.depth(), 0);
        assert!(cursor.is_at_root());
        assert!(!cursor.has_history());
    }

    #[test]
    fn test_cursor_with_history() {
        let tree = create_test_tree();
        let cursor = TreeCursor::with_history(&tree, ElementId::new(1), 10);

        assert!(cursor.has_history());
        assert_eq!(cursor.history_len(), 0);
    }

    #[test]
    fn test_cursor_navigation() {
        let tree = create_test_tree();
        let mut cursor = TreeCursor::new(&tree, ElementId::new(1));

        // Go to first child (2)
        assert!(cursor.go_first_child());
        assert_eq!(cursor.current(), ElementId::new(2));
        assert_eq!(cursor.depth(), 1);

        // Go to next sibling (3)
        assert!(cursor.go_next_sibling());
        assert_eq!(cursor.current(), ElementId::new(3));

        // Go back to parent (1)
        assert!(cursor.go_parent());
        assert_eq!(cursor.current(), ElementId::new(1));
        assert!(cursor.is_at_root());

        // Can't go further up
        assert!(!cursor.go_parent());
    }

    #[test]
    fn test_cursor_child_navigation() {
        let tree = create_test_tree();
        let mut cursor = TreeCursor::new(&tree, ElementId::new(2));

        // Has children
        assert_eq!(cursor.child_count(), 2);
        assert!(!cursor.is_at_leaf());

        // Go to child by index
        assert!(cursor.go_child(1)); // Go to 6
        assert_eq!(cursor.current(), ElementId::new(6));

        // This is a leaf
        assert!(cursor.is_at_leaf());
        assert!(!cursor.go_first_child());
    }

    #[test]
    fn test_cursor_sibling_navigation() {
        let tree = create_test_tree();
        let mut cursor = TreeCursor::new(&tree, ElementId::new(3));

        assert!(cursor.has_prev_sibling());
        assert!(cursor.has_next_sibling());

        assert!(cursor.go_prev_sibling());
        assert_eq!(cursor.current(), ElementId::new(2));

        assert!(!cursor.has_prev_sibling());
        assert!(cursor.has_next_sibling());
    }

    #[test]
    fn test_cursor_history() {
        let tree = create_test_tree();
        let mut cursor = TreeCursor::with_history(&tree, ElementId::new(1), 10);

        let _ = cursor.go_first_child(); // 1 -> 2
        let _ = cursor.go_first_child(); // 2 -> 5

        assert_eq!(cursor.current(), ElementId::new(5));
        assert_eq!(cursor.history_len(), 2);

        // Go back
        assert!(cursor.go_back());
        assert_eq!(cursor.current(), ElementId::new(2));

        assert!(cursor.go_back());
        assert_eq!(cursor.current(), ElementId::new(1));

        // No more history
        assert!(!cursor.go_back());
    }

    #[test]
    fn test_cursor_go_to() {
        let tree = create_test_tree();
        let mut cursor = TreeCursor::new(&tree, ElementId::new(1));

        // Jump to deep node
        assert!(cursor.go_to(ElementId::new(7)));
        assert_eq!(cursor.current(), ElementId::new(7));
        assert_eq!(cursor.depth(), 2);

        // Can't go to non-existent node
        assert!(!cursor.go_to(ElementId::new(99)));
    }

    #[test]
    fn test_cursor_go_root() {
        let tree = create_test_tree();
        let mut cursor = TreeCursor::new(&tree, ElementId::new(7));

        assert_eq!(cursor.depth(), 2);

        cursor.go_root();

        assert_eq!(cursor.current(), ElementId::new(1));
        assert_eq!(cursor.depth(), 0);
        assert!(cursor.is_at_root());
    }

    #[test]
    fn test_cursor_dfs_traversal() {
        let tree = create_test_tree();
        let mut cursor = TreeCursor::new(&tree, ElementId::new(1));

        // Collect DFS order
        let mut visited = vec![cursor.current()];
        while cursor.go_next_dfs() {
            visited.push(cursor.current());
        }

        // Pre-order DFS: 1, 2, 5, 6, 3, 7, 4
        assert_eq!(
            visited,
            vec![
                ElementId::new(1),
                ElementId::new(2),
                ElementId::new(5),
                ElementId::new(6),
                ElementId::new(3),
                ElementId::new(7),
                ElementId::new(4),
            ]
        );
    }

    #[test]
    fn test_cursor_path() {
        let tree = create_test_tree();
        let cursor = TreeCursor::new(&tree, ElementId::new(7));

        let path = cursor.path();
        assert_eq!(path.len(), 3);
        assert_eq!(path.root_id(), Some(ElementId::new(1)));
        assert_eq!(path.target(), Some(ElementId::new(7)));
    }

    #[test]
    fn test_cursor_sibling_index() {
        let tree = create_test_tree();
        let cursor = TreeCursor::new(&tree, ElementId::new(3));

        // 3 is second child of 1
        assert_eq!(cursor.sibling_index(), Some(1));

        let cursor = TreeCursor::new(&tree, ElementId::new(1));
        // Root has no sibling index
        assert_eq!(cursor.sibling_index(), None);
    }

    #[test]
    fn test_cursor_clone() {
        let tree = create_test_tree();
        let cursor = TreeCursor::with_history(&tree, ElementId::new(1), 10);

        let cloned = cursor.clone();
        assert_eq!(cloned.current(), cursor.current());
        assert!(cloned.has_history());
    }

    #[test]
    fn test_cursor_iterators() {
        let tree = create_test_tree();
        let cursor = TreeCursor::new(&tree, ElementId::new(2));

        // Children
        let children: Vec<_> = cursor.children().collect();
        assert_eq!(children, vec![ElementId::new(5), ElementId::new(6)]);

        // Ancestors
        let ancestors: Vec<_> = cursor.ancestors().collect();
        assert_eq!(ancestors, vec![ElementId::new(1)]);

        // Siblings (of node 3)
        let cursor = TreeCursor::new(&tree, ElementId::new(3));
        let siblings: Vec<_> = cursor.siblings().collect();
        assert_eq!(siblings, vec![ElementId::new(2), ElementId::new(4)]);
    }
}
