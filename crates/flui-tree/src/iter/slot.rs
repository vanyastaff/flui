//! Slot - position in parent's child list with tree context.
//!
//! This module provides `Slot` which represents a child's position within
//! its parent, including sibling navigation and depth information.
//!
//! # Overview
//!
//! ```text
//! Parent Node
//!   ├── [Slot 0] Child A  ← Slot { parent, index: 0, prev: None, next: Some(B) }
//!   ├── [Slot 1] Child B  ← Slot { parent, index: 1, prev: Some(A), next: Some(C) }
//!   └── [Slot 2] Child C  ← Slot { parent, index: 2, prev: Some(B), next: None }
//! ```
//!
//! # Usage
//!
//! ```
//! use flui_tree::{Slot, Depth};
//! use flui_foundation::ElementId;
//!
//! // Create slot for second child
//! let slot = Slot::new(
//!     ElementId::new(10),  // parent
//!     1,                   // second child (0-indexed)
//!     Depth::new(2),       // at depth 2
//! );
//!
//! assert_eq!(slot.index(), 1);
//! assert!(!slot.is_first_child());
//! ```

use std::fmt;

use crate::depth::Depth;
use flui_foundation::Identifier;

// ============================================================================
// SLOT
// ============================================================================

/// Slot - position in parent's child list with tree context.
///
/// `Slot` represents where a child is positioned within its parent,
/// along with additional context useful for tree operations:
///
/// - Parent ID (strongly typed)
/// - Sibling references for O(1) navigation
/// - Depth information
///
/// # Design Philosophy
///
/// `Slot` is "tree-aware" and provides rich context for
/// operations like:
///
/// - Child reconciliation during rebuilds
/// - Efficient sibling traversal
/// - Layout positioning
///
/// # Memory Layout
///
/// ```text
/// Slot<ElementId> = 40 bytes
///   - parent: 8 bytes
///   - index: 8 bytes
///   - depth: 8 bytes
///   - previous_sibling: Option<I> = 8 bytes (niche optimized)
///   - next_sibling: Option<I> = 8 bytes
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Slot<I: Identifier> {
    /// Parent node ID.
    parent: I,
    /// Position within parent's children (0-based).
    index: usize,
    /// Depth in tree.
    depth: Depth,
    /// Previous sibling ID (for O(1) backward navigation).
    previous_sibling: Option<I>,
    /// Next sibling ID (for O(1) forward navigation).
    next_sibling: Option<I>,
}

impl<I: Identifier> Slot<I> {
    // === CONSTRUCTORS ===

    /// Creates new slot with minimal data.
    ///
    /// # Arguments
    ///
    /// * `parent` - Parent node ID
    /// * `index` - Position within parent (0-based)
    /// * `depth` - Depth in tree
    #[inline]
    #[must_use]
    pub fn new(parent: I, index: usize, depth: Depth) -> Self {
        Self {
            parent,
            index,
            depth,
            previous_sibling: None,
            next_sibling: None,
        }
    }

    /// Creates slot with sibling information.
    ///
    /// This is the "full" constructor used during reconciliation
    /// when sibling information is available.
    #[inline]
    #[must_use]
    pub fn with_siblings(
        parent: I,
        index: usize,
        depth: Depth,
        previous_sibling: Option<I>,
        next_sibling: Option<I>,
    ) -> Self {
        Self {
            parent,
            index,
            depth,
            previous_sibling,
            next_sibling,
        }
    }

    /// Creates a builder for more complex slot construction.
    #[inline]
    #[must_use]
    pub fn builder(parent: I, index: usize, depth: Depth) -> SlotBuilder<I> {
        SlotBuilder::new(parent, index, depth)
    }

    // === ACCESSORS ===

    /// Returns the parent node ID.
    #[inline]
    #[must_use]
    pub fn parent(&self) -> I {
        self.parent
    }

    /// Returns the index (position within parent).
    #[inline]
    #[must_use]
    pub fn index(&self) -> usize {
        self.index
    }

    /// Returns the depth in tree.
    #[inline]
    #[must_use]
    pub fn depth(&self) -> Depth {
        self.depth
    }

