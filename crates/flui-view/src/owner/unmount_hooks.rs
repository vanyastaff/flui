//! Unmount hooks: work a state registers from `init_state` that runs when its
//! element leaves the tree for good.
//!
//! A value a state opens (a stored document, a router's shared state) can
//! outlive the state through its clones, and its listeners can hold those
//! clones. The hook is the state's unmount moment handed to that value, so it
//! can drop what only the mounted state may keep, without the state having to
//! remember to do it in `dispose`.
//!
//! - Hooks run right after the element's `dispose`, in registration order,
//!   at every unmount: a removal, a subtree eviction, a tree torn down at exit
//!   or session end. A `GlobalKey` move deactivates and reactivates; it is not
//!   an unmount and runs nothing.
//! - Each hook runs under its own containment: a panic does not keep the next
//!   hook or the rest of the teardown from running, and is recorded as the
//!   element's `dispose` failure.
//! - Registering from inside a running hook is refused (a debug assertion),
//!   so teardown never re-enters itself.
//! - The [`UnmountHook`] token unregisters the hook when it drops, without
//!   running it; [`UnmountHook::forget`] keeps the hook registered.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::{Rc, Weak};

use flui_foundation::ElementId;

/// A hook and the id its token names it by.
type Entry = (u64, Box<dyn FnOnce()>);

#[derive(Default)]
struct Inner {
    hooks: HashMap<ElementId, Vec<Entry>>,
    /// The id the next hook takes; ids are never reused, so a stale token
    /// can only name nothing.
    next: u64,
    /// Whether hooks are running now.
    running: bool,
}

/// The presentation's unmount hooks, by element. Owner-local, and cheap to
/// clone into the split-borrow `ElementOwner` and the build capabilities.
#[derive(Clone, Default)]
pub(crate) struct UnmountHooks {
    inner: Rc<RefCell<Inner>>,
}

impl UnmountHooks {
    /// Register `hook` to run when `element` unmounts.
    pub(crate) fn register(&self, element: ElementId, hook: Box<dyn FnOnce()>) -> UnmountHook {
        let mut inner = self.inner.borrow_mut();
        if inner.running {
            debug_assert!(
                false,
                "an unmount hook is registered while unmount hooks run; it is refused"
            );
            drop(inner);
            // Refused: the hook is dropped here, outside the borrow.
            drop(hook);
            return UnmountHook::inert();
        }
        let id = inner.next;
        inner.next = id
            .checked_add(1)
            .expect("BUG: fewer than u64::MAX unmount hooks are ever registered");
        inner.hooks.entry(element).or_default().push((id, hook));
        UnmountHook {
            hooks: Rc::downgrade(&self.inner),
            element: Some(element),
            id: Some(id),
        }
    }

    /// Run `element`'s hooks in registration order, each contained. Returns
    /// the payload of the first that panicked; the later ones still run, and
    /// the payloads after the first are retained.
    pub(crate) fn run(&self, element: ElementId) -> Option<Box<dyn std::any::Any + Send>> {
        let hooks = {
            let mut inner = self.inner.borrow_mut();
            let hooks = inner.hooks.remove(&element)?;
            inner.running = true;
            hooks
        };
        let _running = Running(&self.inner);
        let mut first = None;
        for (_id, hook) in hooks {
            if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(hook)) {
                if first.is_none() {
                    first = Some(payload);
                } else {
                    flui_foundation::panic::retain_opaque_payload(payload);
                }
            }
        }
        first
    }

    #[cfg(test)]
    fn registered(&self, element: ElementId) -> usize {
        self.inner.borrow().hooks.get(&element).map_or(0, Vec::len)
    }
}

/// Clears `running` when the hooks of one element are done, also when a
/// hook's payload drop unwinds.
struct Running<'a>(&'a Rc<RefCell<Inner>>);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.borrow_mut().running = false;
    }
}

/// The registration of one unmount hook, from
/// [`LifecycleContext::on_unmount`](crate::LifecycleContext::on_unmount).
///
/// Dropping it unregisters the hook without running it, so a value that goes
/// before its state leaves nothing behind. [`forget`](Self::forget) keeps the
/// hook registered.
#[must_use = "dropping the token unregisters the hook; call `forget` to keep it"]
pub struct UnmountHook {
    hooks: Weak<RefCell<Inner>>,
    element: Option<ElementId>,
    /// `None` for a refused registration or a forgotten token.
    id: Option<u64>,
}

impl UnmountHook {
    fn inert() -> Self {
        Self {
            hooks: Weak::new(),
            element: None,
            id: None,
        }
    }

    /// Let go of the token and keep the hook: it stays registered and still
    /// runs when the element unmounts.
    pub fn forget(mut self) {
        self.id = None;
    }
}

impl Drop for UnmountHook {
    fn drop(&mut self) {
        let (Some(id), Some(element)) = (self.id.take(), self.element) else {
            return;
        };
        let Some(hooks) = self.hooks.upgrade() else {
            return;
        };
        let removed = {
            let Ok(mut inner) = hooks.try_borrow_mut() else {
                // The hooks are being taken or registered right now; the
                // element's hooks are already out of the table, or this one
                // was not yet in it.
                return;
            };
            let Some(entries) = inner.hooks.get_mut(&element) else {
                return;
            };
            let removed = entries
                .iter()
                .position(|(entry, _)| *entry == id)
                .map(|index| entries.remove(index));
            if entries.is_empty() {
                inner.hooks.remove(&element);
            }
            removed
        };
        // The hook's captures drop outside the table's borrow: their `Drop`
        // may register or unregister another hook.
        drop(removed);
    }
}

impl fmt::Debug for UnmountHook {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UnmountHook")
            .field("element", &self.element)
            .field("registered", &self.id.is_some())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::Cell;

    fn element(n: usize) -> ElementId {
        ElementId::new(n)
    }

    #[test]
    fn a_dropped_token_unregisters_and_a_forgotten_one_keeps_the_hook() {
        let hooks = UnmountHooks::default();
        let ran = Rc::new(Cell::new(0));
        let count = |ran: &Rc<Cell<u32>>| {
            let ran = Rc::clone(ran);
            Box::new(move || ran.set(ran.get() + 1)) as Box<dyn FnOnce()>
        };
        drop(hooks.register(element(1), count(&ran)));
        assert_eq!(hooks.registered(element(1)), 0);
        hooks.register(element(1), count(&ran)).forget();
        assert_eq!(hooks.registered(element(1)), 1);
        assert!(hooks.run(element(1)).is_none());
        assert_eq!(ran.get(), 1, "the forgotten hook runs");
        assert!(hooks.run(element(1)).is_none(), "and only once");
        assert_eq!(ran.get(), 1);
    }

    #[test]
    #[cfg(debug_assertions)]
    fn a_hook_registered_while_hooks_run_is_refused() {
        let hooks = UnmountHooks::default();
        let inner = hooks.clone();
        let late = Rc::new(Cell::new(false));
        let ran = Rc::clone(&late);
        hooks
            .register(
                element(1),
                Box::new(move || {
                    let token = inner.register(element(2), Box::new(move || ran.set(true)));
                    token.forget();
                }),
            )
            .forget();
        let refused = hooks.run(element(1));
        assert!(refused.is_some(), "the refusal is a debug assertion");
        assert_eq!(hooks.registered(element(2)), 0, "nothing is registered");
        assert!(hooks.run(element(2)).is_none());
        assert!(!late.get());
    }
}
