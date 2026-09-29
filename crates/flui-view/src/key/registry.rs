//! Owner-thread scoped handle that `GlobalKey::current_element` /
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
//! mutable aliasing), so flui introduced [`ElementOwner`](crate::ElementOwner)
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
//! registry captures one binding's owner state. The active handle is selected
//! by the [`UiRealm`](../../../flui-runtime/src/ui_realm/mod.rs) entry scope.
//!
//! Activation is thread-local and stack-shaped. Nested realm entry restores
//! the previous handle, including during panic unwinding. A lookup clones the
//! active handle and releases the TLS `RefCell` borrow before invoking either
//! framework or user code.
//!
//! # Re-entrancy
//!
//! A binding holds its own state lock for the whole of a frame, an attach, a
//! detach and a layout-builder build, and runs user code (build, lifecycle
//! hooks, dispose) inside it. A lookup made from that code must not wait on
//! that lock: it would wait on its own thread forever. So a member's closures
//! never block. A member whose lock is already held reports [`RegistryBusy`],
//! the composite moves on to its next member, and only when no member
//! answered does the busy report reach `GlobalKey`, which resolves it to
//! nothing. Bindings are `!Send`, so a held lock can only mean re-entry on
//! the owner thread, never a race with another thread.

use std::{cell::RefCell, mem::ManuallyDrop, sync::Arc};

use crate::view::ElementBase;
use flui_foundation::{ElementId, ViewKey};

// `build_composite` (below) is the sole user of `HashMap`/`Rc` — both go
// unused (and therefore unused-import-warn, `-D warnings`-fail under CI's
// feature-matrix `cargo-hack` sweep) in a build with neither `test` nor
// `runtime-internals` active, e.g. `flui-widgets`'s own `--features images`
// test build, which pulls in `flui-view` with its default feature set only.
// Gated identically to the function that needs them, not a blanket
// `#[allow(unused_imports)]`.
#[cfg(any(test, feature = "runtime-internals"))]
use std::{collections::HashMap, rc::Rc};

/// Snapshot of the framework's global-key lookup surface that
/// `GlobalKey::current_element` / `with_current_state` consult.
///
/// Held by one [`WidgetsBinding`](crate::WidgetsBinding) and activated only
/// while its owning realm is entered.
///
/// The struct is `Clone` so internal copies stay cheap — both
/// invariants funnel through the same `Arc`-shared closure pair.
#[derive(Clone)]
pub(crate) struct GlobalKeyRegistryHandle {
    inner: Arc<GlobalKeyRegistryInner>,
}

/// A registry member could not be read because its owner is inside a frame,
/// attach, detach or layout-builder build on this same thread (see the
/// module's "Re-entrancy" section).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RegistryBusy;

/// Lookup closure type — resolve a `GlobalKey` back to an `ElementId`.
/// Returns `Ok(None)` when no element with that key is currently mounted,
/// and `Err(RegistryBusy)` when the member cannot be read without blocking.
///
/// Takes the key itself, not its hash: the registries behind this closure
/// index by `ViewKey::key_hash` but decide by `ViewKey::key_eq`, so passing
/// only a hash would make two distinct keys that collide indistinguishable
/// at exactly the boundary a caller reaches through `GlobalKey::current_*`.
type LookupFn = dyn Fn(&dyn ViewKey) -> Result<Option<ElementId>, RegistryBusy>;

