//! Tree path representations for navigation, serialization, and debugging.
//!
//! This module provides two complementary path types:
//!
//! - **`TreePath<I>`**: ID-based path (stable, for runtime navigation)
//! - **`IndexPath`**: Index-based path (portable, for serialization)
//!
//! # Overview
//!
//! ```text
//! Tree:                    TreePath:              IndexPath:
//!
//! Root (id=1)              [1]                    []
//!   ├── A (id=2)           [1, 2]                 [0]
//!   │     └── X (id=5)     [1, 2, 5]              [0, 0]
//!   └── B (id=3)           [1, 3]                 [1]
//!         ├── Y (id=6)     [1, 3, 6]              [1, 0]
//!         └── Z (id=7)     [1, 3, 7]              [1, 1]
//! ```
//!
//! # When to Use Each
//!
//! | Use Case | TreePath | IndexPath |
//! |----------|----------|-----------|
//! | Runtime navigation | ✅ | ❌ |
//! | Error reporting | ✅ | ❌ |
//! | DevTools selection | ✅ | ✅ |
//! | Serialization | ❌ | ✅ |
//! | Cross-tree operations | ❌ | ✅ |
//! | Clipboard/undo | ❌ | ✅ |
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_tree::{TreePath, IndexPath, TreeNav, TreeNavPathExt};
//!
//! // Create path from node
//! let path = TreePath::from_node(&tree, deep_node_id);
//! println!("Path: {:?}", path);  // [root, child, grandchild]
//!
//! // Navigate path
//! let parent_path = path.parent().unwrap();
//! let child_path = path.child(new_child_id);
//!
//! // Compare paths
//! if parent_path.is_ancestor_of(&path) {
//!     println!("parent is ancestor of child");
//! }
//!
//! // Resolve back to node
//! if let Some(node_id) = path.resolve(&tree) {
//!     let node = tree.get(node_id);
//! }
//! ```

use std::fmt;
use std::ops::Index;

use smallvec::SmallVec;

use flui_foundation::Identifier;

use crate::traits::TreeNav;

// ============================================================================
// TREE PATH (ID-based)
// ============================================================================

/// A path through a tree represented as a sequence of node IDs.
///
/// `TreePath` stores the complete chain of ancestor IDs from root to target,
/// enabling efficient path comparison, navigation, and debugging.
///
/// # Memory Layout
///
/// Uses `SmallVec<[I; 8]>` - paths up to 8 levels deep use no heap allocation.
/// This covers typical UI tree depths without overhead.
///
/// # Thread Safety
///
/// `TreePath<I>` is `Send + Sync` when `I: Send + Sync` (all FLUI IDs are).
///
/// # Examples
///
/// ```rust,ignore
/// use flui_tree::{TreePath, TreeNav};
///
/// // Create from node
/// let path = TreePath::from_node(&tree, target_id);
///
/// // Navigate
/// let parent = path.parent().unwrap();
/// let child = path.child(child_id);
///
/// // Compare
/// assert!(parent.is_ancestor_of(&path));
/// let common = path.common_prefix(&other_path);
/// ```
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct TreePath<I: Identifier> {
    /// Path segments from root to target (root at index 0).
    segments: SmallVec<[I; 8]>,
}

impl<I: Identifier> TreePath<I> {
    // === CONSTRUCTORS ===

    /// Creates an empty path (represents "no location").
    #[inline]
    #[must_use]
    pub fn empty() -> Self {
        Self {
            segments: SmallVec::new(),
        }
    }

    /// Creates a path containing only the root node.
    #[inline]
    #[must_use]
    pub fn root(root_id: I) -> Self {
        let mut segments = SmallVec::new();
        segments.push(root_id);
        Self { segments }
    }

    /// Creates a path from a target node by walking ancestors.
    ///
    /// The path will contain all nodes from root to target.
    pub fn from_node<T: TreeNav<I> + ?Sized>(tree: &T, target: I) -> Self {
        if !tree.contains(target) {
            return Self::empty();
        }

        // Collect ancestors (target -> ... -> root)
        let mut segments: SmallVec<[I; 8]> = SmallVec::new();
        let mut current = Some(target);

        while let Some(id) = current {
            segments.push(id);
            current = tree.parent(id);
        }

        // Reverse to get root -> target order
        segments.reverse();

        Self { segments }
    }

