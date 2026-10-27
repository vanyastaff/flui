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

    /// Whether the caller is on the thread that made the slot.
    pub(crate) fn is_owner_thread(&self) -> bool {
        thread::current().id() == self.owner
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

    /// A clone of the value, left in place; `None` when empty or off the
    /// owner thread.
    pub(crate) fn cloned(&self) -> Option<T>
    where
        T: Clone,
    {
        let key = self.owned_key()?;
        OWNER_VALUES
            .try_with(|values| {
                values
                    .borrow()
                    .get(&key)
                    .map(|value| downcast_ref::<T>(value.as_ref()).clone())
            })
            .ok()
            .flatten()
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

/// A weak reference to a value of the owner thread, such as a window, that
/// any thread may carry (in a queued close, a state publisher, an opened
/// window's answer): it upgrades on the thread that made it, and nowhere
/// else (ADR-0254 §5).
pub(crate) struct OwnerWeak<T: ?Sized + 'static>(OwnerSlot<std::rc::Weak<T>>);

impl<T: ?Sized + 'static> OwnerWeak<T> {
    /// A weak reference to `value`, made on its owner thread.
    pub(crate) fn new(value: &std::rc::Rc<T>) -> Self {
        let slot = OwnerSlot::new();
        if slot.replace(std::rc::Rc::downgrade(value)).is_err() {
            tracing::error!("a weak owner reference could not be kept: owner slots are exhausted");
        }
        Self(slot)
    }

    /// The value, on the owner thread while it lives; `None` anywhere else.
    pub(crate) fn upgrade(&self) -> Option<std::rc::Rc<T>> {
        self.0.cloned()?.upgrade()
    }
}

/// A value of a `Send + Sync` host structure that only its owner thread
/// reaches, such as a host's table of its open windows, which are not `Send`
/// (ADR-0254 §5).
///
/// The cell binds to the thread that first stores into it
/// ([`Self::with_bound`]): a host stores on its owner thread only, so that is
/// the owner, wherever the structure was built. Every other thread reaches
/// nothing. `T` is borrowed for the closure alone: a caller runs no user code
/// inside it.
pub(crate) struct OwnerCell<T: 'static> {
    slot: std::sync::OnceLock<OwnerSlot<std::rc::Rc<RefCell<T>>>>,
}

impl<T: Default + 'static> OwnerCell<T> {
    /// A cell no thread has stored into yet.
    pub(crate) const fn new() -> Self {
        Self {
            slot: std::sync::OnceLock::new(),
        }
    }

    /// Run `f` with the value, binding the cell to the calling thread when no
    /// thread stored into it yet; `None` on any thread but the one it is
    /// bound to.
    pub(crate) fn with_bound<R>(&self, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        let slot = self.slot.get_or_init(OwnerSlot::new);
        let cell = if let Some(cell) = slot.cloned() {
            cell
        } else {
            let cell = std::rc::Rc::new(RefCell::new(T::default()));
            slot.replace(std::rc::Rc::clone(&cell)).ok()?;
            cell
        };
        let mut value = cell.borrow_mut();
        Some(f(&mut value))
    }

    /// Run `f` with the value on the thread the cell is bound to; `None`
    /// anywhere else, and before any thread stored into it.
    pub(crate) fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        let cell = self.slot.get()?.cloned()?;
        let mut value = cell.borrow_mut();
        Some(f(&mut value))
    }
}

impl<T: Default + 'static> Default for OwnerCell<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// A map of a `Send + Sync` host structure that only its owner thread
/// reaches: an [`OwnerCell`] of a `HashMap`, for a host's table of open
/// windows. Off the owner thread it is empty and takes nothing.
pub(crate) struct OwnerMap<K: 'static, V: 'static> {
    cell: OwnerCell<HashMap<K, V>>,
}

impl<K: std::hash::Hash + Eq + Copy + 'static, V: Clone + 'static> OwnerMap<K, V> {
    /// A map no thread has stored into yet.
    pub(crate) const fn new() -> Self {
        Self {
            cell: OwnerCell::new(),
        }
    }

    /// Store `value` under `key`, binding the map to the calling thread when
    /// nothing was stored yet: what it replaced, or `value` back when the
    /// caller is not the owner.
    ///
    /// # Errors
    ///
    /// Hands `value` back off the owner thread.
    pub(crate) fn insert(&self, key: K, value: V) -> Result<Option<V>, V> {
        let mut value = Some(value);
        self.cell
            .with_bound(|map| map.insert(key, value.take().expect("BUG: stored once")))
            .ok_or_else(|| value.take().expect("BUG: not stored off the owner"))
    }

    /// Take the value under `key` out.
    pub(crate) fn remove(&self, key: &K) -> Option<V> {
        self.cell.with(|map| map.remove(key)).flatten()
    }

    /// A clone of the value under `key`.
    pub(crate) fn get(&self, key: &K) -> Option<V> {
        self.cell.with(|map| map.get(key).cloned()).flatten()
    }

    /// A snapshot of the keys and values: what is done with them next may
    /// open or close windows.
    pub(crate) fn entries(&self) -> Vec<(K, V)> {
        self.cell
            .with(|map| {
                map.iter()
                    .map(|(key, value)| (*key, value.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// A snapshot of the values.
    pub(crate) fn values(&self) -> Vec<V> {
        self.cell
            .with(|map| map.values().cloned().collect())
            .unwrap_or_default()
    }

    /// A snapshot of the keys.
    pub(crate) fn keys(&self) -> Vec<K> {
        self.cell
            .with(|map| map.keys().copied().collect())
            .unwrap_or_default()
    }

    /// Whether the map holds nothing; `true` off the owner thread, which holds
    /// nothing.
    pub(crate) fn is_empty(&self) -> bool {
        self.cell.with(|map| map.is_empty()).unwrap_or(true)
    }
}

impl<K: std::hash::Hash + Eq + Copy + 'static, V: Clone + 'static> Default for OwnerMap<K, V> {
    fn default() -> Self {
        Self::new()
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

fn downcast_ref<T: 'static>(value: &dyn Any) -> &T {
    value
        .downcast_ref::<T>()
        .expect("BUG: a slot key names values of one type")
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
