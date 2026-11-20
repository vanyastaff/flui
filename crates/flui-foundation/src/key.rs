//! The keys a view is matched by when its parent rebuilds.
//!
//! A key is a [`ViewKey`]: two keys match when they are of one type and equal
//! as that type, so a `ValueKey<u32>` never matches a `ValueKey<i32>` of the
//! same number.
//!
//! - [`ValueKey<T>`] matches by value: a list item keyed by its record's id,
//!   or a constant such as `ValueKey::new("header")`.
//! - [`UniqueKey`] matches only itself: a new element on every rebuild that
//!   holds a new one.
//! - `ObjectKey` and `GlobalKey<T>` are `flui-view`'s.
//!
//! # A view's key
//!
//! A view holds its key in a field and marks it `#[view(key)]`; the view
//! derives of `flui-view` read the field through [`AsViewKey`], so the field is
//! a key, an `Option` of one, or an `Rc<dyn ViewKey>` or `Box<dyn ViewKey>`.
//!
//! ```rust,ignore
//! use flui_foundation::ValueKey;
//! use flui_view::StatelessView;
//!
//! // A list item matched by its record's id across rebuilds.
//! #[derive(Clone, StatelessView)]
//! struct TodoItem {
//!     #[view(key)]
//!     key: ValueKey<u64>,
//!     title: String,
//! }
//! ```

use std::{
    any::{Any, TypeId},
    fmt,
    hash::{Hash, Hasher},
    num::NonZeroU64,
};

use crate::sequence::AtomicIdMint;

/// The mint of [`UniqueKey`]: one id domain for the process, since a key is
/// built on whichever thread builds the view that holds it.
static UNIQUE_KEYS: AtomicIdMint<NonZeroU64> = AtomicIdMint::new();

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
/// - `ObjectKey` - Match by object identity (defined in `flui-view`)
/// - [`UniqueKey`] - Never matches (forces new element)
/// - `GlobalKey<T>` - Global access + matching (defined in `flui-view`)
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
pub trait ViewKey: Any {
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

    /// Whether this key is a `GlobalKey` (i.e. should be tracked in the
    /// `BuildOwner` global-key registry for cross-tree lookup and
    /// state-migration reparenting).
    ///
    /// Default returns `false`; only the widget-layer `GlobalKey<T>`
    /// overrides this to return `true`. Used by `Element::mount` /
    /// `ElementTree::insert` to decide whether to call
    /// `ElementOwner::register_global_key` and by `ElementTree::remove`
    /// to decide whether to soft-remove into the inactive queue (R13 /
    /// R14).
    ///
    /// The check is split across two methods (a hash and a "this is a
    /// global key" boolean) because `ViewKey::key_hash()` already returns
    /// `u64` from any key impl (incl. `ValueKey`, `UniqueKey`, `ObjectKey`),
    /// and we need a cheap, type-erased way to skip non-global keys without
    /// an `Any` downcast at every mount site.
    fn is_global_key(&self) -> bool {
        false
    }
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

/// A field a view reads its key from: a [`ViewKey`], one that may be absent, or one that is
/// shared or boxed as a trait object.
///
/// `#[view(key)]` on a field of a `#[derive(StatelessView)]` or `#[derive(StatefulView)]`
/// type writes `View::key` through this trait, so the field keeps the type the view needs:
///
/// | Field type | The view's key |
/// |------------|----------------|
/// | `K: ViewKey` (`ValueKey<T>`, `UniqueKey`, `GlobalKey<T>`, …) | always that key |
/// | `Option<K>` of any row here | the key when there is one, keyless otherwise |
/// | `Rc<dyn ViewKey>`, `Box<dyn ViewKey>` | the key inside |
///
/// The trait lives beside [`ViewKey`] because only the crate that owns `ViewKey` can state
/// that `Option<K>` and `Rc<K>` are not keys themselves, which is what lets these
/// implementations stand beside the one for every `K: ViewKey`.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a field a view can read its key from",
    label = "not a view key",
    note = "a `#[view(key)]` field is a `ViewKey` (`ValueKey<T>`, `UniqueKey`, `GlobalKey<T>`, …), an `Option` of one, or an `Rc<dyn ViewKey>` or `Box<dyn ViewKey>`"
)]
pub trait AsViewKey {
    /// The key this field gives its view, or `None` when it holds none.
    fn as_view_key(&self) -> Option<&dyn ViewKey>;
}

impl<K: ViewKey> AsViewKey for K {
    #[inline]
    fn as_view_key(&self) -> Option<&dyn ViewKey> {
        Some(self)
    }
}

impl AsViewKey for dyn ViewKey {
    #[inline]
    fn as_view_key(&self) -> Option<&dyn ViewKey> {
        Some(self)
    }
}

impl<K: AsViewKey> AsViewKey for Option<K> {
    #[inline]
    fn as_view_key(&self) -> Option<&dyn ViewKey> {
        self.as_ref().and_then(AsViewKey::as_view_key)
    }
}

impl<K: AsViewKey + ?Sized> AsViewKey for std::rc::Rc<K> {
    #[inline]
    fn as_view_key(&self) -> Option<&dyn ViewKey> {
        (**self).as_view_key()
    }
}

