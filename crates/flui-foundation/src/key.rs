//! View keys for element identity and reconciliation
//!
// Allow unsafe code in this module - it's required for NonZeroU64::new_unchecked
// and unsafe Send/Sync implementations for ObjectKey.
#![allow(unsafe_code)]
//!
//! This module provides key types for view identity tracking:
//!
//! # Simple Keys (Copy, lightweight)
//!
//! - [`Key`] - Simple `u64` identifier with niche optimization
//!   - Compile-time constants via FNV-1a hash: `Key::from_str("name")`
//!   - Runtime unique generation: `Key::new()`
//!   - From external ID: `Key::from_u64(id)`
//!
//! # Flutter-Style Keys (for reconciliation)
//!
//! - [`ViewKey`] - Trait for all reconciliation keys
//! - [`ValueKey<T>`] - Key by value (string, number, struct)
//! - [`ObjectKey`] - Key by object identity (pointer equality)
//! - [`UniqueKey`] - Guaranteed unique, never matches another
//! - [`GlobalKey<T>`] - Access element from anywhere in tree
//!
//! # When to Use Which
//!
//! | Key Type | Use Case |
//! |----------|----------|
//! | `Key` | Simple ID, compile-time constants, database IDs |
//! | `ValueKey<T>` | Match by value (list items with unique field) |
//! | `ObjectKey` | Match by object instance |
//! | `UniqueKey` | Force new element on every rebuild |
//! | `GlobalKey<T>` | Access widget state from outside tree |
//!
//! # Examples
//!
//! ```rust,ignore
//! use flui_foundation::{Key, ValueKey, UniqueKey, WithKey};
//!
//! // Simple compile-time key
//! const HEADER: Key = Key::from_str("header");
//!
//! // Value-based key for list items
//! items.iter().map(|item| {
//!     TodoItem::new(item).with_view_key(ValueKey::new(item.id))
//! })
//!
//! // Unique key to force rebuild
//! AnimatedWidget::new().with_view_key(UniqueKey::new())
//! ```

use std::any::{Any, TypeId};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::ElementId;

/// View key with niche optimization
///
/// Thanks to `NonZeroU64`, `Option<Key>` is only 8 bytes instead of 16.
/// This saves memory and improves cache locality when storing many views.
///
/// # Memory Layout
///
/// ```text
/// Option<u64>:      [8 bytes data] + [8 bytes discriminant] = 16 bytes
/// Option<Key>:      [8 bytes NonZeroU64] = 8 bytes (0 means None)
/// ```
///
/// # Creation Methods
///
/// 1. **Compile-time constant** - `Key::from_str("name")`
/// 2. **Runtime unique** - `Key::new()`
/// 3. **Explicit ID** - `Key::from_u64(id)`
///
/// # Performance
///
/// - Key comparison: ~1ns (u64 compare)
/// - Hash computation: O(1) (already hashed)
/// - Creation: 0ns (compile-time) or ~5ns (runtime counter)
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
#[must_use = "keys should be used for widget identification"]
pub struct Key(NonZeroU64);

impl Key {
    /// Create compile-time constant key from string
    ///
    /// Uses FNV-1a hash algorithm which is const-evaluatable.
    /// The hash is computed at compile time with zero runtime cost.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::Key;
    ///
    /// const HEADER: Key = Key::from_str("app_header");
    /// const FOOTER: Key = Key::from_str("app_footer");
    ///
    /// // These are compile-time constants:
    /// assert_eq!(HEADER, Key::from_str("app_header"));
    /// ```
    ///
    /// # Panics
    ///
    /// Never panics - if hash is 0, uses 1 instead.
    #[inline]
    pub const fn from_str(s: &str) -> Self {
        let hash = const_fnv1a_hash(s.as_bytes());
        // Ensure non-zero (use 1 if hash is 0, which is extremely rare)
        let non_zero = if hash == 0 { 1 } else { hash };
        // SAFETY: We just ensured non_zero != 0
        Self(unsafe { NonZeroU64::new_unchecked(non_zero) })
    }

