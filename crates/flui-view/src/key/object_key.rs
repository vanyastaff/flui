//! Object key - key based on object identity (pointer equality).
//!
//! The key holds an `Rc` to the object, so the allocation, and the address it
//! compares, stays alive and unreused for the key's lifetime. Keys live on
//! the owner thread (ADR-0323 §4).

use std::{any::Any, fmt, rc::Rc};

use flui_foundation::ViewKey;

/// A key based on object identity (pointer equality).
///
/// Two `ObjectKey` are equal only if they point to the same object.
/// Useful when you want to key by a specific instance.
///
/// # Example
///
/// ```rust
/// use std::rc::Rc;
///
/// use flui_foundation::ViewKey;
/// use flui_view::ObjectKey;
///
/// let obj1 = Rc::new(42);
/// let obj2 = Rc::new(42); // Same value, different object
///
/// let key1 = ObjectKey::new(&obj1);
/// let key2 = ObjectKey::new(&obj1); // Same object
/// let key3 = ObjectKey::new(&obj2); // Different object
///
/// assert!(key1.key_eq(&key2)); // Same object
/// assert!(!key1.key_eq(&key3)); // Different objects
/// ```
#[derive(Clone)]
pub struct ObjectKey {
    holder: Rc<dyn Any>,
}

impl ObjectKey {
    /// A key for `object`'s allocation; it holds a clone of the `Rc`.
    pub fn new<T: 'static>(object: &Rc<T>) -> Self {
        Self {
            holder: Rc::<T>::clone(object),
        }
    }
}

impl fmt::Debug for ObjectKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObjectKey")
            .field("ptr", &Rc::as_ptr(&self.holder).cast::<()>())
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
            .is_some_and(|other| Rc::ptr_eq(&self.holder, &other.holder))
    }

    fn key_hash(&self) -> u64 {
        Rc::as_ptr(&self.holder).cast::<()>() as u64
    }

    fn clone_key(&self) -> Box<dyn ViewKey> {
        Box::new(self.clone())
    }

    fn debug_fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ObjectKey({:p})", Rc::as_ptr(&self.holder).cast::<()>())
    }
}
