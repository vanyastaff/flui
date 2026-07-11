//! Local history — a mini navigation state inside one route (ADR-0025).
//!
//! While a route holds entries, a pop removes the most recent **entry**
//! instead of the route — Flutter's `LocalHistoryRoute` mixin
//! (`routes.dart:747-973`, applied to `ModalRoute` at `:1266`).
//!
//! # Delivery discipline (binding, per ADR-0025)
//!
//! `on_remove` is user code. An entry popped by `did_pop` is popped **inside
//! the flush, under the history lock** — firing the callback there deadlocks
//! the moment it calls back into the navigator (the `PopScope` fan-out bug,
//! fixed in `7b038dee`, was exactly this shape). So `did_pop` only *records*
//! the owed removal; the flush notes the route id in
//! [`FlushOutcome::local_history_popped`](super::history::FlushOutcome), and
//! `NavigatorShared::apply` drains [`take_owed`](LocalHistoryRegistry::take_owed)
//! with no lock held. `entry_handle.remove()` outside a flush fires directly —
//! the same callback runs in the same lock-free context either way.
//!
//! The registry mutex is a **leaf**: mutate under it, release, then fire. The
//! exactly-once linearization for `remove()` racing `did_pop` is the entry's
//! atomic `removed` flag.
//!
//! # Divergence, named (ADR-0025, corrected)
//!
//! Flutter never clears `_localHistory` on route dispose, so
//! `removeLocalHistoryEntry` after the route died still fires `onRemove`. FLUI
//! **severs at dispose**: live entries' callbacks are dropped un-fired (as
//! Flutter's GC drops them) and the entries are marked removed, so a late
//! `remove()` is a no-op. Keeping callbacks alive past dispose is exactly the
//! `EntryInner → closure → state → handle → EntryInner` cycle Rust would leak
//! forever (ADR-0025); the loyal-but-leaking alternative loses.

// The mechanism ships with a crate-private surface: its only in-crate
// producers are the tests until the public surface lands beside the first
// Catalog consumer (ADR-0025) — the seam-before-consumer shape
// `hero_controller.rs` documents. Deleting and re-deriving later is how a
// seam stops matching the ADR that specified it.
#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use flui_view::impl_inherited_view;
use flui_view::prelude::*;
use parking_lot::Mutex;

/// Fired when its entry leaves the local history — by a pop or an explicit
/// [`remove`](LocalHistoryEntryHandle::remove). Flutter's
/// `LocalHistoryEntry.onRemove` (`routes.dart:711`).
pub(crate) type OnRemoveCallback = Arc<dyn Fn() + Send + Sync>;

/// One live entry. Flutter's `LocalHistoryEntry` (`routes.dart:708-729`),
/// minus `impliesAppBarDismissal` (no `AppBar`; ADR-0025).
struct EntryInner {
    /// Consumed (`Option::take`) when the entry fires or the route dies —
    /// never merely cloned out — so the callback (and whatever widget state it
    /// captures) cannot cycle back to a handle that keeps this inner alive
    /// (ADR-0025).
    on_remove: Mutex<Option<OnRemoveCallback>>,
    /// The exactly-once linearization point: whoever swaps this to `true`
    /// owns the firing of `on_remove`.
    removed: AtomicBool,
}

impl EntryInner {
    /// Claim this entry for removal. `Some(callback)` for the winner; `None`
    /// for a loser of the race (already popped, already removed, or severed
    /// at dispose).
    fn claim(&self) -> Option<OnRemoveCallback> {
        if self.removed.swap(true, Ordering::AcqRel) {
            return None;
        }
        self.on_remove.lock().take()
    }
}

/// A pending local-history entry, built by the caller and consumed by
/// [`LocalHistoryHandle::add`].
pub(crate) struct LocalHistoryEntry {
    on_remove: Option<OnRemoveCallback>,
}

impl LocalHistoryEntry {
    pub(crate) fn new() -> Self {
        Self { on_remove: None }
    }

    /// Called when this entry leaves the history (`routes.dart:711`).
    #[must_use]
    pub(crate) fn on_remove(mut self, callback: impl Fn() + Send + Sync + 'static) -> Self {
        self.on_remove = Some(Arc::new(callback));
        self
    }
}

impl std::fmt::Debug for LocalHistoryEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalHistoryEntry")
            .field("has_on_remove", &self.on_remove.is_some())
            .finish()
    }
}

/// One route's local-history stack — Flutter's `_localHistory`
/// (`routes.dart:748`). Owned by `ModalInner` beside `heroes` and
/// `pop_entries`; the lock is a **leaf** (module docs).
#[derive(Clone, Default)]
pub(crate) struct LocalHistoryRegistry {
    inner: Arc<RegistryInner>,
}