    /// Returns the previous sibling ID.
    #[inline]
    #[must_use]
    pub fn previous_sibling(&self) -> Option<I> {
        self.previous_sibling
    }

    /// Returns the next sibling ID.
    #[inline]
    #[must_use]
    pub fn next_sibling(&self) -> Option<I> {
        self.next_sibling
    }

    // === POSITION QUERIES ===

    /// Returns true if this is the first child (index 0).
    #[inline]
    #[must_use]
    pub fn is_first_child(&self) -> bool {
        self.index == 0
    }

    /// Returns true if this is the last child (no next sibling).
    #[inline]
    #[must_use]
    pub fn is_last_child(&self) -> bool {
        self.next_sibling.is_none()
    }

    /// Returns true if this is an only child (first and last).
    #[inline]
    #[must_use]
    pub fn is_only_child(&self) -> bool {
        self.is_first_child() && self.is_last_child()
    }

    /// Returns true if sibling tracking is available.
    ///
    /// Sibling tracking is considered available if:
    /// - This is the first child (previous is implicitly None), or
    /// - Previous sibling is explicitly set, or
    /// - Next sibling is explicitly set
    #[inline]
    #[must_use]
    pub fn has_sibling_tracking(&self) -> bool {
        self.previous_sibling.is_some() || self.next_sibling.is_some() || self.is_first_child()
    }

    /// Returns true if this node has any siblings.
    #[inline]
    #[must_use]
    pub fn has_siblings(&self) -> bool {
        self.previous_sibling.is_some() || self.next_sibling.is_some()
    }

    // === MUTATORS ===

    /// Sets previous sibling.
    #[inline]
    pub fn set_previous_sibling(&mut self, sibling: Option<I>) {
        self.previous_sibling = sibling;
    }

    /// Sets next sibling.
    #[inline]
    pub fn set_next_sibling(&mut self, sibling: Option<I>) {
        self.next_sibling = sibling;
    }

    /// Updates the index (after reordering).
    #[inline]
    pub fn set_index(&mut self, index: usize) {
        self.index = index;
    }

    // === NAVIGATION ===

    /// Creates slot for next sibling position.
    ///
    /// Useful during child iteration to create slot
    /// for the next child.
    ///
    /// # Arguments
    ///
    /// * `self_id` - ID of the current node (becomes previous sibling)
    #[inline]
    #[must_use]
    pub fn next_slot(&self, self_id: I) -> Self {
        Self {
            parent: self.parent,
            index: self.index + 1,
            depth: self.depth,
            previous_sibling: Some(self_id),
            next_sibling: None,
        }
    }

    /// Creates slot for previous sibling position.
    ///
    /// Returns `None` if this is already the first child.
    ///
    /// # Arguments
    ///
    /// * `self_id` - ID of the current node (becomes next sibling)
    #[inline]
    #[must_use]
    pub fn prev_slot(&self, self_id: I) -> Option<Self> {
        if self.index == 0 {
            return None;
        }
        Some(Self {
            parent: self.parent,
            index: self.index - 1,
            depth: self.depth,
            previous_sibling: None,
            next_sibling: Some(self_id),
        })
    }

    /// Creates slot for a child of this node's element.
    ///
    /// # Arguments
    ///
    /// * `self_id` - ID of the current node (becomes parent)
    /// * `child_index` - Index of the child
    #[inline]
    #[must_use]
    pub fn child_slot(&self, self_id: I, child_index: usize) -> Self {
        Self {
            parent: self_id,
            index: child_index,
            depth: self.depth.child_depth(),
            previous_sibling: None,
            next_sibling: None,
        }
    }
}

impl<I: Identifier> fmt::Display for Slot<I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Slot(parent={}, index={}, depth={})",
            self.parent, self.index, self.depth
        )
    }
}

// ============================================================================
// SLOT BUILDER
// ============================================================================