    /// Generate unique runtime key
    ///
    /// Uses thread-safe atomic counter for guaranteed uniqueness.
    /// Each call returns a new unique key.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::Key;
    ///
    /// let key1 = Key::new();
    /// let key2 = Key::new();
    /// assert_ne!(key1, key2);
    /// ```
    ///
    /// # Thread Safety
    ///
    /// This method is thread-safe and lock-free.
    /// Uses `Ordering::Relaxed` for maximum performance.
    ///
    /// # Panics
    ///
    /// Panics if `u64::MAX` keys have been created (practically impossible).
    /// This prevents undefined behavior from `NonZeroU64::new_unchecked(0)` after overflow.
    #[inline]
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);

        // Always check for overflow, even in release mode
        // UB is never acceptable, even in "impossible" cases
        assert!(
            id != u64::MAX,
            "Key counter overflow! Created {} keys. \
             This should never happen in practice, but UB is never acceptable.",
            u64::MAX
        );

        // SAFETY: We just verified id != u64::MAX, and counter starts at 1
        Self(unsafe { NonZeroU64::new_unchecked(id) })
    }

    /// Create key from existing u64 ID
    ///
    /// Returns `None` if `n` is 0 (invalid for `NonZeroU64`).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::Key;
    ///
    /// // From database ID
    /// let user_id = 42u64;
    /// let key = Key::from_u64(user_id).expect("Invalid ID");
    ///
    /// // Check for 0
    /// assert_eq!(Key::from_u64(0), None);
    /// ```
    #[inline]
    #[must_use]
    pub const fn from_u64(n: u64) -> Option<Self> {
        match NonZeroU64::new(n) {
            Some(nz) => Some(Self(nz)),
            None => None,
        }
    }

    /// Convert key to raw u64
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::Key;
    ///
    /// let key = Key::from_u64(42).unwrap();
    /// assert_eq!(key.as_u64(), 42);
    /// ```
    #[inline]
    #[must_use]
    pub const fn as_u64(&self) -> u64 {
        self.0.get()
    }

    /// Get the inner `NonZeroU64`
    #[inline]
    #[allow(dead_code)]
    pub(crate) const fn inner(self) -> NonZeroU64 {
        self.0
    }
}

impl Default for Key {
    /// Default key is generated uniquely
    ///
    /// Same as calling `Key::new()`.
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Key({})", self.0)
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Hash for Key {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

/// Key reference for `DynView` trait
///
/// This is a lightweight wrapper around `Key` that can be used
/// in the object-safe `DynView` trait. It's essentially the same
/// as `Key` but semantically represents a reference.
#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyRef(Key);

impl KeyRef {
    /// Create from Key
    #[inline]
    #[must_use]
    pub const fn new(key: Key) -> Self {
        Self(key)
    }

    /// Convert to raw u64
    #[inline]
    #[must_use]
    pub const fn as_u64(&self) -> u64 {
        self.0.as_u64()
    }

    /// Get the underlying Key
    #[inline]
    pub const fn key(&self) -> Key {
        self.0
    }
}

impl From<Key> for KeyRef {
    #[inline]
    fn from(k: Key) -> Self {
        Self(k)
    }
}

impl fmt::Debug for KeyRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "KeyRef({})", self.0.as_u64())
    }
}

impl fmt::Display for KeyRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.as_u64())
    }
}

// ============================================================================
// VIEW KEY TRAIT
// ============================================================================

/// Trait for view keys used in reconciliation.
///
/// Keys control how the framework matches old and new views during rebuilds.
/// Two keys are equal if they have the same type and value.
///
/// # Implementations
///
/// - [`ValueKey<T>`] - Match by value
/// - [`ObjectKey`] - Match by object identity
/// - [`UniqueKey`] - Never matches (forces new element)
/// - [`GlobalKey<T>`] - Global access + matching
///
/// # Example
///
/// ```rust,ignore
/// use flui_foundation::{ViewKey, ValueKey};
///
/// let key1 = ValueKey::new(42);
/// let key2 = ValueKey::new(42);
/// assert!(key1.key_eq(&key2));
/// ```
pub trait ViewKey: Send + Sync + 'static {
    /// Get the key as a trait object for downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Check equality with another key.
    fn key_eq(&self, other: &dyn ViewKey) -> bool;

    /// Get a hash of this key for efficient lookup.
    fn key_hash(&self) -> u64;

    /// Clone the key into a boxed trait object.
    fn clone_key(&self) -> Box<dyn ViewKey>;

    /// Debug representation.
    ///
    /// # Errors
    ///
    /// Returns `fmt::Error` if writing to the formatter fails.
    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
}

