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
//! by the [`UiRealm`](../../../flui-app/src/app/ui_realm.rs) entry scope.
//!
//! Activation is thread-local and stack-shaped. Nested realm entry restores
//! the previous handle, including during panic unwinding. A lookup clones the
//! active handle and releases the TLS `RefCell` borrow before invoking either
//! framework or user code.

use std::{cell::RefCell, sync::Arc};

use crate::view::ElementBase;
use flui_foundation::ElementId;

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

/// Lookup closure type — resolve a key hash back to an `ElementId`.
/// Returns `None` when no element with that hash is currently mounted.
type LookupFn = dyn Fn(u64) -> Option<ElementId>;

/// Visit closure type — call the inner `FnMut` once with the
/// `&dyn ElementBase` at the given id. Type-erased here because trait
/// objects can't carry per-call generics; the result-extraction shim
/// for [`GlobalKeyRegistryHandle::with_element`]'s generic `R` return
/// lives in the inner `FnMut`.
type VisitFn = dyn Fn(ElementId, &mut dyn FnMut(&dyn ElementBase));

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
        L: Fn(u64) -> Option<ElementId> + 'static,
        V: Fn(ElementId, &mut dyn FnMut(&dyn ElementBase)) + 'static,
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

thread_local! {
    /// Active registry stack for this owner thread. A stack, rather than a
    /// replaceable singleton, makes nested realm entry restore correctly.
    static REGISTRY_STACK: RefCell<Vec<GlobalKeyRegistryHandle>> = const { RefCell::new(Vec::new()) };
    /// Legacy fixture lane. It never mutates the production activation stack.
    static TEST_REGISTRY: RefCell<Option<GlobalKeyRegistryHandle>> = const { RefCell::new(None) };
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

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::*;

    fn handle(value: usize) -> GlobalKeyRegistryHandle {
        GlobalKeyRegistryHandle::new(move |_| Some(ElementId::new(value + 1)), |_, _| {})
    }

    fn current() -> Option<ElementId> {
        with_registry(|registry| registry.lookup_element(0)).flatten()
    }

    #[test]
    fn no_active_registry_is_none() {
        assert_eq!(current(), None);
    }

    #[test]
    fn nested_activation_restores_previous_registry() {
        let a = handle(1);
        let b = handle(2);
        with_active_registry(&a, || {
            assert_eq!(current(), Some(ElementId::new(2)));
            with_active_registry(&b, || assert_eq!(current(), Some(ElementId::new(3))));
            assert_eq!(current(), Some(ElementId::new(2)));
        });
        assert_eq!(current(), None);
    }

    #[test]
    fn panic_unwind_restores_previous_registry() {
        let a = handle(3);
        let b = handle(4);
        with_active_registry(&a, || {
            let result = catch_unwind(AssertUnwindSafe(|| {
                with_active_registry(&b, || panic!("test panic"));
            }));
            assert!(result.is_err());
            assert_eq!(current(), Some(ElementId::new(4)));
        });
        assert_eq!(current(), None);
    }

    #[test]
    fn lookup_releases_tls_borrow_before_nested_activation() {
        let a = handle(5);
        let b = handle(6);
        with_active_registry(&a, || {
            let observed = with_registry(|registry| {
                assert_eq!(registry.lookup_element(0), Some(ElementId::new(6)));
                with_active_registry(&b, current)
            });
            assert_eq!(observed.flatten(), Some(ElementId::new(7)));
        });
    }

    #[test]
    fn fixture_adapter_cannot_replace_active_realm_and_survives_unwind() {
        let fixture = handle(7);
        let realm = handle(8);
        let _ = install_registry(fixture);

        let result = catch_unwind(AssertUnwindSafe(|| {
            with_active_registry(&realm, || {
                assert_eq!(current(), Some(ElementId::new(9)));
                let _ = install_registry(handle(10));
                assert_eq!(
                    current(),
                    Some(ElementId::new(9)),
                    "fixture lane must not mutate the active realm stack"
                );
                panic!("test unwind");
            });
        }));
        assert!(result.is_err());
        assert_eq!(current(), Some(ElementId::new(11)));
        let _ = take_registry();
        assert_eq!(current(), None);
    }
}
