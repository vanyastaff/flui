//! Object key - key based on object identity (pointer equality).
//!
//! The owning Arc keeps the allocation alive for the key's lifetime.

use std::{any::Any, fmt, sync::Arc};

use flui_foundation::ViewKey;

/// A key based on object identity (pointer equality).
///
/// Two `ObjectKey` are equal only if they point to the same object.
/// Useful when you want to key by a specific instance.
///
/// # Example
///
/// ```rust
/// use std::sync::Arc;
///
/// use flui_foundation::ViewKey;
/// use flui_view::ObjectKey;
///
/// let obj1 = Arc::new(42);
/// let obj2 = Arc::new(42); // Same value, different object
///
/// let key1 = ObjectKey::new(Arc::clone(&obj1));
/// let key2 = ObjectKey::new(Arc::clone(&obj1)); // Same object
/// let key3 = ObjectKey::new(obj2); // Different object
///
/// assert!(key1.key_eq(&key2)); // Same object
/// assert!(!key1.key_eq(&key3)); // Different objects
/// ```
#[derive(Clone)]
pub struct ObjectKey {
    holder: Arc<dyn Any + Send + Sync>,
}

impl ObjectKey {
    /// Create a new object key from an Arc.
    pub fn new<T: Send + Sync + 'static>(object: Arc<T>) -> Self {
        Self { holder: object }
    }
}

impl fmt::Debug for ObjectKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObjectKey")
            .field("ptr", &Arc::as_ptr(&self.holder).cast::<()>())
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
            .is_some_and(|other| Arc::ptr_eq(&self.holder, &other.holder))
    }

    fn key_hash(&self) -> u64 {
        Arc::as_ptr(&self.holder).cast::<()>() as u64
    }

    fn clone_key(&self) -> Box<dyn ViewKey> {
        Box::new(self.clone())
    }

    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ObjectKey({:p})", Arc::as_ptr(&self.holder).cast::<()>())
    }
}
