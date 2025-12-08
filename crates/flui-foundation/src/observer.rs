//! Efficient observer list implementations
//!
//! This module provides specialized list types for managing observers/listeners
//! that are optimized for the common UI pattern of add/remove/iterate.
//!
//! # Features
//!
//! - `ObserverList<T>` - Index-based observer management with O(1) add/remove
//! - `HashedObserverList<T>` - Hash-based for unique observers with O(1) operations
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

use parking_lot::RwLock;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::id::ObserverId;

/// A list of observers with O(1) add/remove by ID.
///
/// Uses `VecDeque` internally for cache-friendly iteration and
/// stable indices during iteration.
///
/// # Thread Safety
///
/// This type is NOT thread-safe by itself. For concurrent access,
/// use `SyncObserverList` or wrap in your own synchronization.
#[derive(Debug)]
pub struct ObserverList<T> {
    observers: VecDeque<Option<(ObserverId, T)>>,
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
    pub const fn new() -> Self {
        Self {
            observers: VecDeque::new(),
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

        if let Some(slot) = self.free_slots.pop() {
            self.observers[slot] = Some((id, observer));
        } else {
            self.observers.push_back(Some((id, observer)));
        }

        self.len += 1;
        id
    }

    /// Removes an observer by its ID.
    ///
    /// Returns the observer if found, or `None` if not found.
    /// This is O(n) in worst case but typically fast due to early exit.
    pub fn remove(&mut self, id: ObserverId) -> Option<T> {
        let idx = self
            .observers
            .iter()
            .position(|slot| slot.as_ref().is_some_and(|(slot_id, _)| *slot_id == id))?;

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
        self.free_slots.clear();
        self.len = 0;
    }

    /// Compacts the list by removing empty slots.
    ///
    /// Call this periodically if you have many add/remove cycles.
    pub fn compact(&mut self) {
        self.observers.retain(Option::is_some);
        self.free_slots.clear();
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
            len: self.len,
            free_slots: self.free_slots.clone(),
            next_id: self.next_id,
        }
    }
}

/// A thread-safe observer list using `parking_lot::RwLock`.
///
/// This wraps `ObserverList` with a read-write lock for safe concurrent access.
///
/// # Examples
///
/// ```rust
/// use flui_foundation::SyncObserverList;
/// use std::sync::Arc;
///
/// let observers: Arc<SyncObserverList<i32>> = Arc::new(SyncObserverList::new());
///
/// // Can be shared across threads
/// let observers_clone = observers.clone();
/// std::thread::spawn(move || {
///     observers_clone.add(42);
/// });
/// ```
#[derive(Debug, Default)]
pub struct SyncObserverList<T> {
    inner: RwLock<ObserverList<T>>,
}

impl<T> SyncObserverList<T> {
    /// Creates a new empty sync observer list.
    #[inline]
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // RwLock::new is not const
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(ObserverList::new()),
        }
    }

    /// Returns the number of observers.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.read().len()
    }

    /// Returns true if the list is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.read().is_empty()
    }

    /// Adds an observer to the list.
    pub fn add(&self, observer: T) -> ObserverId {
        self.inner.write().add(observer)
    }

    /// Removes an observer by its ID.
    pub fn remove(&self, id: ObserverId) -> Option<T> {
        self.inner.write().remove(id)
    }

    /// Executes a function for each observer.
    ///
    /// This holds a read lock for the duration of iteration.
    pub fn for_each<F>(&self, mut f: F)
    where
        F: FnMut(&T),
    {
        let guard = self.inner.read();
        for observer in guard.iter() {
            f(observer);
        }
    }

    /// Clears all observers.
    pub fn clear(&self) {
        self.inner.write().clear();
    }
}

/// A hash-based observer list using `dashmap` for O(1) concurrent operations.
///
/// This is optimized for:
/// - Frequent concurrent add/remove operations
/// - When observer identity is by ID, not value equality
///
/// # Examples
///
/// ```rust
/// use flui_foundation::HashedObserverList;
/// use std::sync::Arc;
///
/// let observers: HashedObserverList<String> = HashedObserverList::new();
///
/// let id = observers.add("observer1".to_string());
/// observers.add("observer2".to_string());
///
/// observers.for_each(|s| println!("{}", s));
///
/// observers.remove(id);
/// ```
#[derive(Debug)]
pub struct HashedObserverList<T> {
    observers: dashmap::DashMap<ObserverId, T>,
    next_id: AtomicUsize,
}

impl<T> HashedObserverList<T> {
    /// Creates a new empty hashed observer list.
    #[inline]
    #[must_use]
    pub fn new() -> Self {
        Self {
            observers: dashmap::DashMap::new(),
            next_id: AtomicUsize::new(1),
        }
    }

    /// Creates a new hashed observer list with the specified capacity.
    #[inline]
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            observers: dashmap::DashMap::with_capacity(capacity),
            next_id: AtomicUsize::new(1),
        }
    }

    /// Returns the number of observers.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.observers.len()
    }

    /// Returns true if the list is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.observers.is_empty()
    }

    /// Adds an observer to the list.
    pub fn add(&self, observer: T) -> ObserverId {
        let id_val = self.next_id.fetch_add(1, Ordering::Relaxed);
        let id = ObserverId::new(id_val);
        self.observers.insert(id, observer);
        id
    }

    /// Removes an observer by its ID.
    pub fn remove(&self, id: ObserverId) -> Option<T> {
        self.observers.remove(&id).map(|(_, v)| v)
    }

    /// Executes a function for each observer.
    pub fn for_each<F>(&self, mut f: F)
    where
        F: FnMut(&T),
    {
        for entry in &self.observers {
            f(entry.value());
        }
    }

    /// Clears all observers.
    pub fn clear(&self) {
        self.observers.clear();
    }
}

impl<T> Default for HashedObserverList<T> {
    fn default() -> Self {
        Self::new()
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
    fn test_sync_observer_list() {
        let list = SyncObserverList::new();

        let id1 = list.add(1);
        let id2 = list.add(2);

        assert_eq!(list.len(), 2);

        let mut sum = 0;
        list.for_each(|&v| sum += v);
        assert_eq!(sum, 3);

        list.remove(id1);
        assert_eq!(list.len(), 1);

        list.remove(id2);
        assert!(list.is_empty());
    }

    #[test]
    fn test_hashed_observer_list() {
        let list: HashedObserverList<i32> = HashedObserverList::new();

        let id1 = list.add(1);
        let id2 = list.add(2);

        assert_eq!(list.len(), 2);

        let mut sum = 0;
        list.for_each(|&v| sum += v);
        assert_eq!(sum, 3);

        assert_eq!(list.remove(id1), Some(1));
        assert_eq!(list.len(), 1);

        assert_eq!(list.remove(id2), Some(2));
        assert!(list.is_empty());
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