/// Visit closure type — call the inner `FnMut` once with the
/// `&dyn ElementBase` at the given id, or report `Err(RegistryBusy)` without
/// calling it. Type-erased here because trait objects can't carry per-call
/// generics; the result-extraction shim for
/// [`GlobalKeyRegistryHandle::with_element`]'s generic `R` return lives in
/// the inner `FnMut`.
type VisitFn = dyn Fn(ElementId, &mut dyn FnMut(&dyn ElementBase)) -> Result<(), RegistryBusy>;

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
    /// `lookup` resolves a key to `Option<ElementId>`. `visit` calls
    /// the inner `FnMut` once with the `&dyn ElementBase` at the given
    /// id; if no element exists at the id, the inner `FnMut` is simply
    /// not called and `with_element` returns `Ok(None)`. Either closure
    /// returns `Err(RegistryBusy)` instead of blocking on a lock its own
    /// thread already holds.
    pub(crate) fn new<L, V>(lookup: L, visit: V) -> Self
    where
        L: Fn(&dyn ViewKey) -> Result<Option<ElementId>, RegistryBusy> + 'static,
        V: Fn(ElementId, &mut dyn FnMut(&dyn ElementBase)) -> Result<(), RegistryBusy> + 'static,
    {
        Self {
            inner: Arc::new(GlobalKeyRegistryInner {
                lookup: Box::new(lookup),
                visit: Box::new(visit),
            }),
        }
    }

    /// Resolve a `GlobalKey` back to the `ElementId` currently holding it.
    ///
    /// # Errors
    ///
    /// [`RegistryBusy`] when the owning binding's lock is held by this thread.
    pub(crate) fn lookup_element(
        &self,
        key: &dyn ViewKey,
    ) -> Result<Option<ElementId>, RegistryBusy> {
        (self.inner.lookup)(key)
    }

    /// Apply `f` to the `&dyn ElementBase` at the given id, returning
    /// the closure's result. Returns `Ok(None)` when the id is no longer
    /// present in the tree.
    ///
    /// # Errors
    ///
    /// [`RegistryBusy`] when the owning binding's lock is held by this thread;
    /// `f` is not called.
    pub(crate) fn with_element<R>(
        &self,
        id: ElementId,
        f: impl FnOnce(&dyn ElementBase) -> R,
    ) -> Result<Option<R>, RegistryBusy> {
        let mut result = None;
        let mut f_opt = Some(f);
        (self.inner.visit)(id, &mut |elem: &dyn ElementBase| {
            if let Some(f) = f_opt.take() {
                result = Some(f(elem));
            }
        })?;
        Ok(result)
    }
}

/// Build one composite handle spanning `members`, consulted in the given
/// order (ADR-0043 §1's realm composite, over per-presentation
/// `WidgetsBinding` registries).
///
/// `GlobalKeyScope`'s uniqueness invariant guarantees at most one member ever
/// answers a given key, so `lookup` trying each member in turn and
/// returning the first hit is exact, not a heuristic. `with_element` cannot
/// re-derive that same answer from an `ElementId` alone — two members' trees
/// may validly reuse the same raw id for unrelated elements — so `lookup`
/// additionally remembers, per id, which member resolved it; `with_element`
/// consults that memory first and only falls back to a try-in-order scan for
/// an id nothing has looked up yet (no production caller does this today —
/// `GlobalKey::with_current_state` always calls `current_element` first —
/// but the fallback keeps the composite correct-by-construction rather than
/// correct-by-caller-discipline). The memory is a plain cache: an entry for
/// an id that has since unmounted is simply a miss on lookup (the owning
/// member's own `with_element` already returns `None` for a gone id), never
/// a dangling reference.
///
/// A busy member (see the module's "Re-entrancy" section) is a miss for that
/// member only: the others are still tried, so a key held by a sibling
/// presentation resolves while this presentation's own frame is running. The
/// composite reports busy only when no member answered and at least one was
/// busy. A visit routed by the cache to a busy member reports busy at once
/// rather than scanning the others, which could reach an unrelated element
/// that reuses the same raw id.
#[cfg(any(test, feature = "runtime-internals"))]
pub(crate) fn build_composite(members: Vec<GlobalKeyRegistryHandle>) -> GlobalKeyRegistryHandle {
    let resolved_by: Rc<RefCell<HashMap<ElementId, usize>>> = Rc::new(RefCell::new(HashMap::new()));
    let lookup_members = members.clone();
    let lookup_cache = Rc::clone(&resolved_by);
    let visit_members = members;

    GlobalKeyRegistryHandle::new(
        move |key| {
            let mut busy = false;
            for (index, member) in lookup_members.iter().enumerate() {
                match member.lookup_element(key) {
                    Ok(Some(id)) => {
                        lookup_cache.borrow_mut().insert(id, index);
                        return Ok(Some(id));
                    }
                    Ok(None) => {}
                    Err(RegistryBusy) => busy = true,
                }
            }
            if busy { Err(RegistryBusy) } else { Ok(None) }
        },
        move |id, f| {
            let cached_index = resolved_by.borrow().get(&id).copied();
            if let Some(index) = cached_index
                && let Some(member) = visit_members.get(index)
                && member.with_element(id, |el| f(el))?.is_some()
            {
                return Ok(());
            }
            let mut busy = false;
            for member in &visit_members {
                match member.with_element(id, |el| f(el)) {
                    Ok(Some(())) => return Ok(()),
                    Ok(None) => {}
                    Err(RegistryBusy) => busy = true,
                }
            }
            if busy { Err(RegistryBusy) } else { Ok(()) }
        },
    )
}

