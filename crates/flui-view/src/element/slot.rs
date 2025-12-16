//! Slot types for element positioning within parent children lists.
//!
//! Slots provide information about where an element fits within its parent's
//! child list. This is particularly important for multi-child elements like
//! Row, Column, Stack, etc.
//!
//! # Flutter Equivalent
//!
//! This corresponds to Flutter's `IndexedSlot<T>` class used by
//! `MultiChildRenderObjectElement` for efficient child management.

use flui_foundation::ElementId;

/// A slot that combines an index with optional reference to a sibling element.
///
/// Used by multi-child elements to track where each child fits in the list.
/// The `value` typically points to the previous sibling, enabling efficient
/// insertion and reordering.
///
/// # Type Parameter
///
/// * `T` - Additional slot data (often `Option<ElementId>` for sibling reference)
///
/// # Flutter Equivalent
///
/// Corresponds to Flutter's `IndexedSlot<T extends Element?>`:
/// ```dart
/// class IndexedSlot<T extends Element?> {
///   const IndexedSlot(this.index, this.value);
///   final T value;
///   final int index;
/// }
/// ```
///
/// # Example
///
/// ```rust
/// use flui_view::IndexedSlot;
/// use flui_foundation::ElementId;
///
/// // First child has no previous sibling
/// let first_slot: IndexedSlot<Option<ElementId>> = IndexedSlot::new(0, None);
///
/// // Second child references the first as previous sibling
/// let second_slot = IndexedSlot::new(1, Some(ElementId::new(1)));
///
/// assert_eq!(first_slot.index(), 0);
/// assert!(first_slot.value().is_none());
/// assert_eq!(second_slot.index(), 1);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IndexedSlot<T = Option<ElementId>> {
    /// The index position in the parent's child list.
    index: usize,
    /// Additional slot information (typically previous sibling).
    value: T,
}

impl<T> IndexedSlot<T> {
    /// Create a new indexed slot.
    ///
    /// # Arguments
    ///
    /// * `index` - The position in the parent's child list
    /// * `value` - Additional slot data (e.g., previous sibling reference)
    pub const fn new(index: usize, value: T) -> Self {
        Self { index, value }
    }

    /// Get the index position.
    pub const fn index(&self) -> usize {
        self.index
    }

    /// Get the slot value.
    pub const fn value(&self) -> &T {
        &self.value
    }

    /// Consume self and return the value.
    pub fn into_value(self) -> T {
        self.value
    }

    /// Map the value to a new type.
    pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> IndexedSlot<U> {
        IndexedSlot {
            index: self.index,
            value: f(self.value),
        }
    }
}

impl<T: Default> IndexedSlot<T> {
    /// Create a slot with the given index and default value.
    pub fn with_index(index: usize) -> Self {
        Self {
            index,
            value: T::default(),
        }
    }
}

impl<T: Default> Default for IndexedSlot<T> {
    fn default() -> Self {
        Self {
            index: 0,
            value: T::default(),
        }
    }
}

/// Type alias for the common case of tracking previous sibling.
pub type ElementSlot = IndexedSlot<Option<ElementId>>;

impl ElementSlot {
    /// Create a slot for the first child (no previous sibling).
    pub const fn first() -> Self {
        Self::new(0, None)
    }

    /// Create a slot for a child after another element.
    ///
    /// # Arguments
    ///
    /// * `index` - The position in the child list
    /// * `previous` - The ElementId of the previous sibling
    pub const fn after(index: usize, previous: ElementId) -> Self {
        Self::new(index, Some(previous))
    }

    /// Get the previous sibling, if any.
    pub const fn previous_sibling(&self) -> Option<ElementId> {
        self.value
    }

    /// Check if this is the first slot (no previous sibling).
    pub const fn is_first(&self) -> bool {
        self.value.is_none()
    }
}

/// Builder for creating a sequence of indexed slots.
///
/// Useful when setting up slots for multiple children.
///
/// # Example
///
/// ```rust
/// use flui_view::IndexedSlotBuilder;
/// use flui_foundation::ElementId;
///
/// let mut builder = IndexedSlotBuilder::new();
///
/// // Add children and get their slots
/// let first_slot = builder.next_slot(ElementId::new(1));
/// let second_slot = builder.next_slot(ElementId::new(2));
/// let third_slot = builder.next_slot(ElementId::new(3));
///
/// assert_eq!(first_slot.index(), 0);
/// assert!(first_slot.previous_sibling().is_none());
///
/// assert_eq!(second_slot.index(), 1);
/// assert_eq!(second_slot.previous_sibling(), Some(ElementId::new(1)));
///
/// assert_eq!(third_slot.index(), 2);
/// assert_eq!(third_slot.previous_sibling(), Some(ElementId::new(2)));
/// ```
#[derive(Debug, Default)]
pub struct IndexedSlotBuilder {
    /// Current index.
    index: usize,
    /// Previous element ID.
    previous: Option<ElementId>,
}

