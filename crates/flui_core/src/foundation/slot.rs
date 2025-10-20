//! Slot - represents position in parent's child list
//!
//! Slots are used to track where a child element is positioned in its parent.

use std::fmt;
use crate::ElementId;

/// Slot - position in parent's child list
///
/// Similar to Flutter's IndexedSlot. Contains the child's index position
/// and optionally a reference to the previous sibling for efficient insertion.
///
/// # Example
///
/// ```
/// use flui_core::Slot;
///
/// let slot = Slot::new(0); // First child
/// assert_eq!(slot.index(), 0);
/// ```
///
/// # Phase 8: IndexedSlot Enhancement
///
/// For efficient RenderObject child insertion, slot can optionally store
/// the previous sibling's ElementId:
///
/// ```rust,ignore
/// // Children: [A, B, C, D]
/// // Inserting C at position 2 (after B):
/// let slot = Slot::with_previous_sibling(2, Some(b_id));
/// // RenderObject can now insert C directly after B
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Slot {
    /// Position in parent's child list (0-based)
    index: usize,

    /// Previous sibling element ID (None if first child or not tracked)
    ///
    /// Phase 8: This allows RenderObject to efficiently insert children
    /// without scanning the child list. When None, RenderObject must scan.
    previous_sibling: Option<ElementId>,
}

impl Slot {
    /// Create a new slot at given index (no sibling reference)
    #[inline]
    pub fn new(index: usize) -> Self {
        Self {
            index,
            previous_sibling: None,
        }
    }

    /// Create a slot with previous sibling reference (Phase 8)
    ///
    /// This is the efficient version used by update_children() for
    /// optimal RenderObject child insertion.
    ///
    /// # Arguments
    ///
    /// * `index` - Position in parent's child list
    /// * `previous_sibling` - ID of previous sibling (None if first child)
    #[inline]
    pub fn with_previous_sibling(index: usize, previous_sibling: Option<ElementId>) -> Self {
        Self {
            index,
            previous_sibling,
        }
    }

    /// Get the slot index
    #[inline]
    pub fn index(self) -> usize {
        self.index
    }

    /// Get the previous sibling (Phase 8)
    ///
    /// Returns the previous sibling's ElementId if tracked,
    /// None if this is the first child or tracking not enabled.
    #[inline]
    pub fn previous_sibling(self) -> Option<ElementId> {
        self.previous_sibling
    }

    /// Check if this slot has sibling tracking enabled (Phase 8)
    #[inline]
    pub fn has_sibling_tracking(self) -> bool {
        self.previous_sibling.is_some() || self.is_first()
    }

    /// Get the next slot (increment index)
    ///
    /// Note: This loses sibling tracking info. Use with care.
    #[inline]
    pub fn next(self) -> Self {
        Self {
            index: self.index + 1,
            previous_sibling: None,
        }
    }

    /// Get the previous slot (decrement index)
    ///
    /// Returns None if already at index 0.
    /// Note: This loses sibling tracking info.
    #[inline]
    pub fn prev(self) -> Option<Self> {
        self.index.checked_sub(1).map(|i| Self {
            index: i,
            previous_sibling: None,
        })
    }

    /// Check if this is the first slot (index 0)
    #[inline]
    pub fn is_first(self) -> bool {
        self.index == 0
    }
}

impl From<usize> for Slot {
    fn from(index: usize) -> Self {
        Self::new(index)
    }
}

impl From<Slot> for usize {
    fn from(slot: Slot) -> Self {
        slot.index()
    }
}

impl fmt::Display for Slot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.previous_sibling {
            Some(sibling) => write!(f, "Slot({}, after {:?})", self.index, sibling),
            None => write!(f, "Slot({})", self.index),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slot_new() {
        let slot = Slot::new(5);
        assert_eq!(slot.index(), 5);
    }

    #[test]
    fn test_slot_next() {
        let slot = Slot::new(0);
        let next = slot.next();
        assert_eq!(next.index(), 1);
    }

    #[test]
    fn test_slot_prev() {
        let slot = Slot::new(5);
        let prev = slot.prev();
        assert_eq!(prev.unwrap().index(), 4);
    }

    #[test]
    fn test_slot_prev_underflow() {
        let slot = Slot::new(0);
        assert_eq!(slot.prev(), None);
    }

    #[test]
    fn test_slot_is_first() {
        assert!(Slot::new(0).is_first());
        assert!(!Slot::new(1).is_first());
    }

    #[test]
    fn test_slot_from_usize() {
        let slot: Slot = 42.into();
        assert_eq!(slot.index(), 42);
    }

    #[test]
    fn test_slot_into_usize() {
        let slot = Slot::new(42);
        let index: usize = slot.into();
        assert_eq!(index, 42);
    }

    #[test]
    fn test_slot_display() {
        let slot = Slot::new(7);
        assert_eq!(slot.to_string(), "Slot(7)");
    }

    // ========== Phase 8: IndexedSlot Tests ==========

    #[test]
    fn test_slot_with_previous_sibling() {
        let sibling_id = ElementId::new();
        let slot = Slot::with_previous_sibling(2, Some(sibling_id));

        assert_eq!(slot.index(), 2);
        assert_eq!(slot.previous_sibling(), Some(sibling_id));
        assert!(slot.has_sibling_tracking());
        assert!(!slot.is_first());
    }

    #[test]
    fn test_slot_first_child_with_tracking() {
        let slot = Slot::with_previous_sibling(0, None);

        assert_eq!(slot.index(), 0);
        assert_eq!(slot.previous_sibling(), None);
        assert!(slot.has_sibling_tracking()); // First child counts as tracked
        assert!(slot.is_first());
    }

    #[test]
    fn test_slot_without_tracking() {
        let slot = Slot::new(5);

        assert_eq!(slot.index(), 5);
        assert_eq!(slot.previous_sibling(), None);
        assert!(!slot.has_sibling_tracking()); // Not first and no sibling
    }

    #[test]
    fn test_slot_display_with_sibling() {
        let sibling_id = ElementId::new();
        let slot = Slot::with_previous_sibling(3, Some(sibling_id));

        let display = slot.to_string();
        assert!(display.contains("Slot(3"));
        assert!(display.contains("after"));
    }

    #[test]
    fn test_slot_new_has_no_tracking() {
        let slot = Slot::new(10);

        assert_eq!(slot.previous_sibling(), None);
        assert!(!slot.has_sibling_tracking());
    }

    #[test]
    fn test_slot_from_usize_has_no_tracking() {
        let slot: Slot = 15.into();

        assert_eq!(slot.index(), 15);
        assert_eq!(slot.previous_sibling(), None);
        assert!(!slot.has_sibling_tracking());
    }
}
