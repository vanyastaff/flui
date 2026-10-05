//! Exceptional-path retention of user-owned values (ADR-0127).
//!
//! After a failure, a container keeps the values whose destruction would run
//! user code. A reference-counted handle that is not the last owner runs no
//! user code when dropped, so it is released normally: retaining a clone would
//! leak the captures of an owner that is still alive and will be dropped later.

use std::{rc::Rc, sync::Arc};

/// A value an exceptional path retains instead of destroying.
pub(crate) trait Retain {
    /// Forget whatever dropping `self` would destroy; release the rest.
    fn retain(self);
}

impl<T: ?Sized> Retain for Rc<T> {
    fn retain(self) {
        if Rc::strong_count(&self) == 1 {
            std::mem::forget(self);
        }
    }
}

impl<T: ?Sized> Retain for Arc<T> {
    /// The same last-owner rule as [`Rc`]. Another thread releasing its clone
    /// between the count and this drop makes this drop the last one, so a
    /// shared `Arc` stays retainable only while its owners are owner-thread.
    fn retain(self) {
        if Arc::strong_count(&self) == 1 {
            std::mem::forget(self);
        }
    }
}

impl<T: ?Sized> Retain for Box<T> {
    fn retain(self) {
        std::mem::forget(self);
    }
}

impl<T: Retain> Retain for Option<T> {
    fn retain(self) {
        if let Some(value) = self {
            value.retain();
        }
    }
}

impl<T: Retain> Retain for Vec<T> {
    fn retain(self) {
        for value in self {
            value.retain();
        }
    }
}

/// A value retained whole: a uniquely owned opaque value such as a user
/// closure, or a shared handle whose other owners are framework structures
/// (a lane's target cell is also held by saved routes) that would otherwise
/// destroy it later, outside the failure that retired it.
pub(crate) struct Owned<T>(pub(crate) T);

impl<T> Retain for Owned<T> {
    fn retain(self) {
        std::mem::forget(self);
    }
}
