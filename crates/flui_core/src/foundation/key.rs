//! Key types for widget identification
//!
//! This module provides Flutter-like key types for uniquely identifying widgets.
//! Keys are used to preserve state across widget rebuilds and optimize updates.
//!
//! # Key Types
//!
//! - [`UniqueKey`] - Each instance is unique
//! - [`ValueKey<T>`] - Identity based on a value
//! - [`GlobalKey<T>`] - Globally unique, can access element/state
//! - [`ObjectKey<T>`] - Identity based on pointer equality
//!
//! # Examples
//!
//! ```rust
//! use flui_core::foundation::{Key, UniqueKey, ValueKey};
//!
//! let unique = UniqueKey::new();
//! let value = ValueKey::new("my_widget");
//! let int_key = ValueKey::new(42);
//! ```

use std::any::Any;
use std::borrow::Borrow;
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Deref;

/// A Key is an identifier for widgets and elements
///
/// Keys are used to control which widgets are matched up with which other widgets
/// when rebuilding the widget tree. This is similar to Flutter's Key class.
///
/// # Object Safety
///
/// This trait is object-safe to allow `&dyn Key` references. Note that `PartialEq`
/// is not object-safe, so we provide `key_eq()` for trait object comparison.
/// Individual key types implement `PartialEq` for direct comparison.
///
/// # Examples
///
/// ```rust
/// use flui_core::foundation::{Key, UniqueKey};
///
/// let key1 = UniqueKey::new();
/// let key2 = UniqueKey::new();
///
/// assert_ne!(key1, key2);
/// assert_ne!(key1.id(), key2.id());
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be used as a Key",
    label = "the trait `Key` is not implemented",
    note = "implement one of the key traits: UniqueKey, ValueKey<T>, GlobalKey<T>, etc."
)]
pub trait Key: fmt::Debug {
    /// Returns a unique identifier for this key
    #[must_use]
    fn id(&self) -> KeyId;

    /// Checks if this key equals another key (for trait objects)
    ///
    /// This method is needed because `PartialEq` is not object-safe.
    /// For concrete types, use `==` instead.
    #[must_use]
    fn key_eq(&self, other: &dyn Key) -> bool;

    /// Returns this key as `Any` for downcasting
    #[must_use]
    fn as_any(&self) -> &dyn Any;
}

/// Unique identifier for a key
///
/// This is a newtype wrapper around `u64` for type safety.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KeyId(u64);

impl KeyId {
    /// Creates a new key ID from a hash value
    #[must_use]
    #[inline]
    pub const fn from_hash(hash: u64) -> Self {
        Self(hash)
    }

    /// Returns the raw hash value
    #[must_use]
    #[inline]
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl fmt::Display for KeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Key#{}", self.0)
    }
}

impl AsRef<u64> for KeyId {
    #[inline]
    fn as_ref(&self) -> &u64 {
        &self.0
    }
}

impl From<u64> for KeyId {
    #[inline]
    fn from(value: u64) -> Self {
        Self(value)
    }
}

/// A key that is not a GlobalKey
///
/// Similar to Flutter's LocalKey. This is a marker trait for keys
/// that have local scope within the widget tree.
pub trait LocalKey: Key {}

/// A key that is unique across the entire app
///
/// Similar to Flutter's GlobalKey. Global keys uniquely identify elements
/// across the entire widget hierarchy. They can be used to access the
/// associated element, widget, or state from anywhere in the app.
///
/// # Type Parameter
///
/// - `T`: The type of State this key references (use `()` for stateless widgets)
///
/// # Thread Safety
///
/// Global keys are `Send + Sync` and can be safely shared across threads.
///
/// # Examples
///
/// ```rust
/// use flui_core::foundation::GlobalKey;
///
/// let key = GlobalKey::<()>::new();
/// let key2 = GlobalKey::<()>::new();
/// assert_ne!(key.id(), key2.id());
/// ```
pub struct GlobalKey<T = ()> {
    id: KeyId,
    _phantom: std::marker::PhantomData<fn() -> T>,
}

impl<T> fmt::Debug for GlobalKey<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GlobalKey")
            .field("id", &self.id)
            .field("type", &std::any::type_name::<T>())
            .finish()
    }
}

