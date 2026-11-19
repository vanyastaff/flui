//! State a provider keeps for one UI runtime (ADR-0152 §5).

use flui_foundation::memory::{MemoryPressureLevel, ReclaimMemory, reclaim_each};
use flui_foundation::panic::FirstPanic;
use std::{
    any::{Any, TypeId},
    cell::{Cell, RefCell},
    collections::HashMap,
    fmt,
    rc::Rc,
};

/// The one place a provider keeps state for one UI runtime, shared by every
/// handle it hands out there: [`ProviderContext::runtime_local`](super::ProviderContext::runtime_local).
///
/// A provider is registered once and asked by every UI runtime, so state it
/// holds itself is shared across runtimes. State that must live and end with
/// one runtime (a read in flight, a connection pool, an open store) goes here,
/// one slot per type: a capability names a private type for its slot, so two
/// capabilities never share one.
///
/// A slot made with [`get_or_default_reclaimed`](Self::get_or_default_reclaimed)
/// is a cache: the runtime trims it under memory pressure
/// ([`reclaim`](Self::reclaim)), so a cache kept for one runtime gives memory
/// back with it and needs no registration of its own.
///
/// Owner-local. The UI runtime retires its slots after its pending
/// completions; a slot the runtime retired is dropped once no handle holds
/// it, and an acquisition after retirement gets a fresh value kept nowhere.
#[derive(Clone, Default)]
pub struct RuntimeLocals {
    inner: Rc<Inner>,
}

#[derive(Default)]
struct Inner {
    slots: RefCell<HashMap<TypeId, Rc<dyn Any>>>,
    /// The slots trimmed under memory pressure, in the order first asked for,
    /// each listed once.
    reclaimed: RefCell<Vec<(TypeId, Rc<dyn ReclaimMemory>)>>,
    retired: Cell<bool>,
}

impl RuntimeLocals {
    /// No slots yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// This runtime's `T`, made with `T::default()` the first time it is
    /// asked.
    ///
    /// `T::default()` runs with no borrow held, so it may ask for another
    /// slot; if it asks for `T` itself, the value it made first is kept and
    /// the later one dropped.
    #[must_use]
    pub fn get_or_default<T: Default + 'static>(&self) -> Rc<T> {
        if let Some(slot) = self.slot::<T>() {
            return slot;
        }
        let made = Rc::new(T::default());
        if self.inner.retired.get() {
            return made;
        }
        let existing = {
            let mut slots = self.inner.slots.borrow_mut();
            let existing = slots.get(&TypeId::of::<T>()).cloned();
            if existing.is_none() {
                slots.insert(TypeId::of::<T>(), Rc::clone(&made) as Rc<dyn Any>);
            }
            existing
        };
        match existing {
            // The value made second is dropped with no borrow held.
            Some(existing) => Self::downcast(existing),
            None => made,
        }
    }

    /// This runtime's `T`, as [`get_or_default`](Self::get_or_default) makes
    /// it, which the runtime also trims under memory pressure
    /// ([`reclaim`](Self::reclaim)) for as long as it keeps the slot.
    #[must_use]
    pub fn get_or_default_reclaimed<T: Default + ReclaimMemory + 'static>(&self) -> Rc<T> {
        let slot = self.get_or_default::<T>();
        if self.inner.retired.get() {
            return slot;
        }
        let mut reclaimed = self.inner.reclaimed.borrow_mut();
        if !reclaimed.iter().any(|(id, _)| *id == TypeId::of::<T>()) {
            reclaimed.push((TypeId::of::<T>(), Rc::clone(&slot) as Rc<dyn ReclaimMemory>));
        }
        drop(reclaimed);
        slot
    }

    /// Trim every slot made with
    /// [`get_or_default_reclaimed`](Self::get_or_default_reclaimed) for
    /// `level`, in the order they were first asked for, with no borrow held
    /// while one runs. A slot that panics leaves the later ones trimmed; the
    /// first panic is held in `failure`.
    pub fn reclaim(&self, level: MemoryPressureLevel, failure: &mut FirstPanic) {
        let slots: Vec<Rc<dyn ReclaimMemory>> = self
            .inner
            .reclaimed
            .borrow()
            .iter()
            .map(|(_, slot)| Rc::clone(slot))
            .collect();
        reclaim_each(slots.iter().map(|slot| &**slot), level, failure);
    }

    fn slot<T: 'static>(&self) -> Option<Rc<T>> {
        let slot = self.inner.slots.borrow().get(&TypeId::of::<T>()).cloned();
        slot.map(Self::downcast)
    }

    fn downcast<T: 'static>(slot: Rc<dyn Any>) -> Rc<T> {
        slot.downcast::<T>()
            .unwrap_or_else(|_| panic!("BUG: a runtime-local slot holds the type it is keyed by"))
    }

    /// End every slot: each is let go of with no borrow held, each drop
    /// contained, so one whose `Drop` panics leaves the others dropped too.
    /// Returns the first failure. A destructor that panics while the thread
    /// is already unwinding aborts, as in std (ADR-0281). Later acquisitions
    /// keep nothing.
    pub fn retire(&self) -> Option<Box<dyn Any + Send>> {
        self.inner.retired.set(true);
        let reclaimed = std::mem::take(&mut *self.inner.reclaimed.borrow_mut());
        let slots = std::mem::take(&mut *self.inner.slots.borrow_mut());
        let mut first = FirstPanic::new();
        // The list holds a second count of each of its slots, so it goes first
        // and each slot ends with the map's count.
        drop(reclaimed);
        for (_, slot) in slots {
            first.contain(move || drop(slot));
        }
        first.into_payload()
    }
}

impl fmt::Debug for RuntimeLocals {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeLocals")
            .field("slots", &self.inner.slots.borrow().len())
            .field("retired", &self.inner.retired.get())
            .finish()
    }
}
