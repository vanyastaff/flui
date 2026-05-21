//! Process-wide handle that `GlobalKey::current_element` /
//! `GlobalKey::with_current_state` read from to resolve a key hash back
//! to the live element + state.
//!
//! # Why this shape
//!
//! Flutter stores `_globalKeyRegistry` directly on `Element._owner`
//! (`framework.dart:3148`) — a `Map<GlobalKey, Element>` carried inside
//! the active `BuildOwner`. Element lifecycle paths reach the map via
//! the element's mutable backreference to its owner. Rust can't take a
//! mutable backreference of that shape (the borrow-checker forbids
//! mutable aliasing), so U8 introduced [`ElementOwner`](crate::ElementOwner)
//! as the split-borrow handle used DURING `mount`/`unmount`. That handle
//! is fine for register/unregister at the lifecycle boundary, but it
//! does NOT solve the OTHER side of the registry: external callers
//! (`GlobalKey::current_element`, `with_current_state`) need to look up
//! a key hash WITHOUT having an owner reference in scope.
//!
//! The pragmatic Rust answer is a process-wide handle protected by a
//! lock. We don't host one registry per `WidgetsBinding` instance
//! because:
//!
//! 1. The application always has exactly one active `WidgetsBinding`
//!    (`impl_binding_singleton!`). Multiple bindings in a single process
//!    are explicitly an anti-pattern in Flutter's docs as well.
//! 2. Tests that spin up local `BuildOwner` + `ElementTree` (the
//!    `ancestor_finders.rs` style) install the handle via
//!    [`crate::test_only_set_global_key_registry`] and clear it after.
//!
//! The handle is a pair of `Arc<RwLock<_>>` references. `current_element`
//! and `with_current_state` acquire a brief read-lock — the lookup
//! lifetime never escapes the closure callback.
//!
//! # Thread-safety
//!
//! The handle is wrapped in `parking_lot::RwLock<Option<GlobalKeyRegistryHandle>>`.
//! Install / take operations are serialized; concurrent reads through
//! `with_registry` are fine.

use std::sync::Arc;

use flui_foundation::ElementId;
use parking_lot::RwLock;

use crate::{owner::BuildOwner, tree::ElementTree, view::ElementBase};

/// Snapshot of the framework's element tree + build owner that
/// `GlobalKey` lookups consult.
///
/// Held inside the module-private `REGISTRY` slot. `WidgetsBinding`
/// installs one of these on construction and clears it on drop /
/// detach; tests use [`crate::test_only_set_global_key_registry`].
#[derive(Clone, Debug)]
pub struct GlobalKeyRegistryHandle {
    pub(crate) tree: Arc<RwLock<ElementTree>>,
    pub(crate) owner: Arc<RwLock<BuildOwner>>,
}

impl GlobalKeyRegistryHandle {
    /// Resolve a key hash back to the `ElementId` currently holding it.
    ///
    /// Acquires a brief read-lock on the build owner. Returns `None`
    /// when no element with that hash is currently mounted.
    pub fn lookup_element(&self, key_hash: u64) -> Option<ElementId> {
        self.owner.read().element_for_global_key(key_hash)
    }

    /// Apply `f` to the `&dyn ElementBase` at the given id, returning
    /// the closure's result. Returns `None` when the id is no longer
    /// present in the tree (e.g. the element was finalized between the
    /// `lookup_element` resolution and the second read-lock).
    ///
    /// Acquires a separate read-lock on the element tree. The lookup
    /// is two-phase (resolve id, then re-borrow for the callback) so
    /// the build-owner read-lock drops before the tree read-lock fires
    /// — mirrors the two-phase pattern in
    /// [`crate::context::ElementBuildContext::find_root_ancestor_state`]
    /// (plan §U11).
    pub fn with_element<R>(
        &self,
        id: ElementId,
        f: impl FnOnce(&dyn ElementBase) -> R,
    ) -> Option<R> {
        let tree = self.tree.read();
        let node = tree.get(id)?;
        Some(f(node.element()))
    }
}

/// Module-private singleton slot. Installed by `WidgetsBinding::new`
/// (production) or `test_only_set_global_key_registry` (tests).
///
/// `parking_lot::RwLock` because we expect lookups to outnumber
/// install/take by orders of magnitude.
static REGISTRY: RwLock<Option<GlobalKeyRegistryHandle>> = RwLock::new(None);

/// Install a new global-key registry handle. Returns the previous
/// handle (or `None` if no handle was installed). The previous handle,
/// if returned, can be re-installed by the caller later — useful for
/// nesting in tests, although the standard pattern is install-then-
/// clear via the public test_only_* shims in `crate::lib`.
pub fn install_registry(handle: GlobalKeyRegistryHandle) -> Option<GlobalKeyRegistryHandle> {
    let mut slot = REGISTRY.write();
    slot.replace(handle)
}

/// Remove the currently-installed handle. Returns the previous handle.
pub fn take_registry() -> Option<GlobalKeyRegistryHandle> {
    REGISTRY.write().take()
}

/// Run `f` against the currently-installed handle, returning the
/// closure's result. Returns `None` when no handle is installed
/// (the quiescent state — e.g. unit tests that bypass the binding).
pub(crate) fn with_registry<R>(f: impl FnOnce(&GlobalKeyRegistryHandle) -> R) -> Option<R> {
    REGISTRY.read().as_ref().map(f)
}