    /// Creates a path from an existing slice of IDs.
    ///
    /// IDs should be in root-to-target order.
    #[inline]
    #[must_use]
    pub fn from_slice(ids: &[I]) -> Self {
        Self {
            segments: SmallVec::from_slice(ids),
        }
    }

    /// Creates a path by collecting IDs from an iterator (root-to-target order).
    ///
    /// This is a convenience method. You can also use `TreePath::from_iter()`
    /// via the `FromIterator` trait.
    #[inline]
    pub fn collect_from(iter: impl IntoIterator<Item = I>) -> Self {
        Self {
            segments: iter.into_iter().collect(),
        }
    }

    // === ACCESSORS ===

    /// Returns the root node ID, or `None` if path is empty.
    #[inline]
    #[must_use]
    pub fn root_id(&self) -> Option<I> {
        self.segments.first().copied()
    }

    /// Returns the target (deepest) node ID, or `None` if path is empty.
    #[inline]
    #[must_use]
    pub fn target(&self) -> Option<I> {
        self.segments.last().copied()
    }

    /// Returns the depth (number of segments - 1, or 0 for empty).
    ///
    /// Note: A single-element path (just root) has depth 0.
    #[inline]
    #[must_use]
    pub fn depth(&self) -> usize {
        self.segments.len().saturating_sub(1)
    }