#[derive(Default)]
struct RegistryInner {
    entries: Mutex<Vec<Arc<EntryInner>>>,
    /// Callbacks owed by pops that happened under the history lock, drained
    /// by `NavigatorShared::apply` outside it. `true` alongside a callback
    /// when that pop emptied the stack (the `changed_internal_state` edge,
    /// `routes.dart:961-963`).
    owed: Mutex<Vec<(Option<OnRemoveCallback>, bool)>>,
    /// Set at route dispose: adds become inert-with-a-warning
    /// (ADR-0025).
    closed: AtomicBool,
}

impl LocalHistoryRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// `willHandlePopInternally` (`routes.dart:970-972`).
    pub(crate) fn is_empty(&self) -> bool {
        self.inner.entries.lock().is_empty()
    }

    /// `addLocalHistoryEntry` (`routes.dart:882-896`). Returns whether the
    /// empty→non-empty edge was crossed (the caller owes
    /// `changed_internal_state`). `None` when the route is already disposed —
    /// inert, logged.
    fn add(&self, entry: Arc<EntryInner>) -> Option<bool> {
        if self.inner.closed.load(Ordering::Acquire) {
            tracing::warn!(
                "LocalHistoryEntry added to a disposed route; the entry is inert \
                 and its on_remove will never fire"
            );
            return None;
        }
        let mut entries = self.inner.entries.lock();
        let was_empty = entries.is_empty();
        entries.push(entry);
        Some(was_empty)
    }

    /// The `didPop` half (`routes.dart:950-965`): pop the most recent live
    /// entry, **recording** its callback as owed rather than firing it — this
    /// runs under the history lock. Returns whether an entry was consumed
    /// (⇒ `did_pop` answers `false` and the route stays).
    pub(crate) fn pop_last_deferred(&self) -> bool {
        let (claimed, emptied) = {
            let mut entries = self.inner.entries.lock();
            let mut claimed = None;
            while let Some(entry) = entries.pop() {
                if let Some(callback) = entry.claim() {
                    claimed = Some(callback);
                    break;
                }
                // A loser of a concurrent `remove()` race: already fired
                // elsewhere; keep popping for a live one.
            }
            (claimed, entries.is_empty())
        };
        match claimed {
            Some(callback) => {
                self.inner.owed.lock().push((Some(callback), emptied));
                true
            }
            None => false,
        }
    }

    /// Drain everything owed by in-flush pops. Called by
    /// `NavigatorShared::apply` with **no lock held**; returns whether any
    /// drained pop crossed the emptied edge (the caller then fires
    /// `changed_internal_state`, also unlocked).
    pub(crate) fn take_owed(&self) -> (Vec<OnRemoveCallback>, bool) {
        let owed = std::mem::take(&mut *self.inner.owed.lock());
        let mut callbacks = Vec::with_capacity(owed.len());
        let mut emptied = false;
        for (callback, edge) in owed {
            callbacks.extend(callback);
            emptied |= edge;
        }
        (callbacks, emptied)
    }

    /// Route dispose: sever every live entry **without firing** (Flutter
    /// GC-drops the list, `routes.dart` never touches it in dispose) and
    /// close the registry against late adds. Runs outside the history lock
    /// (`FlushOutcome::dispose_routes`).
    pub(crate) fn sever(&self) {
        self.inner.closed.store(true, Ordering::Release);
        let entries = std::mem::take(&mut *self.inner.entries.lock());
        for entry in entries {
            entry.removed.store(true, Ordering::Release);
            entry.on_remove.lock().take();
        }
        // Owed callbacks were claimed by real pops before dispose: still
        // delivered by the pending `apply`.
    }

    /// Remove `entry` out-of-band (`removeLocalHistoryEntry`,
    /// `routes.dart:902-927`): drop it from the stack, then fire `on_remove`
    /// synchronously **after** the leaf lock is released. Returns whether the
    /// empty edge was crossed.
    fn remove(&self, entry: &Arc<EntryInner>) -> Option<bool> {
        let callback = entry.claim()?;
        let emptied = {
            let mut entries = self.inner.entries.lock();
            entries.retain(|held| !Arc::ptr_eq(held, entry));
            entries.is_empty()
        };
        callback();
        Some(emptied)
    }
}

impl std::fmt::Debug for LocalHistoryRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalHistoryRegistry")
            .field("entries", &self.inner.entries.lock().len())
            .field("closed", &self.inner.closed.load(Ordering::Relaxed))
            .finish()
    }
}

// ============================================================================
// The page-facing handles (crate-private until the first consumer)
// ============================================================================

