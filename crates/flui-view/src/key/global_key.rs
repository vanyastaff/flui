//! Global key - provides access to the element from anywhere.
//!
//! This module is part of the widgets layer, matching Flutter's architecture
//! where `GlobalKey` is defined in `widgets/framework.dart`.

use std::{
    any::Any,
    fmt,
    hash::{Hash, Hasher},
    marker::PhantomData,
    sync::atomic::{AtomicU64, Ordering},
};

use flui_foundation::{ElementId, ViewKey};

use crate::view::ElementBase;

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
/// use flui_view::GlobalKey;
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
pub struct GlobalKey<T: 'static> {
    id: u64,
    /// `fn() -> T` keeps `GlobalKey<T>` covariant in `T` without
    /// requiring `T: Send + Sync`. The marker is also why the manual
    /// `Clone` impl below sidesteps the `T: Clone` bound a derive
    /// would impose — `PhantomData<fn() -> T>` is always `Clone +
    /// Copy` regardless of `T`.
    _marker: PhantomData<fn() -> T>,
}

// Manual `Clone` impl: do NOT require `T: Clone`. A `GlobalKey<T>` is
// just an `id` + a phantom marker, so cloning is trivial.
impl<T: 'static> Clone for GlobalKey<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            _marker: PhantomData,
        }
    }
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
    /// Returns `None` if no element registered under this `GlobalKey`'s
    /// hash is currently mounted in the registered build owner.
    ///
    /// # Registry access
    ///
    /// Reads the registry handle activated by the current owner-thread realm
    /// scope (or by the legacy test harness adapter). When no realm is active
    /// the method returns `None` — this is the
    /// quiescent state expected in pure-unit tests that bypass the
    /// framework binding.
    ///
    /// Flutter parity: `framework.dart:3163`
    /// `GlobalKey._currentElement` — reads `BuildOwner._globalKeyRegistry`
    /// indexed by the key. We hash-key the registry instead because
    /// `Box<dyn ViewKey>` would need `Hash + Eq` blanket-impls and
    /// hash collisions are documented (§I4).
    #[must_use]
    pub fn current_element(&self) -> Option<ElementId> {
        crate::key::registry::with_registry(|registry| registry.lookup_element(self.id)).flatten()
    }

    /// Run `f` against the current state of the element registered under
    /// this key, downcasting to `R` first.
    ///
    /// Returns `None` if:
    /// - No element is currently registered for this key, OR
    /// - The matched element has no state (e.g. it's a `StatelessElement`), OR
    /// - The state's runtime type doesn't match `T`.
    ///
    /// `T` is the type the `GlobalKey<T>` was instantiated with — by
    /// convention this is the `ViewState` impl tied to the keyed
    /// `StatefulView`. The match is enforced via `Any::downcast_ref::<T>`
    /// at the dispatch boundary so non-`StatefulView` keys (e.g.
    /// `GlobalKey<i32>`) simply never resolve, no compile error.
    ///
    /// The callback shape (`R` returned, state borrowed for the duration
    /// of the call) lets callers extract a snapshot without leaking the
    /// borrow into the rest of `build()`. Same pattern as
    /// [`BuildContextExt::find_state`](crate::BuildContextExt::find_state).
    ///
    /// Flutter parity: `framework.dart:3170`
    /// `GlobalKey<T extends State>.currentState` — returns `T?` after
    /// a runtime-type check. We surface a closure-callback variant so
    /// the read-lock on the element tree drops before the caller does
    /// anything substantial with the value.
    #[must_use]
    pub fn with_current_state<R>(&self, f: impl FnOnce(&T) -> R) -> Option<R>
    where
        T: 'static,
    {
        let element_id = self.current_element()?;

        // `with_registry` yields `Option<...>` itself (None when no
        // realm/fixture handle is active), `with_element` yields another
        // `Option<...>` (None when the id is no longer in the tree),
        // and the inner closure also yields `Option<R>` (None when
        // the state downcast fails). Triple-Option flattens to one
        // `Option<R>` via two flatten() / one ? chain.
        crate::key::registry::with_registry(|registry| {
            registry.with_element(element_id, |element: &dyn ElementBase| {
                let state_any = element.state_as_any()?;
                let typed = state_any.downcast_ref::<T>()?;
                Some(f(typed))
            })
        })
        .flatten() // peel the with_registry None layer
        .flatten() // peel the with_element None layer
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

    fn is_global_key(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_global_key_uniqueness() {
        let key1 = GlobalKey::<i32>::new();
        let key2 = GlobalKey::<i32>::new();

        assert_ne!(key1.id(), key2.id());
        assert!(!key1.key_eq(&key2));
    }

    #[test]
    fn test_global_key_clone_equality() {
        let key1 = GlobalKey::<String>::new();
        let key2 = key1.clone();

        assert_eq!(key1.id(), key2.id());
        assert!(key1.key_eq(&key2));
    }

    #[test]
    fn test_global_key_different_types() {
        let key1 = GlobalKey::<i32>::new();
        let key2 = GlobalKey::<String>::new();

        // Different types should not match even with same underlying id
        // (but they won't have same id anyway due to counter)
        assert!(!key1.key_eq(&key2));
    }

    #[test]
    fn test_global_key_hash() {
        let key = GlobalKey::<Vec<u8>>::new();
        assert_eq!(key.key_hash(), key.id());
    }

    #[test]
    fn test_global_key_default() {
        let key1 = GlobalKey::<()>::default();
        let key2 = GlobalKey::<()>::default();

        assert_ne!(key1.id(), key2.id());
    }

    #[test]
    fn test_global_key_debug() {
        let key = GlobalKey::<i32>::new();
        let debug = format!("{key:?}");

        assert!(debug.contains("GlobalKey"));
        assert!(debug.contains("i32"));
    }
}
