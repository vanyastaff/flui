//! The queue a text-store host keeps for requests made while it is inside a
//! call into a store.
//!
//! A store's callbacks (its observer, its lock grants) are application code,
//! and they may ask the host to focus another field or end a composition while
//! the host is still on the stack under them. The host cannot apply such a
//! request then: it would re-enter state the outer call holds. Every backend
//! (Android, Web, Windows TSF) counts its calls into a store with
//! [`HostQueue::enter`], queues what arrives meanwhile with
//! [`HostQueue::defer`], and applies the queue once the outermost call returns
//! ([`HostQueue::next`]). What a backend does with a failure an operation
//! raises stays the backend's own: TSF keeps the first one (ADR-0142).

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
#[cfg(any(
    target_os = "android",
    target_os = "ios",
    target_os = "macos",
    target_arch = "wasm32",
    test
))]
use std::rc::Rc;

#[cfg(any(
    target_os = "android",
    target_os = "ios",
    target_os = "macos",
    target_arch = "wasm32",
    test
))]
use flui_platform::text_store::TextStore;

#[cfg(test)]
mod tests;

/// The requests a host queued while inside a store call, in order.
pub(crate) struct HostQueue<Op> {
    depth: Cell<u32>,
    pending: RefCell<VecDeque<Op>>,
}

/// One call into a store, counted until it ends, unwinding included. The
/// queue is never drained from here: the host drains it explicitly, after the
/// call's body.
#[must_use = "the call is counted only while the guard lives"]
pub(crate) struct Entered<'a>(&'a Cell<u32>);

impl Drop for Entered<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get().saturating_sub(1));
    }
}

impl<Op> HostQueue<Op> {
    pub(crate) const fn new() -> Self {
        Self {
            depth: Cell::new(0),
            pending: RefCell::new(VecDeque::new()),
        }
    }

    /// A call into a store starts: requests queue until the guard drops.
    pub(crate) fn enter(&self) -> Entered<'_> {
        let depth = self
            .depth
            .get()
            .checked_add(1)
            .expect("BUG: store calls nest deeper than u32::MAX");
        self.depth.set(depth);
        Entered(&self.depth)
    }

    /// Whether the host is inside a call into a store.
    pub(crate) fn is_entered(&self) -> bool {
        self.depth.get() > 0
    }

    /// Queue `op` while the host is inside a store call; give it back when it
    /// is not, for the caller to apply now.
    pub(crate) fn defer(&self, op: Op) -> Option<Op> {
        if self.is_entered() {
            self.pending.borrow_mut().push_back(op);
            None
        } else {
            Some(op)
        }
    }

    /// Queue `op` whatever the depth: the next drain applies it.
    #[cfg(windows)]
    pub(crate) fn push(&self, op: Op) {
        self.pending.borrow_mut().push_back(op);
    }

    /// Hold the queue borrowed, so a request made meanwhile panics: how a test
    /// fails a host operation inside a call.
    #[cfg(all(windows, test))]
    pub(crate) fn hold(&self) -> std::cell::Ref<'_, VecDeque<Op>> {
        self.pending.borrow()
    }

    /// The next queued request, once no store call is running; `None` while
    /// one is, so a request a drained one queues waits for its turn.
    pub(crate) fn next(&self) -> Option<Op> {
        if self.is_entered() {
            return None;
        }
        self.pending.borrow_mut().pop_front()
    }

    /// The newest queued request `pick` answers for.
    pub(crate) fn newest<T>(&self, pick: impl FnMut(&Op) -> Option<T>) -> Option<T> {
        self.pending.borrow().iter().rev().find_map(pick)
    }

    /// Forget every queued request: the host retired with its window.
    #[cfg(any(
        target_os = "android",
        target_os = "ios",
        target_os = "macos",
        target_arch = "wasm32",
        test
    ))]
    pub(crate) fn clear(&self) {
        // Taken out before dropping, so a store a request held cannot reach
        // the queue from its destructor while it is borrowed.
        let pending = std::mem::take(&mut *self.pending.borrow_mut());
        drop(pending);
    }
}

#[cfg(any(
    target_os = "android",
    target_os = "ios",
    target_os = "macos",
    target_arch = "wasm32",
    test
))]
/// A request of a host that mirrors one field to the platform (Android's
/// view, the browser's surface).
pub(crate) enum MirrorHostOp {
    /// Focus this store, or none.
    Focus(Option<Rc<dyn TextStore>>),
    /// End the composition in this store, the one focused when the request
    /// was made.
    Complete(Rc<dyn TextStore>),
}

#[cfg(any(
    target_os = "android",
    target_os = "ios",
    target_os = "macos",
    target_arch = "wasm32",
    test
))]
impl HostQueue<MirrorHostOp> {
    /// The store focused once the queue has run: the newest queued focus
    /// change's, else `current`'s.
    pub(crate) fn focus_after_queue(
        &self,
        current: impl FnOnce() -> Option<Rc<dyn TextStore>>,
    ) -> Option<Rc<dyn TextStore>> {
        self.newest(|op| match op {
            MirrorHostOp::Focus(store) => Some(store.clone()),
            MirrorHostOp::Complete(_) => None,
        })
        .unwrap_or_else(current)
    }
}
