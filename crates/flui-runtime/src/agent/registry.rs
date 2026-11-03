//! The windows the host handed the service, and the conversations to wake when one opens.
//!
//! Locked only to insert, prune, look up or clone a handle out. Nothing that is not this
//! module's runs under the lock: a window let go and a waker woken are dropped and woken after
//! it is released.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use flui_protocol::WindowId;
use flui_view::__runtime::WakeSlot;
use flui_view::dev_agent::AgentWindow;
use parking_lot::Mutex;

/// What the registry knows of a window id.
pub(super) enum Lookup {
    /// Handed over and not yet seen closed.
    Open(AgentWindow),
    /// Handed over, and closed since.
    Closed,
    /// Never handed over.
    Unknown,
}

#[derive(Default)]
pub(super) struct Registry {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    open: BTreeMap<WindowId, AgentWindow>,
    /// Ids seen closed, so a request naming one answers `gone` rather than `unknown_handle`.
    /// Grows by one per window the application opened.
    closed: BTreeSet<WindowId>,
    /// Each live conversation's wake, by conversation.
    watchers: BTreeMap<u64, Arc<WakeSlot>>,
}

impl Registry {
    /// Serve `window`. The wakes of the conversations to tell, for the caller to wake once
    /// this returns.
    #[must_use]
    pub(super) fn insert(&self, window: AgentWindow) -> Vec<Arc<WakeSlot>> {
        let mut inner = self.inner.lock();
        inner.closed.remove(&window.id());
        let displaced = inner.open.insert(window.id(), window);
        let watchers = inner.watchers.values().map(Arc::clone).collect();
        drop(inner);
        drop(displaced);
        watchers
    }

    pub(super) fn lookup(&self, id: WindowId) -> Lookup {
        let inner = self.inner.lock();
        match inner.open.get(&id) {
            Some(window) => Lookup::Open(window.clone()),
            None if inner.closed.contains(&id) => Lookup::Closed,
            None => Lookup::Unknown,
        }
    }

    /// The open windows, in id order; the closed ones move out.
    pub(super) fn list(&self) -> Vec<WindowId> {
        let mut inner = self.inner.lock();
        let closed: Vec<WindowId> = inner
            .open
            .iter()
            .filter(|(_, window)| !window.is_open())
            .map(|(id, _)| *id)
            .collect();
        let mut gone = Vec::with_capacity(closed.len());
        for id in closed {
            gone.extend(inner.open.remove(&id));
            inner.closed.insert(id);
        }
        let open = inner.open.keys().copied().collect();
        drop(inner);
        drop(gone);
        open
    }

    /// Wake `wake` whenever a window is handed over, until [`Self::unwatch`].
    pub(super) fn watch(&self, conversation: u64, wake: Arc<WakeSlot>) {
        self.inner.lock().watchers.insert(conversation, wake);
    }

    pub(super) fn unwatch(&self, conversation: u64) {
        let removed = self.inner.lock().watchers.remove(&conversation);
        drop(removed);
    }

    /// Let go of every window. The wakes of the live conversations, for the caller to wake
    /// once this returns.
    #[must_use]
    pub(super) fn clear(&self) -> Vec<Arc<WakeSlot>> {
        let mut inner = self.inner.lock();
        let open = std::mem::take(&mut inner.open);
        inner.closed.clear();
        let watchers = inner.watchers.values().map(Arc::clone).collect();
        drop(inner);
        drop(open);
        watchers
    }
}

/// Wakes each of `wakes`. A waker that panics is contained by its slot.
pub(super) fn wake_all(wakes: Vec<Arc<WakeSlot>>) {
    for wake in wakes {
        wake.wake();
    }
}