type RegistryStack = RefCell<Vec<GlobalKeyRegistryHandle>>;
type TestRegistrySlot = RefCell<Option<GlobalKeyRegistryHandle>>;
type DropFreeRegistryStack = ManuallyDrop<RegistryStack>;
type DropFreeTestRegistrySlot = ManuallyDrop<TestRegistrySlot>;

// These thread-locals are instantiated inside hot-reloadable cdylibs. A TLS
// destructor owned by such an image can make dlclose defer unmapping it, so a
// same-path reload silently serves stale code. ManuallyDrop prevents destructor
// registration; the scoped activation and explicit fixture take paths remain
// responsible for returning the payloads to their empty quiescent states.
const _: () = assert!(!std::mem::needs_drop::<DropFreeRegistryStack>());
const _: () = assert!(!std::mem::needs_drop::<DropFreeTestRegistrySlot>());

thread_local! {
    /// Active registry stack for this owner thread. A stack, rather than a
    /// replaceable singleton, makes nested realm entry restore correctly.
    ///
    /// `ManuallyDrop` is required because this module can be instantiated in a
    /// hot-reload cdylib. `RegistryActivation` empties the stack explicitly.
    static REGISTRY_STACK: DropFreeRegistryStack = const {
        ManuallyDrop::new(RefCell::new(Vec::new()))
    };
    /// Legacy fixture lane. It never mutates the production activation stack.
    /// `take_registry` is its explicit teardown path.
    static TEST_REGISTRY: DropFreeTestRegistrySlot = const {
        ManuallyDrop::new(RefCell::new(None))
    };
}

/// RAII activation token. Private so only the binding's scoped entry method
/// can manipulate the ambient registry.
#[cfg(any(test, feature = "runtime-internals"))]
struct RegistryActivation {
    expected: GlobalKeyRegistryHandle,
}

#[cfg(any(test, feature = "runtime-internals"))]
impl Drop for RegistryActivation {
    fn drop(&mut self) {
        REGISTRY_STACK.with(|stack| {
            let mut stack = stack.borrow_mut();
            let Some(popped) = stack.pop() else {
                tracing::error!("GlobalKey registry activation stack underflow");
                return;
            };
            if !Arc::ptr_eq(&popped.inner, &self.expected.inner) {
                // Never panic from Drop: a second panic during user-code unwind
                // aborts the process. Preserve the unexpected top for diagnosis.
                stack.push(popped);
                tracing::error!("GlobalKey registry scopes dropped out of order");
            }
        });
    }
}

#[cfg(any(test, feature = "runtime-internals"))]
fn activate_registry(handle: GlobalKeyRegistryHandle) -> RegistryActivation {
    REGISTRY_STACK.with(|stack| stack.borrow_mut().push(handle.clone()));
    RegistryActivation { expected: handle }
}

/// Activate `handle` for the dynamic extent of `f`.
#[cfg(any(test, feature = "runtime-internals"))]
pub(crate) fn with_active_registry<R>(
    handle: &GlobalKeyRegistryHandle,
    f: impl FnOnce() -> R,
) -> R {
    let _activation = activate_registry(handle.clone());
    f()
}

/// Legacy test-fixture adapter: replace the top handle on this thread and
/// return the previous one. Production uses [`with_active_registry`].
pub(crate) fn install_registry(handle: GlobalKeyRegistryHandle) -> Option<GlobalKeyRegistryHandle> {
    TEST_REGISTRY.with(|slot| slot.borrow_mut().replace(handle))
}

/// Legacy test-fixture adapter: remove the active handle on this thread.
pub(crate) fn take_registry() -> Option<GlobalKeyRegistryHandle> {
    TEST_REGISTRY.with(|slot| slot.borrow_mut().take())
}

/// Run `f` against the currently-active realm handle (or isolated legacy
/// fixture lane), returning the closure's result. Returns `None` when neither
/// lane is active
/// (the quiescent state — e.g. unit tests that bypass the binding).
pub(crate) fn with_registry<R>(f: impl FnOnce(&GlobalKeyRegistryHandle) -> R) -> Option<R> {
    let handle = REGISTRY_STACK.with(|stack| stack.borrow().last().cloned());
    let handle = handle.or_else(|| TEST_REGISTRY.with(|slot| slot.borrow().clone()));
    handle.as_ref().map(f)
}