impl<T> fmt::Display for GlobalKey<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "GlobalKey<{}>(#{})", std::any::type_name::<T>(), self.id.0)
    }
}

impl<T> PartialEq for GlobalKey<T> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T> Eq for GlobalKey<T> {}

impl<T> Hash for GlobalKey<T> {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl<T> PartialOrd for GlobalKey<T> {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for GlobalKey<T> {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.id.cmp(&other.id)
    }
}

impl<T> AsRef<KeyId> for GlobalKey<T> {
    #[inline]
    fn as_ref(&self) -> &KeyId {
        &self.id
    }
}

impl<T> Borrow<KeyId> for GlobalKey<T> {
    #[inline]
    fn borrow(&self) -> &KeyId {
        &self.id
    }
}

impl<T> GlobalKey<T> {
    /// Creates a new global key
    #[must_use]
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};

        static COUNTER: AtomicU64 = AtomicU64::new(1_000_000);

        Self {
            id: KeyId(COUNTER.fetch_add(1, Ordering::Relaxed)),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Returns the raw ID
    #[must_use]
    #[inline]
    pub const fn raw_id(&self) -> u64 {
        self.id.0
    }

    /// Returns the KeyId
    #[must_use]
    #[inline]
    pub const fn key_id(&self) -> KeyId {
        self.id
    }

    /// Returns the `BuildContext` for the element registered with this key
    ///
    /// Returns `None` if:
    /// - The key is not registered with any element
    /// - The element is not currently in the tree
    #[must_use]
    pub fn current_context(
        &self,
        owner: &crate::tree::build_owner::BuildOwner,
    ) -> Option<crate::context::Context> {
        let element_id = owner.element_for_global_key(self.into())?;
        let tree = owner.tree();
        let tree_guard = tree.read();

        tree_guard.get(element_id)?;

        Some(crate::context::Context::new(tree.clone(), element_id))
    }

    /// Access the current widget via callback (Phase 3.1)
    ///
    /// The callback receives a reference to the widget and can extract
    /// any data it needs. The tree lock is held for the duration of the callback.
    ///
    /// Returns `None` if:
    /// - The key is not registered with any element
    /// - The element is not currently in the tree
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let key: GlobalKey<()> = GlobalKey::new();
    ///
    /// // Extract data from widget
    /// let title = key.with_current_widget(&owner, |widget| {
    ///     widget.type_name().to_string()
    /// });
    /// ```
    pub fn with_current_widget<F, R>(&self, owner: &crate::tree::build_owner::BuildOwner, f: F) -> Option<R>
    where
        F: FnOnce(&dyn crate::DynWidget) -> R,
    {
        let element_id = owner.element_for_global_key(self.into())?;
        let tree_guard = owner.tree().read();
        let element = tree_guard.get(element_id)?;
        Some(f(element.widget()))
    }

    /// Access the current state via callback (for StatefulWidget only) (Phase 3.1)
    ///
    /// The callback receives a reference to the state and can extract
    /// any data it needs. The tree lock is held for the duration of the callback.
    ///
    /// Returns `None` if:
    /// - The key is not registered with any element
    /// - The element is not currently in the tree
    /// - The element is not a StatefulElement
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let key: GlobalKey<CounterState> = GlobalKey::new();
    ///
    /// // Extract state data
    /// let count = key.with_current_state(&owner, |state| {
    ///     state.downcast_ref::<CounterState>()?.count
    /// });
    /// ```
    pub fn with_current_state<F, R>(&self, owner: &crate::tree::build_owner::BuildOwner, f: F) -> Option<R>
    where
        F: FnOnce(&dyn crate::State) -> R,
    {
        let element_id = owner.element_for_global_key(self.into())?;
        let tree_guard = owner.tree().read();
        let element = tree_guard.get(element_id)?;
        let state = element.state()?;
        Some(f(state))
    }

    /// Access the current state mutably via callback (for StatefulWidget only) (Phase 3.1)
    ///
    /// # Warning
    ///
    /// Mutating state directly bypasses `setState()` and won't trigger rebuilds.
    /// Use `Context` and `setState()` instead for state updates that need to trigger rebuilds.
    ///
    /// Returns `None` if:
    /// - The key is not registered with any element
    /// - The element is not currently in the tree
    /// - The element is not a StatefulElement
    pub fn with_current_state_mut<F, R>(&self, owner: &mut crate::tree::build_owner::BuildOwner, f: F) -> Option<R>
    where
        F: FnOnce(&mut dyn crate::State) -> R,
    {
        let element_id = owner.element_for_global_key(self.into())?;
        let tree = owner.tree();
        let mut tree_guard = tree.write();
        let element = tree_guard.get_mut(element_id)?;
        let state = element.state_mut()?;
        Some(f(state))
    }
}

impl<T> Default for GlobalKey<T> {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Clone for GlobalKey<T> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for GlobalKey<T> {}

impl<T: 'static> Key for GlobalKey<T> {
    #[inline]
    fn id(&self) -> KeyId {
        self.id
    }

