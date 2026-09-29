//! [`IndexedSlot`] — a child's position during reconciliation.

use std::fmt;

use crate::TreeId;

/// Indexed slot for efficient child reconciliation.
///
/// This mirrors Flutter's `IndexedSlot` pattern used in
/// `updateChildren()` for O(1) child insertion.
///
/// When inserting a child, you need to know both the index AND the
/// previous sibling to insert after. Keeping both together enables O(1)
/// insertion in linked structures.
///
/// # Example
///
/// ```
/// use flui_foundation::{ElementId, IndexedSlot};
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
pub struct IndexedSlot<I: TreeId> {
    /// Position index (0-based).
    index: usize,
    /// Previous sibling for O(1) insertion.
    previous: Option<I>,
}

impl<I: TreeId> IndexedSlot<I> {
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

impl<I: TreeId> Default for IndexedSlot<I> {
    fn default() -> Self {
        Self::first()
    }
}

impl<I: TreeId> fmt::Display for IndexedSlot<I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.previous {
            Some(prev) => write!(f, "IndexedSlot({}, after {})", self.index, prev),
            None => write!(f, "IndexedSlot({})", self.index),
        }
    }
}

impl<I: TreeId> From<usize> for IndexedSlot<I> {
    fn from(index: usize) -> Self {
        Self::new(index, None)
    }
}
