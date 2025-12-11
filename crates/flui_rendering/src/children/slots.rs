//! Named slots storage for render objects
//!
//! Flutter equivalent: `SlottedContainerRenderObjectMixin<SlotType, ChildType>`

use std::collections::HashMap;
use std::hash::Hash;

use flui_foundation::RenderId;
use flui_types::Offset;

use crate::protocol::Protocol;

/// Slot key trait
pub trait SlotKey: Hash + Eq + Clone + std::fmt::Debug {}

// Blanket impl for all types that meet requirements
impl<T> SlotKey for T where T: Hash + Eq + Clone + std::fmt::Debug {}

/// Named slots storage (Flutter: SlottedContainerRenderObjectMixin)
///
/// # Type Parameters
///
/// - `P`: Protocol type (BoxProtocol or SliverProtocol)
/// - `S`: Slot key type (e.g., enum SlotName)
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::{Slots, BoxProtocol};
/// use flui_types::Offset;
///
/// #[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
/// enum AppBarSlot {
///     Leading,
///     Title,
///     Actions,
/// }
///
/// struct RenderAppBar {
///     slots: Slots<BoxProtocol, AppBarSlot>,
/// }
///
/// impl RenderAppBar {
///     fn set_leading(&mut self, child: RenderId) {
///         self.slots.insert(AppBarSlot::Leading, child, Offset::ZERO);
///     }
/// }
/// ```
#[derive(Debug)]
pub struct Slots<P: Protocol, S: SlotKey> {
    items: HashMap<S, (RenderId, Offset)>,
    _phantom: std::marker::PhantomData<P>,
}

impl<P: Protocol, S: SlotKey> Slots<P, S> {
    /// Create empty slots storage
    #[inline]
    pub fn new() -> Self {
        Self {
            items: HashMap::new(),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Create with capacity
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            items: HashMap::with_capacity(capacity),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Number of filled slots
    #[inline]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Check if empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Insert or update child in slot
    #[inline]
    pub fn insert(&mut self, slot: S, child: RenderId, offset: Offset) -> Option<(RenderId, Offset)> {
        self.items.insert(slot, (child, offset))
    }

    /// Remove child from slot
    #[inline]
    pub fn remove(&mut self, slot: &S) -> Option<(RenderId, Offset)> {
        self.items.remove(slot)
    }

    /// Get child in slot
    #[inline]
    pub fn get(&self, slot: &S) -> Option<&(RenderId, Offset)> {
        self.items.get(slot)
    }

    /// Get mutable child in slot
    #[inline]
    pub fn get_mut(&mut self, slot: &S) -> Option<&mut (RenderId, Offset)> {
        self.items.get_mut(slot)
    }

    /// Get child ID in slot
    #[inline]
    pub fn get_id(&self, slot: &S) -> Option<RenderId> {
        self.items.get(slot).map(|(id, _)| *id)
    }

    /// Get offset in slot
    #[inline]
    pub fn get_offset(&self, slot: &S) -> Option<Offset> {
        self.items.get(slot).map(|(_, offset)| *offset)
    }

    /// Set offset for slot
    #[inline]
    pub fn set_offset(&mut self, slot: &S, offset: Offset) -> bool {
        if let Some((_, o)) = self.items.get_mut(slot) {
            *o = offset;
            true
        } else {
            false
        }
    }

    /// Check if slot is occupied
    #[inline]
    pub fn contains(&self, slot: &S) -> bool {
        self.items.contains_key(slot)
    }

    /// Iterate over slots
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = (&S, &(RenderId, Offset))> {
        self.items.iter()
    }

    /// Iterate mutably over slots
    #[inline]
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&S, &mut (RenderId, Offset))> {
        self.items.iter_mut()
    }

    /// Iterate over child IDs
    #[inline]
    pub fn iter_ids(&self) -> impl Iterator<Item = RenderId> + '_ {
        self.items.values().map(|(id, _)| *id)
    }

    /// Iterate over (slot, child, offset) tuples
    #[inline]
    pub fn iter_with_data(&self) -> impl Iterator<Item = (&S, RenderId, Offset)> + '_ {
        self.items.iter().map(|(slot, (id, offset))| (slot, *id, *offset))
    }

    /// Clear all slots
    #[inline]
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Retain slots matching predicate
    #[inline]
    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&S, &mut (RenderId, Offset)) -> bool,
    {
        self.items.retain(|slot, item| f(slot, item));
    }

    /// Reserve capacity
    #[inline]
    pub fn reserve(&mut self, additional: usize) {
        self.items.reserve(additional);
    }

    /// Get all slot keys
    #[inline]
    pub fn keys(&self) -> impl Iterator<Item = &S> {
        self.items.keys()
    }

    /// Get all values
    #[inline]
    pub fn values(&self) -> impl Iterator<Item = &(RenderId, Offset)> {
        self.items.values()
    }
}

impl<P: Protocol, S: SlotKey> Default for Slots<P, S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Protocol, S: SlotKey> Clone for Slots<P, S> {
    fn clone(&self) -> Self {
        Self {
            items: self.items.clone(),
            _phantom: std::marker::PhantomData,
        }
    }
}

// ============================================================================
// Type Aliases
// ============================================================================

/// Named Box slots (Flutter: SlottedContainerRenderObjectMixin<SlotType, RenderBox>)
pub type BoxSlots<S> = Slots<crate::protocol::BoxProtocol, S>;

/// Named Sliver slots (Flutter: SlottedContainerRenderObjectMixin<SlotType, RenderSliver>)
pub type SliverSlots<S> = Slots<crate::protocol::SliverProtocol, S>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::BoxProtocol;

    #[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
    enum TestSlot {
        Leading,
        Title,
        Trailing,
    }

    #[test]
    fn test_slots_basic() {
        let mut slots: Slots<BoxProtocol, TestSlot> = Slots::new();
        assert!(slots.is_empty());

        let id1 = RenderId::new(1);
        slots.insert(TestSlot::Leading, id1, Offset::ZERO);

        assert_eq!(slots.len(), 1);
        assert!(slots.contains(&TestSlot::Leading));
        assert_eq!(slots.get_id(&TestSlot::Leading), Some(id1));
    }

    #[test]
    fn test_slots_update() {
        let mut slots: Slots<BoxProtocol, TestSlot> = Slots::new();

        let id1 = RenderId::new(1);
        slots.insert(TestSlot::Title, id1, Offset::ZERO);

        let new_offset = Offset::new(10.0, 20.0);
        assert!(slots.set_offset(&TestSlot::Title, new_offset));
        assert_eq!(slots.get_offset(&TestSlot::Title), Some(new_offset));
    }

    #[test]
    fn test_slots_iteration() {
        let mut slots: Slots<BoxProtocol, TestSlot> = Slots::new();

        slots.insert(TestSlot::Leading, RenderId::new(1), Offset::new(0.0, 0.0));
        slots.insert(TestSlot::Title, RenderId::new(2), Offset::new(10.0, 0.0));
        slots.insert(TestSlot::Trailing, RenderId::new(3), Offset::new(20.0, 0.0));

        assert_eq!(slots.len(), 3);

        let ids: Vec<_> = slots.iter_ids().collect();
        assert_eq!(ids.len(), 3);
    }

    #[test]
    fn test_slots_remove() {
        let mut slots: Slots<BoxProtocol, TestSlot> = Slots::new();

        slots.insert(TestSlot::Leading, RenderId::new(1), Offset::ZERO);
        assert_eq!(slots.len(), 1);

        let removed = slots.remove(&TestSlot::Leading);
        assert!(removed.is_some());
        assert!(slots.is_empty());
    }
}