impl fmt::Debug for dyn ViewKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.debug_fmt(f)
    }
}

impl Clone for Box<dyn ViewKey> {
    fn clone(&self) -> Self {
        self.clone_key()
    }
}

impl PartialEq for dyn ViewKey {
    fn eq(&self, other: &Self) -> bool {
        self.key_eq(other)
    }
}

impl Eq for dyn ViewKey {}

impl Hash for dyn ViewKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.key_hash());
    }
}

// ============================================================================
// VALUE KEY
// ============================================================================

/// A key based on a value.
///
/// Two `ValueKey<T>` are equal if their values are equal.
/// This is the most common type of key for list items.
///
/// # Example
///
/// ```rust
/// use flui_foundation::{ValueKey, ViewKey};
///
/// let key1 = ValueKey::new(42);
/// let key2 = ValueKey::new(42);
/// let key3 = ValueKey::new(99);
///
/// assert!(key1.key_eq(&key2));
/// assert!(!key1.key_eq(&key3));
/// ```
#[derive(Clone)]
pub struct ValueKey<T: Clone + Hash + Eq + Send + Sync + 'static> {
    value: T,
}

impl<T: Clone + Hash + Eq + Send + Sync + 'static> ValueKey<T> {
    /// Create a new value key.
    pub const fn new(value: T) -> Self {
        Self { value }
    }

    /// Get the value.
    pub const fn value(&self) -> &T {
        &self.value
    }
}

impl<T: Clone + Hash + Eq + Send + Sync + 'static> fmt::Debug for ValueKey<T>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ValueKey")
            .field("value", &self.value)
            .finish()
    }
}

impl<T: Clone + Hash + Eq + Send + Sync + fmt::Debug + 'static> ViewKey for ValueKey<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn key_eq(&self, other: &dyn ViewKey) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|other| self.value == other.value)
    }

    fn key_hash(&self) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        let mut hasher = DefaultHasher::new();
        TypeId::of::<T>().hash(&mut hasher);
        self.value.hash(&mut hasher);
        hasher.finish()
    }

    fn clone_key(&self) -> Box<dyn ViewKey> {
        Box::new(self.clone())
    }

    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ValueKey({:?})", self.value)
    }
}

// ============================================================================
// OBJECT KEY
// ============================================================================

/// A key based on object identity (pointer equality).
///
/// Two `ObjectKey` are equal only if they point to the same object.
/// Useful when you want to key by a specific instance.
///
/// # Example
///
/// ```rust
/// use flui_foundation::{ObjectKey, ViewKey};
/// use std::sync::Arc;
///
/// let obj1 = Arc::new(42);
/// let obj2 = Arc::new(42); // Same value, different object
///
/// let key1 = ObjectKey::new(Arc::clone(&obj1));
/// let key2 = ObjectKey::new(Arc::clone(&obj1)); // Same object
/// let key3 = ObjectKey::new(obj2); // Different object
///
/// assert!(key1.key_eq(&key2));  // Same object
/// assert!(!key1.key_eq(&key3)); // Different objects
/// ```
#[derive(Clone)]
pub struct ObjectKey {
    ptr: *const (),
    _holder: Arc<dyn Any + Send + Sync>,
}

// SAFETY: ObjectKey is Send + Sync because:
// 1. The raw pointer `ptr` is never dereferenced - it's only used for identity comparison
//    via `std::ptr::eq()` which compares addresses, not values
// 2. The `_holder` field is `Arc<dyn Any + Send + Sync>` which keeps the object alive
//    and is itself Send + Sync
// 3. The pointer value is derived from Arc::as_ptr() and remains valid as long as
//    the Arc exists, which is guaranteed by the struct's lifetime
unsafe impl Send for ObjectKey {}
unsafe impl Sync for ObjectKey {}

