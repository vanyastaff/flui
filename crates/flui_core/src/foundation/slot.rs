//! Slot - represents position in parent's child list
//!
//! Slots are used to track where a child element is positioned in its parent.

use std::fmt;
use std::ops::{Add, AddAssign, Sub, SubAssign};
use crate::ElementId;

/// Slot - position in parent's child list
///
/// Similar to Flutter's IndexedSlot. Contains the child's index position
/// and optionally a reference to the previous sibling for efficient insertion.
///
/// # Examples
///
/// ```rust
/// use flui_core::Slot;
///
/// let slot = Slot::new(0); // First child
/// assert_eq!(slot.index(), 0);
///
/// // Arithmetic operations
/// let next = slot + 1;
/// assert_eq!(next.index(), 1);
///
/// // Ordering
/// assert!(Slot::new(0) < Slot::new(5));
/// ```
///
/// # IndexedSlot Enhancement
///
/// For efficient RenderObject child insertion, slot can optionally store
/// the previous sibling's ElementId.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Slot {
    /// Position in parent's child list (0-based)
    index: usize,

    /// Previous sibling element ID (None if first child or not tracked)
    #[cfg_attr(feature = "serde", serde(skip))]
    previous_sibling: Option<ElementId>,
}

impl Slot {
    /// Create a new slot at given index (no sibling reference)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::Slot;
    ///
    /// let slot = Slot::new(5);
    /// assert_eq!(slot.index(), 5);
    /// ```
    #[must_use]
    #[inline]
    pub const fn new(index: usize) -> Self {
        Self {
            index,
            previous_sibling: None,
        }
    }

    /// Create a slot with previous sibling reference
    ///
    /// This is the efficient version used by update_children() for
    /// optimal RenderObject child insertion.
    ///
    /// # Arguments
    ///
    /// * `index` - Position in parent's child list
    /// * `previous_sibling` - ID of previous sibling (None if first child)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::{Slot, ElementId};
    ///
    /// let sibling_id = ElementId::new();
    /// let slot = Slot::with_previous_sibling(2, Some(sibling_id));
    /// assert_eq!(slot.index(), 2);
    /// ```
    #[must_use]
    #[inline]
    pub const fn with_previous_sibling(index: usize, previous_sibling: Option<ElementId>) -> Self {
        Self {
            index,
            previous_sibling,
        }
    }

    /// Returns the slot index
    #[must_use]
    #[inline]
    pub const fn index(self) -> usize {
        self.index
    }

    /// Returns the previous sibling
    ///
    /// Returns the previous sibling's ElementId if tracked,
    /// None if this is the first child or tracking not enabled.
    #[must_use]
    #[inline]
    pub const fn previous_sibling(self) -> Option<ElementId> {
        self.previous_sibling
    }

    /// Checks if this slot has sibling tracking enabled
    #[must_use]
    #[inline]
    pub const fn has_sibling_tracking(self) -> bool {
        self.previous_sibling.is_some() || self.is_first()
    }

    /// Checks if this is the first slot (index 0)
    #[must_use]
    #[inline]
    pub const fn is_first(self) -> bool {
        self.index == 0
    }

    /// Returns the next slot (increment index by 1)
    ///
    /// Note: This loses sibling tracking info.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::Slot;
    ///
    /// let slot = Slot::new(0);
    /// let next = slot.next();
    /// assert_eq!(next.index(), 1);
    /// ```
    #[must_use]
    #[inline]
    pub const fn next(self) -> Self {
        Self {
            index: self.index + 1,
            previous_sibling: None,
        }
    }

    /// Returns the previous slot (decrement index by 1)
    ///
    /// Returns None if already at index 0.
    /// Note: This loses sibling tracking info.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::Slot;
    ///
    /// let slot = Slot::new(5);
    /// assert_eq!(slot.prev().unwrap().index(), 4);
    ///
    /// let first = Slot::new(0);
    /// assert!(first.prev().is_none());
    /// ```
    #[must_use]
    #[inline]
    pub const fn prev(self) -> Option<Self> {
        if self.index == 0 {
            None
        } else {
            Some(Self {
                index: self.index - 1,
                previous_sibling: None,
            })
        }
    }

    /// Returns a slot with the same index but updated sibling tracking
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::{Slot, ElementId};
    ///
    /// let slot = Slot::new(5);
    /// let sibling_id = ElementId::new();
    /// let updated = slot.with_sibling(Some(sibling_id));
    /// assert_eq!(updated.index(), 5);
    /// assert_eq!(updated.previous_sibling(), Some(sibling_id));
    /// ```
    #[must_use]
    #[inline]
    pub const fn with_sibling(self, previous_sibling: Option<ElementId>) -> Self {
        Self {
            index: self.index,
            previous_sibling,
        }
    }

