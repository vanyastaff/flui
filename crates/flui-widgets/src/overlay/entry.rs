//! [`OverlayEntry`] — one independently-managed layer of an [`Overlay`].
//!
//! `OverlayEntry`/[`OverlayEntryId`] are published from the crate root (see
//! `docs/adr/ADR-0036-overlay-publication-and-per-entry-scope-marker.md`); the
//! mutation surface (`insert`/`remove`/`mark_needs_build`/…) stays
//! `pub(crate)` — `Navigator` and `Draggable`'s feedback layer are the only
//! in-crate callers for now, and nothing in ADR-0036 widens that.
//!
//! # Flutter parity
//!
//! `.flutter/packages/flutter/lib/src/widgets/overlay.dart` (master
//! `3.33.0-0.0.pre-6280-g88e87cd963f`):
//!
//! - `class OverlayEntry implements Listenable` (`overlay.dart:109`) — **not a
//!   widget**, and not a render object. A handle holding a `WidgetBuilder`.
//! - `OverlayEntry.remove()` (`:226-243`)
//! - `OverlayEntry.markNeedsBuild()` (`:250`)
//!
//! # Divergences from the reference, all deliberate
//!
//! - **No `GlobalKey`.** Flutter's entry owns a `GlobalKey<_OverlayEntryWidgetState>`
//!   (`overlay.dart:214`) for two jobs: reaching the entry's `State` from
//!   `markNeedsBuild`, and keeping the entry's subtree state alive across a
//!   `rearrange` reorder. FLUI does the first by having the entry's `ViewState`
//!   publish its own [`RebuildHandle`] here at `init_state` (ADR-0018's pattern),
//!   and the second through keyed reconciliation. Not using a `GlobalKey` matters
//!   because the registry lookup re-enters `WidgetsBinding::inner.read()`, and
//!   doing that under a `BuildContext`'s tree borrow is a lock-order hazard.
//! - **No mid-frame deferral.** `OverlayEntry.remove` posts a post-frame callback
//!   when it runs during `persistentCallbacks` (`:236-242`), because Dart's
//!   `setState` throws during build. [`RebuildHandle::schedule`] only inserts an
//!   id into an inbox drained by the next `build_scope`, so it is already safe
//!   from any phase and any thread. The hack has no analogue to port.
//! - **No `tickerEnabled: false` for covered entries.** Flutter mutes the tickers
//!   of a `maintainState` entry that an opaque entry covers (`overlay.dart:906`).
//!   FLUI has no per-subtree ticker gate, so a covered entry's animations keep
//!   running. Recorded, not claimed.
//! - **No `canSizeOverlay`.** It only bites under unbounded constraints; see
//!   [`RenderTheater`](flui_objects::RenderTheater).
//! - **Not a `Listenable`, no separate `dispose()`.** Flutter's `OverlayEntry`
//!   implements `Listenable` and carries its own `dispose()`, independent of
//!   `remove()` (`overlay.dart:109-243`): a caller can listen for `mounted`
//!   flipping, and must `dispose()` an entry (even one never inserted, or
//!   already removed) to release its `ChangeNotifier` resources — skipping
//!   that is a leak-tracker failure in the oracle's own test suite. FLUI's
//!   `OverlayEntry` is a cheap `Arc`-backed handle with no listener list and
//!   no disposal step of its own; dropping every clone is enough. The one
//!   behavior from that group FLUI does port — a second `remove()` is inert
//!   rather than panicking (`overlay.dart`'s `assert` in `remove()`,
//!   `:226-243`) — is `overlay/tests.rs`'s
//!   `removed_entry_cannot_reinsert_or_rebuild_silently`.
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

