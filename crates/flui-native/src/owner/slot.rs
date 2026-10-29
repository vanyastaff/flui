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
//! cannot: it leaves its key with the owner's table, which drops the value at
//! its next use on the owner thread (a window's next delivery, the next slot
//! made there). The table leaks rather than drops what it still holds when the
//! thread exits.

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, ThreadId};

thread_local! {
    /// The values of every live slot this thread owns, by slot key.
    static OWNER_VALUES: RefCell<OwnerValues> = RefCell::new(OwnerValues::default());
}

/// The table behind [`OWNER_VALUES`].
///
/// A value still in it when its thread exits is leaked, not dropped: its
/// destructor is host code (a window's teardown) that may reach thread-locals
/// already destroyed, and a panic there aborts the process. A slot dropped on
/// its owner takes its value out first, so only what a slot dropped on another
/// thread, or never dropped (a window in a reference cycle until it closes),
/// left behind is leaked, as it would have been without the table.
#[derive(Default)]
struct OwnerValues {
    values: HashMap<u64, Box<dyn Any>>,
    /// The keys of slots dropped on other threads, whose values this thread
    /// drops at its next use of the table ([`bury_orphans`]).
    orphans: Orphans,
}

/// Keys left by slots dropped off their owner thread.
type Orphans = std::sync::Arc<parking_lot::Mutex<Vec<u64>>>;

impl std::ops::Deref for OwnerValues {
    type Target = HashMap<u64, Box<dyn Any>>;

    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

impl std::ops::DerefMut for OwnerValues {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.values
    }
}

impl Drop for OwnerValues {
    fn drop(&mut self) {
        for (_, value) in self.values.drain() {
            std::mem::forget(value);
        }
    }
}

/// Drop, on the owner thread, the values whose slots were dropped on other
/// threads: outside the table's borrow, each in its own containment, since a
/// value's destructor is host code that may use slots again.
fn bury_orphans() {
    let orphaned: Vec<Box<dyn Any>> = OWNER_VALUES
        .try_with(|table| {
            let mut table = table.borrow_mut();
            let keys = std::mem::take(&mut *table.orphans.lock());
            keys.iter()
                .filter_map(|key| table.values.remove(key))
                .collect()
        })
        .unwrap_or_default();
    for value in orphaned {
        super::panic_boundary::contain_owner_callback(|| drop(value));
    }
}

/// Where a slot made on this thread leaves its key when it is dropped on
/// another one; `None` once the thread's table is gone.
fn orphans_of_this_thread() -> Option<Orphans> {
    OWNER_VALUES
        .try_with(|table| std::sync::Arc::clone(&table.borrow().orphans))
        .ok()
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
    /// Where the slot leaves its key when it is dropped on another thread.
    orphans: Option<Orphans>,
    _value: PhantomData<fn() -> T>,
}

impl<T: 'static> OwnerSlot<T> {
    /// An empty slot owned by the calling thread.
    pub(crate) fn new() -> Self {
        bury_orphans();
        Self {
            owner: thread::current().id(),
            key: next_key(&LAST_SLOT_KEY),
            orphans: orphans_of_this_thread(),
            _value: PhantomData,
        }
    }

    /// Whether the caller is on the thread that made the slot.
    pub(crate) fn is_owner_thread(&self) -> bool {
        thread::current().id() == self.owner
    }

    /// The key, when the caller is on the owner thread, which first drops
    /// what slots dropped elsewhere left in its table.
    fn owned_key(&self) -> Option<u64> {
        let key = (thread::current().id() == self.owner)
            .then_some(self.key)
            .flatten()?;
        bury_orphans();
        Some(key)
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

    /// Whether the caller is on the thread that made the reference.
    pub(crate) fn is_owner_thread(&self) -> bool {
        self.0.is_owner_thread()
    }
}

/// A value of a `Send + Sync` host structure that only its owner thread
/// reaches, such as a host's table of its open windows, which are not `Send`
/// (ADR-0254 §5).
///
/// The cell belongs to the thread that makes it: a host builds the structure
/// on the thread that owns its loop. Every other thread reaches nothing. `T`
/// is borrowed for the closure alone: a caller runs no user code inside it.
pub(crate) struct OwnerCell<T: 'static> {
    slot: OwnerSlot<std::rc::Rc<RefCell<T>>>,
}

