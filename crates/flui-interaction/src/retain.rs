//! Exceptional-path retention of user-owned values (ADR-0127).
//!
//! After a failure, a container keeps the values whose destruction would run
//! user code. A reference-counted handle that is not the last owner runs no
//! user code when dropped, so it is released normally: retaining a clone would
//! leak the captures of an owner that is still alive and will be dropped later.

use std::rc::Rc;

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
