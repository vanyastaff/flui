//! Local history — a mini navigation state inside one route (ADR-0025).
//!
//! While a route holds entries, a pop removes the most recent **entry**
//! instead of the route. It is available to every `ModalRoute`.
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
//! with no lock held and hands each owed callback to the navigator's
//! post-frame queue ([`deliver_on_remove`]). A flush can run inside `build`
//! (a route command or a page update applied there), where a signal write the
//! callback makes through its captured `WriterSource` is refused; after the
//! frame it lands, and each callback runs in its own post-frame entry, so one
//! that panics does not lose the next. A navigator that is no longer mounted
//! has no queue: its owed callbacks are severed like a disposed route's.
//! `entry_handle.remove()` is called from application code outside a flush and
//! fires directly, outside the lock.
//!
//! The registry mutex is a **leaf**: mutate under it, release, then fire. The
//! exactly-once linearization for `remove()` racing `did_pop` is the entry's
//! atomic `removed` flag.
//!
//! # Severed at dispose (ADR-0025, corrected)
//!
//! FLUI **severs at dispose** (a late removal never fires `on_remove`): live
//! entries' callbacks are dropped un-fired and the entries are marked removed,
//! so a late `remove()` is a no-op. Keeping callbacks alive past dispose is
//! exactly the `EntryInner → closure → state → handle → EntryInner` cycle Rust
//! would leak forever (ADR-0025).

// The mechanism ships without a public surface: the module is `pub(crate)`,
// its handles are nameable outside the crate only through the doc-hidden,
// temporary `__test_access` (ADR-0083 §4), and its only producers are the
// tests until the public surface lands beside the first
// Catalog consumer (ADR-0025) — the seam-before-consumer shape
// `hero_controller.rs` documents. Deleting and re-deriving later is how a
// seam stops matching the ADR that specified it.
#![expect(dead_code)]

use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use flui_view::impl_inherited_view;
use flui_view::prelude::*;
use parking_lot::Mutex;

/// Fired when its entry leaves the local history — by a pop or an explicit
/// [`remove`](LocalHistoryEntryHandle::remove).
pub(crate) type OnRemoveCallback = Rc<dyn Fn()>;

/// Deliver the `on_remove`s owed by pops inside one flush, each on its own:
///
/// - a mounted navigator queues each on its post-frame queue, so it runs
///   after the frame, outside any `build`, in its own entry;
/// - a queue that closed healthily hands the callback back and it runs here,
///   contained alone, since no frame will come; a queue that closed with a
///   caught failure in custody retains it, unrun (ADR-0182);
/// - an unmounted navigator (no queue) severs it, as route dispose does: its
///   page subtree is gone, and running it now would run inside whatever
///   element is building (ADR-0025).
///
/// A panic in one callback, or in one capture's drop, does not keep the next
/// from being delivered; the first is resumed by the caller's `recovery`.
pub(crate) fn deliver_on_remove(
    post_frame: Option<&flui_scheduler::PostFrameHandle>,
    callbacks: Vec<OnRemoveCallback>,
    recovery: &mut flui_foundation::panic::PanicRecovery,
) {
    for callback in callbacks {
        let Some(post_frame) = post_frame else {
            recovery.retire(callback);
            continue;
        };
        match post_frame.schedule_or_return(callback, |callback, _timing| callback()) {
            Ok(()) | Err(flui_scheduler::PostFrameRejection::Retained) => {}
            Err(flui_scheduler::PostFrameRejection::Returned(callback)) => {
                recovery.run(|| callback());
                recovery.retire(callback);
            }
        }
    }
}

/// One live entry, with no app-bar-dismissal flag (no `AppBar`; ADR-0025).
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

/// The winner of a [`EntryInner::claim`] race: this entry is now removed, and
/// the claimer owes its `on_remove` — **if it has one**. A callback-less entry
/// is an entry (`on_remove` is optional): it absorbs its pop like any other, it
/// simply has nothing to fire.
struct Claim(Option<OnRemoveCallback>);

impl EntryInner {
    /// Claim this entry for removal. `Some(claim)` for the winner — whose
    /// payload may be `None` when the entry carries no callback — and `None`
    /// for a loser of the race (already popped, already removed, or severed at
    /// dispose).
    ///
    /// The two `None`s are **not** the same answer, and collapsing them was a
    /// real bug: a bare entry read as "someone else already took this", so the
    /// pop walked past it and took the route.
    fn claim(&self) -> Option<Claim> {
        if self.removed.swap(true, Ordering::AcqRel) {
            return None;
        }
        Some(Claim(self.on_remove.lock().take()))
    }
}

/// A pending local-history entry, built by the caller and consumed by
/// [`LocalHistoryHandle::add`].
#[derive(Default)]
pub struct LocalHistoryEntry {
    on_remove: Option<OnRemoveCallback>,
}

impl LocalHistoryEntry {
    /// An entry with no `on_remove` callback.
    #[must_use]
    pub fn new() -> Self {
        Self { on_remove: None }
    }

