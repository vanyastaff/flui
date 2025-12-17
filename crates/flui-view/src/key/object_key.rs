//! Object key - key based on object identity (pointer equality).
//!
//! This module is part of the widgets layer, matching Flutter's architecture
//! where `ObjectKey` is defined in `widgets/framework.dart`.
#![allow(unsafe_code)]

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use flui_foundation::ViewKey;

/// A key based on object identity (pointer equality).
///
/// Two `ObjectKey` are equal only if they point to the same object.
/// Useful when you want to key by a specific instance.
///
/// # Example
///
/// ```rust
/// use flui_view::ObjectKey;
/// use flui_foundation::ViewKey;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_object_key_same_object() {
        let obj = Arc::new(42);
        let key1 = ObjectKey::new(Arc::clone(&obj));
        let key2 = ObjectKey::new(Arc::clone(&obj));

        assert!(key1.key_eq(&key2));
    }

    #[test]
    fn test_object_key_different_objects() {
        let obj1 = Arc::new(42);
        let obj2 = Arc::new(42); // Same value, different object

        let key1 = ObjectKey::new(obj1);
        let key2 = ObjectKey::new(obj2);

        assert!(!key1.key_eq(&key2));
    }

    #[test]
    fn test_object_key_hash() {
        let obj = Arc::new("test");
        let key = ObjectKey::new(obj);

        // Hash should be the pointer value
        assert_ne!(key.key_hash(), 0);
    }

    #[test]
    fn test_object_key_clone() {
        let obj = Arc::new(vec![1, 2, 3]);
        let key1 = ObjectKey::new(obj);
        let key2 = key1.clone();

        assert!(key1.key_eq(&key2));
    }
}