/// Builder for `Slot` with optional fields.
///
/// # Example
///
/// ```
/// use flui_tree::{Slot, Depth};
/// use flui_foundation::ElementId;
///
/// let slot = Slot::builder(
///     ElementId::new(1),
///     2,
///     Depth::new(3),
/// )
///     .with_previous_sibling(ElementId::new(5))
///     .with_next_sibling(ElementId::new(7))
///     .build();
///
/// assert_eq!(slot.parent(), ElementId::new(1));
/// assert_eq!(slot.index(), 2);
/// assert_eq!(slot.previous_sibling(), Some(ElementId::new(5)));
/// assert_eq!(slot.next_sibling(), Some(ElementId::new(7)));
/// ```
#[derive(Debug, Clone)]
pub struct SlotBuilder<I: Identifier> {
    parent: I,
    index: usize,
    depth: Depth,
    previous_sibling: Option<I>,
    next_sibling: Option<I>,
}

impl<I: Identifier> SlotBuilder<I> {
    /// Creates a new builder.
    #[inline]
    #[must_use]
    pub fn new(parent: I, index: usize, depth: Depth) -> Self {
        Self {
            parent,
            index,
            depth,
            previous_sibling: None,
            next_sibling: None,
        }
    }

    /// Sets previous sibling.
    #[inline]
    #[must_use]
    pub fn with_previous_sibling(mut self, sibling: I) -> Self {
        self.previous_sibling = Some(sibling);
        self
    }

    /// Sets next sibling.
    #[inline]
    #[must_use]
    pub fn with_next_sibling(mut self, sibling: I) -> Self {
        self.next_sibling = Some(sibling);
        self
    }

    /// Sets both siblings at once.
    #[inline]
    #[must_use]
    pub fn with_siblings(mut self, previous: Option<I>, next: Option<I>) -> Self {
        self.previous_sibling = previous;
        self.next_sibling = next;
        self
    }

    /// Builds the `Slot`.
    #[inline]
    #[must_use]
    pub fn build(self) -> Slot<I> {
        Slot {
            parent: self.parent,
            index: self.index,
            depth: self.depth,
            previous_sibling: self.previous_sibling,
            next_sibling: self.next_sibling,
        }
    }
}

// ============================================================================
// INDEXED SLOT
// ============================================================================

/// Indexed slot for efficient child reconciliation.
///
/// This mirrors Flutter's `IndexedSlot` pattern used in
/// `updateChildren()` for O(1) child insertion.
///
/// The key insight is: when inserting a child, you need to know
/// both the index AND the previous sibling to insert after.
/// Keeping both together enables O(1) insertion in linked structures.
///
/// # Example
///
/// ```
/// use flui_tree::IndexedSlot;
/// use flui_foundation::ElementId;
///
/// // Start with first slot
/// let slot = IndexedSlot::<ElementId>::first();
/// assert_eq!(slot.index(), 0);
/// assert!(slot.previous().is_none());
///
/// // After mounting a child, advance to the next slot
/// let child1_id = ElementId::new(1);
/// let slot = slot.next(child1_id);
/// assert_eq!(slot.index(), 1);
/// assert_eq!(slot.previous(), Some(child1_id));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IndexedSlot<I: Identifier> {
    /// Position index (0-based).
    index: usize,
    /// Previous sibling for O(1) insertion.
    previous: Option<I>,
}

impl<I: Identifier> IndexedSlot<I> {
    /// Creates new indexed slot.
    #[inline]
    #[must_use]
    pub const fn new(index: usize, previous: Option<I>) -> Self {
        Self { index, previous }
    }

    /// Creates first slot (index 0, no previous).
    #[inline]
    #[must_use]
    pub const fn first() -> Self {
        Self {
            index: 0,
            previous: None,
        }
    }

    /// Gets the index.
    #[inline]
    #[must_use]
    pub const fn index(&self) -> usize {
        self.index
    }

    /// Gets the previous sibling ID.
    #[inline]
    #[must_use]
    pub const fn previous(&self) -> Option<I> {
        self.previous
    }

    /// Returns true if this is the first slot.
    #[inline]
    #[must_use]
    pub const fn is_first(&self) -> bool {
        self.index == 0
    }

    /// Creates the next indexed slot.
    ///
    /// # Arguments
    ///
    /// * `current_id` - ID of the node at current slot (becomes previous)
    #[inline]
    #[must_use]
    pub fn next(self, current_id: I) -> Self {
        Self {
            index: self.index + 1,
            previous: Some(current_id),
        }
    }