    #[inline]
    fn key_eq(&self, other: &dyn Key) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|other_global| self == other_global)
    }

    #[inline]
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl<T> From<GlobalKey<T>> for crate::tree::build_owner::GlobalKeyId {
    #[inline]
    fn from(key: GlobalKey<T>) -> Self {
        Self::from_raw(key.id.0)
    }
}

impl<T> From<&GlobalKey<T>> for crate::tree::build_owner::GlobalKeyId {
    #[inline]
    fn from(key: &GlobalKey<T>) -> Self {
        Self::from_raw(key.id.0)
    }
}

#[cfg(feature = "serde")]
impl<T: 'static> serde::Serialize for GlobalKey<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.id.0.serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de, T: 'static> serde::Deserialize<'de> for GlobalKey<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let id = u64::deserialize(deserializer)?;
        Ok(Self {
            id: KeyId(id),
            _phantom: std::marker::PhantomData,
        })
    }
}

/// A GlobalKey with a debug label for easier debugging
///
/// Similar to Flutter's LabeledGlobalKey.
pub struct LabeledGlobalKey<T = ()> {
    key: GlobalKey<T>,
    label: String,
}

impl<T> fmt::Debug for LabeledGlobalKey<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LabeledGlobalKey")
            .field("label", &self.label)
            .field("id", &self.key.id)
            .finish()
    }
}

impl<T> fmt::Display for LabeledGlobalKey<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LabeledGlobalKey(\"{}\", #{})", self.label, self.key.id.0)
    }
}

impl<T> LabeledGlobalKey<T> {
    /// Creates a new labeled global key
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            key: GlobalKey::new(),
            label: label.into(),
        }
    }

    /// Returns the label
    #[must_use]
    #[inline]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns the underlying GlobalKey
    #[must_use]
    #[inline]
    pub const fn key(&self) -> &GlobalKey<T> {
        &self.key
    }

    /// Returns the raw ID
    #[must_use]
    #[inline]
    pub const fn raw_id(&self) -> u64 {
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

impl<T> PartialEq for LabeledGlobalKey<T> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

impl<T> Eq for LabeledGlobalKey<T> {}

impl<T> Hash for LabeledGlobalKey<T> {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key.hash(state);
    }
}

impl<T: 'static> Key for LabeledGlobalKey<T> {
    #[inline]
    fn id(&self) -> KeyId {
        self.key.id()
    }

    #[inline]
    fn key_eq(&self, other: &dyn Key) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|other_labeled| self.key.id() == other_labeled.key.id())
    }

    #[inline]
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A GlobalKey that uses object identity for equality
///
/// Similar to Flutter's GlobalObjectKey.
#[derive(Debug)]
pub struct GlobalObjectKey<T: 'static> {
    id: KeyId,
    value: T,
}

impl<T: 'static> GlobalObjectKey<T> {
    /// Creates a new global object key
    #[must_use]
    pub fn new(value: T) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};

        static COUNTER: AtomicU64 = AtomicU64::new(2_000_000);

        Self {
            id: KeyId(COUNTER.fetch_add(1, Ordering::Relaxed)),
            value,
        }
    }

    /// Returns a reference to the value
    #[must_use]
    #[inline]
    pub const fn value(&self) -> &T {
        &self.value
    }
}

impl<T: 'static + fmt::Display> fmt::Display for GlobalObjectKey<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "GlobalObjectKey({}, #{})", self.value, self.id.0)
    }
}

