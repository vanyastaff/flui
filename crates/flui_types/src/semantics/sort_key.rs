//! Sort keys for ordering semantic nodes

use std::cmp::Ordering;

/// Base trait for semantics sort keys
///
/// Similar to Flutter's `SemanticsSortKey`. Used to determine the
/// traversal order of semantic nodes for accessibility.
pub trait SemanticsSortKey: std::fmt::Debug {
    /// Compares this sort key with another
    fn compare_to(&self, other: &dyn SemanticsSortKey) -> Ordering;

    /// Returns a name for this sort key type
    fn key_name(&self) -> &str;
}

/// A sort key that orders nodes by ordinal value
///
/// Similar to Flutter's `OrdinalSortKey`. Nodes with lower ordinal
/// values come first in traversal order.
///
/// # Examples
///
/// ```
/// use flui_types::semantics::OrdinalSortKey;
///
/// let key1 = OrdinalSortKey::new(1.0);
/// let key2 = OrdinalSortKey::new(2.0);
///
/// // Use PartialOrd to compare by ordinal value
/// assert!(key1 < key2);
/// assert!(key2 > key1);
///
/// // Named keys can be compared by name using SemanticsSortKey::compare_to
/// let named1 = OrdinalSortKey::with_name(1.0, "aaa");
/// let named2 = OrdinalSortKey::with_name(2.0, "bbb");
/// assert!(named1 < named2); // Compares by order value
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OrdinalSortKey {
    order: f32,
    name: Option<&'static str>,
}

impl OrdinalSortKey {
    /// Creates a new ordinal sort key
    pub const fn new(order: f32) -> Self {
        Self { order, name: None }
    }

    /// Creates a new ordinal sort key with a name for debugging
    pub const fn with_name(order: f32, name: &'static str) -> Self {
        Self {
            order,
            name: Some(name),
        }
    }

    /// Returns the ordinal value
    pub fn order(&self) -> f32 {
        self.order
    }

    /// Returns the name, if set
    pub fn name(&self) -> Option<&str> {
        self.name
    }
}

impl SemanticsSortKey for OrdinalSortKey {
    fn compare_to(&self, other: &dyn SemanticsSortKey) -> Ordering {
        // In a full implementation, we would need to compare different types
        // For now, we just compare by type name as a simple fallback
        self.key_name().cmp(other.key_name())
    }

    fn key_name(&self) -> &str {
        self.name.unwrap_or("OrdinalSortKey")
    }
}

impl PartialOrd for OrdinalSortKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        // Compare by order value directly
        self.order.partial_cmp(&other.order)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ordinal_sort_key_new() {
        let key = OrdinalSortKey::new(1.0);
        assert_eq!(key.order(), 1.0);
        assert_eq!(key.name(), None);
    }

    #[test]
    fn test_ordinal_sort_key_with_name() {
        let key = OrdinalSortKey::with_name(2.0, "test_key");
        assert_eq!(key.order(), 2.0);
        assert_eq!(key.name(), Some("test_key"));
    }

    #[test]
    fn test_ordinal_sort_key_compare() {
        // compare_to compares by type name
        let key1 = OrdinalSortKey::new(1.0);
        let key2 = OrdinalSortKey::new(2.0);

        // Both have the same type name, so they're equal in compare_to
        assert_eq!(key1.compare_to(&key2), Ordering::Equal);

        // Named keys can differ
        let named1 = OrdinalSortKey::with_name(1.0, "aaa");
        let named2 = OrdinalSortKey::with_name(2.0, "bbb");

        assert_eq!(named1.compare_to(&named2), Ordering::Less);
        assert_eq!(named2.compare_to(&named1), Ordering::Greater);
    }

    #[test]
    fn test_ordinal_sort_key_partial_ord() {
        let key1 = OrdinalSortKey::new(1.0);
        let key2 = OrdinalSortKey::new(2.0);

        assert!(key1 < key2);
        assert!(key2 > key1);
        assert_eq!(key1.partial_cmp(&key1), Some(Ordering::Equal));
    }

    #[test]
    fn test_ordinal_sort_key_name() {
        let key = OrdinalSortKey::new(1.0);
        assert_eq!(key.key_name(), "OrdinalSortKey");

        let named_key = OrdinalSortKey::with_name(1.0, "custom");
        assert_eq!(named_key.key_name(), "custom");
    }
}
