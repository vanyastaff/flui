//! Efficient observer list implementations
//!
//! This module provides specialized list types for managing observers/listeners
//! that are optimized for the common UI pattern of add/remove/iterate.
//!
//! # Features
//!
//! - `ObserverList<T>` - Observer management with O(1) add/remove via `HashMap`
//!   index
//!
//! # Examples
//!
//! ```rust
//! use flui_foundation::ObserverList;
//!
//! let mut observers: ObserverList<i32> = ObserverList::new();
//!
//! let id1 = observers.add(42);
//! let id2 = observers.add(100);
//!
//! // Iterate over all observers
//! for observer in observers.iter() {
//!     println!("Observer: {}", observer);
//! }
//!
//! // Remove by ID (O(1))
//! observers.remove(id1);
//! ```

use std::collections::{HashMap, VecDeque};

use crate::id::ObserverId;

/// A list of observers with O(1) add/remove by ID.
///
/// Uses `VecDeque` internally for cache-friendly iteration and
/// a `HashMap<ObserverId, usize>` index for O(1) removal by ID.
///
/// # Thread Safety
///
/// This type is NOT thread-safe by itself. For concurrent access,
/// wrap in your own synchronization primitive (e.g. `parking_lot::RwLock`).
#[derive(Debug)]
pub struct ObserverList<T> {
    observers: VecDeque<Option<(ObserverId, T)>>,
    /// Maps `ObserverId` → slot index for O(1) removal.
    id_to_index: HashMap<ObserverId, usize>,
    len: usize,
    /// Indices of removed slots for reuse
    free_slots: Vec<usize>,
    /// Next ID to assign (starts at 1)
    next_id: usize,
}

impl<T> ObserverList<T> {
    /// Creates a new empty observer list.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self {
            observers: VecDeque::new(),
            id_to_index: HashMap::new(),
            len: 0,
            free_slots: Vec::new(),
            next_id: 1,
        }
    }

    /// Creates a new observer list with the specified capacity.
    #[inline]
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            observers: VecDeque::with_capacity(capacity),
            id_to_index: HashMap::with_capacity(capacity),
            len: 0,
            free_slots: Vec::new(),
            next_id: 1,
        }
    }

    /// Returns the number of observers.
    #[inline]
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Returns true if the list is empty.
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Adds an observer to the list, returning its unique ID.
    ///
    /// The ID can be used to remove the observer later in O(1) time.
    pub fn add(&mut self, observer: T) -> ObserverId {
        let id = ObserverId::new(self.next_id);
        self.next_id += 1;

        let slot_idx = if let Some(slot) = self.free_slots.pop() {
            self.observers[slot] = Some((id, observer));
            slot
        } else {
            let idx = self.observers.len();
            self.observers.push_back(Some((id, observer)));
            idx
        };

        self.id_to_index.insert(id, slot_idx);
        self.len += 1;
        id
    }

    /// Removes an observer by its ID in O(1) time.
    ///
    /// Returns the observer if found, or `None` if not found.
    pub fn remove(&mut self, id: ObserverId) -> Option<T> {
        let idx = self.id_to_index.remove(&id)?;
        let (_, observer) = self.observers[idx].take()?;
        self.free_slots.push(idx);
        self.len -= 1;
        Some(observer)
    }

    /// Returns an iterator over the observers.
    ///
    /// The iterator skips removed slots automatically.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.observers
            .iter()
            .filter_map(|slot| slot.as_ref().map(|(_, v)| v))
    }

    /// Returns a mutable iterator over the observers.
    #[inline]
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.observers
            .iter_mut()
            .filter_map(|slot| slot.as_mut().map(|(_, v)| v))
    }

    /// Clears all observers from the list.
    pub fn clear(&mut self) {
        self.observers.clear();
        self.id_to_index.clear();
        self.free_slots.clear();
        self.len = 0;
    }

    /// Compacts the list by removing empty slots and rebuilding the index.
    ///
    /// Call this periodically if you have many add/remove cycles.
    pub fn compact(&mut self) {
        self.observers.retain(Option::is_some);
        self.free_slots.clear();
        // Rebuild the id-to-index map after slot positions changed
        self.id_to_index.clear();
        for (idx, slot) in self.observers.iter().enumerate() {
            if let Some((id, _)) = slot {
                self.id_to_index.insert(*id, idx);
            }
        }
    }
}

impl<T> Default for ObserverList<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone> Clone for ObserverList<T> {
    fn clone(&self) -> Self {
        Self {
            observers: self.observers.clone(),
            id_to_index: self.id_to_index.clone(),
            len: self.len,
            free_slots: self.free_slots.clone(),
            next_id: self.next_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_observer_list_add_remove() {
        let mut list: ObserverList<i32> = ObserverList::new();

        let id1 = list.add(1);
        let id2 = list.add(2);
        let id3 = list.add(3);

        assert_eq!(list.len(), 3);

        assert_eq!(list.remove(id2), Some(2));
        assert_eq!(list.len(), 2);

        // id2 is gone
        assert_eq!(list.remove(id2), None);

        // Others still there
        let values: Vec<_> = list.iter().copied().collect();
        assert!(values.contains(&1));
        assert!(values.contains(&3));
        assert!(!values.contains(&2));

        assert_eq!(list.remove(id1), Some(1));
        assert_eq!(list.remove(id3), Some(3));
        assert!(list.is_empty());
    }

    #[test]
    fn test_observer_list_iter() {
        let mut list: ObserverList<i32> = ObserverList::new();
        let _ = list.add(1);
        let _ = list.add(2);
        let _ = list.add(3);

        let values: Vec<_> = list.iter().copied().collect();
        assert_eq!(values, vec![1, 2, 3]);
    }

    #[test]
    fn test_observer_list_slot_reuse() {
        let mut list: ObserverList<i32> = ObserverList::new();

        let id1 = list.add(1);
        let _ = list.add(2);
        list.remove(id1);

        // Should reuse the slot
        let _ = list.add(3);
        assert_eq!(list.observers.len(), 2); // Still 2 slots
        assert_eq!(list.len(), 2); // 2 active observers
    }

    #[test]
    fn test_observer_list_compact() {
        let mut list: ObserverList<i32> = ObserverList::new();

        let id1 = list.add(1);
        let id2 = list.add(2);
        let _ = list.add(3);

        list.remove(id1);
        list.remove(id2);

        assert_eq!(list.observers.len(), 3); // 3 slots, 2 empty
        list.compact();
        assert_eq!(list.observers.len(), 1); // Only 1 slot now
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn test_observer_id_uniqueness() {
        let id1 = ObserverId::new(1);
        let id2 = ObserverId::new(2);
        let id3 = ObserverId::new(3);

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }
}