    /// Called when this entry leaves the history.
    #[must_use]
    pub fn on_remove(mut self, callback: impl Fn() + 'static) -> Self {
        self.on_remove = Some(Rc::new(callback));
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

/// One route's local-history stack. Owned by `ModalInner` beside `heroes` and
/// `pop_entries`; the lock is a **leaf** (module docs).
#[derive(Clone, Default)]
pub(crate) struct LocalHistoryRegistry {
    inner: Arc<RegistryInner>,
}

#[derive(Default)]
struct RegistryInner {
    entries: Mutex<Vec<Arc<EntryInner>>>,
    /// What pops that happened under the history lock owe: the entry's
    /// `on_remove` when it had one, and whether that pop emptied the stack (the
    /// `changed_internal_state` edge). A callback-less
    /// entry still records its edge — it just has nothing to fire.
    owed: Mutex<Vec<(Option<OnRemoveCallback>, bool)>>,
    /// Set at route dispose: adds become inert-with-a-warning
    /// (ADR-0025).
    closed: AtomicBool,
}

impl Drop for RegistryInner {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Release);
        let entries = super::lifecycle::RetiredValues(std::mem::take(self.entries.get_mut()));
        let owed = super::lifecycle::RetiredValues(std::mem::take(self.owed.get_mut()));
        drop(entries);
        drop(owed);
    }
}