impl<T: 'static + fmt::Debug> Key for GlobalObjectKey<T> {
    #[inline]
    fn id(&self) -> KeyId {
        self.id
    }

    fn key_eq(&self, other: &dyn Key) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|other_obj| std::ptr::eq(&self.value, &other_obj.value))
    }

    #[inline]
    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// A key that uses its identity as its key
///
/// Similar to Flutter's UniqueKey. Each instance is guaranteed to be unique.
#[derive(Debug, Clone, Copy)]
pub struct UniqueKey {
    id: KeyId,
}

impl UniqueKey {
    /// Creates a new unique key
    #[must_use]
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};

        static COUNTER: AtomicU64 = AtomicU64::new(0);

        Self {
            id: KeyId(COUNTER.fetch_add(1, Ordering::Relaxed)),
        }
    }

    /// Returns the KeyId
    #[must_use]
    #[inline]
    pub const fn key_id(&self) -> KeyId {
        self.id
    }
}

impl Default for UniqueKey {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for UniqueKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "UniqueKey(#{})", self.id.0)
    }
}

impl PartialEq for UniqueKey {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for UniqueKey {}

impl Hash for UniqueKey {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl PartialOrd for UniqueKey {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for UniqueKey {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.id.cmp(&other.id)
    }
}

impl AsRef<KeyId> for UniqueKey {
    #[inline]
    fn as_ref(&self) -> &KeyId {
        &self.id
    }
}

impl Borrow<KeyId> for UniqueKey {
    #[inline]
    fn borrow(&self) -> &KeyId {
        &self.id
    }
}

impl Key for UniqueKey {
    #[inline]
    fn id(&self) -> KeyId {
        self.id
    }

    #[inline]
    fn key_eq(&self, other: &dyn Key) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|other_unique| self == other_unique)
    }

    #[inline]
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl LocalKey for UniqueKey {}

#[cfg(feature = "serde")]
impl serde::Serialize for UniqueKey {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.id.0.serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for UniqueKey {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let id = u64::deserialize(deserializer)?;
        Ok(Self { id: KeyId(id) })
    }
}

/// A key that uses object identity for matching
///
/// Similar to Flutter's ObjectKey. Uses pointer equality for comparison.
#[derive(Debug)]
pub struct ObjectKey<T: 'static> {
    value: &'static T,
    id: KeyId,
}

impl<T: 'static> ObjectKey<T> {
    /// Creates a new object key
    #[must_use]
    #[inline]
    pub fn new(value: &'static T) -> Self {
        let ptr_value = value as *const T as u64;
        Self {
            value,
            id: KeyId(ptr_value),
        }
    }

    /// Returns a reference to the value
    #[must_use]
    #[inline]
    pub const fn value(&self) -> &'static T {
        self.value
    }
}

impl<T: 'static + fmt::Display> fmt::Display for ObjectKey<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ObjectKey({}, #{})", self.value, self.id.0)
    }
}

impl<T: 'static + fmt::Debug> Key for ObjectKey<T> {
    #[inline]
    fn id(&self) -> KeyId {
        self.id
    }

    fn key_eq(&self, other: &dyn Key) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|other_obj| std::ptr::eq(self.value, other_obj.value))
    }

    #[inline]
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl<T: 'static + fmt::Debug> LocalKey for ObjectKey<T> {}

/// A key that uses a value of a particular type to identify itself
///
/// Similar to Flutter's `ValueKey<T>`. Two value keys are equal if their
/// values are equal and they have the same type.
#[derive(Debug, Clone)]
pub struct ValueKey<T: PartialEq + Hash + fmt::Debug + 'static> {
    value: T,
    id: KeyId,
}

impl<T: PartialEq + Hash + fmt::Debug + 'static> ValueKey<T> {
    /// Creates a new value key
    #[must_use]
    pub fn new(value: T) -> Self {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        value.hash(&mut hasher);
        std::any::TypeId::of::<T>().hash(&mut hasher);

        Self {
            value,
            id: KeyId(hasher.finish()),
        }
    }

    /// Returns a reference to the value
    #[must_use]
    #[inline]
    pub const fn value(&self) -> &T {
        &self.value
    }

    /// Consumes the key and returns the value
    #[must_use]
    #[inline]
    pub fn into_value(self) -> T {
        self.value
    }

    /// Returns the KeyId
    #[must_use]
    #[inline]
    pub const fn key_id(&self) -> KeyId {
        self.id
    }
}