impl ObjectKey {
    /// Create a new object key from an Arc.
    pub fn new<T: Send + Sync + 'static>(object: Arc<T>) -> Self {
        let ptr = Arc::as_ptr(&object).cast::<()>();
        Self {
            ptr,
            _holder: object,
        }
    }
}

impl fmt::Debug for ObjectKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObjectKey")
            .field("ptr", &self.ptr)
            .finish_non_exhaustive()
    }
}

impl ViewKey for ObjectKey {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn key_eq(&self, other: &dyn ViewKey) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|other| std::ptr::eq(self.ptr, other.ptr))
    }

    fn key_hash(&self) -> u64 {
        self.ptr as u64
    }

    fn clone_key(&self) -> Box<dyn ViewKey> {
        Box::new(self.clone())
    }

    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ObjectKey({:p})", self.ptr)
    }
}

// ============================================================================
// UNIQUE KEY
// ============================================================================

/// A key that is guaranteed to be unique.
///
/// Each `UniqueKey` instance is different from all other keys.
/// Useful when you need a key but don't have a natural identifier,
/// or when you want to force a new element on every rebuild.
///
/// # Example
///
/// ```rust
/// use flui_foundation::{UniqueKey, ViewKey};
///
/// let key1 = UniqueKey::new();
/// let key2 = UniqueKey::new();
///
/// assert!(!key1.key_eq(&key2)); // Always different
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct UniqueKey {
    id: u64,
}

impl UniqueKey {
    /// Create a new unique key.
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self { id }
    }

    /// Get the unique ID.
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }
}

impl Default for UniqueKey {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for UniqueKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UniqueKey").field("id", &self.id).finish()
    }
}

impl ViewKey for UniqueKey {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn key_eq(&self, other: &dyn ViewKey) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|other| self.id == other.id)
    }

    fn key_hash(&self) -> u64 {
        self.id
    }

    fn clone_key(&self) -> Box<dyn ViewKey> {
        Box::new(*self)
    }

    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "UniqueKey({})", self.id)
    }
}

// ============================================================================
// GLOBAL KEY
// ============================================================================

/// A key that provides access to the element from anywhere.
///
/// Unlike regular keys which only affect reconciliation, `GlobalKey`
/// also allows you to access the element's state from outside the tree.
///
/// # Use Cases
///
/// - Access state of a widget from a parent or sibling
/// - Trigger methods on a widget programmatically
/// - Get the render object for measurements/positioning
///
/// # Example
///
/// ```rust,ignore
/// use flui_foundation::GlobalKey;
///
/// // Create a global key
/// let form_key = GlobalKey::<FormState>::new();
///
/// // Use in widget tree
/// Form::new().with_view_key(form_key.clone())
///
/// // Access from anywhere
/// if let Some(state) = form_key.current_state() {
///     state.validate();
/// }
/// ```
///
/// # Performance Note
///
/// Global keys have overhead compared to local keys because they
/// maintain a registry. Use sparingly.
#[derive(Clone)]
pub struct GlobalKey<T: 'static> {
    id: u64,
    _marker: PhantomData<fn() -> T>,
}

impl<T: 'static> GlobalKey<T> {
    /// Create a new global key.
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self {
            id,
            _marker: PhantomData,
        }
    }

    /// Get the unique ID of this key.
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }

    /// Get the current element ID associated with this key.
    ///
    /// Returns `None` if the element is not currently mounted.
    #[must_use]
    pub const fn current_element(&self) -> Option<ElementId> {
        // TODO: Implement via GlobalKeyRegistry
        None
    }

    /// Get the current state associated with this key.
    ///
    /// Only works for `StatefulView` widgets.
    #[must_use]
    pub const fn current_state(&self) -> Option<Arc<T>>
    where
        T: Send + Sync,
    {
        // TODO: Implement via GlobalKeyRegistry
        None
    }
}

