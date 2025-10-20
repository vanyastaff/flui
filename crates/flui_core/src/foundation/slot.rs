//! Slot - represents position in parent's child list
//!
//! Slots are used to track where a child element is positioned in its parent.

use std::fmt;

/// Slot - position in parent's child list
///
/// in its parent's child list.
///
/// # Example
///
/// ```
/// use flui_core::Slot;
///
/// let slot = Slot::new(0); // First child
/// assert_eq!(slot.index(), 0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Slot(usize);

impl Slot {
    /// Create a new slot at given index
    #[inline]
    pub fn new(index: usize) -> Self {
        Self(index)
    }

    /// Get the slot index
    #[inline]
    pub fn index(self) -> usize {
        self.0
    }

    /// Get the next slot (increment index)
    #[inline]
    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }

    /// Get the previous slot (decrement index)
    ///
    /// Returns None if already at index 0.
    #[inline]
    pub fn prev(self) -> Option<Self> {
        self.0.checked_sub(1).map(Self)
    }

    /// Check if this is the first slot (index 0)
    #[inline]
    pub fn is_first(self) -> bool {
        self.0 == 0
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
        write!(f, "Slot({})", self.0)
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
}