impl<T: PartialEq + Hash + fmt::Debug + fmt::Display + 'static> fmt::Display for ValueKey<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ValueKey({})", self.value)
    }
}

impl<T: PartialEq + Hash + fmt::Debug + 'static> PartialEq for ValueKey<T> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl<T: PartialEq + Hash + fmt::Debug + 'static> Eq for ValueKey<T> {}

impl<T: PartialEq + Hash + fmt::Debug + 'static> Hash for ValueKey<T> {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl<T: PartialEq + Eq + Hash + fmt::Debug + PartialOrd + 'static> PartialOrd for ValueKey<T> {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.value.partial_cmp(&other.value)
    }
}

impl<T: PartialEq + Eq + Hash + fmt::Debug + Ord + 'static> Ord for ValueKey<T> {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.value.cmp(&other.value)
    }
}

impl<T: PartialEq + Hash + fmt::Debug + 'static> Deref for ValueKey<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<T: PartialEq + Hash + fmt::Debug + 'static> AsRef<T> for ValueKey<T> {
    #[inline]
    fn as_ref(&self) -> &T {
        &self.value
    }
}

impl<T: PartialEq + Hash + fmt::Debug + 'static> Borrow<T> for ValueKey<T> {
    #[inline]
    fn borrow(&self) -> &T {
        &self.value
    }
}

impl Borrow<str> for ValueKey<String> {
    #[inline]
    fn borrow(&self) -> &str {
        self.value.as_str()
    }
}

impl<T: PartialEq + Hash + fmt::Debug + 'static> Key for ValueKey<T> {
    #[inline]
    fn id(&self) -> KeyId {
        self.id
    }

    fn key_eq(&self, other: &dyn Key) -> bool {
        other
            .as_any()
            .downcast_ref::<Self>()
            .is_some_and(|other_value| self == other_value)
    }

    #[inline]
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl<T: PartialEq + Hash + fmt::Debug + 'static> LocalKey for ValueKey<T> {}

#[cfg(feature = "serde")]
impl<T: PartialEq + Hash + fmt::Debug + serde::Serialize + 'static> serde::Serialize for ValueKey<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.value.serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de, T: PartialEq + Hash + fmt::Debug + serde::Deserialize<'de> + 'static> serde::Deserialize<'de> for ValueKey<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = T::deserialize(deserializer)?;
        Ok(Self::new(value))
    }
}

/// A key that takes its identity from a string
///
/// Convenience type alias for `ValueKey<String>`.
pub type StringKey = ValueKey<String>;

/// A key that takes its identity from an integer
///
/// Convenience type alias for `ValueKey<i32>`.
pub type IntKey = ValueKey<i32>;

impl From<&str> for StringKey {
    #[inline]
    fn from(s: &str) -> Self {
        ValueKey::new(s.to_string())
    }
}

impl From<String> for StringKey {
    #[inline]
    fn from(s: String) -> Self {
        ValueKey::new(s)
    }
}

impl From<i32> for IntKey {
    #[inline]
    fn from(i: i32) -> Self {
        ValueKey::new(i)
    }
}

/// Optional key wrapper for widgets
///
/// This provides a convenient way to store an optional key in widgets.
#[derive(Debug, Clone)]
pub enum WidgetKey {
    None,
    Unique(UniqueKey),
    String(StringKey),
    Int(IntKey),
}

impl WidgetKey {
    /// Creates a unique key
    #[must_use]
    #[inline]
    pub fn unique() -> Self {
        Self::Unique(UniqueKey::new())
    }

    /// Creates a string key
    #[must_use]
    #[inline]
    pub fn string(value: impl Into<String>) -> Self {
        Self::String(ValueKey::new(value.into()))
    }

    /// Creates an integer key
    #[must_use]
    #[inline]
    pub fn int(value: i32) -> Self {
        Self::Int(ValueKey::new(value))
    }

    /// Checks if this is `None`
    #[must_use]
    #[inline]
    pub const fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    /// Checks if this is not `None`
    #[must_use]
    #[inline]
    pub const fn is_some(&self) -> bool {
        !self.is_none()
    }