/// The capability a page uses to push local-history entries onto its route —
/// `ModalRoute.of(context)` narrowed to `addLocalHistoryEntry`
/// (ADR-0025). Acquire in `init_state`/`did_change_dependencies`, fire
/// from event or animation callbacks (trigger #22 discipline: `add` is
/// rebuild-adjacent).
#[derive(Clone)]
pub(crate) struct LocalHistoryHandle {
    registry: LocalHistoryRegistry,
    /// The route's rebuild hook for the empty↔non-empty edges — the
    /// `changed_internal_state` half `add`/`remove` owe (`routes.dart:886-895`).
    changed_internal_state: Arc<dyn Fn() + Send + Sync>,
}

impl LocalHistoryHandle {
    pub(crate) fn new(
        registry: LocalHistoryRegistry,
        changed_internal_state: Arc<dyn Fn() + Send + Sync>,
    ) -> Self {
        Self {
            registry,
            changed_internal_state,
        }
    }

    /// The enclosing route's handle, or `None` outside any route.
    pub(crate) fn maybe_of(ctx: &dyn BuildContext) -> Option<Self> {
        ctx.get::<LocalHistoryScope, _>(|scope| scope.handle.clone())
    }

    /// `addLocalHistoryEntry` (`routes.dart:882-896`). On a disposed route
    /// the entry is inert (logged), and the returned handle's `remove` is a
    /// no-op.
    pub(crate) fn add(&self, entry: LocalHistoryEntry) -> LocalHistoryEntryHandle {
        let inner = Arc::new(EntryInner {
            on_remove: Mutex::new(entry.on_remove),
            removed: AtomicBool::new(false),
        });
        match self.registry.add(Arc::clone(&inner)) {
            Some(true) => (self.changed_internal_state)(),
            Some(false) => {}
            None => {
                // Disposed route: mark the orphan removed so `remove` no-ops.
                inner.removed.store(true, Ordering::Release);
                inner.on_remove.lock().take();
            }
        }
        LocalHistoryEntryHandle {
            inner,
            registry: Arc::downgrade(&self.registry.inner),
            changed_internal_state: Arc::clone(&self.changed_internal_state),
        }
    }
}

impl std::fmt::Debug for LocalHistoryHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalHistoryHandle")
            .field("registry", &self.registry)
            .finish_non_exhaustive()
    }
}

/// Removes the one entry it was minted for — `LocalHistoryEntry.remove()`
/// (`routes.dart:726-729`), with cross-entry theft syntactically absent.
/// Holds the registry weakly (ADR-0025): a handle outliving its route
/// keeps no route state alive.
#[derive(Clone)]
pub(crate) struct LocalHistoryEntryHandle {
    inner: Arc<EntryInner>,
    registry: Weak<RegistryInner>,
    changed_internal_state: Arc<dyn Fn() + Send + Sync>,
}

impl LocalHistoryEntryHandle {
    /// Remove the entry; `on_remove` fires synchronously, outside the leaf
    /// lock (`routes.dart:902-927`). Idempotent, and exactly-once against a
    /// racing pop (the entry's atomic flag is the linearization point). A
    /// no-op after the route died (module-doc divergence).
    pub(crate) fn remove(&self) {
        let Some(registry) = self.registry.upgrade() else {
            return;
        };
        let registry = LocalHistoryRegistry { inner: registry };
        if registry.remove(&self.inner) == Some(true) {
            (self.changed_internal_state)();
        }
    }
}

impl std::fmt::Debug for LocalHistoryEntryHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalHistoryEntryHandle")
            .field("removed", &self.inner.removed.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

// ============================================================================
// The ambient scope
// ============================================================================

/// Provides the enclosing route's [`LocalHistoryHandle`] to the page subtree —
/// the `PopEntryScope`/`HeroScope` pattern. Never notifies: the handle is
/// fixed for the route's lifetime.
#[derive(Clone)]
pub(crate) struct LocalHistoryScope {
    handle: LocalHistoryHandle,
    child: BoxedView,
}

impl LocalHistoryScope {
    pub(crate) fn new(handle: LocalHistoryHandle, child: impl IntoView) -> Self {
        Self {
            handle,
            child: BoxedView(Box::new(child.into_view())),
        }
    }
}

impl std::fmt::Debug for LocalHistoryScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalHistoryScope")
            .field("handle", &self.handle)
            .finish_non_exhaustive()
    }
}

impl InheritedView for LocalHistoryScope {
    type Data = LocalHistoryHandle;

    fn data(&self) -> &Self::Data {
        &self.handle
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, _old: &Self) -> bool {
        false
    }
}

impl_inherited_view!(LocalHistoryScope);
