//! The windows the host has handed the server.
//!
//! Locked only to insert, prune, or clone a handle out; never across a wait
//! for the owner's answer.

use std::collections::{BTreeMap, BTreeSet};

use flui_protocol::WindowId;
use flui_sdk::view::dev_agent::AgentWindow;
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
    /// Ids seen closed, so a request naming one answers `gone` rather than
    /// `unknown_handle`. Grows by one per window the application opened.
    closed: BTreeSet<WindowId>,
}

impl Registry {
    pub(super) fn insert(&self, window: AgentWindow) {
        let mut inner = self.inner.lock();
        inner.closed.remove(&window.id());
        inner.open.insert(window.id(), window);
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
        for id in closed {
            inner.open.remove(&id);
            inner.closed.insert(id);
        }
        inner.open.keys().copied().collect()
    }

    /// Let go of every window.
    pub(super) fn clear(&self) {
        let mut inner = self.inner.lock();
        inner.open.clear();
        inner.closed.clear();
    }
}