    /// Creates previous indexed slot.
    ///
    /// Returns `None` if this is already the first slot.
    ///
    /// Note: The previous sibling of the previous slot is not known,
    /// so it's set to `None`.
    #[inline]
    #[must_use]
    pub fn prev(self) -> Option<Self> {
        if self.index == 0 {
            None
        } else {
            Some(Self {
                index: self.index - 1,
                previous: None, // Unknown
            })
        }
    }

    /// Creates with a specific previous sibling.
    #[inline]
    #[must_use]
    pub fn with_previous(mut self, previous: I) -> Self {
        self.previous = Some(previous);
        self
    }
}

impl<I: Identifier> Default for IndexedSlot<I> {
    fn default() -> Self {
        Self::first()
    }
}

impl<I: Identifier> fmt::Display for IndexedSlot<I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.previous {
            Some(prev) => write!(f, "IndexedSlot({}, after {})", self.index, prev),
            None => write!(f, "IndexedSlot({})", self.index),
        }
    }
}

impl<I: Identifier> From<usize> for IndexedSlot<I> {
    fn from(index: usize) -> Self {
        Self::new(index, None)
    }
}

// ============================================================================
// SLOT ITERATOR
// ============================================================================

/// Iterator that produces indexed slots for child mounting.
///
/// This is useful when mounting multiple children in order,
/// automatically tracking the previous sibling.
///
/// # Example
///
/// ```
/// use flui_tree::SlotIter;
/// use flui_foundation::ElementId;
///
/// let mut slots = SlotIter::<ElementId>::new();
/// assert_eq!(slots.index(), 0);
///
/// // Simulate mounting children in order
/// let child_ids = [ElementId::new(1), ElementId::new(2), ElementId::new(3)];
/// for &child_id in &child_ids {
///     let slot = slots.current();
///     // Use slot.index() and slot.previous() during mount...
///     slots.advance(child_id);
/// }
///
/// assert_eq!(slots.index(), 3);
/// assert_eq!(slots.current().previous(), Some(ElementId::new(3)));
/// ```
#[derive(Debug, Clone)]
pub struct SlotIter<I: Identifier> {
    current: IndexedSlot<I>,
}

impl<I: Identifier> SlotIter<I> {
    /// Creates a new slot iterator starting at index 0.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self {
            current: IndexedSlot::first(),
        }
    }

    /// Creates starting at a specific index.
    #[inline]
    #[must_use]
    pub fn starting_at(index: usize) -> Self {
        Self {
            current: IndexedSlot::new(index, None),
        }
    }

    /// Returns the current slot.
    #[inline]
    #[must_use]
    pub fn current(&self) -> IndexedSlot<I> {
        self.current
    }

    /// Returns the current index.
    #[inline]
    #[must_use]
    pub fn index(&self) -> usize {
        self.current.index
    }

    /// Advances to the next slot.
    ///
    /// # Arguments
    ///
    /// * `current_id` - ID of the node mounted at current slot
    #[inline]
    pub fn advance(&mut self, current_id: I) {
        self.current = self.current.next(current_id);
    }

    /// Resets to the beginning.
    #[inline]
    pub fn reset(&mut self) {
        self.current = IndexedSlot::first();
    }
}

impl<I: Identifier> Default for SlotIter<I> {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_foundation::ElementId;

    // === SLOT TESTS ===

    #[test]
    fn test_slot_new() {
        let parent = ElementId::new(1);
        let slot = Slot::new(parent, 2, Depth::new(3));

        assert_eq!(slot.parent(), parent);
        assert_eq!(slot.index(), 2);
        assert_eq!(slot.depth(), Depth::new(3));
        assert!(slot.previous_sibling().is_none());
        assert!(slot.next_sibling().is_none());
    }

    #[test]
    fn test_slot_with_siblings() {
        let parent = ElementId::new(1);
        let prev = ElementId::new(5);
        let next = ElementId::new(7);

        let slot = Slot::with_siblings(parent, 2, Depth::new(1), Some(prev), Some(next));

        assert_eq!(slot.previous_sibling(), Some(prev));
        assert_eq!(slot.next_sibling(), Some(next));
        assert!(slot.has_sibling_tracking());
        assert!(slot.has_siblings());
    }

