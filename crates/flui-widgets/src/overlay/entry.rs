//! [`OverlayEntry`] — one independently-managed layer of an [`Overlay`].
//!
//! `OverlayEntry`/[`OverlayEntryId`] and the mutation surface
//! (`insert`/`remove`/`mark_needs_build`) are public so design systems and
//! apps can place their own layers (ADR-0076).
//!
//! An entry is **not a widget**, and not a render object: it is a handle holding
//! a builder.
//!
//! # Design notes
//!
//! - **No `GlobalKey`.** The entry needs to reach its own `ViewState` from
//!   `mark_needs_build`, and to keep its subtree state alive across a
//!   `rearrange` reorder. The first is done by having the entry's `ViewState`
//!   publish its own [`RebuildHandle`] here at `init_state` (ADR-0018's pattern),
//!   and the second through keyed reconciliation. Not using a `GlobalKey` matters
//!   because the registry lookup re-enters `WidgetsBinding::inner.read()`, and
//!   doing that under a `BuildContext`'s tree borrow is a lock-order hazard.
//! - **No mid-frame deferral.** [`RebuildHandle::schedule`] only inserts an
//!   id into an inbox drained by the next `build_scope`, so `remove` is already
//!   safe from any phase and any thread; there is no need to defer it to a
//!   post-frame callback.
//! - **No ticker muting for covered entries.** There is no per-subtree ticker
//!   gate, so the animations of a `maintain_state` entry that an opaque entry
//!   covers keep running. Recorded, not claimed.
//! - **No overlay sizing under unbounded constraints.** It only bites there; see
//!   [`RenderTheater`](flui_objects::RenderTheater).
//! - **Not a listenable, no separate `dispose()`.** `OverlayEntry`
//!   is a cheap `Arc`-backed handle with no listener list and
//!   no disposal step of its own; dropping every clone is enough. A second
//!   `remove()` is inert rather than panicking.
//!
//! [`Overlay`]: super::Overlay
//! [`RebuildHandle`]: flui_view::RebuildHandle

use std::fmt;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use flui_view::{BoxedView, BuildContext, RebuildHandle};
use parking_lot::Mutex;

use super::OverlayShared;

/// Builds an entry's subtree.
///
/// `Rc<dyn Fn>` rather than `Box<dyn FnOnce>`: an entry is rebuilt many times,
/// and the [`OverlayEntry`] handle is cloned into the view tree on every overlay
/// build.
pub(crate) type OverlayBuilder = Rc<dyn Fn(&dyn BuildContext) -> BoxedView>;

/// Process-unique identity for an [`OverlayEntry`].
///
/// Not a slab index, so the repo's 1-based `NonZeroUsize` ID-offset convention
/// does not apply. It exists to key the entry's view (so keyed reconciliation
/// preserves subtree state across a reorder) and to find the entry for removal
/// without requiring `PartialEq` on the builder closure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OverlayEntryId(u64);

impl OverlayEntryId {
    fn next() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// The raw value, used as the `ValueKey` payload of the entry's view.
    pub(crate) fn get(self) -> u64 {
        self.0
    }
}

/// The state an [`OverlayEntry`] shares between its handle clones, its mounted
/// `ViewState`, and the [`Overlay`](super::Overlay) holding it.
struct EntryInner {
    id: OverlayEntryId,

    builder: crate::support::retirement::Terminal<OverlayBuilder>,

    /// Published by the entry's `ViewState` in `init_state`, cleared in
    /// `dispose`. `None` before mount and after unmount, which makes
    /// [`OverlayEntry::mark_needs_build`] correctly inert in both windows.
    ///
    /// Acquired in `init_state` — never in `build`.
    rebuild: Mutex<Option<RebuildHandle>>,

    /// Whether this entry occludes the whole overlay, so the ones below it need
    /// not be built.
    opaque: AtomicBool,

    /// Whether this entry stays in the tree even when an [`opaque`] entry covers
    /// it.
    ///
    /// [`opaque`]: EntryInner::opaque
    maintain_state: AtomicBool,

    /// The overlay currently holding this entry, or `None` when detached.
    ///
    /// `Weak`, so an entry outliving its overlay does not keep the overlay's
    /// entry list alive, and so [`OverlayEntry::remove`] on a dropped overlay is
    /// a no-op rather than a resurrection.
    overlay: Mutex<Option<Weak<OverlayShared>>>,
}

impl Drop for EntryInner {
    fn drop(&mut self) {
        let builder = self.builder.withdraw();
        let rebuild = crate::support::retirement::Terminal::new(self.rebuild.get_mut().take());
        self.overlay.get_mut().take();
        drop(builder);
        drop(rebuild);
    }
}

/// A cheap, cloneable handle to one overlay layer.
///
/// Cloning an `OverlayEntry` clones the handle, not the layer: every clone names
/// the same entry. This is what lets the caller keep a handle, the overlay keep
/// one in its list, and the view tree hold a third.
#[derive(Clone)]
pub struct OverlayEntry {
    inner: Arc<EntryInner>,
}