    /// Returns the number of segments (0 for empty path).
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.segments.len()
    }

    /// Returns true if the path is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    /// Returns the ID at the given index (0 = root).
    #[inline]
    #[must_use]
    pub fn at(&self, index: usize) -> Option<I> {
        self.segments.get(index).copied()
    }

    /// Returns the path as a slice of IDs (root-to-target order).
    #[inline]
    #[must_use]
    pub fn as_slice(&self) -> &[I] {
        &self.segments
    }

    /// Iterates over IDs from root to target.
    #[inline]
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = I> + ExactSizeIterator + '_ {
        self.segments.iter().copied()
    }

    /// Iterates over IDs from target to root.
    #[inline]
    pub fn iter_reverse(&self) -> impl DoubleEndedIterator<Item = I> + ExactSizeIterator + '_ {
        self.segments.iter().copied().rev()
    }

    // === NAVIGATION ===

    /// Returns the parent path (all but last segment).
    ///
    /// Returns `None` if path is empty or contains only root.
    #[must_use]
    pub fn parent(&self) -> Option<Self> {
        if self.segments.len() <= 1 {
            return None;
        }

        let mut segments = self.segments.clone();
        segments.pop();
        Some(Self { segments })
    }

    /// Returns a new path with the given child appended.
    ///
    /// Does NOT validate that `child` is actually a child of target.
    #[must_use]
    pub fn child(&self, child: I) -> Self {
        let mut segments = self.segments.clone();
        segments.push(child);
        Self { segments }
    }

    /// Returns a new path extended with multiple children.
    #[must_use]
    pub fn extend(&self, children: impl IntoIterator<Item = I>) -> Self {
        let mut segments = self.segments.clone();
        segments.extend(children);
        Self { segments }
    }

    /// Returns the path truncated to the given length.
    ///
    /// If length >= current length, returns a clone.
    #[must_use]
    pub fn truncate(&self, length: usize) -> Self {
        if length >= self.segments.len() {
            return self.clone();
        }

        Self {
            segments: self.segments[..length].into(),
        }
    }

    /// Returns the path up to (but not including) the given depth from target.
    ///
    /// `ancestor(0)` returns self, `ancestor(1)` returns parent path, etc.
    #[must_use]
    pub fn ancestor(&self, levels_up: usize) -> Option<Self> {
        if levels_up >= self.segments.len() {
            return None;
        }

        let new_len = self.segments.len() - levels_up;
        Some(self.truncate(new_len))
    }

    // === COMPARISON ===

    /// Returns true if `self` is an ancestor path of `other`.
    ///
    /// A path is an ancestor if `other` starts with all segments of `self`.
    /// A path is NOT an ancestor of itself.
    #[must_use]
    pub fn is_ancestor_of(&self, other: &Self) -> bool {
        if self.is_empty() || other.is_empty() {
            return false;
        }

        if self.segments.len() >= other.segments.len() {
            return false;
        }

        self.segments
            .iter()
            .zip(other.segments.iter())
            .all(|(a, b)| a == b)
    }

    /// Returns true if `self` is a descendant path of `other`.
    #[inline]
    #[must_use]
    pub fn is_descendant_of(&self, other: &Self) -> bool {
        other.is_ancestor_of(self)
    }

    /// Returns true if the paths are equal.
    #[inline]
    #[must_use]
    pub fn is_same_as(&self, other: &Self) -> bool {
        self.segments == other.segments
    }

    /// Returns the common prefix (shared ancestor path).
    ///
    /// Returns empty path if paths share no common root.
    #[must_use]
    pub fn common_prefix(&self, other: &Self) -> Self {
        let common_len = self
            .segments
            .iter()
            .zip(other.segments.iter())
            .take_while(|(a, b)| a == b)
            .count();

        self.truncate(common_len)
    }

    /// Returns the relative path from `ancestor` to `self`.
    ///
    /// Returns `None` if `ancestor` is not actually an ancestor of `self`.
    #[must_use]
    pub fn relative_to(&self, ancestor: &Self) -> Option<Self> {
        if !ancestor.is_ancestor_of(self) && !ancestor.is_same_as(self) {
            return None;
        }

        Some(Self {
            segments: self.segments[ancestor.len()..].into(),
        })
    }

    /// Returns the depth difference from `self` to `other`.
    ///
    /// Positive if `self` is deeper, negative if shallower.
    #[must_use]
    #[allow(clippy::cast_possible_wrap)]
    pub fn depth_difference(&self, other: &Self) -> isize {
        self.len() as isize - other.len() as isize
    }

    // === RESOLUTION ===

    /// Validates that the path exists in the tree and relationships are correct.
    ///
    /// Returns `true` if every segment is a child of the previous segment.
    pub fn validate<T: TreeNav<I>>(&self, tree: &T) -> bool {
        if self.is_empty() {
            return true;
        }

        // Check all nodes exist and parent relationships are correct
        for window in self.segments.windows(2) {
            let parent = window[0];
            let child = window[1];

            if !tree.contains(parent) || !tree.contains(child) {
                return false;
            }

            if tree.parent(child) != Some(parent) {
                return false;
            }
        }

        // Check last node exists
        if let Some(last) = self.target() {
            if !tree.contains(last) {
                return false;
            }
        }

        true
    }

    /// Resolves the path to the target node ID if valid.
    ///
    /// Unlike `target()`, this validates the entire path exists in tree.
    pub fn resolve<T: TreeNav<I>>(&self, tree: &T) -> Option<I> {
        if !self.validate(tree) {
            return None;
        }
        self.target()
    }

    /// Walks the path, calling visitor for each node.
    ///
    /// Returns `true` if walk completed, `false` if visitor returned `false`.
    pub fn walk<F>(&self, mut visitor: F) -> bool
    where
        F: FnMut(I, usize) -> bool,
    {
        for (depth, &id) in self.segments.iter().enumerate() {
            if !visitor(id, depth) {
                return false;
            }
        }
        true
    }
}

// === TRAIT IMPLEMENTATIONS ===

impl<I: Identifier> Default for TreePath<I> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<I: Identifier> fmt::Debug for TreePath<I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TreePath[")?;
        for (i, id) in self.segments.iter().enumerate() {
            if i > 0 {
                write!(f, " -> ")?;
            }
            write!(f, "{id}")?;
        }
        write!(f, "]")
    }
}

impl<I: Identifier> fmt::Display for TreePath<I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            return write!(f, "(empty)");
        }

        for (i, id) in self.segments.iter().enumerate() {
            if i > 0 {
                write!(f, " → ")?;
            }
            write!(f, "{id}")?;
        }
        Ok(())
    }
}

impl<I: Identifier> From<I> for TreePath<I> {
    fn from(id: I) -> Self {
        Self::root(id)
    }
}

impl<I: Identifier> FromIterator<I> for TreePath<I> {
    fn from_iter<T: IntoIterator<Item = I>>(iter: T) -> Self {
        Self::collect_from(iter)
    }
}

impl<I: Identifier> Index<usize> for TreePath<I> {
    type Output = I;

    fn index(&self, index: usize) -> &Self::Output {
        &self.segments[index]
    }
}

