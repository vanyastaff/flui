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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ElementId;

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
        let prev = slot.prev().expect("index 3 has a previous slot");

        assert_eq!(prev.index(), 2);
        assert!(prev.previous().is_none()); // Unknown

        let first = IndexedSlot::<ElementId>::first();
        assert!(first.prev().is_none());
    }

    #[test]
    fn test_indexed_slot_display() {
        // ElementId::new(5) is 1-based: index()=4, Display="Element(4:1)".
        let slot = IndexedSlot::new(2, Some(ElementId::new(5)));
        let display = format!("{slot}");
        // Slot position index must appear.
        assert!(display.contains('2'), "slot index 2 not in {display:?}");
        // ElementId's Display embeds its 0-based slot index (4 for new(5)).
        assert!(
            display.contains('4'),
            "ElementId index 4 (from new(5)) not in {display:?}"
        );

        let first = IndexedSlot::<ElementId>::first();
        let display = format!("{first}");
        assert!(
            display.contains('0'),
            "first slot display missing 0: {display:?}"
        );
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
}
