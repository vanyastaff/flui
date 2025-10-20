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

/// A key that is unique across the entire app.
///
/// Similar to Flutter's GlobalKey. Global keys uniquely identify elements across
/// the entire widget hierarchy. They can be used to access the associated element,
/// widget, or state from anywhere in the app.
///
/// # Type Parameter
///
/// - `T`: The type of State this key references (use `()` for non-stateful)
///
/// # Example
///
/// ```
/// use crate::foundation::{GlobalKey, Key};
///
/// // Create a global key
/// let key = GlobalKey::<()>::new();
///
/// // Keys have unique IDs
/// let key2 = GlobalKey::<()>::new();
/// assert_ne!(key.id(), key2.id());
/// ```
pub struct GlobalKey<T = ()> {
    id: KeyId,
    _phantom: std::marker::PhantomData<T>,
}

impl<T> std::fmt::Debug for GlobalKey<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlobalKey")
            .field("id", &self.id)
            .finish()
    }
}

impl<T> GlobalKey<T> {
    /// Create a new global key.
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1_000_000); // Start high to avoid UniqueKey collision
        Self {
            id: KeyId(COUNTER.fetch_add(1, Ordering::SeqCst)),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Convert to GlobalKeyId for BuildOwner registry
    ///
    /// This allows registering the key with BuildOwner while maintaining type safety.
    pub fn to_global_key_id(&self) -> crate::tree::build_owner::GlobalKeyId {
        crate::tree::build_owner::GlobalKeyId::from_raw(self.id.0)
    }

    /// Get the raw ID (useful for debugging)
    pub fn raw_id(&self) -> u64 {
        self.id.0
    }

    /// Get the BuildContext for the element registered with this key
    ///
    /// Returns `None` if:
    /// - The key is not registered with any element
    /// - The element is not currently in the tree
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let key = GlobalKey::<()>::new();
    /// // ... mount widget with this key ...
    /// if let Some(context) = key.current_context(&owner) {
    ///     // Use context
    /// }
    /// ```
    pub fn current_context(&self, owner: &crate::tree::build_owner::BuildOwner) -> Option<crate::context::Context> {
        let element_id = owner.get_element_for_global_key(self.to_global_key_id())?;
        let tree = owner.tree();
        let tree_guard = tree.read();

        // Check if element exists in tree
        tree_guard.get(element_id)?;

        // Create Context from tree and element_id
        Some(crate::context::Context::new(tree.clone(), element_id))
    }

    /// Get the Widget for the element registered with this key
    ///
    /// Returns `None` if:
    /// - The key is not registered with any element
    /// - The element is not currently in the tree
    ///
    /// Note: This returns a reference that's valid only during the read lock.
    /// For now, this method is marked as unimplemented since we need to solve
    /// lifetime issues with the tree lock.
    pub fn current_widget(&self, _owner: &crate::tree::build_owner::BuildOwner) -> Option<()> {
        // TODO: Implement this when we have a way to return widget references
        // The challenge is that we need to keep the tree lock alive while
        // returning a reference to the widget.
        None
    }

    /// Get the State object for the StatefulElement registered with this key
    ///
    /// Returns `None` if:
    /// - The key is not registered with any element
    /// - The element is not a StatefulElement
    /// - The state type doesn't match T
    ///
    /// Note: This method is marked as unimplemented since we need to solve
    /// lifetime issues and add downcasting support for State objects.
    pub fn current_state(&self, _owner: &crate::tree::build_owner::BuildOwner) -> Option<()> {
        // TODO: Implement this when we have:
        // 1. A way to downcast AnyElement to StatefulElement
        // 2. A way to access State from StatefulElement
        // 3. Lifetime management for the returned reference
        None
    }
}

impl<T> Default for GlobalKey<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Clone for GlobalKey<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for GlobalKey<T> {}

impl<T: 'static> Key for GlobalKey<T> {
    fn id(&self) -> KeyId {
        self.id
    }

    fn equals(&self, other: &dyn Key) -> bool {
        if let Some(other_global) = other.as_any().downcast_ref::<GlobalKey<T>>() {
            self.id == other_global.id
        } else {
            false
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A GlobalKey with a debug label for easier debugging.
///
/// Similar to Flutter's LabeledGlobalKey.
pub struct LabeledGlobalKey<T = ()> {
    key: GlobalKey<T>,
    label: String,
}

impl<T> std::fmt::Debug for LabeledGlobalKey<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LabeledGlobalKey")
            .field("label", &self.label)
            .field("id", &self.key.id)
            .finish()
    }
}

impl<T> LabeledGlobalKey<T> {
    /// Create a new labeled global key.
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            key: GlobalKey::new(),
            label: label.into(),
        }
    }

    /// Get the label.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Get the underlying GlobalKey.
    pub fn key(&self) -> &GlobalKey<T> {
        &self.key
    }

    /// Get the raw ID.
    pub fn raw_id(&self) -> u64 {
        self.key.raw_id()
    }
}