impl IndexedSlotBuilder {
    /// Create a new slot builder starting at index 0.
    pub const fn new() -> Self {
        Self {
            index: 0,
            previous: None,
        }
    }

    /// Get the next slot for the given element.
    ///
    /// Returns a slot with the current index and previous sibling reference,
    /// then advances the builder state.
    pub fn next_slot(&mut self, element_id: ElementId) -> ElementSlot {
        let slot = ElementSlot::new(self.index, self.previous);
        self.index += 1;
        self.previous = Some(element_id);
        slot
    }

    /// Get the current index without advancing.
    pub const fn current_index(&self) -> usize {
        self.index
    }

    /// Reset the builder to initial state.
    pub fn reset(&mut self) {
        self.index = 0;
        self.previous = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_indexed_slot_creation() {
        let slot: IndexedSlot<i32> = IndexedSlot::new(5, 42);
        assert_eq!(slot.index(), 5);
        assert_eq!(*slot.value(), 42);
    }

    #[test]
    fn test_indexed_slot_default() {
        let slot: IndexedSlot<i32> = IndexedSlot::default();
        assert_eq!(slot.index(), 0);
        assert_eq!(*slot.value(), 0);
    }

    #[test]
    fn test_indexed_slot_with_index() {
        let slot: IndexedSlot<i32> = IndexedSlot::with_index(10);
        assert_eq!(slot.index(), 10);
        assert_eq!(*slot.value(), 0);
    }

    #[test]
    fn test_indexed_slot_map() {
        let slot = IndexedSlot::new(3, 10);
        let mapped = slot.map(|v| v * 2);
        assert_eq!(mapped.index(), 3);
        assert_eq!(*mapped.value(), 20);
    }

    #[test]
    fn test_indexed_slot_into_value() {
        let slot = IndexedSlot::new(0, String::from("hello"));
        let value = slot.into_value();
        assert_eq!(value, "hello");
    }

    #[test]
    fn test_element_slot_first() {
        let slot = ElementSlot::first();
        assert_eq!(slot.index(), 0);
        assert!(slot.is_first());
        assert!(slot.previous_sibling().is_none());
    }

    #[test]
    fn test_element_slot_after() {
        let prev_id = ElementId::new(42);
        let slot = ElementSlot::after(5, prev_id);
        assert_eq!(slot.index(), 5);
        assert!(!slot.is_first());
        assert_eq!(slot.previous_sibling(), Some(prev_id));
    }

    #[test]
    fn test_indexed_slot_equality() {
        let slot1 = IndexedSlot::new(1, Some(ElementId::new(5)));
        let slot2 = IndexedSlot::new(1, Some(ElementId::new(5)));
        let slot3 = IndexedSlot::new(2, Some(ElementId::new(5)));
        let slot4 = IndexedSlot::new(1, Some(ElementId::new(6)));

        assert_eq!(slot1, slot2);
        assert_ne!(slot1, slot3); // different index
        assert_ne!(slot1, slot4); // different value
    }

    #[test]
    fn test_indexed_slot_builder() {
        let mut builder = IndexedSlotBuilder::new();
        assert_eq!(builder.current_index(), 0);

        let id1 = ElementId::new(10);
        let id2 = ElementId::new(20);
        let id3 = ElementId::new(30);

        let slot1 = builder.next_slot(id1);
        assert_eq!(slot1.index(), 0);
        assert!(slot1.previous_sibling().is_none());

        let slot2 = builder.next_slot(id2);
        assert_eq!(slot2.index(), 1);
        assert_eq!(slot2.previous_sibling(), Some(id1));

        let slot3 = builder.next_slot(id3);
        assert_eq!(slot3.index(), 2);
        assert_eq!(slot3.previous_sibling(), Some(id2));

        assert_eq!(builder.current_index(), 3);
    }

    #[test]
    fn test_indexed_slot_builder_reset() {
        let mut builder = IndexedSlotBuilder::new();
        let id = ElementId::new(1);

        builder.next_slot(id);
        builder.next_slot(id);
        assert_eq!(builder.current_index(), 2);

        builder.reset();
        assert_eq!(builder.current_index(), 0);

        let slot = builder.next_slot(id);
        assert_eq!(slot.index(), 0);
        assert!(slot.previous_sibling().is_none());
    }

    #[test]
    fn test_indexed_slot_hash() {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        set.insert(IndexedSlot::new(0, 10));
        set.insert(IndexedSlot::new(1, 20));
        set.insert(IndexedSlot::new(0, 10)); // duplicate

        assert_eq!(set.len(), 2);
    }
}