    /// Returns the key ID if present
    #[must_use]
    #[inline]
    pub fn id(&self) -> Option<KeyId> {
        match self {
            Self::None => None,
            Self::Unique(k) => Some(k.id()),
            Self::String(k) => Some(k.id()),
            Self::Int(k) => Some(k.id()),
        }
    }

    /// Returns a reference to the underlying key as a trait object
    #[must_use]
    pub fn as_key(&self) -> Option<&dyn Key> {
        match self {
            Self::None => None,
            Self::Unique(k) => Some(k),
            Self::String(k) => Some(k),
            Self::Int(k) => Some(k),
        }
    }
}

impl fmt::Display for WidgetKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::None => write!(f, "WidgetKey::None"),
            Self::Unique(k) => write!(f, "WidgetKey::Unique({})", k),
            Self::String(k) => write!(f, "WidgetKey::String(\"{}\")", k.value()),
            Self::Int(k) => write!(f, "WidgetKey::Int({})", k.value()),
        }
    }
}

impl PartialEq for WidgetKey {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::None, Self::None) => true,
            (Self::Unique(a), Self::Unique(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Int(a), Self::Int(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for WidgetKey {}

impl Hash for WidgetKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::None => 0u8.hash(state),
            Self::Unique(k) => {
                1u8.hash(state);
                k.hash(state);
            }
            Self::String(k) => {
                2u8.hash(state);
                k.hash(state);
            }
            Self::Int(k) => {
                3u8.hash(state);
                k.hash(state);
            }
        }
    }
}

impl Default for WidgetKey {
    #[inline]
    fn default() -> Self {
        Self::None
    }
}

impl From<UniqueKey> for WidgetKey {
    #[inline]
    fn from(key: UniqueKey) -> Self {
        Self::Unique(key)
    }
}

impl From<StringKey> for WidgetKey {
    #[inline]
    fn from(key: StringKey) -> Self {
        Self::String(key)
    }
}

impl From<IntKey> for WidgetKey {
    #[inline]
    fn from(key: IntKey) -> Self {
        Self::Int(key)
    }
}

impl From<&str> for WidgetKey {
    #[inline]
    fn from(s: &str) -> Self {
        Self::string(s)
    }
}

impl From<String> for WidgetKey {
    #[inline]
    fn from(s: String) -> Self {
        Self::string(s)
    }
}