impl<T> Clone for LabeledGlobalKey<T> {
    fn clone(&self) -> Self {
        Self {
            key: self.key,
            label: self.label.clone(),
        }
    }
}

impl<T: 'static> Key for LabeledGlobalKey<T> {
    fn id(&self) -> KeyId {
        self.key.id()
    }

    fn equals(&self, other: &dyn Key) -> bool {
        if let Some(other_labeled) = other.as_any().downcast_ref::<LabeledGlobalKey<T>>() {
            self.key.id() == other_labeled.key.id()
        } else {
            false
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A GlobalKey that uses object identity for equality.
///
/// Similar to Flutter's GlobalObjectKey.
#[derive(Debug)]
pub struct GlobalObjectKey<T: 'static> {
    id: KeyId,
    value: T,
}

impl<T: 'static> GlobalObjectKey<T> {
    /// Create a new global object key.
    pub fn new(value: T) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(2_000_000);
        Self {
            id: KeyId(COUNTER.fetch_add(1, Ordering::SeqCst)),
            value,
        }
    }

    /// Get the value.
    pub fn value(&self) -> &T {
        &self.value
    }
}

impl<T: 'static + std::fmt::Debug> Key for GlobalObjectKey<T> {
    fn id(&self) -> KeyId {
        self.id
    }

    fn equals(&self, other: &dyn Key) -> bool {
        if let Some(other_obj) = other.as_any().downcast_ref::<GlobalObjectKey<T>>() {
            // Use pointer identity for objects
            std::ptr::eq(&self.value, &other_obj.value)
        } else {
            false
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

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

/// A key that uses object identity for matching.
///
/// Similar to Flutter's ObjectKey. Uses pointer equality for comparison.
#[derive(Debug)]
pub struct ObjectKey<T: 'static> {
    value: &'static T,
    id: KeyId,
}

impl<T: 'static> ObjectKey<T> {
    /// Create a new object key.
    ///
    /// # Safety
    ///
    /// The referenced object must have a 'static lifetime.
    pub fn new(value: &'static T) -> Self {
        let ptr_value = value as *const T as u64;
        Self {
            value,
            id: KeyId(ptr_value),
        }
    }
}

impl<T: 'static + std::fmt::Debug> Key for ObjectKey<T> {
    fn id(&self) -> KeyId {
        self.id
    }

    fn equals(&self, other: &dyn Key) -> bool {
        if let Some(other_obj) = other.as_any().downcast_ref::<ObjectKey<T>>() {
            std::ptr::eq(self.value, other_obj.value)
        } else {
            false
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl<T: 'static + std::fmt::Debug> LocalKey for ObjectKey<T> {}

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

    #[test]
    fn test_global_key() {
        let key1 = GlobalKey::<()>::new();
        let key2 = GlobalKey::<()>::new();

        // Different global keys should have different IDs
        assert_ne!(key1.id(), key2.id());

        // Clone should have same ID
        let key1_clone = key1;
        assert_eq!(key1.id(), key1_clone.id());

        // Global key should equal itself
        assert!(key1.equals(&key1 as &dyn Key));

        // Different global keys should not equal
        assert!(!key1.equals(&key2 as &dyn Key));
    }

    #[test]
    fn test_labeled_global_key() {
        let key1 = LabeledGlobalKey::<()>::new("my_widget");
        let key2 = LabeledGlobalKey::<()>::new("other_widget");

        assert_eq!(key1.label(), "my_widget");
        assert_eq!(key2.label(), "other_widget");

        // Different keys should not equal
        assert!(!key1.equals(&key2 as &dyn Key));

        // Clone should preserve label
        let key1_clone = key1.clone();
        assert_eq!(key1_clone.label(), "my_widget");
        assert!(key1.equals(&key1_clone as &dyn Key));
    }

    #[test]
    fn test_object_key() {
        static OBJ1: i32 = 42;
        static OBJ2: i32 = 100;

        let key1 = ObjectKey::new(&OBJ1);
        let key2 = ObjectKey::new(&OBJ1);
        let key3 = ObjectKey::new(&OBJ2);

        // Same object should produce equal keys
        assert!(key1.equals(&key2 as &dyn Key));

        // Different objects should not equal
        assert!(!key1.equals(&key3 as &dyn Key));
    }

    #[test]
    fn test_global_object_key() {
        let key1 = GlobalObjectKey::new("test");
        let key2 = GlobalObjectKey::new("test");

        // Global object keys use ID, not value comparison
        assert_ne!(key1.id(), key2.id());
        assert!(!key1.equals(&key2 as &dyn Key));

        assert_eq!(*key1.value(), "test");
    }

    #[test]
    fn test_global_key_raw_id() {
        let key = GlobalKey::<()>::new();
        let raw_id = key.raw_id();

        // Raw ID should match the KeyId
        assert_eq!(raw_id, key.id().hash());

        // Raw ID should be in the global key range
        assert!(raw_id >= 1_000_000);
    }
}
