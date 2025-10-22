//! Observer list implementations for efficient listener management
//!
//! This module provides two observer list implementations:
//! - [`ObserverList`] - Simple Vec-based list, optimized for iteration
//! - [`HashedObserverList`] - AHashSet-based list, optimized for O(1) removal
//!
//! # Performance Note
//!
//! `HashedObserverList` uses `AHashSet` from the `ahash` crate, which provides
//! significantly better performance than the standard library's `HashSet`,
//! especially for small keys and DoS protection.

use ahash::AHashSet;
use parking_lot::RwLock;
use std::hash::Hash;
use std::sync::Arc;

// ============================================================================
// ObserverList - Vec-based implementation
// ============================================================================

/// A simple list of observers stored in a Vec
///
/// This is optimized for the common case where:
/// - Observers are rarely removed
/// - Iteration over all observers is frequent
/// - The number of observers is small to medium
///
/// # Performance characteristics
/// - Add: O(1) amortized
/// - Remove: O(n) - requires linear search
/// - Iterate: O(n) - cache-friendly sequential access
/// - Contains: O(n) - requires linear search
///
/// # Thread safety
/// Uses `RwLock` for interior mutability, allowing multiple concurrent readers.
///
/// # Example
///
/// ```
/// use flui_core::foundation::observer_list::ObserverList;
///
/// let mut list = ObserverList::new();
/// let observer1 = 1;
/// let observer2 = 2;
///
/// list.add(observer1);
/// list.add(observer2);
///
/// assert_eq!(list.len(), 2);
/// assert!(list.contains(&observer1));
///
/// list.remove(&observer1);
/// assert_eq!(list.len(), 1);
/// assert!(!list.contains(&observer1));
/// ```
#[derive(Debug)]
pub struct ObserverList<T> {
    observers: Arc<RwLock<Vec<T>>>,
}

impl<T> ObserverList<T> {
    /// Creates a new empty observer list
    #[must_use]
    pub fn new() -> Self {
        Self {
            observers: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Creates a new observer list with the specified capacity
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            observers: Arc::new(RwLock::new(Vec::with_capacity(capacity))),
        }
    }

    /// Returns the number of observers in the list
    #[must_use]
    pub fn len(&self) -> usize {
        self.observers.read().len()
    }

    /// Returns true if the list contains no observers
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.observers.read().is_empty()
    }

    /// Adds an observer to the list
    ///
    /// # Note
    /// This does not check for duplicates. The same observer can be added multiple times.
    pub fn add(&mut self, observer: T) {
        self.observers.write().push(observer);
    }

    /// Removes all occurrences of the observer from the list
    ///
    /// Returns the number of observers removed.
    ///
    /// # Performance
    /// This is O(n) as it requires a linear search through the list.
    pub fn remove(&mut self, observer: &T) -> usize
    where
        T: PartialEq,
    {
        let mut observers = self.observers.write();
        let original_len = observers.len();
        observers.retain(|o| o != observer);
        original_len - observers.len()
    }

    /// Removes the first occurrence of the observer from the list
    ///
    /// Returns `true` if an observer was removed, `false` otherwise.
    pub fn remove_once(&mut self, observer: &T) -> bool
    where
        T: PartialEq,
    {
        let mut observers = self.observers.write();
        if let Some(pos) = observers.iter().position(|o| o == observer) {
            observers.remove(pos);
            true
        } else {
            false
        }
    }

    /// Checks if the list contains the specified observer
    ///
    /// # Performance
    /// This is O(n) as it requires a linear search.
    #[must_use]
    pub fn contains(&self, observer: &T) -> bool
    where
        T: PartialEq,
    {
        self.observers.read().contains(observer)
    }

    /// Removes all observers from the list
    pub fn clear(&mut self) {
        self.observers.write().clear();
    }

    /// Iterates over all observers, calling the provided closure for each
    ///
    /// The closure receives a reference to each observer.
    ///
    /// # Example
    ///
    /// ```
    /// use flui_core::foundation::observer_list::ObserverList;
    ///
    /// let mut list = ObserverList::new();
    /// list.add(1);
    /// list.add(2);
    ///
    /// let mut sum = 0;
    /// list.for_each(|&observer| {
    ///     sum += observer;
    /// });
    /// assert_eq!(sum, 3);
    /// ```
    pub fn for_each<F>(&self, mut f: F)
    where
        F: FnMut(&T),
    {
        let observers = self.observers.read();
        for observer in observers.iter() {
            f(observer);
        }
    }

    /// Returns a copy of all observers as a Vec
    ///
    /// This is useful when you need to iterate over observers
    /// without holding a lock.
    #[must_use]
    pub fn to_vec(&self) -> Vec<T>
    where
        T: Clone,
    {
        self.observers.read().clone()
    }

    /// Retains only the observers that satisfy the predicate
    ///
    /// Removes all observers for which the predicate returns `false`.
    /// This is more efficient than manually removing each observer.
    ///
    /// # Example
    ///
    /// ```
    /// use flui_core::foundation::observer_list::ObserverList;
    ///
    /// let mut list = ObserverList::new();
    /// list.add(1);
    /// list.add(2);
    /// list.add(3);
    /// list.add(4);
    ///
    /// // Retain only even numbers
    /// list.retain(|&x| x % 2 == 0);
    ///
    /// assert_eq!(list.len(), 2);
    /// assert!(list.contains(&2));
    /// assert!(list.contains(&4));
    /// assert!(!list.contains(&1));
    /// assert!(!list.contains(&3));
    /// ```
    pub fn retain<F>(&mut self, mut f: F)
    where
        T: PartialEq,
        F: FnMut(&T) -> bool,
    {
        self.observers.write().retain(|x| f(x));
    }

    /// Returns an iterator that yields clones of all observers
    ///
    /// Note: This creates a snapshot of the current observers and returns
    /// an iterator over that snapshot. Changes to the list after calling
    /// this method won't be reflected in the iterator.
    ///
    /// # Example
    ///
    /// ```
    /// use flui_core::foundation::observer_list::ObserverList;
    ///
    /// let mut list = ObserverList::new();
    /// list.add(1);
    /// list.add(2);
    /// list.add(3);
    ///
    /// let sum: i32 = list.iter().sum();
    /// assert_eq!(sum, 6);
    /// ```
    pub fn iter(&self) -> impl Iterator<Item = T> + '_
    where
        T: Clone,
    {
        self.to_vec().into_iter()
    }
}

