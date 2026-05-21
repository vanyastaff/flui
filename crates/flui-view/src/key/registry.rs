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
//! # Decoupling shape
//!
//! The registry handle is a pair of type-erased closures rather than
//! direct `Arc<RwLock<ElementTree>>` / `Arc<RwLock<BuildOwner>>` field
//! references. That keeps the framework's storage layout free —
//! `WidgetsBinding` continues to own its `BuildOwner` and `ElementTree`
//! inline behind a single `RwLock<WidgetsBindingInner>` — and the
//! registry just captures `WidgetsBinding::instance()` inside its
//! closures so reads go through whatever shape the binding uses. Tests
//! install a different pair of closures pointing at mock state.
//!
//! # Thread-safety
//!
//! The handle slot is wrapped in `parking_lot::RwLock<Option<GlobalKeyRegistryHandle>>`.
//! Install / take operations are serialized; concurrent reads through
//! `with_registry` are fine. The closures themselves must be
//! `Fn + Send + Sync` so they can be called from any thread.

use std::sync::Arc;

use flui_foundation::ElementId;
use parking_lot::RwLock;

use crate::view::ElementBase;

/// Snapshot of the framework's global-key lookup surface that
/// `GlobalKey::current_element` / `with_current_state` consult.
///
/// Held inside the module-private `REGISTRY` slot.
/// [`WidgetsBinding::new`](crate::WidgetsBinding::new) installs one of
/// these on construction (the binding's drop also clears it); tests use
/// [`crate::test_only_set_global_key_registry`].
///
/// The struct is `Clone` so internal copies stay cheap — both
/// invariants funnel through the same `Arc`-shared closure pair.
#[derive(Clone)]
pub(crate) struct GlobalKeyRegistryHandle {
    inner: Arc<GlobalKeyRegistryInner>,
}

/// Lookup closure type — resolve a key hash back to an `ElementId`.
/// Returns `None` when no element with that hash is currently mounted.
type LookupFn = dyn Fn(u64) -> Option<ElementId> + Send + Sync;

/// Visit closure type — call the inner `FnMut` once with the
/// `&dyn ElementBase` at the given id. Type-erased here because trait
/// objects can't carry per-call generics; the result-extraction shim
/// for [`GlobalKeyRegistryHandle::with_element`]'s generic `R` return
/// lives in the inner `FnMut`.
type VisitFn = dyn Fn(ElementId, &mut dyn FnMut(&dyn ElementBase)) + Send + Sync;

struct GlobalKeyRegistryInner {
    lookup: Box<LookupFn>,
    visit: Box<VisitFn>,
}

impl std::fmt::Debug for GlobalKeyRegistryHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlobalKeyRegistryHandle").finish()
    }
}

impl GlobalKeyRegistryHandle {
    /// Build a handle from two closures.
    ///
    /// `lookup` resolves `key_hash` → `Option<ElementId>`. `visit` calls
    /// the inner `FnMut` once with the `&dyn ElementBase` at the given
    /// id; if no element exists at the id, the inner `FnMut` is simply
    /// not called and `with_element` returns `None`.
    pub(crate) fn new<L, V>(lookup: L, visit: V) -> Self
    where
        L: Fn(u64) -> Option<ElementId> + Send + Sync + 'static,
        V: Fn(ElementId, &mut dyn FnMut(&dyn ElementBase)) + Send + Sync + 'static,
    {
        Self {
            inner: Arc::new(GlobalKeyRegistryInner {
                lookup: Box::new(lookup),
                visit: Box::new(visit),
            }),
        }
    }

    /// Resolve a key hash back to the `ElementId` currently holding it.
    pub(crate) fn lookup_element(&self, key_hash: u64) -> Option<ElementId> {
        (self.inner.lookup)(key_hash)
    }

    /// Apply `f` to the `&dyn ElementBase` at the given id, returning
    /// the closure's result. Returns `None` when the id is no longer
    /// present in the tree.
    pub(crate) fn with_element<R>(
        &self,
        id: ElementId,
        f: impl FnOnce(&dyn ElementBase) -> R,
    ) -> Option<R> {
        let mut result = None;
        let mut f_opt = Some(f);
        (self.inner.visit)(id, &mut |elem: &dyn ElementBase| {
            if let Some(f) = f_opt.take() {
                result = Some(f(elem));
            }
        });
        result
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
pub(crate) fn install_registry(handle: GlobalKeyRegistryHandle) -> Option<GlobalKeyRegistryHandle> {
    let mut slot = REGISTRY.write();
    slot.replace(handle)
}

/// Remove the currently-installed handle. Returns the previous handle.
pub(crate) fn take_registry() -> Option<GlobalKeyRegistryHandle> {
    REGISTRY.write().take()
}

/// Run `f` against the currently-installed handle, returning the
/// closure's result. Returns `None` when no handle is installed
/// (the quiescent state — e.g. unit tests that bypass the binding).
pub(crate) fn with_registry<R>(f: impl FnOnce(&GlobalKeyRegistryHandle) -> R) -> Option<R> {
    REGISTRY.read().as_ref().map(f)
}
