//! Render-specific iterators.
//!
//! These iterators filter to only render elements, skipping
//! non-render elements like `StatelessView` wrappers.
//!
//! # Iterator Types
//!
//! - [`RenderAncestors`] - Walk up the tree through render elements
//! - [`RenderDescendants`] - Walk down the tree through all render elements
//! - [`RenderChildren`] - Find immediate render children (stops at render boundaries)
//! - [`RenderChildrenWithIndex`] - Like `RenderChildren` but with indices
//! - [`RenderSiblings`] - Iterate over render siblings
//! - [`RenderSubtree`] - BFS traversal of render subtree with depth info
//! - [`RenderLeaves`] - Find leaf render elements (no render children)
//!
//! # Arity Integration
//!
//! Use [`RenderChildrenCollector`] to collect render children with compile-time
//! arity validation:
//!
//! ```rust,ignore
//! use flui_tree::{RenderChildrenCollector, Single, Variable, Arity};
//!
//! // Collect into a typed accessor with runtime validation
//! let collector = RenderChildrenCollector::new(tree, parent_id);
//! if let Some(children) = collector.try_into_arity::<Single>() {
//!     let child = children.single();
//!     // ...
//! }
//!
//! // Or use the infallible version for Variable arity
//! let children = collector.into_variable();
//! for child in children.copied() {
//!     // ...
//! }
//! ```
//!
//! # Performance Notes
//!
//! All iterators are zero-allocation during iteration (only initial Vec allocation
//! for stack-based iterators). Use `with_capacity` hints when known.

use super::access::RenderTreeAccess;
use flui_foundation::ElementId;
use flui_tree::arity::{Arity, SliceChildren, Variable};
use smallvec::SmallVec;

// ============================================================================
// RENDER ANCESTORS ITERATOR
// ============================================================================

/// Iterator over render ancestors.
///
/// Like [`Ancestors`](super::Ancestors) but only yields elements that
/// are render elements (have a `RenderObject`).
///
/// # Use Case
///
/// Finding the render parent when the element tree contains non-render
/// elements (Component, Provider wrappers).
///
/// # Example
///
/// ```rust,ignore
/// // Tree: RenderBox -> StatelessWrapper -> RenderFlex
/// // RenderAncestors from RenderFlex yields: [RenderFlex, RenderBox]
/// // (skipping StatelessWrapper)
/// ```
#[derive(Debug)]
pub struct RenderAncestors<'a, T: RenderTreeAccess + ?Sized> {
    tree: &'a T,
    current: Option<ElementId>,
}

impl<'a, T: RenderTreeAccess + ?Sized> RenderAncestors<'a, T> {
    /// Creates a new render ancestors iterator.
    #[inline]
    pub fn new(tree: &'a T, start: ElementId) -> Self {
        Self {
            tree,
            current: Some(start),
        }
    }

    /// Creates iterator starting from parent (skipping start element).
    #[inline]
    pub fn from_parent(tree: &'a T, start: ElementId) -> Self {
        Self {
            tree,
            current: tree.parent(start),
        }
    }
}

impl<T: RenderTreeAccess + ?Sized> Iterator for RenderAncestors<'_, T> {
    type Item = ElementId;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let current = self.current?;

            if !self.tree.contains(current) {
                self.current = None;
                return None;
            }

            // Move to parent for next iteration
            self.current = self.tree.parent(current);

            // Only yield if it's a render element
            if self.tree.is_render_element(current) {
                return Some(current);
            }
            // Otherwise continue to next ancestor
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        if self.current.is_some() {
            // If we have a current node, there's at least 1 more element (itself)
            // The upper bound is the maximum possible depth of the tree.
            // Use default MAX_DEPTH value (32) as a conservative estimate.
            // Render elements are typically a subset of all elements, so this is safe.
            (1, Some(32))
        } else {
            (0, Some(0))
        }
    }
}

impl<T: RenderTreeAccess + ?Sized> std::iter::FusedIterator for RenderAncestors<'_, T> {}

// ============================================================================
// RENDER DESCENDANTS ITERATOR
// ============================================================================

/// Iterator over render descendants.
///
/// Like [`Descendants`](super::Descendants) but only yields elements
/// that are render elements.
///
/// # Use Case
///
/// Collecting all render objects that need layout/paint, skipping
/// wrapper elements.
///
/// # Performance
///
/// Uses `SmallVec` with inline capacity to avoid heap allocation for
/// typical tree structures (up to 16 elements on stack).
#[derive(Debug)]
pub struct RenderDescendants<'a, T: RenderTreeAccess + ?Sized> {
    tree: &'a T,
    stack: SmallVec<[ElementId; 16]>,
}