impl<T> Default for ObserverList<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Clone for ObserverList<T> {
    fn clone(&self) -> Self {
        Self {
            observers: Arc::clone(&self.observers),
        }
    }
}

// ============================================================================
// HashedObserverList - HashSet-based implementation
// ============================================================================

/// A hashed list of observers stored in an AHashSet
///
/// This is optimized for the case where:
/// - Observers are frequently added and removed
/// - Fast removal is important (O(1) instead of O(n))
/// - The number of observers is large
/// - Order of observers doesn't matter
///
/// # Performance characteristics
/// - Add: O(1) average case
/// - Remove: O(1) average case
/// - Iterate: O(n) - potentially less cache-friendly than Vec
/// - Contains: O(1) average case
///
/// # Performance Note
///
/// Uses `AHashSet` from the `ahash` crate, which provides:
/// - Faster hashing than the standard library (especially for small keys)
/// - DoS protection without performance penalty
/// - Better performance characteristics for typical use cases
///
/// # Thread safety
/// Uses `RwLock` for interior mutability, allowing multiple concurrent readers.
///
/// # Requirements
/// The observer type `T` must implement `Hash` and `Eq`.
///
/// # Example
///
/// ```
/// use flui_core::foundation::observer_list::HashedObserverList;
///
/// let mut list = HashedObserverList::new();
/// let observer1 = 1;
/// let observer2 = 2;
///
/// list.add(observer1);
/// list.add(observer2);
///
/// assert_eq!(list.len(), 2);
/// assert!(list.contains(&observer1));
///
/// list.remove(&observer1);
/// assert_eq!(list.len(), 1);
/// assert!(!list.contains(&observer1));
/// ```
#[derive(Debug)]
pub struct HashedObserverList<T> {
    observers: Arc<RwLock<AHashSet<T>>>,
}

impl<T: Hash + Eq> HashedObserverList<T> {
    /// Creates a new empty hashed observer list
    #[must_use]
    pub fn new() -> Self {
        Self {
            observers: Arc::new(RwLock::new(AHashSet::new())),
        }
    }