impl<T: Default + 'static> OwnerCell<T> {
    /// A cell of the calling thread, holding `T::default()`.
    pub(crate) fn new() -> Self {
        let slot = OwnerSlot::new();
        if slot
            .replace(std::rc::Rc::new(RefCell::new(T::default())))
            .is_err()
        {
            tracing::error!("an owner cell holds nothing: owner slots are exhausted");
        }
        Self { slot }
    }

    /// Run `f` with the value on the thread that made the cell; `None`
    /// anywhere else.
    pub(crate) fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> Option<R> {
        let cell = self.slot.cloned()?;
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
    /// An empty map of the calling thread.
    pub(crate) fn new() -> Self {
        Self {
            cell: OwnerCell::new(),
        }
    }

    /// Store `value` under `key` on the thread that made the map: what it
    /// replaced, or `value` back when the caller is not that thread.
    ///
    /// # Errors
    ///
    /// Hands `value` back off the owner thread.
    pub(crate) fn insert(&self, key: K, value: V) -> Result<Option<V>, V> {
        let mut value = Some(value);
        self.cell
            .with(|map| map.insert(key, value.take().expect("BUG: stored once")))
            .ok_or_else(|| value.take().expect("BUG: not stored off the owner"))
    }

    /// Take the value under `key` out.
    pub(crate) fn remove(&self, key: &K) -> Option<V> {
        self.cell.with(|map| map.remove(key)).flatten()
    }
}

/// How the winit and X11 hosts read their window tables.
#[cfg(any(
    all(
        feature = "winit-backend",
        any(target_os = "windows", target_os = "macos", target_os = "linux")
    ),
    all(target_os = "linux", feature = "x11")
))]
impl<K: std::hash::Hash + Eq + Copy + 'static, V: Clone + 'static> OwnerMap<K, V> {
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
}

/// How the winit, AppKit, Wayland and X11 hosts read their window tables.
#[cfg(any(winit_host, target_os = "macos", wayland_host, x11_host))]
impl<K: std::hash::Hash + Eq + Copy + 'static, V: Clone + 'static> OwnerMap<K, V> {
    /// A snapshot of the values.
    pub(crate) fn values(&self) -> Vec<V> {
        self.cell
            .with(|map| map.values().cloned().collect())
            .unwrap_or_default()
    }

    /// A snapshot of the keys.
    // The X11 host walks its table by entries.
    #[cfg(any(winit_host, target_os = "macos"))]
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
        if !self.is_owner_thread() {
            // The value lives in the owner's table: the owner drops it at its
            // next use of the table.
            if let (Some(key), Some(orphans)) = (self.key, &self.orphans) {
                orphans.lock().push(key);
            }
            return;
        }
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

    /// A value that counts the times it was dropped.
    struct Counted(std::sync::Arc<std::sync::atomic::AtomicUsize>);

    impl Drop for Counted {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn a_value_whose_slot_dropped_elsewhere_is_dropped_at_the_owners_next_use() {
        let drops = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let slot = OwnerSlot::<Rc<Counted>>::new();
        assert!(
            slot.replace(Rc::new(Counted(std::sync::Arc::clone(&drops))))
                .is_ok_and(|replaced| replaced.is_none())
        );
        std::thread::spawn(move || drop(slot))
            .join()
            .expect("foreign thread");
        assert_eq!(
            drops.load(Ordering::SeqCst),
            0,
            "another thread cannot drop the owner's value"
        );
        // The owner's next use of its table.
        let next = OwnerSlot::<u8>::new();
        assert_eq!(
            drops.load(Ordering::SeqCst),
            1,
            "the owner drops what a slot dropped elsewhere left behind"
        );
        drop(next);
    }

    #[test]
    fn an_owner_map_belongs_to_the_thread_that_made_it() {
        let map = std::sync::Arc::new(OwnerMap::<u8, Rc<u8>>::new());
        let foreign = std::sync::Arc::clone(&map);
        std::thread::spawn(move || {
            // `Rc` cannot cross, so the foreign thread offers a plain value it
            // builds there.
            let refused = foreign.insert(1, Rc::new(1));
            assert!(refused.is_err(), "another thread's store is refused");
            assert!(foreign.remove(&1).is_none());
        })
        .join()
        .expect("foreign thread");
        assert_eq!(map.insert(2, Rc::new(2)).ok().flatten(), None);
        assert_eq!(map.remove(&2).as_deref(), Some(&2));
    }
}
