//! Element identifiers
//!
//! Unique identifiers for elements in the tree.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

/// Unique identifier for elements in the tree
///
/// Each element gets a unique ID when created. IDs are never reused.
///
/// # Example
///
/// ```
/// use flui_core::ElementId;
///
/// let id1 = ElementId::new();
/// let id2 = ElementId::new();
///
/// assert_ne!(id1, id2);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ElementId(u64);

impl ElementId {
    /// Generate a new unique element ID
    ///
    /// IDs are monotonically increasing and thread-safe.
    ///
    /// # Example
    ///
    /// ```
    /// use flui_core::ElementId;
    ///
    /// let id = ElementId::new();
    /// println!("Created element: {}", id);
    /// ```
    #[inline]
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// Get the raw ID value
    ///
    /// Mainly for debugging and logging.
    #[inline]
    pub fn as_u64(self) -> u64 {
        self.0
    }

    /// Create an ElementId from a raw u64 (test only)
    ///
    /// In tests, you can create arbitrary element IDs for testing purposes.
    #[inline]
    #[cfg(test)]
    pub fn from_raw(raw: u64) -> Self {
        Self(raw)
    }
}

impl Default for ElementId {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ElementId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
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
    }

    #[test]
    fn test_element_id_default() {
        let id = ElementId::default();
        assert!(id.0 > 0);
    }

    #[test]
    fn test_element_id_display() {
        let id = ElementId::from_raw(42);
        assert_eq!(id.to_string(), "#42");
    }

    #[test]
    fn test_element_id_as_u64() {
        let id = ElementId::from_raw(123);
        assert_eq!(id.as_u64(), 123);
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
    }
}