impl<I: Identifier> IntoIterator for TreePath<I> {
    type Item = I;
    type IntoIter = smallvec::IntoIter<[I; 8]>;

    fn into_iter(self) -> Self::IntoIter {
        self.segments.into_iter()
    }
}

impl<'a, I: Identifier> IntoIterator for &'a TreePath<I> {
    type Item = I;
    type IntoIter = std::iter::Copied<std::slice::Iter<'a, I>>;

    fn into_iter(self) -> Self::IntoIter {
        self.segments.iter().copied()
    }
}

// ============================================================================
// INDEX PATH (Index-based)
// ============================================================================

/// A path through a tree represented as child indices from root.
///
/// Unlike `TreePath<I>`, `IndexPath` uses position-based indices:
/// - `[]` - root node
/// - `[0]` - first child of root
/// - `[0, 2]` - third child of first child of root
///
/// # Advantages
///
/// - **Portable**: Indices are meaningful across serialization boundaries
/// - **Compact**: 4 bytes per level vs 8 bytes for IDs
/// - **Tree-agnostic**: Can represent paths in any tree structure
///
/// # Disadvantages
///
/// - **Fragile**: Invalid after tree mutations that change child order
/// - **Resolution cost**: Must traverse from root to resolve
///
/// # Use Cases
///
/// - Clipboard operations (copy path, paste elsewhere)
/// - Test fixtures (predictable paths)
/// - Cross-process communication
/// - Undo/redo path storage
///
/// # Example
///
/// ```rust,ignore
/// use flui_tree::IndexPath;
///
/// let path = IndexPath::new(&[0, 2, 1]);  // root -> 1st child -> 3rd child -> 2nd child
///
/// // Navigate
/// let parent = path.parent().unwrap();
/// let child = path.child(0);
///
/// // Resolve in a tree
/// if let Some(node_id) = path.resolve(&tree, root_id) {
///     println!("Found node: {:?}", node_id);
/// }
/// ```
#[derive(Clone, PartialEq, Eq, Hash, Default)]
pub struct IndexPath {
    /// Child indices from root (root is implicit at depth 0).
    indices: SmallVec<[u32; 16]>,
}

impl IndexPath {
    // === CONSTRUCTORS ===

    /// Creates a path representing the root node.
    #[inline]
    #[must_use]
    pub fn root() -> Self {
        Self {
            indices: SmallVec::new(),
        }
    }

    /// Creates a path from child indices.
    #[inline]
    #[must_use]
    pub fn new(indices: &[u32]) -> Self {
        Self {
            indices: SmallVec::from_slice(indices),
        }
    }

    /// Creates a path from a `TreePath` by computing child indices.
    pub fn from_tree_path<I, T>(tree: &T, path: &TreePath<I>) -> Self
    where
        I: Identifier,
        T: TreeNav<I> + ?Sized,
    {
        if path.len() <= 1 {
            return Self::root();
        }

        let mut indices = SmallVec::new();

        for window in path.as_slice().windows(2) {
            let parent = window[0];
            let child = window[1];

            // Find child's index among siblings
            if let Some(index) = tree.children(parent).position(|c| c == child) {
                // Safe: UI trees won't have more than u32::MAX children per node
                #[allow(clippy::cast_possible_truncation)]
                indices.push(index as u32);
            } else {
                // Child not found - return partial path
                break;
            }
        }

        Self { indices }
    }

    /// Creates a path from a node by computing indices up to root.
    pub fn from_node<I, T>(tree: &T, node: I) -> Self
    where
        I: Identifier,
        T: TreeNav<I> + ?Sized,
    {
        let tree_path = TreePath::from_node(tree, node);
        Self::from_tree_path(tree, &tree_path)
    }

    // === ACCESSORS ===

    /// Returns the depth (number of indices, 0 = root).
    #[inline]
    #[must_use]
    pub fn depth(&self) -> usize {
        self.indices.len()
    }