    /// Returns a slot without sibling tracking
    #[must_use]
    #[inline]
    pub const fn without_tracking(self) -> Self {
        Self {
            index: self.index,
            previous_sibling: None,
        }
    }

    /// Checked addition
    ///
    /// Returns None if overflow would occur.
    #[must_use]
    #[inline]
    pub const fn checked_add(self, rhs: usize) -> Option<Self> {
        if let Some(new_index) = self.index.checked_add(rhs) {
            Some(Self {
                index: new_index,
                previous_sibling: None,
            })
        } else {
            None
        }
    }

    /// Checked subtraction
    ///
    /// Returns None if underflow would occur.
    #[must_use]
    #[inline]
    pub const fn checked_sub(self, rhs: usize) -> Option<Self> {
        if let Some(new_index) = self.index.checked_sub(rhs) {
            Some(Self {
                index: new_index,
                previous_sibling: None,
            })
        } else {
            None
        }
    }

    /// Saturating addition
    ///
    /// Adds rhs to index, saturating at usize::MAX.
    #[must_use]
    #[inline]
    pub const fn saturating_add(self, rhs: usize) -> Self {
        Self {
            index: self.index.saturating_add(rhs),
            previous_sibling: None,
        }
    }

    /// Saturating subtraction
    ///
    /// Subtracts rhs from index, saturating at 0.
    #[must_use]
    #[inline]
    pub const fn saturating_sub(self, rhs: usize) -> Self {
        Self {
            index: self.index.saturating_sub(rhs),
            previous_sibling: None,
        }
    }
}

impl Default for Slot {
    /// Returns the first slot (index 0)
    #[inline]
    fn default() -> Self {
        Self::new(0)
    }
}

impl PartialOrd for Slot {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Slot {
    #[inline]
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.index.cmp(&other.index)
    }
}

impl From<usize> for Slot {
    #[inline]
    fn from(index: usize) -> Self {
        Self::new(index)
    }
}

impl From<Slot> for usize {
    #[inline]
    fn from(slot: Slot) -> Self {
        slot.index
    }
}

impl AsRef<usize> for Slot {
    #[inline]
    fn as_ref(&self) -> &usize {
        &self.index
    }
}

impl std::convert::TryFrom<isize> for Slot {
    type Error = SlotConversionError;

    fn try_from(value: isize) -> Result<Self, Self::Error> {
        if value < 0 {
            Err(SlotConversionError::NegativeI(value))
        } else {
            Ok(Self::new(value as usize))
        }
    }
}

impl std::convert::TryFrom<i32> for Slot {
    type Error = SlotConversionError;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        if value < 0 {
            Err(SlotConversionError::Negative32(value))
        } else {
            Ok(Self::new(value as usize))
        }
    }
}

impl std::convert::TryFrom<i64> for Slot {
    type Error = SlotConversionError;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        if value < 0 {
            Err(SlotConversionError::Negative64(value))
        } else {
            Ok(Self::new(value as usize))
        }
    }
}

impl Add<usize> for Slot {
    type Output = Self;

    #[inline]
    fn add(self, rhs: usize) -> Self::Output {
        Self {
            index: self.index + rhs,
            previous_sibling: None,
        }
    }
}

impl AddAssign<usize> for Slot {
    #[inline]
    fn add_assign(&mut self, rhs: usize) {
        self.index += rhs;
        self.previous_sibling = None;
    }
}

impl Sub<usize> for Slot {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: usize) -> Self::Output {
        Self {
            index: self.index - rhs,
            previous_sibling: None,
        }
    }
}

impl SubAssign<usize> for Slot {
    #[inline]
    fn sub_assign(&mut self, rhs: usize) {
        self.index -= rhs;
        self.previous_sibling = None;
    }
}

impl fmt::Display for Slot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.previous_sibling {
            Some(sibling) => write!(f, "Slot({}, after {})", self.index, sibling),
            None => write!(f, "Slot({})", self.index),
        }
    }
}

/// Error type for Slot conversion
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotConversionError {
    /// Attempted to convert negative isize value
    NegativeI(isize),
    /// Attempted to convert negative i32 value
    Negative32(i32),
    /// Attempted to convert negative i64 value
    Negative64(i64),
}

impl fmt::Display for SlotConversionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NegativeI(value) => {
                write!(f, "cannot convert negative isize value {} to Slot", value)
            }
            Self::Negative32(value) => {
                write!(f, "cannot convert negative i32 value {} to Slot", value)
            }
            Self::Negative64(value) => {
                write!(f, "cannot convert negative i64 value {} to Slot", value)
            }
        }
    }
}

