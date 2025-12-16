//! GlobalKey - Keys that provide access to elements across the tree.
//!
//! GlobalKeys allow finding and reparenting elements anywhere in the tree.
//! They're useful for:
//! - Accessing element state from outside the tree
//! - Reparenting elements with state preservation
//! - Triggering actions on distant elements

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

/// Unique identifier for a GlobalKey.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlobalKeyId(u64);

impl GlobalKeyId {
    /// Create a new unique GlobalKeyId.
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// Create a GlobalKeyId from a raw value (for testing/debugging).
    pub fn from_raw(value: u64) -> Self {
        Self(value)
    }

    /// Get the raw value.
    pub fn raw(&self) -> u64 {
        self.0
    }
}

impl Default for GlobalKeyId {
    fn default() -> Self {
        Self::new()
    }
}

/// A key that provides access to an element across the entire tree.
///
/// GlobalKeys are unique identifiers that allow:
/// - Looking up elements anywhere in the tree
/// - Reparenting elements while preserving state
/// - Accessing element state from outside the widget tree
///
/// # Flutter Equivalent
///
/// This corresponds to Flutter's `GlobalKey<T>` class:
///
/// ```dart
/// final GlobalKey<FormState> _formKey = GlobalKey<FormState>();
///
/// Form(
///   key: _formKey,
///   child: ...,
/// )
///
/// // Later:
/// _formKey.currentState?.validate();
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use flui_view::{GlobalKey, StatefulView};
///
/// // Create a global key
/// let form_key = GlobalKey::new();
///
/// // Use in a view
/// Form::new()
///     .key(form_key.clone())
///     .child(...)
///
/// // Later, access the element
/// if let Some(element_id) = form_key.current_element() {
///     // ...
/// }
/// ```
///
/// # Warning
///
/// GlobalKeys should be used sparingly. They:
/// - Prevent tree optimizations
/// - Can cause unexpected rebuilds
/// - Make code harder to reason about
///
/// Consider using callbacks or state management instead.
#[derive(Debug, Clone)]
pub struct GlobalKey {
    /// Unique identifier for this key.
    id: GlobalKeyId,
    /// Optional debug label.
    debug_label: Option<String>,
}

impl GlobalKey {
    /// Create a new GlobalKey.
    pub fn new() -> Self {
        Self {
            id: GlobalKeyId::new(),
            debug_label: None,
        }
    }

    /// Create a GlobalKey with a debug label.
    pub fn with_label(label: impl Into<String>) -> Self {
        Self {
            id: GlobalKeyId::new(),
            debug_label: Some(label.into()),
        }
    }

    /// Get the unique identifier.
    pub fn id(&self) -> GlobalKeyId {
        self.id
    }

    /// Get the debug label if any.
    pub fn debug_label(&self) -> Option<&str> {
        self.debug_label.as_deref()
    }

    /// Get the hash for use in lookup tables.
    pub fn hash_value(&self) -> u64 {
        self.id.0
    }
}

impl Default for GlobalKey {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for GlobalKey {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for GlobalKey {}

impl Hash for GlobalKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

/// A value-based key for list reconciliation.
///
/// Unlike GlobalKey which uses identity, ValueKey uses the value itself
/// for equality comparison. Useful for keying list items by their ID.
///
/// # Example
///
/// ```rust,ignore
/// items.iter().map(|item| {
///     ListTile::new(item.title.clone())
///         .key(ValueKey::new(item.id))
/// })
/// ```
#[derive(Debug, Clone)]
pub struct ValueKey<T: Hash + Eq + Clone + Send + Sync + 'static> {
    value: T,
    hash: u64,
}

impl<T: Hash + Eq + Clone + Send + Sync + 'static> ValueKey<T> {
    /// Create a new ValueKey with the given value.
    pub fn new(value: T) -> Self {
        let hash = {
            let mut hasher = DefaultHasher::new();
            value.hash(&mut hasher);
            hasher.finish()
        };
        Self { value, hash }
    }

    /// Get the value.
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Get the hash value.
    pub fn hash_value(&self) -> u64 {
        self.hash
    }
}

impl<T: Hash + Eq + Clone + Send + Sync + 'static> PartialEq for ValueKey<T> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl<T: Hash + Eq + Clone + Send + Sync + 'static> Eq for ValueKey<T> {}

impl<T: Hash + Eq + Clone + Send + Sync + 'static> Hash for ValueKey<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.value.hash(state);
    }
}

/// A unique key that uses object identity.
///
/// Each ObjectKey is unique - two ObjectKeys are only equal if they
/// are the same instance.
///
/// # Flutter Equivalent
///
/// This corresponds to Flutter's `ObjectKey`.
#[derive(Debug, Clone)]
pub struct ObjectKey {
    id: u64,
}

impl ObjectKey {
    /// Create a new unique ObjectKey.
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self {
            id: COUNTER.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// Get the hash value.
    pub fn hash_value(&self) -> u64 {
        self.id
    }
}

impl Default for ObjectKey {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for ObjectKey {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for ObjectKey {}

impl Hash for ObjectKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_global_key_uniqueness() {
        let key1 = GlobalKey::new();
        let key2 = GlobalKey::new();

        assert_ne!(key1, key2);
        assert_ne!(key1.id(), key2.id());
    }

    #[test]
    fn test_global_key_clone_equality() {
        let key1 = GlobalKey::new();
        let key2 = key1.clone();

        assert_eq!(key1, key2);
        assert_eq!(key1.id(), key2.id());
    }

    #[test]
    fn test_global_key_with_label() {
        let key = GlobalKey::with_label("test_key");

        assert_eq!(key.debug_label(), Some("test_key"));
    }

    #[test]
    fn test_value_key_equality() {
        let key1 = ValueKey::new(42u32);
        let key2 = ValueKey::new(42u32);
        let key3 = ValueKey::new(99u32);

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_value_key_hash() {
        let key1 = ValueKey::new("hello");
        let key2 = ValueKey::new("hello");
        let key3 = ValueKey::new("world");

        assert_eq!(key1.hash_value(), key2.hash_value());
        assert_ne!(key1.hash_value(), key3.hash_value());
    }

    #[test]
    fn test_object_key_uniqueness() {
        let key1 = ObjectKey::new();
        let key2 = ObjectKey::new();

        assert_ne!(key1, key2);
    }

    #[test]
    fn test_object_key_clone_equality() {
        let key1 = ObjectKey::new();
        let key2 = key1.clone();

        assert_eq!(key1, key2);
    }
}
