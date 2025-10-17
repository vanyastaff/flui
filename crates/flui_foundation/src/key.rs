//! Key types for widget identification
//!
//! This module provides Flutter-like key types for uniquely identifying widgets.
//! Keys are used to preserve state across widget rebuilds and optimize updates.

use std::any::Any;
use std::fmt;
use std::hash::{Hash, Hasher};

/// A Key is an identifier for widgets and elements.
///
/// Similar to Flutter's Key class. Keys are used to control which widgets are
/// matched up with which other widgets when rebuilding the widget tree.
pub trait Key: fmt::Debug {
    /// Get a unique identifier for this key.
    fn id(&self) -> KeyId;

    /// Check if this key equals another key.
    fn equals(&self, other: &dyn Key) -> bool;

    /// Get this key as Any for downcasting.
    fn as_any(&self) -> &dyn Any;
}

/// Unique identifier for a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyId(u64);

impl KeyId {
    /// Create a new key ID from a hash.
    pub fn from_hash(hash: u64) -> Self {
        Self(hash)
    }

    /// Get the raw hash value.
    pub fn hash(&self) -> u64 {
        self.0
    }
}

/// A key that is not a GlobalKey.
///
/// Similar to Flutter's LocalKey.
pub trait LocalKey: Key {}

/// A key that uses its identity as its key.
///
/// Similar to Flutter's UniqueKey. Each instance is unique.
#[derive(Debug, Clone, Copy)]
pub struct UniqueKey {
    id: KeyId,
}

impl UniqueKey {
    /// Create a new unique key.
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        Self {
            id: KeyId(COUNTER.fetch_add(1, Ordering::SeqCst)),
        }
    }
}

impl Default for UniqueKey {
    fn default() -> Self {
        Self::new()
    }
}

impl Key for UniqueKey {
    fn id(&self) -> KeyId {
        self.id
    }

    fn equals(&self, other: &dyn Key) -> bool {
        if let Some(other_unique) = other.as_any().downcast_ref::<UniqueKey>() {
            self.id == other_unique.id
        } else {
            false
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl LocalKey for UniqueKey {}

/// A key that uses a value of a particular type to identify itself.
///
/// Similar to Flutter's `ValueKey<T>`.
#[derive(Debug, Clone)]
pub struct ValueKey<T: PartialEq + Hash + fmt::Debug + 'static> {
    value: T,
    id: KeyId,
}

impl<T: PartialEq + Hash + fmt::Debug + 'static> ValueKey<T> {
    /// Create a new value key.
    pub fn new(value: T) -> Self {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        value.hash(&mut hasher);
        std::any::TypeId::of::<T>().hash(&mut hasher);

        Self {
            value,
            id: KeyId(hasher.finish()),
        }
    }

    /// Get the value.
    pub fn value(&self) -> &T {
        &self.value
    }
}

impl<T: PartialEq + Hash + fmt::Debug + 'static> Key for ValueKey<T> {
    fn id(&self) -> KeyId {
        self.id
    }