impl<'a, T: RenderTreeAccess + ?Sized> RenderDescendants<'a, T> {
    /// Creates a new render descendants iterator.
    #[inline]
    pub fn new(tree: &'a T, root: ElementId) -> Self {
        let mut stack = SmallVec::new();

        if tree.contains(root) {
            stack.push(root);
        }

        Self { tree, stack }
    }

    /// Creates with custom capacity hint.
    ///
    /// Note: `SmallVec` will use inline storage for up to 16 elements regardless.
    #[inline]
    pub fn with_capacity(tree: &'a T, root: ElementId, capacity: usize) -> Self {
        let mut stack = SmallVec::with_capacity(capacity);

        if tree.contains(root) {
            stack.push(root);
        }

        Self { tree, stack }
    }
}

impl<T: RenderTreeAccess + ?Sized> Iterator for RenderDescendants<'_, T> {
    type Item = ElementId;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let current = self.stack.pop()?;

            if !self.tree.contains(current) {
                continue;
            }

            // Always push children (even from non-render elements)
            // Use SmallVec to avoid heap allocation for typical cases
            let children: SmallVec<[ElementId; 8]> = self.tree.children(current).collect();
            for child in children.into_iter().rev() {
                self.stack.push(child);
            }

            // Only yield if it's a render element
            if self.tree.is_render_element(current) {
                return Some(current);
            }
            // Otherwise continue to next element
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, None)
    }
}

impl<T: RenderTreeAccess + ?Sized> std::iter::FusedIterator for RenderDescendants<'_, T> {}

// ============================================================================
// RENDER CHILDREN ITERATOR
// ============================================================================

/// Iterator that finds render children of a render element.
///
/// Unlike `RenderDescendants`, this stops at render boundaries.
/// It finds the immediate render children, skipping non-render
/// wrapper elements but not recursing into other render subtrees.
///
/// # Use Case
///
/// During layout, a render parent needs to find its render children
/// to call `performLayout` on them.
///
/// # Performance
///
/// Uses `SmallVec` with inline capacity to avoid heap allocation for
/// typical layouts (up to 8 children on stack).
#[derive(Debug)]
pub struct RenderChildren<'a, T: RenderTreeAccess + ?Sized> {
    tree: &'a T,
    stack: SmallVec<[ElementId; 8]>,
}

impl<'a, T: RenderTreeAccess + ?Sized> RenderChildren<'a, T> {
    /// Creates a new render children iterator.
    #[inline]
    pub fn new(tree: &'a T, parent: ElementId) -> Self {
        let mut stack = SmallVec::new();

        // Start with direct children
        if tree.contains(parent) {
            let children: SmallVec<[ElementId; 8]> = tree.children(parent).collect();
            for child in children.into_iter().rev() {
                stack.push(child);
            }
        }

        Self { tree, stack }
    }

    /// Creates with custom capacity hint.
    ///
    /// Note: `SmallVec` will use inline storage for up to 8 elements regardless.
    #[inline]
    pub fn with_capacity(tree: &'a T, parent: ElementId, capacity: usize) -> Self {
        let mut stack = SmallVec::with_capacity(capacity);

        if tree.contains(parent) {
            let children: SmallVec<[ElementId; 8]> = tree.children(parent).collect();
            for child in children.into_iter().rev() {
                stack.push(child);
            }
        }

        Self { tree, stack }
    }
}

impl<T: RenderTreeAccess + ?Sized> Iterator for RenderChildren<'_, T> {
    type Item = ElementId;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let current = self.stack.pop()?;

            if !self.tree.contains(current) {
                continue;
            }

            if self.tree.is_render_element(current) {
                // Found a render child - don't recurse further
                return Some(current);
            }

            // Non-render element - look at its children
            // Use SmallVec to avoid heap allocation for typical cases
            let children: SmallVec<[ElementId; 8]> = self.tree.children(current).collect();
            for child in children.into_iter().rev() {
                self.stack.push(child);
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.stack.len()))
    }
}

impl<T: RenderTreeAccess + ?Sized> std::iter::FusedIterator for RenderChildren<'_, T> {}

// ============================================================================
// RENDER CHILDREN WITH INDEX ITERATOR
// ============================================================================

/// Iterator that yields render children with their index.
///
/// The index is the position among render children (0, 1, 2, ...),
/// not the position in the element tree.
///
/// # Use Case
///
/// Useful for layout algorithms that need child positions (e.g., Flex layout).
#[derive(Debug)]
pub struct RenderChildrenWithIndex<'a, T: RenderTreeAccess + ?Sized> {
    inner: RenderChildren<'a, T>,
    index: usize,
}

