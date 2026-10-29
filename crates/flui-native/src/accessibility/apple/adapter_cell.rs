//! An accessibility adapter shared by a UIKit window's bridge and its view,
//! held until the window's teardown takes it. UIKit can be inside the
//! adapter (a forwarded container method) when the teardown asks, so a
//! detach then waits for the outermost call to return. Pure Rust, so its
//! rules run on every host.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The adapter, `A`, and whether a detach waits on a call inside it.
pub(crate) struct AdapterCell<A> {
    adapter: RefCell<Option<A>>,
    detach_pending: Cell<bool>,
}

impl<A> AdapterCell<A> {
    /// A cell holding `adapter`, to share.
    pub(crate) fn new(adapter: A) -> Rc<Self> {
        Rc::new(Self {
            adapter: RefCell::new(Some(adapter)),
            detach_pending: Cell::new(false),
        })
    }

    /// `body` with the adapter, while there is one; nested calls share it. A
    /// detach asked for during `body` takes effect once the outermost call
    /// returns.
    pub(crate) fn with<R>(&self, body: impl FnOnce(&A) -> R) -> Option<R> {
        let answer = {
            let adapter = self.adapter.try_borrow().ok()?;
            adapter.as_ref().map(body)
        };
        self.finish_pending_detach();
        answer
    }

    /// Drop the adapter now, or, while a call is inside it, as soon as the
    /// outermost one returns. Later calls find none. Idempotent.
    pub(crate) fn detach(&self) {
        self.detach_pending.set(true);
        self.finish_pending_detach();
    }

    /// Take a pending detach's adapter, unless a call is still inside it.
    /// The adapter drops outside the borrow: its teardown may call back.
    fn finish_pending_detach(&self) {
        if !self.detach_pending.get() {
            return;
        }
        let Ok(mut slot) = self.adapter.try_borrow_mut() else {
            return;
        };
        self.detach_pending.set(false);
        let adapter = slot.take();
        drop(slot);
        drop(adapter);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::AdapterCell;

    /// An adapter that counts its drops.
    struct Counted(Rc<Cell<usize>>);

    impl Drop for Counted {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    #[test]
    fn a_detached_cell_answers_nothing() {
        let drops = Rc::new(Cell::new(0));
        let cell = AdapterCell::new(Counted(Rc::clone(&drops)));
        assert_eq!(cell.with(|_| "answered"), Some("answered"));
        cell.detach();
        assert_eq!(drops.get(), 1, "the adapter drops at the detach");
        assert_eq!(cell.with(|_| "answered"), None);
        cell.detach();
        assert_eq!(drops.get(), 1, "a second detach does nothing");
    }

    /// The teardown asks while UIKit is inside the adapter: the adapter
    /// stays for that call and every call it nests, and goes when the
    /// outermost one returns.
    #[test]
    fn a_detach_during_a_call_takes_effect_when_the_call_returns() {
        let drops = Rc::new(Cell::new(0));
        let cell = AdapterCell::new(Counted(Rc::clone(&drops)));
        let inner = cell.with(|_| {
            cell.detach();
            assert_eq!(drops.get(), 0, "not while the call is inside it");
            cell.with(|_| "nested")
                .expect("a nested call still reaches it")
        });
        assert_eq!(inner, Some("nested"));
        assert_eq!(drops.get(), 1, "gone once the outermost call returned");
        assert_eq!(cell.with(|_| ()), None);
    }
}