impl AsViewKey for Box<dyn ViewKey> {
    #[inline]
    fn as_view_key(&self) -> Option<&dyn ViewKey> {
        Some(&**self)
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
pub struct ValueKey<T: Clone + Hash + Eq + 'static> {
    value: T,
}

impl<T: Clone + Hash + Eq + 'static> ValueKey<T> {
    /// Create a new value key.
    pub const fn new(value: T) -> Self {
        Self { value }
    }

    /// Get the value.
    pub const fn value(&self) -> &T {
        &self.value
    }
}

impl<T: Clone + Hash + Eq + 'static> fmt::Debug for ValueKey<T>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ValueKey")
            .field("value", &self.value)
            .finish()
    }
}

impl<T: Clone + Hash + Eq + fmt::Debug + 'static> ViewKey for ValueKey<T> {
    fn key_eq(&self, other: &dyn ViewKey) -> bool {
        let other: &dyn Any = other;
        other
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
// SALTED KEY
// ============================================================================

/// A key that a *wrapper* element carries on behalf of the child it wraps.
///
/// A lazy sliver reconciles its children by key, and each item's key must
/// live outside the per-item `RepaintBoundary`. The wrapper owns the render
/// node the sliver needs, so the wrapper carries the key itself — salted, for
/// two reasons:
///
/// - **it is not the item's key.** Two elements may not answer to the same
///   key in one parent; the salt makes the wrapper's key equal only to another
///   wrapper's salt of an equal inner key.
/// - **it is never a `GlobalKey`.** [`ViewKey::is_global_key`] is `false`
///   whatever the inner key is, so the wrapper registers nothing in the
///   `GlobalKey` registry and the item inside registers its own key exactly
///   once. Forwarding a raw `GlobalKey` made the wrapper and the item both
///   claim it: a debug panic at mount, a duplicate report in release.
///
/// A lazy sliver looking an item up by key ([`SaltedKey::unsalt`]) sees
/// through the salt to the item's own key, which is what a delegate's
/// `find_index_by_key` is written against.
pub struct SaltedKey {
    inner: Box<dyn ViewKey>,
}

impl SaltedKey {
    /// Salt `inner` for the wrapper that will carry it.
    #[must_use]
    pub fn new(inner: &dyn ViewKey) -> Self {
        Self {
            inner: inner.clone_key(),
        }
    }

    /// The item's own key.
    #[must_use]
    pub fn inner(&self) -> &dyn ViewKey {
        &*self.inner
    }

    /// `key` itself, or the item key inside it when `key` is a salt.
    #[must_use]
    pub fn unsalt(key: &dyn ViewKey) -> &dyn ViewKey {
        let any: &dyn Any = key;
        any.downcast_ref::<Self>().map_or(key, SaltedKey::inner)
    }
}

impl Clone for SaltedKey {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone_key(),
        }
    }
}

impl fmt::Debug for SaltedKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SaltedKey({:?})", &*self.inner)
    }
}

impl ViewKey for SaltedKey {
    fn key_eq(&self, other: &dyn ViewKey) -> bool {
        let other: &dyn Any = other;
        other
            .downcast_ref::<Self>()
            .is_some_and(|other| self.inner.key_eq(&*other.inner))
    }

    fn key_hash(&self) -> u64 {
        // A fixed salt keeps a wrapper's hash apart from its item's, so the
        // reconciler's hash pre-check never pairs the two.
        const SALT: u64 = 0x5a17_ed4b_1e9c_7f03;
        self.inner.key_hash() ^ SALT
    }

    fn clone_key(&self) -> Box<dyn ViewKey> {
        Box::new(Self {
            inner: self.inner.clone_key(),
        })
    }

    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SaltedKey({:?})", &*self.inner)
    }

    // `is_global_key` keeps the trait default `false` on purpose: the salt
    // exists so the wrapper never registers the item's GlobalKey.
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
    id: NonZeroU64,
}

impl UniqueKey {
    /// A key no other key matches.
    ///
    /// There is no `Default`: each call returns a different key.
    ///
    /// # Panics
    ///
    /// Once the process has issued 2^64 − 1 unique keys, which a process
    /// that issued one every nanosecond would reach after five centuries.
    #[expect(
        clippy::new_without_default,
        reason = "a default would be a different key on every call"
    )]
    #[must_use = "a discarded key only spends an id"]
    pub fn new() -> Self {
        let id = UNIQUE_KEYS
            .next()
            .expect("BUG: a process issues fewer than 2^64 - 1 unique keys");
        Self { id }
    }

    /// The key's id, for diagnostics.
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id.get()
    }
}

impl fmt::Debug for UniqueKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UniqueKey")
            .field("id", &self.id.get())
            .finish()
    }
}

impl ViewKey for UniqueKey {
    fn key_eq(&self, other: &dyn ViewKey) -> bool {
        let other: &dyn Any = other;
        other
            .downcast_ref::<Self>()
            .is_some_and(|other| self.id == other.id)
    }

    fn key_hash(&self) -> u64 {
        self.id.get()
    }

    fn clone_key(&self) -> Box<dyn ViewKey> {
        Box::new(*self)
    }

    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "UniqueKey({})", self.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_unique_key_matches_only_itself_and_no_other_key_type() {
        let first = UniqueKey::new();
        let second = UniqueKey::new();
        assert!(first.key_eq(&first));
        assert!(!first.key_eq(&second));
        let number = ValueKey::<u64>::new(first.id());
        assert!(!first.key_eq(&number));
        assert!(!number.key_eq(&first));
    }
}