    #[test]
    fn test_slot_first_child() {
        let parent = ElementId::new(1);
        let slot = Slot::new(parent, 0, Depth::new(1));

        assert!(slot.is_first_child());
        assert!(slot.has_sibling_tracking()); // First child implicitly has tracking
    }

    #[test]
    fn test_slot_last_child() {
        let parent = ElementId::new(1);
        let slot = Slot::new(parent, 5, Depth::new(1));

        assert!(!slot.is_first_child());
        assert!(slot.is_last_child()); // No next sibling set
    }

    #[test]
    fn test_slot_only_child() {
        let parent = ElementId::new(1);
        let slot = Slot::new(parent, 0, Depth::new(1));

        assert!(slot.is_only_child()); // First and no next
    }

    #[test]
    fn test_slot_builder() {
        let parent = ElementId::new(1);
        let prev = ElementId::new(5);
        let next = ElementId::new(7);

        let slot = Slot::builder(parent, 1, Depth::new(2))
            .with_previous_sibling(prev)
            .with_next_sibling(next)
            .build();

        assert_eq!(slot.parent(), parent);
        assert_eq!(slot.index(), 1);
        assert_eq!(slot.depth(), Depth::new(2));
        assert_eq!(slot.previous_sibling(), Some(prev));
        assert_eq!(slot.next_sibling(), Some(next));
    }

    #[test]
    fn test_slot_next() {
        let parent = ElementId::new(1);
        let self_id = ElementId::new(10);
        let slot = Slot::new(parent, 0, Depth::new(1));

        let next = slot.next_slot(self_id);

        assert_eq!(next.parent(), parent);
        assert_eq!(next.index(), 1);
        assert_eq!(next.depth(), Depth::new(1));
        assert_eq!(next.previous_sibling(), Some(self_id));
    }

    #[test]
    fn test_slot_prev() {
        let parent = ElementId::new(1);
        let self_id = ElementId::new(10);
        let slot = Slot::new(parent, 2, Depth::new(1));

        let prev = slot.prev_slot(self_id).unwrap();

        assert_eq!(prev.index(), 1);
        assert_eq!(prev.next_sibling(), Some(self_id));

        // First child has no previous
        let first = Slot::new(parent, 0, Depth::new(1));
        assert!(first.prev_slot(self_id).is_none());
    }

    #[test]
    fn test_slot_child() {
        let parent = ElementId::new(1);
        let self_id = ElementId::new(10);
        let slot = Slot::new(parent, 0, Depth::new(2));

        let child = slot.child_slot(self_id, 0);

        assert_eq!(child.parent(), self_id);
        assert_eq!(child.index(), 0);
        assert_eq!(child.depth(), Depth::new(3)); // Parent depth + 1
    }

    #[test]
    fn test_slot_display() {
        let slot = Slot::new(ElementId::new(1), 2, Depth::new(3));
        let display = format!("{}", slot);
        assert!(display.contains("index=2"));
        assert!(display.contains("depth=3"));
    }

    // === INDEXED SLOT TESTS ===

    #[test]
    fn test_indexed_slot_first() {
        let slot = IndexedSlot::<ElementId>::first();
        assert_eq!(slot.index(), 0);
        assert!(slot.previous().is_none());
        assert!(slot.is_first());
    }

    #[test]
    fn test_indexed_slot_next() {
        let slot = IndexedSlot::<ElementId>::first();
        let next = slot.next(ElementId::new(1));

        assert_eq!(next.index(), 1);
        assert_eq!(next.previous(), Some(ElementId::new(1)));
        assert!(!next.is_first());
    }

    #[test]
    fn test_indexed_slot_prev() {
        let slot = IndexedSlot::<ElementId>::new(3, Some(ElementId::new(2)));
        let prev = slot.prev().unwrap();

        assert_eq!(prev.index(), 2);
        assert!(prev.previous().is_none()); // Unknown

        let first = IndexedSlot::<ElementId>::first();
        assert!(first.prev().is_none());
    }