impl<'a, T: RenderTreeAccess + ?Sized> RenderChildrenWithIndex<'a, T> {
    /// Creates a new indexed render children iterator.
    #[inline]
    pub fn new(tree: &'a T, parent: ElementId) -> Self {
        Self {
            inner: RenderChildren::new(tree, parent),
            index: 0,
        }
    }
}

impl<T: RenderTreeAccess + ?Sized> Iterator for RenderChildrenWithIndex<'_, T> {
    type Item = (usize, ElementId);

    fn next(&mut self) -> Option<Self::Item> {
        let child = self.inner.next()?;
        let index = self.index;
        self.index += 1;
        Some((index, child))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<T: RenderTreeAccess + ?Sized> std::iter::FusedIterator for RenderChildrenWithIndex<'_, T> {}

// ============================================================================
// RENDER SIBLINGS ITERATOR
// ============================================================================

/// Direction for sibling iteration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum SiblingDirection {
    /// Iterate towards earlier siblings (left/previous).
    Previous,
    /// Iterate towards later siblings (right/next).
    #[default]
    Next,
    /// Iterate all siblings (excluding self).
    All,
}

/// Iterator over render siblings.
///
/// Finds other render children of the same render parent.
///
/// # Use Case
///
/// Hit testing often needs to check siblings. Layout algorithms
/// may need to query adjacent elements.
#[derive(Debug)]
pub struct RenderSiblings<'a, T: RenderTreeAccess + ?Sized> {
    _tree: &'a T,
    siblings: Vec<ElementId>,
    self_id: ElementId,
    current_idx: usize,
    direction: SiblingDirection,
    started: bool,
}

impl<'a, T: RenderTreeAccess + ?Sized> RenderSiblings<'a, T> {
    /// Creates a new render siblings iterator.
    pub fn new(tree: &'a T, id: ElementId, direction: SiblingDirection) -> Self {
        // Find render parent
        let render_parent = RenderAncestors::from_parent(tree, id).next();

        // Collect all render children of the render parent
        let siblings = if let Some(parent) = render_parent {
            RenderChildren::new(tree, parent).collect()
        } else {
            Vec::new()
        };

        // Find self position in siblings
        let self_idx = siblings.iter().position(|&s| s == id).unwrap_or(0);

        let current_idx = match direction {
            SiblingDirection::Previous => self_idx.saturating_sub(1),
            SiblingDirection::Next | SiblingDirection::All => {
                if self_idx + 1 < siblings.len() {
                    self_idx + 1
                } else {
                    siblings.len()
                }
            }
        };

        Self {
            _tree: tree,
            siblings,
            self_id: id,
            current_idx,
            direction,
            started: false,
        }
    }

    /// Creates iterator for previous siblings only.
    #[inline]
    pub fn previous(tree: &'a T, id: ElementId) -> Self {
        Self::new(tree, id, SiblingDirection::Previous)
    }

    /// Creates iterator for next siblings only.
    #[inline]
    pub fn next_siblings(tree: &'a T, id: ElementId) -> Self {
        Self::new(tree, id, SiblingDirection::Next)
    }

    /// Creates iterator for all siblings.
    #[inline]
    pub fn all(tree: &'a T, id: ElementId) -> Self {
        Self::new(tree, id, SiblingDirection::All)
    }
}

impl<T: RenderTreeAccess + ?Sized> Iterator for RenderSiblings<'_, T> {
    type Item = ElementId;

    fn next(&mut self) -> Option<Self::Item> {
        match self.direction {
            SiblingDirection::Previous => {
                if self.current_idx < self.siblings.len() {
                    let sibling = self.siblings[self.current_idx];
                    if self.current_idx > 0 {
                        self.current_idx -= 1;
                    } else {
                        self.current_idx = self.siblings.len(); // Exhausted
                    }
                    if sibling != self.self_id {
                        return Some(sibling);
                    }
                }
                None
            }
            SiblingDirection::Next => {
                while self.current_idx < self.siblings.len() {
                    let sibling = self.siblings[self.current_idx];
                    self.current_idx += 1;
                    if sibling != self.self_id {
                        return Some(sibling);
                    }
                }
                None
            }
            SiblingDirection::All => {
                while self.current_idx < self.siblings.len() {
                    let sibling = self.siblings[self.current_idx];
                    self.current_idx += 1;
                    if sibling != self.self_id {
                        return Some(sibling);
                    }
                }
                // After exhausting forward, restart from beginning
                if !self.started {
                    self.started = true;
                    self.current_idx = 0;
                    let self_idx = self
                        .siblings
                        .iter()
                        .position(|&s| s == self.self_id)
                        .unwrap_or(0);
                    while self.current_idx < self_idx {
                        let sibling = self.siblings[self.current_idx];
                        self.current_idx += 1;
                        if sibling != self.self_id {
                            return Some(sibling);
                        }
                    }
                }
                None
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.siblings.len().saturating_sub(1); // Minus self
        (0, Some(remaining))
    }
}

impl<T: RenderTreeAccess + ?Sized> std::iter::FusedIterator for RenderSiblings<'_, T> {}

// ============================================================================
// RENDER SUBTREE ITERATOR (BFS with depth)
// ============================================================================

/// Item yielded by `RenderSubtree` iterator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderSubtreeItem {
    /// The element ID.
    pub id: ElementId,
    /// Depth relative to root (root = 0).
    pub depth: usize,
}

