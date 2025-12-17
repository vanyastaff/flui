//! Global key - provides access to the element from anywhere.
//!
//! This module is part of the widgets layer, matching Flutter's architecture
//! where `GlobalKey` is defined in `widgets/framework.dart`.

use std::any::Any;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use flui_foundation::{ElementId, ViewKey};

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
        let debug = format!("{:?}", key);

        assert!(debug.contains("GlobalKey"));
        assert!(debug.contains("i32"));
    }
}