    /// Creates a new hashed observer list with the specified capacity
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            observers: Arc::new(RwLock::new(AHashSet::with_capacity(capacity))),
        }
    }

    /// Returns the number of observers in the list
    #[must_use]
    pub fn len(&self) -> usize {
        self.observers.read().len()
    }

    /// Returns true if the list contains no observers
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.observers.read().is_empty()
    }

    /// Adds an observer to the list
    ///
    /// Returns `true` if the observer was added, `false` if it was already present.
    ///
    /// # Note
    /// Unlike `ObserverList`, this prevents duplicates due to HashSet semantics.
    pub fn add(&mut self, observer: T) -> bool {
        self.observers.write().insert(observer)
    }

    /// Removes the observer from the list
    ///
    /// Returns `true` if the observer was present and removed, `false` otherwise.
    ///
    /// # Performance
    /// This is O(1) average case, much faster than `ObserverList::remove`.
    pub fn remove(&mut self, observer: &T) -> bool {
        self.observers.write().remove(observer)
    }

    /// Checks if the list contains the specified observer
    ///
    /// # Performance
    /// This is O(1) average case.
    #[must_use]
    pub fn contains(&self, observer: &T) -> bool {
        self.observers.read().contains(observer)
    }

    /// Removes all observers from the list
    pub fn clear(&mut self) {
        self.observers.write().clear();
    }

    /// Iterates over all observers, calling the provided closure for each
    ///
    /// The closure receives a reference to each observer.
    ///
    /// # Note
    /// The order of iteration is not guaranteed.
    ///
    /// # Example
    ///
    /// ```
    /// use flui_core::foundation::observer_list::HashedObserverList;
    ///
    /// let mut list = HashedObserverList::new();
    /// list.add(1);
    /// list.add(2);
    ///
    /// let mut sum = 0;
    /// list.for_each(|&observer| {
    ///     sum += observer;
    /// });
    /// assert_eq!(sum, 3);
    /// ```
    pub fn for_each<F>(&self, mut f: F)
    where
        F: FnMut(&T),
    {
        let observers = self.observers.read();
        for observer in observers.iter() {
            f(observer);
        }
    }

    /// Returns a copy of all observers as a Vec
    ///
    /// This is useful when you need to iterate over observers
    /// without holding a lock.
    ///
    /// # Note
    /// The order of observers in the Vec is not guaranteed.
    #[must_use]
    pub fn to_vec(&self) -> Vec<T>
    where
        T: Clone,
    {
        self.observers.read().iter().cloned().collect()
    }

    /// Retains only the observers that satisfy the predicate
    ///
    /// Removes all observers for which the predicate returns `false`.
    /// This is more efficient than manually removing each observer.
    ///
    /// # Example
    ///
    /// ```
    /// use flui_core::foundation::observer_list::HashedObserverList;
    ///
    /// let mut list = HashedObserverList::new();
    /// list.add(1);
    /// list.add(2);
    /// list.add(3);
    /// list.add(4);
    ///
    /// // Retain only even numbers
    /// list.retain(|&x| x % 2 == 0);
    ///
    /// assert_eq!(list.len(), 2);
    /// assert!(list.contains(&2));
    /// assert!(list.contains(&4));
    /// assert!(!list.contains(&1));
    /// assert!(!list.contains(&3));
    /// ```
    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&T) -> bool,
    {
        self.observers.write().retain(|x| f(x));
    }

    /// Returns an iterator that yields clones of all observers
    ///
    /// Note: This creates a snapshot of the current observers and returns
    /// an iterator over that snapshot. Changes to the list after calling
    /// this method won't be reflected in the iterator.
    ///
    /// The order of iteration is not guaranteed.
    ///
    /// # Example
    ///
    /// ```
    /// use flui_core::foundation::observer_list::HashedObserverList;
    ///
    /// let mut list = HashedObserverList::new();
    /// list.add(1);
    /// list.add(2);
    /// list.add(3);
    ///
    /// let sum: i32 = list.iter().sum();
    /// assert_eq!(sum, 6);
    /// ```
    pub fn iter(&self) -> impl Iterator<Item = T> + '_
    where
        T: Clone,
    {
        self.to_vec().into_iter()
    }
}