/// BFS iterator over render subtree with depth information.
///
/// Unlike `RenderDescendants` (DFS), this uses breadth-first traversal
/// and provides depth information.
///
/// # Use Case
///
/// Useful for level-order operations, accessibility tree building,
/// or when you need to process parents before children.
#[derive(Debug)]
pub struct RenderSubtree<'a, T: RenderTreeAccess + ?Sized> {
    tree: &'a T,
    queue: std::collections::VecDeque<(ElementId, usize)>,
}

impl<'a, T: RenderTreeAccess + ?Sized> RenderSubtree<'a, T> {
    /// Creates a new BFS render subtree iterator.
    pub fn new(tree: &'a T, root: ElementId) -> Self {
        let mut queue = std::collections::VecDeque::with_capacity(16);

        if tree.contains(root) && tree.is_render_element(root) {
            queue.push_back((root, 0));
        }

        Self { tree, queue }
    }
}

impl<T: RenderTreeAccess + ?Sized> Iterator for RenderSubtree<'_, T> {
    type Item = RenderSubtreeItem;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some((current, depth)) = self.queue.pop_front() {
            if !self.tree.contains(current) {
                continue;
            }

            // Queue render children at next depth
            for child in RenderChildren::new(self.tree, current) {
                self.queue.push_back((child, depth + 1));
            }

            return Some(RenderSubtreeItem { id: current, depth });
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.queue.len(), None)
    }
}

impl<T: RenderTreeAccess + ?Sized> std::iter::FusedIterator for RenderSubtree<'_, T> {}

// ============================================================================
// RENDER LEAVES ITERATOR
// ============================================================================

/// Iterator that finds leaf render elements (elements with no render children).
///
/// # Use Case
///
/// Bottom-up layout algorithms start from leaves. Also useful for finding
/// text nodes or other terminal render elements.
#[derive(Debug)]
pub struct RenderLeaves<'a, T: RenderTreeAccess + ?Sized> {
    inner: RenderDescendants<'a, T>,
    tree: &'a T,
}

impl<'a, T: RenderTreeAccess + ?Sized> RenderLeaves<'a, T> {
    /// Creates a new render leaves iterator.
    #[inline]
    pub fn new(tree: &'a T, root: ElementId) -> Self {
        Self {
            inner: RenderDescendants::new(tree, root),
            tree,
        }
    }
}

impl<T: RenderTreeAccess + ?Sized> Iterator for RenderLeaves<'_, T> {
    type Item = ElementId;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let id = self.inner.next()?;

            // Check if this render element has any render children
            if RenderChildren::new(self.tree, id).next().is_none() {
                return Some(id);
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, None)
    }
}

impl<T: RenderTreeAccess + ?Sized> std::iter::FusedIterator for RenderLeaves<'_, T> {}

// ============================================================================
// RENDER PATH ITERATOR
// ============================================================================

/// Iterator that yields the path from root to a target element.
///
/// The path consists of all render elements between root and target,
/// yielded in root-to-target order.
///
/// # Use Case
///
/// Hit testing needs to know the full path from root to hit element.
/// Useful for focus management and accessibility.
#[derive(Debug)]
pub struct RenderPath<'a, T: RenderTreeAccess + ?Sized> {
    _tree: &'a T,
    path: Vec<ElementId>,
    current_idx: usize,
}

impl<'a, T: RenderTreeAccess + ?Sized> RenderPath<'a, T> {
    /// Creates a new render path iterator from root to target.
    pub fn new(tree: &'a T, target: ElementId) -> Self {
        // Collect path from target to root
        let path: Vec<_> = RenderAncestors::new(tree, target).collect();
        // Reverse to get root-to-target order
        let path: Vec<_> = path.into_iter().rev().collect();

        Self {
            _tree: tree,
            path,
            current_idx: 0,
        }
    }

    /// Returns the path length.
    #[inline]
    pub fn len(&self) -> usize {
        self.path.len()
    }

    /// Returns true if the path is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.path.is_empty()
    }

    /// Returns the path as a slice.
    #[inline]
    pub fn as_slice(&self) -> &[ElementId] {
        &self.path
    }
}

