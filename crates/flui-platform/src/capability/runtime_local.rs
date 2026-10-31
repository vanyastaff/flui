//! State a provider keeps for one UI runtime (ADR-0152 §5).

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
    /// Returns the first failure. While the thread unwinds, the slots are
    /// leaked rather than dropped into a second panic. Later acquisitions
    /// keep nothing.
    pub fn retire(&self) -> Option<Box<dyn Any + Send>> {
        self.inner.retired.set(true);
        let slots = std::mem::take(&mut *self.inner.slots.borrow_mut());
        let mut first = None;
        for (_, slot) in slots {
            if std::thread::panicking() {
                std::mem::forget(slot);
                continue;
            }
            if let Err(payload) =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(slot)))
            {
                match first {
                    None => first = Some(payload),
                    // The first failure goes on; a later one is kept, not
                    // dropped into a second panic.
                    Some(_) => flui_foundation::panic::retain_opaque_payload(payload),
                }
            }
        }
        first
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