impl<T: Hash + Eq> Default for HashedObserverList<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Clone for HashedObserverList<T> {
    fn clone(&self) -> Self {
        Self {
            observers: Arc::clone(&self.observers),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_observer_list_basic() {
        let mut list = ObserverList::new();
        assert_eq!(list.len(), 0);
        assert!(list.is_empty());

        list.add(1);
        list.add(2);
        list.add(3);

        assert_eq!(list.len(), 3);
        assert!(!list.is_empty());
        assert!(list.contains(&1));
        assert!(list.contains(&2));
        assert!(list.contains(&3));
        assert!(!list.contains(&4));
    }

    #[test]
    fn test_observer_list_remove() {
        let mut list = ObserverList::new();
        list.add(1);
        list.add(2);
        list.add(3);

        assert_eq!(list.remove(&2), 1);
        assert_eq!(list.len(), 2);
        assert!(!list.contains(&2));
        assert!(list.contains(&1));
        assert!(list.contains(&3));
    }

    #[test]
    fn test_observer_list_remove_duplicates() {
        let mut list = ObserverList::new();
        list.add(1);
        list.add(2);
        list.add(1);
        list.add(1);

        assert_eq!(list.len(), 4);
        assert_eq!(list.remove(&1), 3);
        assert_eq!(list.len(), 1);
        assert!(!list.contains(&1));
    }

    #[test]
    fn test_observer_list_remove_once() {
        let mut list = ObserverList::new();
        list.add(1);
        list.add(1);
        list.add(1);

        assert!(list.remove_once(&1));
        assert_eq!(list.len(), 2);
        assert!(list.contains(&1));

        assert!(list.remove_once(&1));
        assert_eq!(list.len(), 1);

        assert!(list.remove_once(&1));
        assert_eq!(list.len(), 0);

        assert!(!list.remove_once(&1));
    }

    #[test]
    fn test_observer_list_clear() {
        let mut list = ObserverList::new();
        list.add(1);
        list.add(2);
        list.add(3);

        list.clear();
        assert_eq!(list.len(), 0);
        assert!(list.is_empty());
    }

    #[test]
    fn test_observer_list_for_each() {
        let mut list = ObserverList::new();
        list.add(1);
        list.add(2);
        list.add(3);

        let mut sum = 0;
        list.for_each(|&observer| {
            sum += observer;
        });
        assert_eq!(sum, 6);
    }

    #[test]
    fn test_observer_list_to_vec() {
        let mut list = ObserverList::new();
        list.add(1);
        list.add(2);
        list.add(3);

        let vec = list.to_vec();
        assert_eq!(vec, vec![1, 2, 3]);
    }

    #[test]
    fn test_observer_list_clone() {
        let mut list1 = ObserverList::new();
        list1.add(1);
        list1.add(2);

        let list2 = list1.clone();
        assert_eq!(list1.len(), list2.len());
        assert!(list2.contains(&1));
        assert!(list2.contains(&2));
    }

    #[test]
    fn test_hashed_observer_list_basic() {
        let mut list = HashedObserverList::new();
        assert_eq!(list.len(), 0);
        assert!(list.is_empty());

        assert!(list.add(1));
        assert!(list.add(2));
        assert!(list.add(3));

        assert_eq!(list.len(), 3);
        assert!(!list.is_empty());
        assert!(list.contains(&1));
        assert!(list.contains(&2));
        assert!(list.contains(&3));
        assert!(!list.contains(&4));
    }

    #[test]
    fn test_hashed_observer_list_no_duplicates() {
        let mut list = HashedObserverList::new();

        assert!(list.add(1));
        assert!(!list.add(1)); // Duplicate
        assert_eq!(list.len(), 1);
    }

    #[test]
    fn test_hashed_observer_list_remove() {
        let mut list = HashedObserverList::new();
        list.add(1);
        list.add(2);
        list.add(3);

        assert!(list.remove(&2));
        assert_eq!(list.len(), 2);
        assert!(!list.contains(&2));
        assert!(list.contains(&1));
        assert!(list.contains(&3));

        assert!(!list.remove(&2)); // Already removed
    }

    #[test]
    fn test_hashed_observer_list_clear() {
        let mut list = HashedObserverList::new();
        list.add(1);
        list.add(2);
        list.add(3);

        list.clear();
        assert_eq!(list.len(), 0);
        assert!(list.is_empty());
    }

    #[test]
    fn test_hashed_observer_list_for_each() {
        let mut list = HashedObserverList::new();
        list.add(1);
        list.add(2);
        list.add(3);

        let mut sum = 0;
        list.for_each(|&observer| {
            sum += observer;
        });
        assert_eq!(sum, 6);
    }

    #[test]
    fn test_hashed_observer_list_to_vec() {
        let mut list = HashedObserverList::new();
        list.add(1);
        list.add(2);
        list.add(3);

        let mut vec = list.to_vec();
        vec.sort(); // HashSet doesn't guarantee order
        assert_eq!(vec, vec![1, 2, 3]);
    }

    #[test]
    fn test_hashed_observer_list_clone() {
        let mut list1 = HashedObserverList::new();
        list1.add(1);
        list1.add(2);

        let list2 = list1.clone();
        assert_eq!(list1.len(), list2.len());
        assert!(list2.contains(&1));
        assert!(list2.contains(&2));
    }
}