    #[test]
    fn test_indexed_slot_display() {
        let slot = IndexedSlot::new(2, Some(ElementId::new(5)));
        let display = format!("{}", slot);
        assert!(display.contains("2"));
        assert!(display.contains("5"));

        let first = IndexedSlot::<ElementId>::first();
        let display = format!("{}", first);
        assert!(display.contains("0"));
    }

    // === SLOT ITER TESTS ===

    #[test]
    fn test_slot_iter() {
        let mut iter = SlotIter::<ElementId>::new();

        assert_eq!(iter.index(), 0);
        assert!(iter.current().is_first());

        iter.advance(ElementId::new(1));
        assert_eq!(iter.index(), 1);
        assert_eq!(iter.current().previous(), Some(ElementId::new(1)));

        iter.advance(ElementId::new(2));
        assert_eq!(iter.index(), 2);
        assert_eq!(iter.current().previous(), Some(ElementId::new(2)));
    }

    #[test]
    fn test_slot_iter_reset() {
        let mut iter = SlotIter::<ElementId>::new();
        iter.advance(ElementId::new(1));
        iter.advance(ElementId::new(2));

        assert_eq!(iter.index(), 2);

        iter.reset();
        assert_eq!(iter.index(), 0);
        assert!(iter.current().is_first());
    }

    #[test]
    fn test_slot_iter_starting_at() {
        let iter = SlotIter::<ElementId>::starting_at(5);
        assert_eq!(iter.index(), 5);
    }

    // === EDGE-CASE TESTS ===

    #[test]
    fn test_slot_iter_fused() {
        // Advance a SlotIter through 3 children, collect indexed slots,
        // then verify that after reset the iterator restarts cleanly.
        // This tests that the iterator state is always well-defined
        // (analogous to FusedIterator guarantees).
        let mut iter = SlotIter::<ElementId>::new();

        let child_ids = [ElementId::new(1), ElementId::new(2), ElementId::new(3)];
        let mut collected = Vec::new();

        for &child_id in &child_ids {
            collected.push(iter.current());
            iter.advance(child_id);
        }

        assert_eq!(collected.len(), 3);
        assert_eq!(collected[0].index(), 0);
        assert!(collected[0].previous().is_none());
        assert_eq!(collected[1].index(), 1);
        assert_eq!(collected[1].previous(), Some(ElementId::new(1)));
        assert_eq!(collected[2].index(), 2);
        assert_eq!(collected[2].previous(), Some(ElementId::new(2)));

        // After exhausting, the iterator should remain in a consistent state
        // Calling current() repeatedly should return the same slot
        let post_slot = iter.current();
        assert_eq!(post_slot.index(), 3);
        assert_eq!(post_slot.previous(), Some(ElementId::new(3)));

        for _ in 0..5 {
            let repeated = iter.current();
            assert_eq!(repeated.index(), post_slot.index());
            assert_eq!(repeated.previous(), post_slot.previous());
        }
    }

    #[test]
    fn test_indexed_slot_boundary() {
        // Only child scenario: index 0, count 1
        // prev() should be None, next slot moves past the single child.
        // is_first() should be true. With no next sibling, this is
        // effectively the last child too.
        let slot = IndexedSlot::<ElementId>::new(0, None);

        // prev() should be None (already at index 0)
        assert!(slot.prev().is_none());

        // is_first() should be true
        assert!(slot.is_first());

        // After advancing to next, we're past the only child
        let next = slot.next(ElementId::new(42));
        assert_eq!(next.index(), 1);
        assert_eq!(next.previous(), Some(ElementId::new(42)));
        assert!(!next.is_first());
    }

    #[test]
    fn test_slot_builder_empty() {
        // Build a slot at index 0 with no siblings configured.
        // The resulting slot should have no sibling references.
        let parent_id = ElementId::new(1);
        let slot = Slot::builder(parent_id, 0, Depth::new(0)).build();

        assert_eq!(slot.parent(), parent_id);
        assert_eq!(slot.index(), 0);
        assert!(slot.previous_sibling().is_none());
        assert!(slot.next_sibling().is_none());
        assert!(slot.is_first_child());
        assert!(slot.is_last_child());
        assert!(slot.is_only_child());
        assert!(!slot.has_siblings());
    }
}
