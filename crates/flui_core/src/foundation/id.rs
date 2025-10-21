//! Element identifiers
//!
//! Unique identifiers for elements in the widget tree.

use std::borrow::Borrow;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

/// Unique identifier for elements in the widget tree
///
/// Each element gets a unique ID when created. IDs are never reused,
/// even after the element is removed from the tree.
///
/// # Guarantees
///
/// - **Uniqueness**: No two elements will ever have the same ID
/// - **Monotonicity**: IDs increase over time (newer elements have higher IDs)
/// - **Thread-safety**: IDs can be generated safely from multiple threads
///
/// # Examples
///
/// ```rust
/// use flui_core::ElementId;
/// use std::collections::HashMap;
///
/// let id = ElementId::new();
/// println!("Element created with ID: {}", id);
///
/// // IDs can be compared
/// let id2 = ElementId::new();
/// assert!(id2 > id);
///
/// // Used as HashMap keys
/// let mut map = HashMap::new();
/// map.insert(id, "data");
///
/// // Can lookup by raw u64 (thanks to Borrow<u64>)
/// let raw_id = id.as_u64();
/// assert_eq!(map.get(&raw_id), Some(&"data"));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct ElementId(u64);

impl ElementId {
    /// Generates a new unique element ID
    ///
    /// IDs are monotonically increasing and thread-safe. Each call to `new()`
    /// is guaranteed to return a unique ID that has never been returned before
    /// and will never be returned again.
    ///
    /// # Performance
    ///
    /// This operation uses atomic fetch-add with relaxed ordering, which is
    /// very fast (typically just a single CPU instruction).
    ///
    /// # Overflow
    ///
    /// The internal counter is `u64`, which starts at 1 and increments by 1
    /// for each ID. At 1 billion IDs per second, it would take ~584 years
    /// to overflow. In practice, overflow is not a concern.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::ElementId;
    ///
    /// let id = ElementId::new();
    /// println!("Created element: {}", id);
    ///
    /// // Each ID is unique
    /// let id2 = ElementId::new();
    /// assert_ne!(id, id2);
    /// ```
    #[must_use = "creating an ID without using it is pointless"]
    #[inline]
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// Returns the raw ID value as a `u64`
    ///
    /// This is primarily useful for debugging, logging, and serialization.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::ElementId;
    ///
    /// let id = ElementId::new();
    /// let raw = id.as_u64();
    /// println!("Element ID: {}", raw);
    /// ```
    #[must_use]
    #[inline]
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    /// Creates an `ElementId` from a raw `u64` value
    ///
    /// # Safety
    ///
    /// This function is marked as `unsafe` because creating arbitrary IDs
    /// can break the uniqueness guarantee. The caller must ensure that:
    /// - The ID is not already in use by another element
    /// - The ID will not collide with future generated IDs
    ///
    /// # Use Cases
    ///
    /// This is primarily useful for:
    /// - Deserializing IDs from external sources
    /// - Testing scenarios where specific IDs are needed
    /// - Debugging and diagnostics
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::ElementId;
    ///
    /// // In tests or deserialization
    /// let id = unsafe { ElementId::from_raw(42) };
    /// assert_eq!(id.as_u64(), 42);
    /// ```
    #[must_use]
    #[inline]
    pub const unsafe fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Returns the next sequential ID (unsafe, for testing only)
    ///
    /// # Safety
    ///
    /// This creates an ID that may collide with naturally generated IDs.
    /// Only use this in controlled test environments where you need
    /// predictable ID sequences.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::ElementId;
    ///
    /// let id = unsafe { ElementId::from_raw(100) };
    /// let next = unsafe { id.next() };
    /// assert_eq!(next.as_u64(), 101);
    /// ```
    #[must_use]
    #[inline]
    pub const unsafe fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }

    /// Returns the previous sequential ID (unsafe, for testing only)
    ///
    /// Returns None if this is ID 0.
    ///
    /// # Safety
    ///
    /// This creates an ID that may collide with naturally generated IDs.
    /// Only use this in controlled test environments.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::ElementId;
    ///
    /// let id = unsafe { ElementId::from_raw(100) };
    /// let prev = unsafe { id.prev() }.unwrap();
    /// assert_eq!(prev.as_u64(), 99);
    /// ```
    #[must_use]
    #[inline]
    pub const unsafe fn prev(self) -> Option<Self> {
        if self.0 == 0 {
            None
        } else {
            Some(Self(self.0 - 1))
        }
    }

    /// Checks if this ID was created before another ID
    ///
    /// Since IDs are monotonically increasing, this can be used to determine
    /// the relative creation order of elements.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::ElementId;
    ///
    /// let old_id = ElementId::new();
    /// let new_id = ElementId::new();
    ///
    /// assert!(old_id.is_before(new_id));
    /// assert!(!new_id.is_before(old_id));
    /// ```
    #[must_use]
    #[inline]
    pub const fn is_before(self, other: Self) -> bool {
        self.0 < other.0
    }

    /// Checks if this ID was created after another ID
    ///
    /// Since IDs are monotonically increasing, this can be used to determine
    /// the relative creation order of elements.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::ElementId;
    ///
    /// let old_id = ElementId::new();
    /// let new_id = ElementId::new();
    ///
    /// assert!(new_id.is_after(old_id));
    /// assert!(!old_id.is_after(new_id));
    /// ```
    #[must_use]
    #[inline]
    pub const fn is_after(self, other: Self) -> bool {
        self.0 > other.0
    }

    /// Returns the absolute difference between two IDs
    ///
    /// This can be used to estimate how many elements were created between
    /// two IDs. Note that this is only an approximation if elements are
    /// created concurrently.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::ElementId;
    ///
    /// let id1 = ElementId::new();
    /// let id2 = ElementId::new();
    /// let id3 = ElementId::new();
    ///
    /// assert_eq!(id1.distance_to(id3), 2);
    /// assert_eq!(id3.distance_to(id1), 2); // Symmetric
    /// ```
    #[must_use]
    #[inline]
    pub const fn distance_to(self, other: Self) -> u64 {
        self.0.abs_diff(other.0)
    }

    /// Returns the numeric difference (self - other)
    ///
    /// Returns None if the subtraction would underflow.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_core::ElementId;
    ///
    /// let id1 = unsafe { ElementId::from_raw(10) };
    /// let id2 = unsafe { ElementId::from_raw(5) };
    ///
    /// assert_eq!(id1.checked_offset_from(id2), Some(5));
    /// assert_eq!(id2.checked_offset_from(id1), None);
    /// ```
    #[must_use]
    #[inline]
    pub const fn checked_offset_from(self, other: Self) -> Option<u64> {
        if self.0 >= other.0 {
            Some(self.0 - other.0)
        } else {
            None
        }
    }
}