impl<T: 'static> Default for GlobalKey<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: 'static> fmt::Debug for GlobalKey<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GlobalKey")
            .field("id", &self.id)
            .field("type", &std::any::type_name::<T>())
            .finish()
    }
}

impl<T: 'static> PartialEq for GlobalKey<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T: 'static> Eq for GlobalKey<T> {}

impl<T: 'static> Hash for GlobalKey<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl<T: 'static> ViewKey for GlobalKey<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn key_eq(&self, other: &dyn ViewKey) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|other| self.id == other.id)
    }

    fn key_hash(&self) -> u64 {
        self.id
    }

    fn clone_key(&self) -> Box<dyn ViewKey> {
        Box::new(Self {
            id: self.id,
            _marker: PhantomData,
        })
    }

    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "GlobalKey<{}>({})", std::any::type_name::<T>(), self.id)
    }
}

// ============================================================================
// KEYED WRAPPER
// ============================================================================

/// A view with an associated key.
///
/// Created by calling `.with_view_key()` on a view.
#[derive(Debug)]
pub struct Keyed<V> {
    /// The wrapped view.
    pub view: V,
    /// The key.
    pub key: Box<dyn ViewKey>,
}

impl<V> Keyed<V> {
    /// Create a new keyed view.
    pub fn new(view: V, key: impl ViewKey) -> Self {
        Self {
            view,
            key: Box::new(key),
        }
    }

    /// Get the key.
    pub fn key(&self) -> &dyn ViewKey {
        &*self.key
    }

    /// Unwrap into the inner view.
    pub fn into_inner(self) -> V {
        self.view
    }
}

// ============================================================================
// WITH KEY TRAIT
// ============================================================================

/// Extension trait to add a key to any view.
pub trait WithKey: Sized {
    /// Attach a key to this view.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let view = MyView::new().with_view_key(ValueKey::new(item.id));
    /// ```
    fn with_view_key(self, key: impl ViewKey) -> Keyed<Self> {
        Keyed::new(self, key)
    }

    /// Attach a value key to this view (convenience).
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let view = MyView::new().with_value_key(item.id);
    /// ```
    fn with_value_key<T>(self, value: T) -> Keyed<Self>
    where
        T: Clone + Hash + Eq + Send + Sync + fmt::Debug + 'static,
    {
        Keyed::new(self, ValueKey::new(value))
    }

    /// Attach a unique key to this view.
    fn with_unique_key(self) -> Keyed<Self> {
        Keyed::new(self, UniqueKey::new())
    }
}

// Blanket implementation for all types
impl<T> WithKey for T {}

// ============================================================================
// SERDE SUPPORT
// ============================================================================

#[cfg(feature = "serde")]
impl serde::Serialize for Key {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u64(self.as_u64())
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Key {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let id = u64::deserialize(deserializer)?;
        Self::from_u64(id).ok_or_else(|| {
            serde::de::Error::custom("Key cannot be zero (uses NonZeroU64 internally)")
        })
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for KeyRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.key().serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for KeyRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let key = Key::deserialize(deserializer)?;
        Ok(Self::new(key))
    }
}

// ============================================================================
// FNV-1A HASH
// ============================================================================

