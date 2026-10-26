//! Owner-thread-only storage for a value that is not `Send` (ADR-0082 §4).
//!
//! A [`OwnerSlot`] lives in a `Send + Sync` window, but the value it holds
//! lives in its owner thread's own table and is created, reached and dropped
//! on that thread only. A store from any other thread is refused and the value
//! handed back, so it is dropped where it was made; a read from another thread
//! sees nothing. That is what lets a window take an input callback that
//! captures an `Rc`: no other thread can ever run, move or drop it.
//!
//! A slot dropped on its owner drops its value there, unless the drop is part
//! of an unwind, which retains the value instead. A slot dropped elsewhere
//! cannot, so its value stays in the owner's table until that thread exits.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, ThreadId};

thread_local! {
    /// The values of every live slot this thread owns, by slot key.
    static OWNER_VALUES: RefCell<HashMap<u64, Box<dyn Any>>> = RefCell::new(HashMap::new());
}

/// The last slot key issued; `u64::MAX` once exhausted.
static LAST_SLOT_KEY: AtomicU64 = AtomicU64::new(0);

/// Issue a slot key that was never issued before, or `None` once exhausted.
fn next_key(last: &AtomicU64) -> Option<u64> {
    last.try_update(Ordering::Relaxed, Ordering::Relaxed, |last| {
        last.checked_add(1)
    })
    .ok()
    .map(|previous| previous + 1)
}

/// A value of type `T` held for the thread that made the slot.
pub(crate) struct OwnerSlot<T: 'static> {
    owner: ThreadId,
    /// `None` once the process issued every key: the slot refuses every store.
    key: Option<u64>,
    _value: PhantomData<fn() -> T>,
}

impl<T: 'static> OwnerSlot<T> {
    /// An empty slot owned by the calling thread.
    pub(crate) fn new() -> Self {
        Self {
            owner: thread::current().id(),
            key: next_key(&LAST_SLOT_KEY),
            _value: PhantomData,
        }
    }

    /// The key, when the caller is on the owner thread.
    fn owned_key(&self) -> Option<u64> {
        (thread::current().id() == self.owner)
            .then_some(self.key)
            .flatten()
    }

    /// Store `value`, returning what it replaced; the caller drops that
    /// outside any borrow.
    ///
    /// # Errors
    ///
    /// Hands `value` back from any thread but the owner, or once keys ran out.
    pub(crate) fn replace(&self, value: T) -> Result<Option<T>, T> {
        let Some(key) = self.owned_key() else {
            return Err(value);
        };
        let mut value = Some(value);
        let replaced = OWNER_VALUES
            .try_with(|values| {
                let value = value.take().expect("BUG: the value is stored once");
                values.borrow_mut().insert(key, Box::new(value))
            })
            .ok();
        match value {
            Some(value) => Err(value),
            None => Ok(replaced.flatten().map(downcast)),
        }
    }

    /// Store `value` only if the slot is empty.
    ///
    /// # Errors
    ///
    /// Hands `value` back when the slot is occupied or the caller is not its
    /// owner.
    pub(crate) fn restore(&self, value: T) -> Result<(), T> {
        if self.is_occupied() != Some(false) {
            return Err(value);
        }
        match self.replace(value) {
            Ok(None) => Ok(()),
            Ok(Some(_)) => unreachable!("BUG: an empty owner slot was checked just before"),
            Err(value) => Err(value),
        }
    }

    /// Take the value out; `None` when empty or off the owner thread.
    pub(crate) fn take(&self) -> Option<T> {
        let key = self.owned_key()?;
        OWNER_VALUES
            .try_with(|values| values.borrow_mut().remove(&key))
            .ok()
            .flatten()
            .map(downcast)
    }

    /// Whether a value is stored; `None` off the owner thread, which cannot
    /// tell.
    pub(crate) fn is_occupied(&self) -> Option<bool> {
        let key = self.owned_key()?;
        OWNER_VALUES
            .try_with(|values| values.borrow().contains_key(&key))
            .ok()
    }
}

impl<T: 'static> Drop for OwnerSlot<T> {
    fn drop(&mut self) {
        let value = self.take();
        if thread::panicking() {
            // A slot destroyed by an unwind retains its value: its destructor
            // is user code, and the failure unwinding stays the only one
            // (ADR-0127 §2).
            std::mem::forget(value);
        } else {
            super::panic_boundary::contain_owner_callback(|| drop(value));
        }
    }
}

fn downcast<T: 'static>(value: Box<dyn Any>) -> T {
    *value
        .downcast::<T>()
        .expect("BUG: a slot key names values of one type")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    #[test]
    fn exhausted_keys_refuse_permanently() {
        let last = AtomicU64::new(u64::MAX - 1);
        assert_eq!(next_key(&last), Some(u64::MAX));
        assert_eq!(next_key(&last), None);
        assert_eq!(next_key(&last), None);
    }

    #[test]
    fn only_the_owner_stores_and_reaches_the_value() {
        let slot = std::sync::Arc::new(OwnerSlot::<Rc<u8>>::new());
        assert!(slot.replace(Rc::new(1)).expect("owner stores").is_none());
        let foreign = std::sync::Arc::clone(&slot);
        std::thread::spawn(move || {
            assert_eq!(foreign.take(), None, "another thread reaches nothing");
            assert_eq!(foreign.is_occupied(), None);
        })
        .join()
        .expect("foreign thread");
        assert_eq!(slot.take().as_deref(), Some(&1));
        assert_eq!(slot.is_occupied(), Some(false));
    }
}