impl Default for ElementId {
    /// Creates a new unique ID
    ///
    /// This is equivalent to calling `ElementId::new()`.
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ElementId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Element#{}", self.0)
    }
}

impl AsRef<u64> for ElementId {
    #[inline]
    fn as_ref(&self) -> &u64 {
        &self.0
    }
}

impl Borrow<u64> for ElementId {
    #[inline]
    fn borrow(&self) -> &u64 {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_element_id_unique() {
        let id1 = ElementId::new();
        let id2 = ElementId::new();
        let id3 = ElementId::new();

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_element_id_monotonic() {
        let id1 = ElementId::new();
        let id2 = ElementId::new();

        assert!(id2.0 > id1.0);
        assert!(id2 > id1);
    }

    #[test]
    fn test_element_id_default() {
        let id = ElementId::default();
        assert!(id.0 > 0);
    }

    #[test]
    fn test_element_id_display() {
        let id = unsafe { ElementId::from_raw(42) };
        assert_eq!(id.to_string(), "Element#42");
    }

    #[test]
    fn test_element_id_as_u64() {
        let id = unsafe { ElementId::from_raw(123) };
        assert_eq!(id.as_u64(), 123);
    }

    #[test]
    fn test_element_id_as_ref() {
        let id = unsafe { ElementId::from_raw(123) };
        let r: &u64 = id.as_ref();
        assert_eq!(*r, 123);
    }

    #[test]
    fn test_element_id_borrow() {
        use std::collections::HashMap;

        let id = unsafe { ElementId::from_raw(42) };
        let mut map = HashMap::new();
        map.insert(id, "value");

        // Can lookup by u64 thanks to Borrow<u64>
        assert_eq!(map.get(&42u64), Some(&"value"));
    }

    #[test]
    fn test_element_id_hash() {
        use std::collections::HashMap;

        let id1 = ElementId::new();
        let id2 = ElementId::new();

        let mut map = HashMap::new();
        map.insert(id1, "first");
        map.insert(id2, "second");

        assert_eq!(map.get(&id1), Some(&"first"));
        assert_eq!(map.get(&id2), Some(&"second"));
    }

    #[test]
    fn test_element_id_ord() {
        let id1 = ElementId::new();
        let id2 = ElementId::new();

        assert!(id1 < id2);
        assert!(id2 > id1);
        assert!(id1 <= id2);
        assert!(id2 >= id1);

        // Sorting
        let mut vec = vec![id2, id1];
        vec.sort();
        assert_eq!(vec, vec![id1, id2]);
    }

    #[test]
    fn test_element_id_is_before_after() {
        let old_id = ElementId::new();
        let new_id = ElementId::new();

        assert!(old_id.is_before(new_id));
        assert!(!new_id.is_before(old_id));
        assert!(new_id.is_after(old_id));
        assert!(!old_id.is_after(new_id));
    }

    #[test]
    fn test_element_id_distance() {
        let id1 = ElementId::new();
        let id2 = ElementId::new();
        let id3 = ElementId::new();

        assert_eq!(id1.distance_to(id3), 2);
        assert_eq!(id3.distance_to(id1), 2);

        assert_eq!(id1.distance_to(id2), 1);
        assert_eq!(id1.distance_to(id1), 0);
    }

    #[test]
    fn test_element_id_checked_offset() {
        let id1 = unsafe { ElementId::from_raw(10) };
        let id2 = unsafe { ElementId::from_raw(5) };

        assert_eq!(id1.checked_offset_from(id2), Some(5));
        assert_eq!(id2.checked_offset_from(id1), None);
        assert_eq!(id1.checked_offset_from(id1), Some(0));
    }

    #[test]
    fn test_element_id_next() {
        let id = unsafe { ElementId::from_raw(100) };
        let next = unsafe { id.next() };
        assert_eq!(next.as_u64(), 101);

        // Test saturation
        let max_id = unsafe { ElementId::from_raw(u64::MAX) };
        let next_max = unsafe { max_id.next() };
        assert_eq!(next_max.as_u64(), u64::MAX);
    }

    #[test]
    fn test_element_id_prev() {
        let id = unsafe { ElementId::from_raw(100) };
        let prev = unsafe { id.prev() }.unwrap();
        assert_eq!(prev.as_u64(), 99);

        let zero_id = unsafe { ElementId::from_raw(0) };
        assert!(unsafe { zero_id.prev() }.is_none());
    }

    #[test]
    fn test_element_id_copy() {
        let id1 = ElementId::new();
        let id2 = id1;

        assert_eq!(id1, id2);
        assert_eq!(id1.as_u64(), id2.as_u64());
    }

    #[test]
    fn test_element_id_const() {
        const fn check_const() -> u64 {
            let id = unsafe { ElementId::from_raw(42) };
            id.as_u64()
        }

        assert_eq!(check_const(), 42);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_element_id_serde() {
        let id = ElementId::new();
        let json = serde_json::to_string(&id).unwrap();
        let deserialized: ElementId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, deserialized);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_element_id_serde_transparent() {
        let id = unsafe { ElementId::from_raw(42) };
        let json = serde_json::to_string(&id).unwrap();
        // Should serialize as just the number, not {"0": 42}
        assert_eq!(json, "42");
    }
}