impl<T: RenderTreeAccess + ?Sized> Iterator for RenderPath<'_, T> {
    type Item = ElementId;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_idx < self.path.len() {
            let id = self.path[self.current_idx];
            self.current_idx += 1;
            Some(id)
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.path.len() - self.current_idx;
        (remaining, Some(remaining))
    }
}

impl<T: RenderTreeAccess + ?Sized> ExactSizeIterator for RenderPath<'_, T> {}
impl<T: RenderTreeAccess + ?Sized> std::iter::FusedIterator for RenderPath<'_, T> {}

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================

/// Finds the nearest render ancestor of an element.
///
/// Convenience function wrapping `RenderAncestors`.
#[inline]
pub fn find_render_ancestor<T: RenderTreeAccess + ?Sized>(
    tree: &T,
    id: ElementId,
) -> Option<ElementId> {
    RenderAncestors::from_parent(tree, id).next()
}

/// Finds the render parent of an element.
///
/// Same as `find_render_ancestor` but more semantic.
#[inline]
pub fn render_parent<T: RenderTreeAccess + ?Sized>(tree: &T, id: ElementId) -> Option<ElementId> {
    find_render_ancestor(tree, id)
}

/// Collects all render children of a render element.
#[inline]
pub fn collect_render_children<T: RenderTreeAccess + ?Sized>(
    tree: &T,
    parent: ElementId,
) -> Vec<ElementId> {
    RenderChildren::new(tree, parent).collect()
}

/// Counts render elements in a subtree.
#[inline]
pub fn count_render_elements<T: RenderTreeAccess + ?Sized>(tree: &T, root: ElementId) -> usize {
    RenderDescendants::new(tree, root).count()
}

/// Counts render children of an element.
#[inline]
pub fn count_render_children<T: RenderTreeAccess + ?Sized>(tree: &T, parent: ElementId) -> usize {
    RenderChildren::new(tree, parent).count()
}

/// Finds the first render child of an element.
#[inline]
pub fn first_render_child<T: RenderTreeAccess + ?Sized>(
    tree: &T,
    parent: ElementId,
) -> Option<ElementId> {
    RenderChildren::new(tree, parent).next()
}

/// Finds the last render child of an element.
#[inline]
pub fn last_render_child<T: RenderTreeAccess + ?Sized>(
    tree: &T,
    parent: ElementId,
) -> Option<ElementId> {
    RenderChildren::new(tree, parent).last()
}

/// Finds the nth render child of an element.
#[inline]
pub fn nth_render_child<T: RenderTreeAccess + ?Sized>(
    tree: &T,
    parent: ElementId,
    n: usize,
) -> Option<ElementId> {
    RenderChildren::new(tree, parent).nth(n)
}

/// Checks if an element has any render children.
#[inline]
pub fn has_render_children<T: RenderTreeAccess + ?Sized>(tree: &T, parent: ElementId) -> bool {
    RenderChildren::new(tree, parent).next().is_some()
}

/// Checks if element is a render leaf (no render children).
#[inline]
pub fn is_render_leaf<T: RenderTreeAccess + ?Sized>(tree: &T, id: ElementId) -> bool {
    tree.is_render_element(id) && !has_render_children(tree, id)
}

/// Finds the render root (topmost render ancestor).
#[inline]
pub fn find_render_root<T: RenderTreeAccess + ?Sized>(
    tree: &T,
    id: ElementId,
) -> Option<ElementId> {
    RenderAncestors::new(tree, id).last()
}

/// Calculates render depth (number of render ancestors including self).
#[inline]
pub fn render_depth<T: RenderTreeAccess + ?Sized>(tree: &T, id: ElementId) -> usize {
    RenderAncestors::new(tree, id).count()
}

/// Checks if `descendant` is a render descendant of `ancestor`.
#[inline]
pub fn is_render_descendant<T: RenderTreeAccess + ?Sized>(
    tree: &T,
    descendant: ElementId,
    ancestor: ElementId,
) -> bool {
    RenderAncestors::new(tree, descendant).any(|id| id == ancestor)
}

/// Finds the lowest common render ancestor of two elements.
pub fn lowest_common_render_ancestor<T: RenderTreeAccess + ?Sized>(
    tree: &T,
    a: ElementId,
    b: ElementId,
) -> Option<ElementId> {
    // Collect ancestors of `a` into a set
    let a_ancestors: std::collections::HashSet<_> = RenderAncestors::new(tree, a).collect();

    // Find first ancestor of `b` that's also an ancestor of `a`
    RenderAncestors::new(tree, b).find(|&id| a_ancestors.contains(&id))
}

// ============================================================================
// ARITY-AWARE COLLECTION
// ============================================================================