    fn equals(&self, other: &dyn Key) -> bool {
        if let Some(other_value) = other.as_any().downcast_ref::<ValueKey<T>>() {
            self.value == other_value.value
        } else {
            false
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl<T: PartialEq + Hash + fmt::Debug + 'static> LocalKey for ValueKey<T> {}

/// A key that takes its identity from a string.
///
/// Convenience type for `ValueKey<String>`.
pub type StringKey = ValueKey<String>;

/// A key that takes its identity from an integer.
///
/// Convenience type for `ValueKey<i32>`.
pub type IntKey = ValueKey<i32>;

/// Helper to create keys from various types.
pub struct KeyFactory;

impl KeyFactory {
    /// Create a unique key.
    pub fn unique() -> UniqueKey {
        UniqueKey::new()
    }

    /// Create a value key from any hashable value.
    pub fn value<T: PartialEq + Hash + fmt::Debug + 'static>(value: T) -> ValueKey<T> {
        ValueKey::new(value)
    }

    /// Create a string key.
    pub fn string(value: impl Into<String>) -> StringKey {
        ValueKey::new(value.into())
    }

    /// Create an integer key.
    pub fn int(value: i32) -> IntKey {
        ValueKey::new(value)
    }
}

/// Optional key wrapper for widgets.
///
/// This provides a convenient way to store an optional key.
#[derive(Debug, Clone)]
pub enum WidgetKey {
    /// No key
    None,
    /// Unique key
    Unique(UniqueKey),
    /// String key
    String(StringKey),
    /// Integer key
    Int(IntKey),
}

impl WidgetKey {
    /// Create from a unique key.
    pub fn unique() -> Self {
        Self::Unique(UniqueKey::new())
    }

    /// Create from a string.
    pub fn string(value: impl Into<String>) -> Self {
        Self::String(ValueKey::new(value.into()))
    }

    /// Create from an integer.
    pub fn int(value: i32) -> Self {
        Self::Int(ValueKey::new(value))
    }

    /// Check if this is None.
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    /// Check if this is Some.
    pub fn is_some(&self) -> bool {
        !self.is_none()
    }

    /// Get the key ID if present.
    pub fn id(&self) -> Option<KeyId> {
        match self {
            Self::None => None,
            Self::Unique(k) => Some(k.id()),
            Self::String(k) => Some(k.id()),
            Self::Int(k) => Some(k.id()),
        }
    }
}

impl Default for WidgetKey {
    fn default() -> Self {
        Self::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unique_key() {
        let key1 = UniqueKey::new();
        let key2 = UniqueKey::new();

        // Each unique key should have a different ID
        assert_ne!(key1.id(), key2.id());

        // Unique key should equal itself
        assert!(key1.equals(&key1 as &dyn Key));

        // Different unique keys should not equal each other
        assert!(!key1.equals(&key2 as &dyn Key));
    }

    #[test]
    fn test_value_key_string() {
        let key1 = ValueKey::new("test".to_string());
        let key2 = ValueKey::new("test".to_string());
        let key3 = ValueKey::new("other".to_string());

        // Same value should produce equal keys
        assert!(key1.equals(&key2 as &dyn Key));

        // Different values should produce different keys
        assert!(!key1.equals(&key3 as &dyn Key));

        // Should have same ID for same value
        assert_eq!(key1.id(), key2.id());
        assert_ne!(key1.id(), key3.id());
    }

    #[test]
    fn test_value_key_int() {
        let key1 = ValueKey::new(42);
        let key2 = ValueKey::new(42);
        let key3 = ValueKey::new(100);

        assert!(key1.equals(&key2 as &dyn Key));
        assert!(!key1.equals(&key3 as &dyn Key));
        assert_eq!(key1.id(), key2.id());
    }

    #[test]
    fn test_key_factory() {
        let unique = KeyFactory::unique();
        let string = KeyFactory::string("test");
        let int = KeyFactory::int(42);

        assert_ne!(unique.id(), string.id());
        assert_ne!(string.id(), int.id());
    }

    #[test]
    fn test_widget_key() {
        let none = WidgetKey::None;
        let unique = WidgetKey::unique();
        let string = WidgetKey::string("test");
        let int = WidgetKey::int(42);

        assert!(none.is_none());
        assert!(!unique.is_none());
        assert!(unique.is_some());

        assert!(none.id().is_none());
        assert!(unique.id().is_some());
        assert!(string.id().is_some());
        assert!(int.id().is_some());
    }

    #[test]
    fn test_value_key_different_types() {
        let string_key = ValueKey::new("42".to_string());
        let int_key = ValueKey::new(42);

        // Keys with different types should not equal each other
        // (even if the value looks similar)
        assert!(!string_key.equals(&int_key as &dyn Key));
    }

    #[test]
    fn test_string_key_type_alias() {
        let key: StringKey = ValueKey::new("test".to_string());
        assert_eq!(key.value(), "test");
    }

    #[test]
    fn test_int_key_type_alias() {
        let key: IntKey = ValueKey::new(42);
        assert_eq!(*key.value(), 42);
    }
}