impl OverlayEntry {
    /// An entry that builds its subtree with `builder`, attached to no overlay.
    ///
    /// `opaque` and `maintain_state` both default to `false`. The builder runs on each build of this entry's
    /// layer (its first build, [`mark_needs_build`](Self::mark_needs_build),
    /// and an ancestor rebuild that reaches it), never on insertion.
    #[must_use]
    pub fn new(builder: impl Fn(&dyn BuildContext) -> BoxedView + 'static) -> Self {
        Self {
            inner: Arc::new(EntryInner {
                id: OverlayEntryId::next(),
                builder: crate::support::retirement::Terminal::new(Rc::new(builder)),
                rebuild: Mutex::new(None),
                opaque: AtomicBool::new(false),
                maintain_state: AtomicBool::new(false),
                overlay: Mutex::new(None),
            }),
        }
    }

    /// Builder form of [`set_opaque`](Self::set_opaque), for an entry that is not
    /// yet attached (so no rebuild is needed).
    pub(crate) fn with_opaque(self, opaque: bool) -> Self {
        self.inner.opaque.store(opaque, Ordering::Relaxed);
        self
    }

    /// Builder form of [`set_maintain_state`](Self::set_maintain_state).
    pub(crate) fn with_maintain_state(self, maintain_state: bool) -> Self {
        self.inner
            .maintain_state
            .store(maintain_state, Ordering::Relaxed);
        self
    }

    /// Whether this entry occludes the entire overlay.
    pub(crate) fn opaque(&self) -> bool {
        self.inner.opaque.load(Ordering::Relaxed)
    }

    /// Whether this entry stays built even when covered by an opaque entry.
    pub(crate) fn maintain_state(&self) -> bool {
        self.inner.maintain_state.load(Ordering::Relaxed)
    }

    /// Set whether this entry covers the whole overlay, so the entries below it
    /// that do not [`maintain state`](Self::set_maintain_state) are not built.
    ///
    /// A change rebuilds the **overlay**, not the entry, because the overlay's
    /// build reads it.
    /// Setting the value it already has does nothing. On an entry that is not
    /// [attached](Self::is_attached), or whose overlay is unmounted, the flag is
    /// stored and read by the next build that includes the entry.
    pub fn set_opaque(&self, opaque: bool) {
        self.set_build_flag(&self.inner.opaque, opaque);
    }

    /// Set whether this entry stays built (its state kept) while an
    /// [opaque](Self::set_opaque) entry above covers it.
    ///
    /// Same rebuild and no-op rules as [`set_opaque`](Self::set_opaque).
    pub fn set_maintain_state(&self, maintain_state: bool) {
        self.set_build_flag(&self.inner.maintain_state, maintain_state);
    }

    /// Store `value`, and rebuild the whole overlay only if it changed.
    fn set_build_flag(&self, flag: &AtomicBool, value: bool) {
        if flag.swap(value, Ordering::Relaxed) == value {
            return;
        }
        if let Some(shared) = self.attached_overlay() {
            shared.schedule_rebuild();
        }
    }

    /// This entry's stable identity.
    pub(crate) fn id(&self) -> OverlayEntryId {
        self.inner.id
    }

    pub(crate) fn builder(&self) -> &OverlayBuilder {
        &self.inner.builder
    }

    /// Whether the entry's subtree is currently mounted, i.e. whether its
    /// `ViewState` exists.
    pub(crate) fn is_mounted(&self) -> bool {
        self.inner
            .rebuild
            .lock()
            .as_ref()
            .is_some_and(RebuildHandle::is_active)
    }

    /// Whether this entry is in an overlay's list right now (inserted and not
    /// yet removed). It says nothing about whether the entry's subtree is
    /// built, or whether that overlay is mounted; for the overlay, see
    /// [`OverlayHandle::is_mounted`](super::OverlayHandle::is_mounted).
    ///
    /// `true` from
    /// [`OverlayHandle::insert`](super::OverlayHandle::insert) (or `rearrange`)
    /// until [`remove`](Self::remove), including while the overlay is not
    /// mounted; `false` once every handle to the overlay is dropped.
    #[must_use]
    pub fn is_attached(&self) -> bool {
        self.attached_overlay().is_some()
    }

    /// The element hosting this entry's layer, or `None` when unmounted.
    ///
    /// The identity that proves a `rearrange` *moved* a layer rather than
    /// rebuilding it in place.
    pub(crate) fn element_id(&self) -> Option<flui_foundation::ElementId> {
        self.inner
            .rebuild
            .lock()
            .as_ref()
            .and_then(RebuildHandle::element_id)
    }