/// Collects render children into an owned buffer for arity-aware access.
///
/// This collector bridges the lazy iteration of [`RenderChildren`] with the
/// compile-time arity system. Use it when you need to validate child count
/// at runtime and get a typed accessor.
///
/// # Example
///
/// ```rust,ignore
/// use flui_tree::{RenderChildrenCollector, Single, Variable, Arity};
///
/// let collector = RenderChildrenCollector::new(tree, parent_id);
///
/// // Try to get exactly one child
/// match collector.try_into_single() {
///     Some(children) => {
///         let child_id = *children.single();
///         // layout single child...
///     }
///     None => {
///         // Handle wrong child count
///     }
/// }
///
/// // Or get children with any arity
/// let collector = RenderChildrenCollector::new(tree, parent_id);
/// let children = collector.into_variable();
/// for &child_id in children.iter() {
///     // layout child...
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RenderChildrenCollector {
    children: Vec<ElementId>,
}

impl RenderChildrenCollector {
    /// Creates a new collector by eagerly collecting render children.
    pub fn new<T: RenderTreeAccess + ?Sized>(tree: &T, parent: ElementId) -> Self {
        Self {
            children: collect_render_children(tree, parent),
        }
    }

    /// Creates a collector with a pre-allocated capacity.
    pub fn with_capacity<T: RenderTreeAccess + ?Sized>(
        tree: &T,
        parent: ElementId,
        capacity: usize,
    ) -> Self {
        let mut children = Vec::with_capacity(capacity);
        children.extend(RenderChildren::new(tree, parent));
        Self { children }
    }

    /// Returns the number of collected children.
    #[inline]
    pub fn len(&self) -> usize {
        self.children.len()
    }

    /// Returns true if no children were collected.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    /// Returns the children as a slice.
    #[inline]
    pub fn as_slice(&self) -> &[ElementId] {
        &self.children
    }