/// FNV-1a hash for compile-time evaluation
///
/// This is a simple, fast hash function that can be evaluated at compile time.
/// Used for creating constant keys from string literals.
///
/// # Algorithm
///
/// FNV-1a (Fowler-Noll-Vo) is a non-cryptographic hash function:
/// - Fast and simple
/// - Good distribution
/// - Const-evaluatable in Rust
///
/// # References
///
/// - <http://www.isthe.com/chongo/tech/comp/fnv/>
const fn const_fnv1a_hash(bytes: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 14_695_981_039_346_656_037;
    const FNV_PRIME: u64 = 1_099_511_628_211;

    let mut hash = FNV_OFFSET;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::mem::size_of;

    #[test]
    fn test_niche_optimization() {
        // Key uses niche optimization
        assert_eq!(size_of::<Option<Key>>(), size_of::<Key>());
        assert_eq!(size_of::<Option<Key>>(), 8);

        // Regular u64 doesn't
        assert_eq!(size_of::<Option<u64>>(), 16);
    }

    #[test]
    fn test_compile_time_keys() {
        const K1: Key = Key::from_str("test");
        const K2: Key = Key::from_str("test");
        const K3: Key = Key::from_str("other");

        // Same string = same key
        assert_eq!(K1, K2);

        // Different string = different key
        assert_ne!(K1, K3);

        // Runtime matches compile-time
        assert_eq!(K1, Key::from_str("test"));
    }

    #[test]
    fn test_runtime_keys() {
        let k1 = Key::new();
        let k2 = Key::new();
        let k3 = Key::new();

        // All unique
        assert_ne!(k1, k2);
        assert_ne!(k2, k3);
        assert_ne!(k1, k3);
    }

    #[test]
    fn test_explicit_keys() {
        assert_eq!(Key::from_u64(0), None);
        assert!(Key::from_u64(1).is_some());
        assert!(Key::from_u64(u64::MAX).is_some());

        let key = Key::from_u64(42).unwrap();
        assert_eq!(key.as_u64(), 42);
    }

    #[test]
    fn test_key_ref() {
        let key = Key::new();
        let key_ref = KeyRef::from(key);

        assert_eq!(key_ref.as_u64(), key.as_u64());
        assert_eq!(key_ref.key(), key);
    }

    #[test]
    fn test_hash_consistency() {
        let key = Key::new();
        let mut set = HashSet::new();

        set.insert(key);
        assert!(set.contains(&key));

        // Same key hashes the same
        let key_copy = key;
        assert!(set.contains(&key_copy));
    }

    #[test]
    fn test_default() {
        let k1 = Key::default();
        let k2 = Key::default();

        // Default creates unique keys
        assert_ne!(k1, k2);
    }

    #[test]
    fn test_debug_display() {
        let key = Key::from_u64(42).unwrap();

        assert_eq!(format!("{:?}", key), "Key(42)");
        assert_eq!(format!("{}", key), "42");

        let key_ref = KeyRef::from(key);
        assert_eq!(format!("{:?}", key_ref), "KeyRef(42)");
        assert_eq!(format!("{}", key_ref), "42");
    }

    #[test]
    fn test_fnv1a_hash() {
        // Known FNV-1a hash values
        const EMPTY: u64 = const_fnv1a_hash(b"");
        const HELLO: u64 = const_fnv1a_hash(b"hello");

        assert_ne!(EMPTY, HELLO);
        assert_ne!(EMPTY, 0); // Empty string should not hash to 0
    }

    #[test]
    fn test_thread_safety() {
        use std::thread;

        // Generate keys in parallel
        let handles: Vec<_> = (0..10)
            .map(|_| {
                thread::spawn(|| {
                    let keys: Vec<_> = (0..100).map(|_| Key::new()).collect();
                    keys
                })
            })
            .collect();

        let mut all_keys = Vec::new();
        for handle in handles {
            all_keys.extend(handle.join().unwrap());
        }

        // All keys should be unique
        let unique: HashSet<_> = all_keys.iter().collect();
        assert_eq!(unique.len(), all_keys.len());
    }

    #[test]
    fn test_const_evaluation() {
        // This compiles if Key::from_str is truly const
        const _: Key = Key::from_str("compile_time_test");
        const KEYS: [Key; 3] = [
            Key::from_str("one"),
            Key::from_str("two"),
            Key::from_str("three"),
        ];

        assert_eq!(KEYS[0], Key::from_str("one"));
        assert_eq!(KEYS[1], Key::from_str("two"));
        assert_eq!(KEYS[2], Key::from_str("three"));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_key_serde_roundtrip() {
        let key = Key::new();
        let json = serde_json::to_string(&key).unwrap();

        let deserialized: Key = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.as_u64(), key.as_u64());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_key_serde_zero_rejection() {
        let json = "0";
        let result: Result<Key, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn test_key_ref_serde_roundtrip() {
        let key = Key::new();
        let key_ref = KeyRef::from(key);
        let json = serde_json::to_string(&key_ref).unwrap();

        let deserialized: KeyRef = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.as_u64(), key_ref.as_u64());
    }
}