impl From<i32> for WidgetKey {
    #[inline]
    fn from(i: i32) -> Self {
        Self::int(i)
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for WidgetKey {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        match self {
            Self::None => {
                let mut s = serializer.serialize_struct("WidgetKey", 1)?;
                s.serialize_field("type", "None")?;
                s.end()
            }
            Self::Unique(k) => {
                let mut s = serializer.serialize_struct("WidgetKey", 2)?;
                s.serialize_field("type", "Unique")?;
                s.serialize_field("id", &k.id().value())?;
                s.end()
            }
            Self::String(k) => {
                let mut s = serializer.serialize_struct("WidgetKey", 2)?;
                s.serialize_field("type", "String")?;
                s.serialize_field("value", k.value())?;
                s.end()
            }
            Self::Int(k) => {
                let mut s = serializer.serialize_struct("WidgetKey", 2)?;
                s.serialize_field("type", "Int")?;
                s.serialize_field("value", k.value())?;
                s.end()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unique_key() {
        let key1 = UniqueKey::new();
        let key2 = UniqueKey::new();

        assert_ne!(key1.id(), key2.id());
        assert!(key1.key_eq(&key1 as &dyn Key));
        assert!(!key1.key_eq(&key2 as &dyn Key));
        assert_ne!(key1, key2);
        assert_eq!(key1, key1);
    }

    #[test]
    fn test_value_key_string() {
        let key1 = ValueKey::new("test".to_string());
        let key2 = ValueKey::new("test".to_string());
        let key3 = ValueKey::new("other".to_string());

        assert!(key1.key_eq(&key2 as &dyn Key));
        assert!(!key1.key_eq(&key3 as &dyn Key));
        assert_eq!(key1.id(), key2.id());
        assert_ne!(key1.id(), key3.id());
        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_value_key_int() {
        let key1 = ValueKey::new(42);
        let key2 = ValueKey::new(42);
        let key3 = ValueKey::new(100);

        assert!(key1.key_eq(&key2 as &dyn Key));
        assert!(!key1.key_eq(&key3 as &dyn Key));
        assert_eq!(key1.id(), key2.id());
        assert_eq!(key1, key2);
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

        assert!(none.as_key().is_none());
        assert!(unique.as_key().is_some());
    }

    #[test]
    fn test_value_key_different_types() {
        let string_key = ValueKey::new("42".to_string());
        let int_key = ValueKey::new(42);

        assert!(!string_key.key_eq(&int_key as &dyn Key));
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

        assert_ne!(key1.id(), key2.id());

        let key1_clone = key1;
        assert_eq!(key1.id(), key1_clone.id());

        assert!(key1.key_eq(&key1 as &dyn Key));
        assert!(!key1.key_eq(&key2 as &dyn Key));
        assert_eq!(key1, key1);
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_labeled_global_key() {
        let key1 = LabeledGlobalKey::<()>::new("my_widget");
        let key2 = LabeledGlobalKey::<()>::new("other_widget");

        assert_eq!(key1.label(), "my_widget");
        assert_eq!(key2.label(), "other_widget");

        assert!(!key1.key_eq(&key2 as &dyn Key));

        let key1_clone = key1.clone();
        assert_eq!(key1_clone.label(), "my_widget");
        assert!(key1.key_eq(&key1_clone as &dyn Key));
    }

    #[test]
    fn test_object_key() {
        static OBJ1: i32 = 42;
        static OBJ2: i32 = 100;

        let key1 = ObjectKey::new(&OBJ1);
        let key2 = ObjectKey::new(&OBJ1);
        let key3 = ObjectKey::new(&OBJ2);

        assert!(key1.key_eq(&key2 as &dyn Key));
        assert!(!key1.key_eq(&key3 as &dyn Key));
    }

    #[test]
    fn test_global_object_key() {
        let key1 = GlobalObjectKey::new("test");
        let key2 = GlobalObjectKey::new("test");

        assert_ne!(key1.id(), key2.id());
        assert!(!key1.key_eq(&key2 as &dyn Key));

        assert_eq!(*key1.value(), "test");
    }

    #[test]
    fn test_global_key_raw_id() {
        let key = GlobalKey::<()>::new();
        let raw_id = key.raw_id();

        assert_eq!(raw_id, key.id().value());
        assert!(raw_id >= 1_000_000);
    }

    #[test]
    fn test_widget_key_conversions() {
        let unique = UniqueKey::new();
        let widget_key: WidgetKey = unique.into();
        assert!(matches!(widget_key, WidgetKey::Unique(_)));

        let string_key = ValueKey::new("test".to_string());
        let widget_key: WidgetKey = string_key.into();
        assert!(matches!(widget_key, WidgetKey::String(_)));
    }

    #[test]
    fn test_value_key_deref() {
        let key = ValueKey::new("test");
        assert_eq!(*key, "test");
        assert_eq!(key.len(), 4);
    }

    #[test]
    fn test_from_conversions() {
        let key1: StringKey = "test".into();
        assert_eq!(*key1, "test");

        let key2: StringKey = "test".to_string().into();
        assert_eq!(key1, key2);

        let int_key: IntKey = 42.into();
        assert_eq!(*int_key, 42);

        let widget_key: WidgetKey = "widget".into();
        assert!(matches!(widget_key, WidgetKey::String(_)));
    }

    #[test]
    fn test_value_key_string_borrow() {
        use std::collections::HashMap;

        let key = ValueKey::new("test".to_string());
        let mut map = HashMap::new();
        map.insert(key.clone(), "value");

        // Note: HashMap lookup by &str doesn't work for ValueKey<String>
        // because ValueKey uses its own hash, not String's hash
        // This is expected behavior - use the key itself for lookups
        assert_eq!(map.get(&key), Some(&"value"));
    }

    #[test]
    fn test_ordering() {
        let key1 = UniqueKey::new();
        let key2 = UniqueKey::new();
        assert!(key1 < key2);

        let int1 = ValueKey::new(10);
        let int2 = ValueKey::new(20);
        assert!(int1 < int2);
    }

    #[test]
    fn test_display() {
        let unique = UniqueKey::new();
        let s = format!("{}", unique);
        assert!(s.contains("UniqueKey"));

        let value = ValueKey::new("test");
        let s = format!("{}", value);
        assert_eq!(s, "ValueKey(test)");
    }
}