    /// Returns the number of indices (alias for depth).
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.indices.len()
    }

    /// Returns true if this represents the root.
    #[inline]
    #[must_use]
    pub fn is_root(&self) -> bool {
        self.indices.is_empty()
    }

    /// Returns true if path is empty (alias for is_root).
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    /// Returns the index at the given depth.
    #[inline]
    #[must_use]
    pub fn at(&self, depth: usize) -> Option<u32> {
        self.indices.get(depth).copied()
    }

    /// Returns the last index (child index at deepest level).
    #[inline]
    #[must_use]
    pub fn last_index(&self) -> Option<u32> {
        self.indices.last().copied()
    }

    /// Returns indices as a slice.
    #[inline]
    #[must_use]
    pub fn as_slice(&self) -> &[u32] {
        &self.indices
    }

    /// Iterates over indices from root to target.
    #[inline]
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = u32> + ExactSizeIterator + '_ {
        self.indices.iter().copied()
    }

    // === NAVIGATION ===

    /// Returns parent path (removes last index).
    #[must_use]
    pub fn parent(&self) -> Option<Self> {
        if self.is_root() {
            return None;
        }

        let mut indices = self.indices.clone();
        indices.pop();
        Some(Self { indices })
    }

    /// Returns path with child index appended.
    #[must_use]
    pub fn child(&self, index: u32) -> Self {
        let mut indices = self.indices.clone();
        indices.push(index);
        Self { indices }
    }

    /// Returns sibling path (same parent, different index).
    #[must_use]
    pub fn sibling(&self, index: u32) -> Option<Self> {
        if self.is_root() {
            return None;
        }

        let mut indices = self.indices.clone();
        *indices.last_mut()? = index;
        Some(Self { indices })
    }

    /// Returns next sibling path (index + 1).
    #[must_use]
    pub fn next_sibling(&self) -> Option<Self> {
        let last = self.last_index()?;
        self.sibling(last.checked_add(1)?)
    }

    /// Returns previous sibling path (index - 1).
    #[must_use]
    pub fn prev_sibling(&self) -> Option<Self> {
        let last = self.last_index()?;
        if last == 0 {
            return None;
        }
        self.sibling(last - 1)
    }

    // === COMPARISON ===

    /// Returns true if self is an ancestor of other.
    #[must_use]
    pub fn is_ancestor_of(&self, other: &Self) -> bool {
        if self.indices.len() >= other.indices.len() {
            return false;
        }

        self.indices
            .iter()
            .zip(other.indices.iter())
            .all(|(a, b)| a == b)
    }

    /// Returns true if self is a descendant of other.
    #[inline]
    #[must_use]
    pub fn is_descendant_of(&self, other: &Self) -> bool {
        other.is_ancestor_of(self)
    }

    /// Returns common prefix path.
    #[must_use]
    pub fn common_prefix(&self, other: &Self) -> Self {
        let common_len = self
            .indices
            .iter()
            .zip(other.indices.iter())
            .take_while(|(a, b)| a == b)
            .count();

        Self {
            indices: self.indices[..common_len].into(),
        }
    }

    // === RESOLUTION ===

    /// Resolves this path to a node ID in the tree.
    ///
    /// Returns `None` if path doesn't exist (index out of bounds).
    pub fn resolve<I, T>(&self, tree: &T, root: I) -> Option<I>
    where
        I: Identifier,
        T: TreeNav<I> + ?Sized,
    {
        let mut current = root;

        for &index in &self.indices {
            current = tree.children(current).nth(index as usize)?;
        }

        Some(current)
    }

    /// Converts to a `TreePath` by resolving in tree.
    pub fn to_tree_path<I, T>(&self, tree: &T, root: I) -> Option<TreePath<I>>
    where
        I: Identifier,
        T: TreeNav<I>,
    {
        let target = self.resolve(tree, root)?;
        Some(TreePath::from_node(tree, target))
    }
}

// === TRAIT IMPLEMENTATIONS ===

impl fmt::Debug for IndexPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "IndexPath{:?}", self.indices.as_slice())
    }
}

impl fmt::Display for IndexPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_root() {
            return write!(f, "[root]");
        }

        write!(f, "[")?;
        for (i, index) in self.indices.iter().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{index}")?;
        }
        write!(f, "]")
    }
}

impl From<&[u32]> for IndexPath {
    fn from(indices: &[u32]) -> Self {
        Self::new(indices)
    }
}

impl FromIterator<u32> for IndexPath {
    fn from_iter<T: IntoIterator<Item = u32>>(iter: T) -> Self {
        Self {
            indices: iter.into_iter().collect(),
        }
    }
}

