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
//! by the [`UiRealm`](../../../flui-app/src/app/ui_realm/mod.rs) entry scope.
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

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::*;

    /// A stand-in `GlobalKey` whose identity and hash are both the numeral
    /// it was built from, so a test can name a key as compactly as it used
    /// to name a hash while still going through the identity path.
    #[derive(Clone, Copy, PartialEq, Eq)]
    struct TestKey(u64);

    impl ViewKey for TestKey {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn key_eq(&self, other: &dyn ViewKey) -> bool {
            other
                .as_any()
                .downcast_ref::<Self>()
                .is_some_and(|other| self.0 == other.0)
        }

        fn key_hash(&self) -> u64 {
            self.0
        }

        fn clone_key(&self) -> Box<dyn ViewKey> {
            Box::new(*self)
        }

        fn debug_fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "TestKey({})", self.0)
        }

        fn is_global_key(&self) -> bool {
            true
        }
    }

    fn handle(value: usize) -> GlobalKeyRegistryHandle {
        GlobalKeyRegistryHandle::new(move |_| Ok(Some(ElementId::new(value + 1))), |_, _| Ok(()))
    }

    fn current() -> Option<ElementId> {
        with_registry(|registry| registry.lookup_element(&TestKey(0)))
            .and_then(|result| result.expect("test handles are never busy"))
    }

    #[test]
    fn no_active_registry_is_none() {
        assert_eq!(current(), None);
    }

    #[test]
    fn tls_storage_has_no_drop_glue() {
        assert!(!std::mem::needs_drop::<DropFreeRegistryStack>());
        assert!(!std::mem::needs_drop::<DropFreeTestRegistrySlot>());
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
                assert_eq!(
                    registry.lookup_element(&TestKey(0)),
                    Ok(Some(ElementId::new(6)))
                );
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

    // ========================================================================
    // `build_composite` — the realm composite (ADR-0043 §1)
    // ========================================================================

    /// A member whose lookup/visit are driven by a plain `HashMap` the test
    /// controls directly, so a composite test can construct exact,
    /// deliberately colliding scenarios (same `ElementId` numeral valid in
    /// two different members) rather than depending on a real element tree.
    fn member(entries: Vec<(u64, ElementId, &'static str)>) -> GlobalKeyRegistryHandle {
        let by_hash: HashMap<u64, ElementId> =
            entries.iter().map(|(hash, id, _)| (*hash, *id)).collect();
        let by_id: HashMap<ElementId, &'static str> = entries
            .into_iter()
            .map(|(_, id, label)| (id, label))
            .collect();
        GlobalKeyRegistryHandle::new(
            move |key| Ok(by_hash.get(&key.key_hash()).copied()),
            move |id, f| {
                if let Some(label) = by_id.get(&id) {
                    f(&LabeledElement(label));
                }
                Ok(())
            },
        )
    }

    /// A member whose owner is mid-frame on this thread: every read is busy.
    fn busy_member() -> GlobalKeyRegistryHandle {
        GlobalKeyRegistryHandle::new(|_| Err(RegistryBusy), |_, _| Err(RegistryBusy))
    }

    /// Minimal `ElementBase` test double: every lifecycle/build method is
    /// `unreachable!()` because this composite test never drives lifecycle —
    /// it only exercises `with_element`'s visit path, which reads
    /// `state_as_any` and nothing else.
    struct LabeledElement(&'static str);

    impl ElementBase for LabeledElement {
        fn view_type_id(&self) -> std::any::TypeId {
            std::any::TypeId::of::<Self>()
        }

        fn depth(&self) -> usize {
            0
        }

        fn lifecycle(&self) -> crate::element::Lifecycle {
            crate::element::Lifecycle::Active
        }

        fn mount(
            &mut self,
            _parent: Option<ElementId>,
            _slot: usize,
            _owner: &mut crate::ElementOwner<'_>,
        ) {
            unreachable!("test double: mount is never exercised by this composite test")
        }

        fn unmount(&mut self, _owner: &mut crate::ElementOwner<'_>) {
            unreachable!("test double: unmount is never exercised by this composite test")
        }

        fn activate(&mut self, _owner: &mut crate::ElementOwner<'_>) {}

        fn deactivate(&mut self, _owner: &mut crate::ElementOwner<'_>) {}

        fn update(
            &mut self,
            _new_view: &dyn crate::view::View,
            _owner: &mut crate::ElementOwner<'_>,
        ) {
            unreachable!("test double: update is never exercised by this composite test")
        }

        fn mark_needs_build(&mut self) {}

        fn build_into_views(
            &mut self,
            _owner: &mut crate::ElementOwner<'_>,
        ) -> Vec<Box<dyn crate::view::View>> {
            Vec::new()
        }

        fn state_as_any(&self) -> Option<&dyn std::any::Any> {
            Some(&self.0)
        }
    }

    fn try_visited_label(
        registry: &GlobalKeyRegistryHandle,
        id: ElementId,
    ) -> Result<Option<&'static str>, RegistryBusy> {
        registry.with_element(id, |el| {
            *el.state_as_any()
                .expect("LabeledElement always has state")
                .downcast_ref::<&'static str>()
                .expect("LabeledElement's state is always &str")
        })
    }

    fn visited_label(registry: &GlobalKeyRegistryHandle, id: ElementId) -> Option<&'static str> {
        try_visited_label(registry, id).expect("no member of this composite is busy")
    }

    #[test]
    fn composite_lookup_tries_members_in_order_and_returns_the_first_hit() {
        let a = member(vec![(1, ElementId::new(5), "a")]);
        let b = member(vec![(2, ElementId::new(5), "b")]);
        let composite = build_composite(vec![a, b]);

        assert_eq!(
            composite.lookup_element(&TestKey(1)),
            Ok(Some(ElementId::new(5)))
        );
        assert_eq!(
            composite.lookup_element(&TestKey(2)),
            Ok(Some(ElementId::new(5)))
        );
        assert_eq!(composite.lookup_element(&TestKey(3)), Ok(None));
    }

    #[test]
    fn composite_skips_a_busy_member_and_resolves_through_the_next() {
        let composite = build_composite(vec![
            busy_member(),
            member(vec![(1, ElementId::new(5), "b")]),
        ]);

        assert_eq!(
            composite.lookup_element(&TestKey(1)),
            Ok(Some(ElementId::new(5)))
        );
        assert_eq!(visited_label(&composite, ElementId::new(5)), Some("b"));
    }

    #[test]
    fn composite_reports_busy_only_when_no_member_answered() {
        let composite = build_composite(vec![
            busy_member(),
            member(vec![(1, ElementId::new(5), "b")]),
        ]);

        assert_eq!(composite.lookup_element(&TestKey(2)), Err(RegistryBusy));
        assert_eq!(
            try_visited_label(&composite, ElementId::new(6)),
            Err(RegistryBusy)
        );
        assert_eq!(
            build_composite(vec![member(vec![])]).lookup_element(&TestKey(2)),
            Ok(None),
            "a miss with no busy member stays a plain miss"
        );
    }

    /// Once a lookup has routed an id to one member, a busy visit of that
    /// member must not fall back to a sibling that reuses the raw id.
    #[test]
    fn composite_visit_routed_to_a_busy_member_does_not_scan_the_others() {
        let busy_after_lookup = Rc::new(std::cell::Cell::new(false));
        let busy = Rc::clone(&busy_after_lookup);
        let first = GlobalKeyRegistryHandle::new(
            |key| Ok((key.key_hash() == 1).then_some(ElementId::new(5))),
            move |_, f| {
                if busy.get() {
                    return Err(RegistryBusy);
                }
                f(&LabeledElement("a"));
                Ok(())
            },
        );
        let composite = build_composite(vec![first, member(vec![(2, ElementId::new(5), "b")])]);

        assert_eq!(
            composite.lookup_element(&TestKey(1)),
            Ok(Some(ElementId::new(5)))
        );
        busy_after_lookup.set(true);
        assert_eq!(
            try_visited_label(&composite, ElementId::new(5)),
            Err(RegistryBusy),
            "the id belongs to the busy member; b's unrelated id 5 must not answer"
        );
    }

    /// The correctness property `build_composite` exists for: two members
    /// both validly using `ElementId::new(5)` for unrelated elements must not
    /// cross-contaminate a `with_element` call once `lookup` has resolved
    /// which member owns a given hash — even though `with_element` itself
    /// only ever receives the bare id, never the hash.
    #[test]
    fn composite_routes_with_element_to_the_member_that_resolved_the_lookup() {
        let a = member(vec![(1, ElementId::new(5), "a")]);
        let b = member(vec![(2, ElementId::new(5), "b")]);
        let composite = build_composite(vec![a, b]);

        assert_eq!(
            composite.lookup_element(&TestKey(1)),
            Ok(Some(ElementId::new(5)))
        );
        assert_eq!(
            visited_label(&composite, ElementId::new(5)),
            Some("a"),
            "the id lookup(1) just resolved must route to member a, not b's \
             colliding id 5"
        );

        assert_eq!(
            composite.lookup_element(&TestKey(2)),
            Ok(Some(ElementId::new(5)))
        );
        assert_eq!(
            visited_label(&composite, ElementId::new(5)),
            Some("b"),
            "re-resolving the same numeral through member b must now route \
             to b"
        );
    }

    /// No preceding `lookup` for this id: falls back to a try-in-order scan
    /// rather than returning nothing.
    #[test]
    fn composite_with_element_without_a_preceding_lookup_falls_back_to_scanning_members() {
        let a = member(vec![(1, ElementId::new(9), "a")]);
        let composite = build_composite(vec![a]);

        assert_eq!(visited_label(&composite, ElementId::new(9)), Some("a"));
    }

    #[test]
    fn composite_over_zero_members_resolves_nothing() {
        let composite = build_composite(vec![]);
        assert_eq!(composite.lookup_element(&TestKey(1)), Ok(None));
        assert_eq!(visited_label(&composite, ElementId::new(1)), None);
    }
}