    /// Tries to convert to a typed accessor for the given arity.
    ///
    /// Returns `None` if the child count doesn't match the arity.
    #[inline]
    pub fn try_into_arity<A: Arity>(&self) -> Option<A::Accessor<'_, ElementId>> {
        A::try_from_slice(&self.children)
    }

    /// Converts to an accessor, panicking in debug mode if count doesn't match.
    ///
    /// # Panics (debug only)
    ///
    /// Panics in debug builds if the child count doesn't match the arity.
    #[inline]
    pub fn into_arity<A: Arity>(&self) -> A::Accessor<'_, ElementId> {
        A::from_slice(&self.children)
    }

    /// Converts to a [`SliceChildren`] accessor (always succeeds).
    ///
    /// This is the most flexible conversion - use when you don't know
    /// the child count at compile time.
    #[inline]
    pub fn into_variable(&self) -> SliceChildren<'_, ElementId> {
        Variable::from_slice(&self.children)
    }

    /// Consumes the collector and returns the underlying `Vec`.
    #[inline]
    pub fn into_vec(self) -> Vec<ElementId> {
        self.children
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_tree::iter::{Ancestors, DescendantsWithDepth};

    use flui_tree::{TreeNav, TreeRead};
    use std::any::Any;

    struct TestNode {
        parent: Option<ElementId>,
        children: Vec<ElementId>,
        is_render: bool,
    }

    struct TestTree {
        nodes: Vec<Option<TestNode>>,
    }

    impl TestTree {
        fn new() -> Self {
            Self { nodes: Vec::new() }
        }

        fn insert(&mut self, parent: Option<ElementId>, is_render: bool) -> ElementId {
            let id = ElementId::new(self.nodes.len() + 1);
            self.nodes.push(Some(TestNode {
                parent,
                children: Vec::new(),
                is_render,
            }));

            if let Some(parent_id) = parent {
                if let Some(Some(p)) = self.nodes.get_mut(parent_id.get() as usize - 1) {
                    p.children.push(id);
                }
            }

            id
        }

        fn insert_render(&mut self, parent: Option<ElementId>) -> ElementId {
            self.insert(parent, true)
        }

        fn insert_component(&mut self, parent: Option<ElementId>) -> ElementId {
            self.insert(parent, false)
        }
    }

    impl TreeRead<ElementId> for TestTree {
        type Node = TestNode;

        fn get(&self, id: ElementId) -> Option<&TestNode> {
            self.nodes.get(id.get() as usize - 1)?.as_ref()
        }

        fn len(&self) -> usize {
            self.nodes.iter().filter(|n| n.is_some()).count()
        }

        fn node_ids(&self) -> impl Iterator<Item = ElementId> + '_ {
            (0..self.nodes.len()).filter_map(|i| {
                if self.nodes[i].is_some() {
                    Some(ElementId::new(i + 1))
                } else {
                    None
                }
            })
        }
    }

    impl TreeNav<ElementId> for TestTree {
        fn parent(&self, id: ElementId) -> Option<ElementId> {
            self.get(id)?.parent
        }

        fn children(&self, id: ElementId) -> impl Iterator<Item = ElementId> + '_ {
            self.get(id)
                .map(|node| node.children.iter().copied())
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
            let parent = self.parent(id);
            parent
                .into_iter()
                .flat_map(move |p| self.children(p).filter(move |&c| c != id))
        }
    }

    impl RenderTreeAccess for TestTree {
        fn render_object(&self, id: ElementId) -> Option<&dyn Any> {
            if self.get(id)?.is_render {
                Some(&() as &dyn Any)
            } else {
                None
            }
        }

        fn render_object_mut(&mut self, id: ElementId) -> Option<&mut dyn Any> {
            None
        }

        fn render_state(&self, id: ElementId) -> Option<&dyn Any> {
            self.render_object(id)
        }

        fn render_state_mut(&mut self, id: ElementId) -> Option<&mut dyn Any> {
            None
        }
    }

    #[test]
    fn test_render_ancestors() {
        let mut tree = TestTree::new();

        // Build: render1 -> component -> render2
        let render1 = tree.insert_render(None);
        let component = tree.insert_component(Some(render1));
        let render2 = tree.insert_render(Some(component));

        let ancestors: Vec<_> = RenderAncestors::new(&tree, render2).collect();
        assert_eq!(ancestors, vec![render2, render1]);
    }

    #[test]
    fn test_render_ancestors_from_parent() {
        let mut tree = TestTree::new();

        let render1 = tree.insert_render(None);
        let component = tree.insert_component(Some(render1));
        let render2 = tree.insert_render(Some(component));

        // Should skip render2 and start from parent
        let ancestors: Vec<_> = RenderAncestors::from_parent(&tree, render2).collect();
        assert_eq!(ancestors, vec![render1]);
    }

    #[test]
    fn test_render_descendants() {
        let mut tree = TestTree::new();

        // Build: render1 -> [component -> render2, render3]
        let render1 = tree.insert_render(None);
        let component = tree.insert_component(Some(render1));
        let render2 = tree.insert_render(Some(component));
        let render3 = tree.insert_render(Some(render1));

        let descendants: Vec<_> = RenderDescendants::new(&tree, render1).collect();
        assert_eq!(descendants, vec![render1, render2, render3]);
    }

    #[test]
    fn test_render_children() {
        let mut tree = TestTree::new();

        // Build: render1 -> [component -> [render2, render3], render4]
        let render1 = tree.insert_render(None);
        let component = tree.insert_component(Some(render1));
        let render2 = tree.insert_render(Some(component));
        let render3 = tree.insert_render(Some(component));
        let render4 = tree.insert_render(Some(render1));

        let children: Vec<_> = RenderChildren::new(&tree, render1).collect();
        // Should find render2, render3 (through component), and render4
        assert_eq!(children.len(), 3);
        assert!(children.contains(&render2));
        assert!(children.contains(&render3));
        assert!(children.contains(&render4));
    }

    #[test]
    fn test_render_children_with_index() {
        let mut tree = TestTree::new();

        let render1 = tree.insert_render(None);
        let render2 = tree.insert_render(Some(render1));
        let render3 = tree.insert_render(Some(render1));

        let indexed: Vec<_> = RenderChildrenWithIndex::new(&tree, render1).collect();
        assert_eq!(indexed.len(), 2);
        assert_eq!(indexed[0].0, 0);
        assert_eq!(indexed[1].0, 1);
    }

    #[test]
    fn test_render_subtree_bfs() {
        let mut tree = TestTree::new();

        // Build: render1 -> [render2 -> render4, render3]
        let render1 = tree.insert_render(None);
        let render2 = tree.insert_render(Some(render1));
        let render3 = tree.insert_render(Some(render1));
        let render4 = tree.insert_render(Some(render2));

        let items: Vec<_> = RenderSubtree::new(&tree, render1).collect();

        // BFS order: render1 (depth 0), render2 (depth 1), render3 (depth 1), render4 (depth 2)
        assert_eq!(items.len(), 4);
        assert_eq!(
            items[0],
            RenderSubtreeItem {
                id: render1,
                depth: 0
            }
        );
        assert_eq!(items[1].depth, 1);
        assert_eq!(items[2].depth, 1);
        assert_eq!(items[3].depth, 2);
    }

    #[test]
    fn test_render_leaves() {
        let mut tree = TestTree::new();

        // Build: render1 -> [render2 -> render4, render3]
        let render1 = tree.insert_render(None);
        let render2 = tree.insert_render(Some(render1));
        let render3 = tree.insert_render(Some(render1));
        let render4 = tree.insert_render(Some(render2));

        let leaves: Vec<_> = RenderLeaves::new(&tree, render1).collect();

        // render3 and render4 are leaves
        assert_eq!(leaves.len(), 2);
        assert!(leaves.contains(&render3));
        assert!(leaves.contains(&render4));
    }

    #[test]
    fn test_render_path() {
        let mut tree = TestTree::new();

        let render1 = tree.insert_render(None);
        let component = tree.insert_component(Some(render1));
        let render2 = tree.insert_render(Some(component));
        let render3 = tree.insert_render(Some(render2));

        let path = RenderPath::new(&tree, render3);

        assert_eq!(path.len(), 3);
        let path_vec: Vec<_> = path.collect();
        assert_eq!(path_vec, vec![render1, render2, render3]);
    }

    #[test]
    fn test_find_render_ancestor() {
        let mut tree = TestTree::new();

        let render1 = tree.insert_render(None);
        let component = tree.insert_component(Some(render1));
        let render2 = tree.insert_render(Some(component));

        assert_eq!(find_render_ancestor(&tree, render2), Some(render1));
        assert_eq!(find_render_ancestor(&tree, render1), None);
    }

    #[test]
    fn test_count_render_elements() {
        let mut tree = TestTree::new();

        let render1 = tree.insert_render(None);
        let component = tree.insert_component(Some(render1));
        let render2 = tree.insert_render(Some(component));
        let render3 = tree.insert_render(Some(render1));

        assert_eq!(count_render_elements(&tree, render1), 3);
    }

    #[test]
    fn test_is_render_leaf() {
        let mut tree = TestTree::new();

        let render1 = tree.insert_render(None);
        let render2 = tree.insert_render(Some(render1));

        assert!(!is_render_leaf(&tree, render1));
        assert!(is_render_leaf(&tree, render2));
    }

    #[test]
    fn test_lowest_common_render_ancestor() {
        let mut tree = TestTree::new();

        // Build: render1 -> [render2 -> render4, render3 -> render5]
        let render1 = tree.insert_render(None);
        let render2 = tree.insert_render(Some(render1));
        let render3 = tree.insert_render(Some(render1));
        let render4 = tree.insert_render(Some(render2));
        let render5 = tree.insert_render(Some(render3));

        assert_eq!(
            lowest_common_render_ancestor(&tree, render4, render5),
            Some(render1)
        );
        assert_eq!(
            lowest_common_render_ancestor(&tree, render4, render2),
            Some(render2)
        );
    }

    #[test]
    fn test_render_children_collector() {
        use crate::arity::{ChildrenAccess, Exact, Single};

        let mut tree = TestTree::new();

        // Build: render1 -> [render2, render3]
        let render1 = tree.insert_render(None);
        let render2 = tree.insert_render(Some(render1));
        let render3 = tree.insert_render(Some(render1));

        let collector = RenderChildrenCollector::new(&tree, render1);
        assert_eq!(collector.len(), 2);
        assert!(!collector.is_empty());

        // Should fail for Single (needs exactly 1)
        assert!(collector.try_into_arity::<Single>().is_none());

        // Should succeed for Exact<2>
        let children = collector.try_into_arity::<Exact<2>>().unwrap();
        assert_eq!(children.first(), &render2);
        assert_eq!(children.second(), &render3);

        // Variable should always work
        let children = collector.into_variable();
        assert_eq!(children.len(), 2);

        // Iterate by value using copied()
        let ids: Vec<_> = children.copied().collect();
        assert_eq!(ids, vec![render2, render3]);
    }

    #[test]
    fn test_render_children_collector_single() {
        use crate::arity::Single;

        let mut tree = TestTree::new();

        // Build: render1 -> render2 (single child)
        let render1 = tree.insert_render(None);
        let render2 = tree.insert_render(Some(render1));

        let collector = RenderChildrenCollector::new(&tree, render1);
        assert_eq!(collector.len(), 1);

        // Should succeed for Single
        let children = collector.try_into_arity::<Single>().unwrap();
        assert_eq!(children.single(), &render2);
    }

    #[test]
    fn test_render_children_collector_empty() {
        use crate::arity::{Leaf, Optional};

        let mut tree = TestTree::new();

        // Build: render1 (no children)
        let render1 = tree.insert_render(None);

        let collector = RenderChildrenCollector::new(&tree, render1);
        assert!(collector.is_empty());
        assert_eq!(collector.len(), 0);

        // Should succeed for Leaf
        assert!(collector.try_into_arity::<Leaf>().is_some());

        // Should succeed for Optional
        let opt = collector.try_into_arity::<Optional>().unwrap();
        assert!(opt.is_none());
    }
}