impl Index<usize> for IndexPath {
    type Output = u32;

    fn index(&self, index: usize) -> &Self::Output {
        &self.indices[index]
    }
}

// ============================================================================
// EXTENSION TRAIT
// ============================================================================

/// Extension trait adding path operations to TreeNav.
///
/// This trait is automatically implemented for all types that implement `TreeNav`.
pub trait TreeNavPathExt<I: Identifier>: TreeNav<I> {
    /// Creates a TreePath from a node.
    #[inline]
    fn path_to(&self, target: I) -> TreePath<I> {
        TreePath::from_node(self, target)
    }

    /// Creates an IndexPath from a node.
    #[inline]
    fn index_path_to(&self, target: I) -> IndexPath {
        IndexPath::from_node(self, target)
    }

    /// Resolves an IndexPath to a node ID.
    #[inline]
    fn resolve_index_path(&self, path: &IndexPath, root: I) -> Option<I> {
        path.resolve(self, root)
    }

    /// Computes the index of a child within its parent.
    ///
    /// Returns `None` if the child has no parent or if the index exceeds `u32::MAX`.
    fn child_index(&self, child: I) -> Option<u32> {
        let parent = self.parent(child)?;
        self.children(parent)
            .position(|c| c == child)
            .and_then(|i| u32::try_from(i).ok())
    }

    /// Returns true if `potential_ancestor` is an ancestor of `node`.
    fn is_path_ancestor(&self, potential_ancestor: I, node: I) -> bool {
        let ancestor_path = self.path_to(potential_ancestor);
        let node_path = self.path_to(node);
        ancestor_path.is_ancestor_of(&node_path)
    }
}