/// Builds an entry's subtree. Flutter's `WidgetBuilder`.
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

    builder: OverlayBuilder,

    /// Published by the entry's `ViewState` in `init_state`, cleared in
    /// `dispose`. `None` before mount and after unmount, which makes
    /// [`OverlayEntry::mark_needs_build`] correctly inert in both windows.
    ///
    /// Acquired in `init_state` — never in `build` — per port-check trigger #22.
    rebuild: Mutex<Option<RebuildHandle>>,

    /// Whether this entry occludes the whole overlay, so the ones below it need
    /// not be built. Flutter's `OverlayEntry.opaque` (`overlay.dart:136-146`).
    opaque: AtomicBool,

    /// Whether this entry stays in the tree even when an [`opaque`] entry covers
    /// it. Flutter's `OverlayEntry.maintainState` (`overlay.dart:163-173`).
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
    /// `opaque` and `maintain_state` both default to `false`, as in Flutter
    /// (`overlay.dart:117-121`).
    pub(crate) fn new(builder: impl Fn(&dyn BuildContext) -> BoxedView + 'static) -> Self {
        Self {
            inner: Arc::new(EntryInner {
                id: OverlayEntryId::next(),
                builder: Rc::new(builder),
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

    /// Flutter's `opaque` setter (`overlay.dart:138-146`): a change rebuilds the
    /// **overlay**, not the entry, because `OverlayState.build` reads it.
    pub(crate) fn set_opaque(&self, opaque: bool) {
        self.set_build_flag(&self.inner.opaque, opaque);
    }

    /// Flutter's `maintainState` setter (`overlay.dart:165-173`), which likewise
    /// goes through `_didChangeEntryOpacity`.
    pub(crate) fn set_maintain_state(&self, maintain_state: bool) {
        self.set_build_flag(&self.inner.maintain_state, maintain_state);
    }

    /// Store `value`, and rebuild the whole overlay only if it changed —
    /// Flutter's `if (_opaque == value) return;` short-circuit.
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

    /// Whether the entry's subtree is currently mounted — Flutter's
    /// `OverlayEntry.mounted` (`overlay.dart:196`), which likewise reports
    /// whether the entry's `State` exists.
    pub(crate) fn is_mounted(&self) -> bool {
        self.inner
            .rebuild
            .lock()
            .as_ref()
            .is_some_and(RebuildHandle::is_active)
    }

    /// Whether this entry currently belongs to an overlay whose state is alive.
    pub(crate) fn is_attached(&self) -> bool {
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

    /// Rebuild **only this entry's** subtree on the next frame.
    ///
    /// Flutter's `OverlayEntry.markNeedsBuild` (`overlay.dart:250`), which reaches
    /// one `_OverlayEntryWidgetState` through the entry's `GlobalKey` and calls
    /// `setState` on it — deliberately *not* rebuilding the whole `Overlay`.
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
    pub(crate) fn mark_needs_build(&self) {
        if let Some(handle) = self.inner.rebuild.lock().as_ref() {
            handle.schedule();
        }
    }

    /// Detach from the overlay holding this entry and schedule that overlay to
    /// rebuild without it.
    ///
    /// Flutter's `OverlayEntry.remove` (`overlay.dart:226-243`). Two of its three
    /// guards are ported; the third has no analogue:
    ///
    /// - *"An OverlayEntry should be removed only once"* — Flutter `assert`s.
    ///   Removing twice is caller error, not a framework invariant, so
    ///   [`PANIC-POLICY`] forbids a panic here: the second call logs and returns.
    /// - `if (!overlay.mounted) return;` — a dropped overlay makes this a no-op.
    ///   Here the `Weak` upgrade fails and we return, so a stale entry handle can
    ///   never resurrect a dead overlay.
    /// - the `persistentCallbacks` post-frame deferral is unnecessary (see module
    ///   docs).
    ///
    /// [`PANIC-POLICY`]: ../../../../../docs/PANIC-POLICY.md
    pub(crate) fn remove(&self) {
        let Some(shared) = self.detach() else {
            tracing::error!(
                entry = self.inner.id.get(),
                "OverlayEntry::remove on an entry that belongs to no overlay — \
                 an entry should be removed exactly once"
            );
            return;
        };

        // `if (!overlay.mounted) return;` (`overlay.dart:231-233`) — Flutter detaches
        // the entry but leaves the unmounted overlay's list alone. Found by a
        // parity re-check: FLUI used to mutate it regardless.
        if !shared.is_mounted() {
            return;
        }

        shared.retain_entries(|entry| entry.id() != self.inner.id);
        shared.schedule_rebuild();
    }

    // ── Overlay-facing plumbing ──────────────────────────────────────────────

    /// Take the overlay back-reference, upgrading it. `None` when this entry is
    /// attached to nothing, or when its overlay's shared state has been dropped.
    fn attached_overlay(&self) -> Option<Arc<OverlayShared>> {
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
    /// Re-attaching a previously removed entry is legal — Flutter also allows it
    /// (`_overlay` is nulled, not poisoned; only `dispose` is terminal).
    pub(crate) fn attach(&self, shared: &Arc<OverlayShared>) {
        *self.inner.overlay.lock() = Some(Arc::downgrade(shared));
    }

    /// Publish the mounted subtree's rebuild capability. Called from the entry
    /// view's `init_state`.
    pub(crate) fn publish_rebuild(&self, handle: RebuildHandle) {
        *self.inner.rebuild.lock() = Some(handle);
    }

    /// Drop the rebuild capability. Called from the entry view's `dispose`.
    pub(crate) fn clear_rebuild(&self) {
        *self.inner.rebuild.lock() = None;
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