impl LocalHistoryRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Whether the stack is empty, i.e. the route does not handle a pop
    /// internally.
    pub(crate) fn is_empty(&self) -> bool {
        self.inner.entries.lock().is_empty()
    }

    /// Push an entry. Returns whether the
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

    /// The `did_pop` half: pop the most recent live entry, **recording** its callback as owed rather than firing it — this
    /// runs under the history lock. Returns whether an entry was consumed
    /// (⇒ `did_pop` answers `false` and the route stays).
    pub(crate) fn pop_last_deferred(&self) -> bool {
        let (claimed, emptied) = {
            let mut entries = self.inner.entries.lock();
            let mut claimed = None;
            while let Some(entry) = entries.pop() {
                if let Some(claim) = entry.claim() {
                    claimed = Some(claim);
                    break;
                }
                // A loser of a concurrent `remove()` race: someone already
                // fired it. Keep popping for a live entry.
            }
            (claimed, entries.is_empty())
        };
        match claimed {
            Some(Claim(callback)) => {
                self.inner.owed.lock().push((callback, emptied));
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

    /// Route dispose: sever every live entry **without firing** and
    /// close the registry against late adds. Runs outside the history lock
    /// (`FlushOutcome::dispose_routes`).
    pub(crate) fn sever(&self) {
        self.inner.closed.store(true, Ordering::Release);
        let entries = std::mem::take(&mut *self.inner.entries.lock());
        for entry in entries {
            entry.removed.store(true, Ordering::Release);
            let _prev = entry.on_remove.lock().take();
        }
        // Owed callbacks were claimed by real pops before dispose: still
        // delivered by the pending `apply`.
    }

    /// Remove `entry` out-of-band: drop it from the stack, then fire `on_remove`
    /// synchronously **after** the leaf lock is released. Returns whether the
    /// empty edge was crossed.
    fn remove(&self, entry: &Arc<EntryInner>) -> Option<bool> {
        let Claim(callback) = entry.claim()?;
        let emptied = {
            let mut entries = self.inner.entries.lock();
            entries.retain(|held| !Arc::ptr_eq(held, entry));
            entries.is_empty()
        };
        // Fired outside the registry lock (it is a leaf), and a callback-less
        // entry simply has nothing to fire.
        if let Some(callback) = callback {
            callback();
        }
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
// The page-facing handles (not public API until the first consumer)
// ============================================================================

/// The capability a page uses to push local-history entries onto its route —
/// `ModalRoute.of(context)` narrowed to `addLocalHistoryEntry`
/// (ADR-0025). Acquire in `init_state`/`did_change_dependencies`, fire
/// from event or animation callbacks (lifecycle-only discipline: `add` is
/// rebuild-adjacent).
#[derive(Clone)]
pub struct LocalHistoryHandle {
    registry: LocalHistoryRegistry,
    /// The route's rebuild hook for the empty↔non-empty edges — the
    /// `changed_internal_state` half `add`/`remove` owe.
    changed_internal_state: Rc<dyn Fn()>,
}

impl LocalHistoryHandle {
    pub(crate) fn new(
        registry: LocalHistoryRegistry,
        changed_internal_state: Rc<dyn Fn()>,
    ) -> Self {
        Self {
            registry,
            changed_internal_state,
        }
    }

    /// The enclosing route's handle, or `None` outside any route.
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<Self> {
        ctx.get::<LocalHistoryScope, _>(|scope| scope.handle.clone())
    }

    /// Add an entry to the route's local history. On a disposed route
    /// the entry is inert (logged), and the returned handle's `remove` is a
    /// no-op.
    pub fn add(&self, entry: LocalHistoryEntry) -> LocalHistoryEntryHandle {
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
                let _prev = inner.on_remove.lock().take();
            }
        }
        LocalHistoryEntryHandle {
            inner,
            registry: Arc::downgrade(&self.registry.inner),
            changed_internal_state: Rc::clone(&self.changed_internal_state),
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

/// Removes the one entry it was minted for, with cross-entry theft
/// syntactically absent.
/// Holds the registry weakly (ADR-0025): a handle outliving its route
/// keeps no route state alive.
#[derive(Clone)]
pub struct LocalHistoryEntryHandle {
    inner: Arc<EntryInner>,
    registry: Weak<RegistryInner>,
    changed_internal_state: Rc<dyn Fn()>,
}

impl LocalHistoryEntryHandle {
    /// Remove the entry; `on_remove` fires synchronously, outside the leaf
    /// lock. Idempotent, and exactly-once against a
    /// racing pop (the entry's atomic flag is the linearization point). A
    /// no-op after the route died (see the module docs).
    pub fn remove(&self) {
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
pub(crate) struct LocalHistoryScope {
    handle: crate::navigator::lifecycle::Terminal<LocalHistoryHandle>,
    child: crate::navigator::lifecycle::Terminal<BoxedView>,
}

impl Clone for LocalHistoryScope {
    fn clone(&self) -> Self {
        Self {
            handle: crate::navigator::lifecycle::Terminal::new(self.handle.clone()),
            child: crate::navigator::lifecycle::Terminal::new(self.child.clone()),
        }
    }
}

impl Drop for LocalHistoryScope {
    fn drop(&mut self) {
        let handle = self.handle.withdraw();
        let child = self.child.withdraw();
        drop((handle, child));
    }
}

impl LocalHistoryScope {
    pub(crate) fn new(handle: LocalHistoryHandle, child: impl IntoView) -> Self {
        Self {
            handle: crate::navigator::lifecycle::Terminal::new(handle),
            child: crate::navigator::lifecycle::Terminal::new(BoxedView::new(child.into_view())),
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
        &*self.child
    }

    fn update_should_notify(&self, _old: &Self) -> bool {
        false
    }
}

impl_inherited_view!(LocalHistoryScope);

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use flui_foundation::panic::PanicRecovery;
    use flui_scheduler::{OwnerFrame, UpdateScheduler};

    use super::*;

    struct Dropped(Rc<Cell<u32>>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    /// Deliver `callbacks` the way `NavigatorShared::apply` does, and return
    /// the text of the first panic, if any.
    fn deliver(
        post_frame: Option<&flui_scheduler::PostFrameHandle>,
        callbacks: Vec<OnRemoveCallback>,
    ) -> Option<String> {
        catch_unwind(AssertUnwindSafe(|| {
            let mut recovery = PanicRecovery::new();
            deliver_on_remove(post_frame, callbacks, &mut recovery);
            recovery.finish();
        }))
        .err()
        .map(|payload| {
            flui_foundation::panic::payload_text(payload.as_ref())
                .unwrap_or_default()
                .to_owned()
        })
    }

    /// A queue that closed healthily hands each owed callback back, and each
    /// runs at once, contained on its own: the first panic does not keep the
    /// second from running, and it is the one reported.
    ///
    /// Red-check: run the returned callbacks inside one `recovery.run` — the
    /// second never runs.
    #[test]
    fn a_closed_queue_runs_each_owed_removal_contained_alone() {
        let scheduler = UpdateScheduler::new();
        let owner = OwnerFrame::new(&scheduler).expect("fresh owner");
        let handle = owner.post_frame_handle();
        assert!(owner.retire().is_none(), "a healthy close");
        let ran = Rc::new(Cell::new(0));
        let second_ran = Rc::clone(&ran);
        let callbacks: Vec<OnRemoveCallback> = vec![
            Rc::new(|| panic!("first owed removal")),
            Rc::new(move || second_ran.set(second_ran.get() + 1)),
        ];

        let panic = deliver(Some(&handle), callbacks);

        assert_eq!(panic.as_deref(), Some("first owed removal"));
        assert_eq!(ran.get(), 1, "the second removal still ran");
    }

    /// An unmounted navigator has no queue: its owed callbacks are severed,
    /// never run, and their captures dropped once.
    ///
    /// Red-check: run the callback when there is no queue — `ran` is 1.
    #[test]
    fn no_queue_severs_the_owed_removal() {
        let ran = Rc::new(Cell::new(0));
        let drops = Rc::new(Cell::new(0));
        let capture = Dropped(Rc::clone(&drops));
        let ran_in_callback = Rc::clone(&ran);
        let callback: OnRemoveCallback = Rc::new(move || {
            let _ = &capture;
            ran_in_callback.set(ran_in_callback.get() + 1);
        });

        assert_eq!(deliver(None, vec![callback]), None);

        assert_eq!(ran.get(), 0, "a severed removal never runs");
        assert_eq!(drops.get(), 1, "its captures are dropped once");
    }
}