// Blanket implementation
impl<I: Identifier, T: TreeNav<I>> TreeNavPathExt<I> for T {}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_foundation::ElementId;

    // === TREE PATH TESTS ===

    #[test]
    fn test_tree_path_empty() {
        let path = TreePath::<ElementId>::empty();
        assert!(path.is_empty());
        assert_eq!(path.len(), 0);
        assert!(path.root_id().is_none());
        assert!(path.target().is_none());
    }

    #[test]
    fn test_tree_path_root() {
        let path = TreePath::root(ElementId::new(1));
        assert!(!path.is_empty());
        assert_eq!(path.len(), 1);
        assert_eq!(path.depth(), 0);
        assert_eq!(path.root_id(), Some(ElementId::new(1)));
        assert_eq!(path.target(), Some(ElementId::new(1)));
    }

    #[test]
    fn test_tree_path_from_slice() {
        let path = TreePath::from_slice(&[ElementId::new(1), ElementId::new(2), ElementId::new(3)]);

        assert_eq!(path.len(), 3);
        assert_eq!(path.depth(), 2);
        assert_eq!(path.root_id(), Some(ElementId::new(1)));
        assert_eq!(path.target(), Some(ElementId::new(3)));
        assert_eq!(path.at(1), Some(ElementId::new(2)));
    }

    #[test]
    fn test_tree_path_navigation() {
        let path = TreePath::from_slice(&[ElementId::new(1), ElementId::new(2)]);

        // Parent
        let parent = path.parent().unwrap();
        assert_eq!(parent.len(), 1);
        assert_eq!(parent.target(), Some(ElementId::new(1)));

        // Root has no parent
        assert!(parent.parent().is_none());

        // Child
        let child = path.child(ElementId::new(3));
        assert_eq!(child.len(), 3);
        assert_eq!(child.target(), Some(ElementId::new(3)));
    }

    #[test]
    fn test_tree_path_comparison() {
        let root = TreePath::root(ElementId::new(1));
        let child = TreePath::from_slice(&[ElementId::new(1), ElementId::new(2)]);
        let grandchild =
            TreePath::from_slice(&[ElementId::new(1), ElementId::new(2), ElementId::new(3)]);

        assert!(root.is_ancestor_of(&child));
        assert!(root.is_ancestor_of(&grandchild));
        assert!(child.is_ancestor_of(&grandchild));

        assert!(!child.is_ancestor_of(&root));
        assert!(!root.is_ancestor_of(&root)); // Not ancestor of self

        assert!(grandchild.is_descendant_of(&root));
        assert!(grandchild.is_descendant_of(&child));
    }

    #[test]
    fn test_tree_path_common_prefix() {
        let path1 =
            TreePath::from_slice(&[ElementId::new(1), ElementId::new(2), ElementId::new(3)]);
        let path2 =
            TreePath::from_slice(&[ElementId::new(1), ElementId::new(2), ElementId::new(4)]);

        let common = path1.common_prefix(&path2);
        assert_eq!(common.len(), 2);
        assert_eq!(common.target(), Some(ElementId::new(2)));
    }

    #[test]
    fn test_tree_path_relative() {
        let ancestor = TreePath::from_slice(&[ElementId::new(1), ElementId::new(2)]);
        let descendant =
            TreePath::from_slice(&[ElementId::new(1), ElementId::new(2), ElementId::new(3)]);

        let relative = descendant.relative_to(&ancestor).unwrap();
        assert_eq!(relative.len(), 1);
        assert_eq!(relative.at(0), Some(ElementId::new(3)));
    }

    #[test]
    fn test_tree_path_truncate() {
        let path = TreePath::from_slice(&[
            ElementId::new(1),
            ElementId::new(2),
            ElementId::new(3),
            ElementId::new(4),
        ]);

        let truncated = path.truncate(2);
        assert_eq!(truncated.len(), 2);
        assert_eq!(truncated.target(), Some(ElementId::new(2)));
    }

    #[test]
    fn test_tree_path_display() {
        let path = TreePath::from_slice(&[ElementId::new(1), ElementId::new(2)]);
        let display = format!("{}", path);
        assert!(display.contains("1"));
        assert!(display.contains("2"));

        let empty = TreePath::<ElementId>::empty();
        assert_eq!(format!("{}", empty), "(empty)");
    }

    #[test]
    fn test_tree_path_iterate() {
        let path = TreePath::from_slice(&[ElementId::new(1), ElementId::new(2), ElementId::new(3)]);

        let collected: Vec<_> = path.iter().collect();
        assert_eq!(collected.len(), 3);
        assert_eq!(collected[0], ElementId::new(1));

        let reversed: Vec<_> = path.iter_reverse().collect();
        assert_eq!(reversed[0], ElementId::new(3));
    }

    // === INDEX PATH TESTS ===

    #[test]
    fn test_index_path_root() {
        let path = IndexPath::root();
        assert!(path.is_root());
        assert_eq!(path.depth(), 0);
    }

    #[test]
    fn test_index_path_new() {
        let path = IndexPath::new(&[0, 2, 1]);
        assert!(!path.is_root());
        assert_eq!(path.depth(), 3);
        assert_eq!(path.at(0), Some(0));
        assert_eq!(path.at(1), Some(2));
        assert_eq!(path.at(2), Some(1));
    }

    #[test]
    fn test_index_path_navigation() {
        let path = IndexPath::new(&[0, 2]);

        // Parent
        let parent = path.parent().unwrap();
        assert_eq!(parent.depth(), 1);
        assert_eq!(parent.at(0), Some(0));

        // Child
        let child = path.child(3);
        assert_eq!(child.depth(), 3);
        assert_eq!(child.at(2), Some(3));

        // Sibling
        let sibling = path.sibling(5).unwrap();
        assert_eq!(sibling.at(1), Some(5));

        // Next/prev sibling
        let next = path.next_sibling().unwrap();
        assert_eq!(next.at(1), Some(3));

        let prev = path.prev_sibling().unwrap();
        assert_eq!(prev.at(1), Some(1));
    }

    #[test]
    fn test_index_path_comparison() {
        let parent = IndexPath::new(&[0]);
        let child = IndexPath::new(&[0, 2]);
        let sibling = IndexPath::new(&[0, 3]);

        assert!(parent.is_ancestor_of(&child));
        assert!(!parent.is_ancestor_of(&parent));
        assert!(!sibling.is_ancestor_of(&child));

        let common = child.common_prefix(&sibling);
        assert_eq!(common.depth(), 1);
    }

    #[test]
    fn test_index_path_display() {
        let path = IndexPath::new(&[0, 2, 1]);
        let display = format!("{}", path);
        assert!(display.contains("0"));
        assert!(display.contains("2"));
        assert!(display.contains("1"));

        let root = IndexPath::root();
        assert_eq!(format!("{}", root), "[root]");
    }
}