impl std::error::Error for SlotConversionError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slot_new() {
        let slot = Slot::new(5);
        assert_eq!(slot.index(), 5);
    }

    #[test]
    fn test_slot_default() {
        let slot = Slot::default();
        assert_eq!(slot.index(), 0);
        assert!(slot.is_first());
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
    fn test_slot_as_ref() {
        let slot = Slot::new(42);
        let index: &usize = slot.as_ref();
        assert_eq!(*index, 42);
    }

    #[test]
    fn test_slot_try_from_isize() {
        let slot: Slot = 42isize.try_into().unwrap();
        assert_eq!(slot.index(), 42);

        let err = Slot::try_from(-1isize);
        assert!(err.is_err());
    }

    #[test]
    fn test_slot_ord() {
        let slot1 = Slot::new(0);
        let slot2 = Slot::new(5);
        let slot3 = Slot::new(10);

        assert!(slot1 < slot2);
        assert!(slot2 < slot3);
        assert!(slot1 < slot3);

        let mut vec = vec![slot3, slot1, slot2];
        vec.sort();
        assert_eq!(vec, vec![slot1, slot2, slot3]);
    }

    #[test]
    fn test_slot_add() {
        let slot = Slot::new(5);
        let result = slot + 3;
        assert_eq!(result.index(), 8);
    }

    #[test]
    fn test_slot_sub() {
        let slot = Slot::new(10);
        let result = slot - 3;
        assert_eq!(result.index(), 7);
    }

    #[test]
    fn test_slot_add_assign() {
        let mut slot = Slot::new(5);
        slot += 3;
        assert_eq!(slot.index(), 8);
    }

    #[test]
    fn test_slot_sub_assign() {
        let mut slot = Slot::new(10);
        slot -= 3;
        assert_eq!(slot.index(), 7);
    }

    #[test]
    fn test_slot_checked_add() {
        let slot = Slot::new(5);
        assert_eq!(slot.checked_add(3).unwrap().index(), 8);

        let max_slot = Slot::new(usize::MAX);
        assert!(max_slot.checked_add(1).is_none());
    }

    #[test]
    fn test_slot_checked_sub() {
        let slot = Slot::new(10);
        assert_eq!(slot.checked_sub(3).unwrap().index(), 7);

        let zero_slot = Slot::new(0);
        assert!(zero_slot.checked_sub(1).is_none());
    }

    #[test]
    fn test_slot_saturating_add() {
        let slot = Slot::new(5);
        assert_eq!(slot.saturating_add(3).index(), 8);

        let max_slot = Slot::new(usize::MAX);
        assert_eq!(max_slot.saturating_add(1).index(), usize::MAX);
    }

    #[test]
    fn test_slot_saturating_sub() {
        let slot = Slot::new(10);
        assert_eq!(slot.saturating_sub(3).index(), 7);

        let zero_slot = Slot::new(0);
        assert_eq!(zero_slot.saturating_sub(1).index(), 0);
    }

    #[test]
    fn test_slot_display() {
        let slot = Slot::new(7);
        assert_eq!(slot.to_string(), "Slot(7)");
    }

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
        assert!(slot.has_sibling_tracking());
        assert!(slot.is_first());
    }

    #[test]
    fn test_slot_without_tracking() {
        let slot = Slot::new(5);

        assert_eq!(slot.index(), 5);
        assert_eq!(slot.previous_sibling(), None);
        assert!(!slot.has_sibling_tracking());
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
    fn test_slot_with_sibling() {
        let slot = Slot::new(5);
        let sibling_id = ElementId::new();

        let updated = slot.with_sibling(Some(sibling_id));
        assert_eq!(updated.index(), 5);
        assert_eq!(updated.previous_sibling(), Some(sibling_id));
    }

    #[test]
    fn test_slot_without_tracking_method() {
        let sibling_id = ElementId::new();
        let slot = Slot::with_previous_sibling(5, Some(sibling_id));

        let without = slot.without_tracking();
        assert_eq!(without.index(), 5);
        assert_eq!(without.previous_sibling(), None);
    }

    #[test]
    fn test_arithmetic_loses_tracking() {
        let sibling_id = ElementId::new();
        let slot = Slot::with_previous_sibling(5, Some(sibling_id));

        let next = slot.next();
        assert_eq!(next.previous_sibling(), None);

        let added = slot + 1;
        assert_eq!(added.previous_sibling(), None);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_slot_serde() {
        let slot = Slot::new(42);
        let json = serde_json::to_string(&slot).unwrap();
        let deserialized: Slot = serde_json::from_str(&json).unwrap();
        assert_eq!(slot.index(), deserialized.index());
    }
}