    /// Rebuild **only this entry's** subtree on the next frame. Call it when
    /// state the builder reads has changed outside the widget tree.
    ///
    /// Deliberately *not* rebuilding the whole `Overlay`.
    ///
    /// Inert before mount and after unmount.
    ///
    /// Deliberately **not** guarded against the window between [`remove`] and the
    /// frame that unmounts the layer: scheduling there is already harmless. The
    /// overlay's own rebuild removes the child before the drained dirty id is
    /// processed, and [`RebuildHandle::schedule`] documents a vanished element as
    /// a no-op. An extra `removed` flag was written, found unreachable by
    /// red-check, and deleted rather than shipped untested.
    ///
    /// [`remove`]: Self::remove
    pub fn mark_needs_build(&self) {
        if let Some(handle) = self.inner.rebuild.lock().as_ref() {
            handle.schedule(flui_view::RebuildReason::StateChange);
        }
    }

    /// Detach from the overlay holding this entry and schedule that overlay to
    /// rebuild without it; the layer's state is disposed on that frame.
    ///
    /// Guards:
    ///
    /// - Removing an entry twice is caller error, not a framework invariant, so
    ///   [`PANIC-POLICY`] forbids a panic here: the second call logs and returns.
    /// - There is **no** early return for an unmounted overlay: the
    ///   [`OverlayHandle`](super::OverlayHandle) owns the list and a later mount
    ///   builds it (ADR-0076 §2), so returning early would leave a detached
    ///   entry in the list for that mount to build, with no way left to remove
    ///   it. The entry always leaves the list; the rebuild is scheduled only if
    ///   the overlay is mounted. A dropped overlay (every handle gone) still
    ///   makes this a no-op: the `Weak` upgrade fails and nothing is resurrected.
    /// - No post-frame deferral is needed (see module docs).
    ///
    /// After `remove`, [`is_attached`](Self::is_attached) is `false` and the
    /// entry may be inserted again.
    ///
    /// [`PANIC-POLICY`]: ../../../../../docs/PANIC-POLICY.md
    pub fn remove(&self) {
        let Some(shared) = self.detach() else {
            tracing::error!(
                entry = self.inner.id.get(),
                "OverlayEntry::remove on an entry that belongs to no overlay — \
                 an entry should be removed exactly once"
            );
            return;
        };

        // Always out of the list, mounted or not: the handle's list outlives the
        // mounted overlay (see the doc above for why there is no early return
        // for an unmounted overlay). `schedule_rebuild` is inert when unmounted.
        shared.retain_entries(|entry| entry.id() != self.inner.id);
        shared.schedule_rebuild();
    }

    // ── Overlay-facing plumbing ──────────────────────────────────────────────

    /// Take the overlay back-reference, upgrading it. `None` when this entry is
    /// attached to nothing, or when its overlay's shared state has been dropped.
    pub(super) fn attached_overlay(&self) -> Option<Arc<OverlayShared>> {
        self.inner.overlay.lock().as_ref().and_then(Weak::upgrade)
    }

    /// Clear the back-reference and return the overlay it pointed at, so a
    /// second [`remove`](Self::remove) finds nothing and cannot evict a
    /// same-position entry that took this one's place.
    fn detach(&self) -> Option<Arc<OverlayShared>> {
        let weak = self.inner.overlay.lock().take()?;
        weak.upgrade()
    }

    /// Bind this entry to `shared`. Called by the overlay on insertion.
    ///
    /// Re-attaching a previously removed entry is legal: removal only clears the
    /// back-reference and does not poison the entry.
    pub(crate) fn attach(&self, shared: &Arc<OverlayShared>) {
        *self.inner.overlay.lock() = Some(Arc::downgrade(shared));
    }

    /// Publish the mounted subtree's rebuild capability. Called from the entry
    /// view's `init_state`.
    pub(crate) fn publish_rebuild(&self, handle: RebuildHandle) {
        let _prev = self.inner.rebuild.lock().replace(handle);
    }

    /// Drop the rebuild capability, but only if `element` published it.
    /// Called from the entry view's `dispose`.
    ///
    /// Owner-aware for the same reason as the overlay's own slot: an entry
    /// moved from one overlay to another within a frame briefly has two
    /// views, and the new one may publish before the old one is disposed.
    /// An unconditional clear would then revoke the live view's capability,
    /// leaving `mark_needs_build` inert on an entry that is on screen.
    pub(crate) fn clear_rebuild(&self, element: Option<flui_foundation::ElementId>) {
        let mut slot = self.inner.rebuild.lock();
        if slot
            .as_ref()
            .is_some_and(|held| held.element_id() == element)
        {
            let _prev = slot.take();
        }
    }

    /// Whether two handles name the same entry.
    pub(crate) fn is_same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl fmt::Debug for OverlayEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OverlayEntry")
            .field("id", &self.inner.id.get())
            .field("mounted", &self.is_mounted())
            .field("attached", &self.inner.overlay.lock().is_some())
            .finish()
    }
}
