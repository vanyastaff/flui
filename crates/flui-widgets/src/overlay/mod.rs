//! [`Overlay`] — an insertion-ordered stack of independently-managed layers.
//!
//! The first prerequisite for `Navigator`. [`Overlay`], [`OverlayEntry`],
//! [`OverlayEntryId`] and [`OverlayHandle`] are published from the crate root
//! (`docs/adr/ADR-0036-overlay-publication-and-per-entry-scope-marker.md`);
//! everything else here — the mutation surface, [`OverlayScope`], the
//! `Theater`/`OverlayState`/`OverlayEntryView` machinery — stays
//! `pub(crate)`. `Navigator` and `Draggable`'s feedback layer are the only
//! in-crate consumers of the mutation surface for now.
//!
//! # Flutter parity
//!
//! `.flutter/packages/flutter/lib/src/widgets/overlay.dart` (master
//! `3.33.0-0.0.pre-6280-g88e87cd963f`): `Overlay`, `OverlayState`,
//! `OverlayEntry`, `_OverlayEntryWidget`, `_Theater` / `_RenderTheater`.
//!
//! The load-bearing contract, which the tests pin: **`entries` is ordered
//! bottom → top, and the last entry paints on top.** Flutter establishes this by
//! filling `children` from `_entries.reversed` and then reversing again
//! (`overlay.dart:894`, `:916`), with `_RenderTheater.paint` walking
//! first-onstage → last (`:1157-1161`).
//!
//! # `opaque` / `maintainState` / `skipCount`
//!
//! This originally shipped as a plain `Stack` with `StackFit::Expand`, deferring
//! the three flags; they landed later, because `ModalRoute`'s
//! `maintainState` would otherwise be a field that lies.
//!
//! [`OverlayState::build`] is now a port of `overlay.dart:886-918`: walk the
//! entries **top-first**, keep building until an [`opaque`] entry is reached, then
//! keep only the entries below it that set [`maintain_state`]. The kept-but-covered
//! entries end up as the *leading* children of the reversed list, so they are
//! exactly the first `skip_count` children — which [`RenderTheater`] does not lay
//! out, paint or hit-test.
//!
//! An entry below an opaque one **without** `maintain_state` is absent from the
//! view tree entirely: its state is disposed, and rebuilt fresh when it is
//! uncovered. That is Flutter's contract, and routes depend on it.
//!
//! Two divergences, both recorded in [`entry`]: no `tickerEnabled: false` for the
//! covered entries, and no `canSizeOverlay`.
//!
//! [`opaque`]: OverlayEntry::opaque
//! [`maintain_state`]: OverlayEntry::maintain_state
//! [`RenderTheater`]: flui_objects::RenderTheater
//!
//! # Threading and locks
//!
//! [`OverlayHandle`] answers "how does something outside the tree mutate the
//! overlay?": an owned, `'static`, cloneable capability, not a `GlobalKey`
//! lookup. Mutation takes a private `Mutex` and then schedules a rebuild
//! through the [`RebuildHandle`] the state published at `init_state`. No
//! element-tree borrow is held, no second lock is taken under a first, and no
//! `GlobalKey` registry is consulted. `Navigator` will reach its overlay the
//! same way.
//!
//! [`RebuildHandle`]: flui_view::RebuildHandle

// `Overlay`/`OverlayEntry`/`OverlayEntryId`/`OverlayHandle` are published
// (ADR-0036: the `Overlay::of`/`maybe_of` lookup contract). The mutation
// surface — `insert`/`insert_all`/`rearrange`/`InsertPosition`/`entry_ids`,
// the builder-form constructors, `mark_needs_build` — stays `pub(crate)`:
// `Navigator` and `Draggable`'s feedback layer are its only callers today,
// and widening it (a public `Overlay::wrap`/`initialEntries` constructor, a
// public `OverlayHandle::insert`) is a separate, not-yet-taken gate — see
// ADR-0036's deferrals.
//
// The `navigator` module needs no such allow: every item there has a production
// caller or a `#[cfg(test)]`.
#![allow(dead_code)]

mod entry;
mod theater;

#[cfg(test)]
mod tests;

use std::fmt;
use std::sync::Arc;

pub use entry::{OverlayEntry, OverlayEntryId};
use flui_foundation::ViewKey;
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_view::{BoxedView, InheritedView, RebuildHandle, ValueKey, impl_inherited_view};
use parking_lot::Mutex;

use self::theater::Theater;

/// Where [`OverlayHandle::insert`] places a new entry.
///
/// Flutter passes `above:`/`below:` named arguments and asserts they are not both
/// given (`overlay.dart:661`); an enum makes that unrepresentable instead.
/// Resolves to Flutter's `_insertionIndex` (`overlay.dart:660-669`).
#[derive(Debug, Clone)]
pub(crate) enum InsertPosition {
    /// Append — the new entry paints above every existing one. Flutter's default.
    Top,
    /// Directly above `.0`, i.e. at `index_of(entry) + 1`.
    Above(OverlayEntry),
    /// Directly below `.0`, i.e. at `index_of(entry)`.
    Below(OverlayEntry),
}

/// The entry list plus the capability to rebuild the mounted [`Overlay`].
///
/// Shared by `Arc` between the [`OverlayHandle`] the caller holds and the
/// `OverlayState` the framework owns. This is deliberate: mutation arrives from
/// outside the tree (from a `Navigator`'s route flush), so the list cannot
/// live behind `&mut OverlayState` — nothing can obtain one.
pub(crate) struct OverlayShared {
    /// Bottom → top. The last entry paints on top.
    entries: Mutex<Vec<OverlayEntry>>,

    /// `Some` only while the `Overlay` is mounted; published in `init_state` and
    /// cleared in `dispose`, per port-check trigger #22 (never acquired in
    /// `build`). A handle for an unmounted overlay is the reason a stale
    /// [`OverlayHandle`] is inert rather than a panic.
    rebuild: Mutex<Option<RebuildHandle>>,
}

impl OverlayShared {
    /// Schedule the mounted overlay to rebuild. No-op when unmounted.
    ///
    /// Flutter's `OverlayState._markDirty` (`overlay.dart:848-852`), which is
    /// `if (mounted) setState((){})`.
    pub(crate) fn schedule_rebuild(&self) {
        if let Some(handle) = self.rebuild.lock().as_ref() {
            handle.schedule();
        }
    }

    /// Whether the overlay is mounted. Flutter's `OverlayState.mounted`, consulted
    /// by `OverlayEntry.remove` before it touches the entry list
    /// (`overlay.dart:233`).
    pub(crate) fn is_mounted(&self) -> bool {
        self.rebuild
            .lock()
            .as_ref()
            .is_some_and(RebuildHandle::is_active)
    }

    /// Retain the entries matching `keep`. Used by [`OverlayEntry::remove`].
    pub(crate) fn retain_entries(&self, keep: impl FnMut(&OverlayEntry) -> bool) {
        self.entries.lock().retain(keep);
    }
}

/// An owned, `'static` capability to mutate an [`Overlay`]'s entry list.
///
/// Create one, hand it to `Overlay::new` (crate-internal — `Navigator` is the
/// only caller today), and keep a clone: every clone names the same overlay.
/// Mutating before mount is legal — the first build reads
/// whatever the list holds — and mutating after unmount is a silent no-op.
///
/// This replaces Flutter's `GlobalKey<OverlayState>` (`navigator.dart:3746`),
/// which `Navigator` uses purely to call `rearrange`. The `GlobalKey` route is
/// not merely unnecessary but hazardous here: resolving it from inside a
/// tree-borrow callback would nest the `WidgetsBinding` registry lock inside
/// the lock already held for the ancestor walk.
#[derive(Clone)]
pub struct OverlayHandle {
    shared: Arc<OverlayShared>,
}

impl OverlayHandle {
    /// A handle to an overlay with no entries, not yet mounted.
    pub(crate) fn new() -> Self {
        Self {
            shared: Arc::new(OverlayShared {
                entries: Mutex::new(Vec::new()),
                rebuild: Mutex::new(None),
            }),
        }
    }

    /// Whether the overlay this handle names is currently mounted.
    pub(crate) fn is_mounted(&self) -> bool {
        self.shared
            .rebuild
            .lock()
            .as_ref()
            .is_some_and(RebuildHandle::is_active)
    }

    /// The entries, bottom → top.
    pub(crate) fn entry_ids(&self) -> Vec<OverlayEntryId> {
        self.shared
            .entries
            .lock()
            .iter()
            .map(OverlayEntry::id)
            .collect()
    }

    pub(crate) fn len(&self) -> usize {
        self.shared.entries.lock().len()
    }

    /// Whether two handles name the same overlay. Identity, not structural
    /// equality — mirrors [`OverlayEntry::is_same`]. Used by [`OverlayScope`]'s
    /// [`InheritedView::update_should_notify`] so a rebuild that hands down
    /// the *same* handle again does not churn dependents.
    pub(crate) fn is_same(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }

    /// Insert `entry` at `position` and schedule a rebuild.
    ///
    /// Flutter's `OverlayState.insert` (`overlay.dart:742-749`).
    pub(crate) fn insert(&self, entry: &OverlayEntry, position: &InsertPosition) {
        self.insert_all(std::slice::from_ref(entry), position);
    }

    /// Insert `entries` as a contiguous group at `position`, preserving their
    /// relative order, and schedule a rebuild.
    ///
    /// Flutter's `OverlayState.insertAll` (`overlay.dart:758-771`), which
    /// early-returns on an empty iterable.
    pub(crate) fn insert_all(&self, entries: &[OverlayEntry], position: &InsertPosition) {
        if entries.is_empty() {
            return;
        }
        for entry in entries {
            entry.attach(&self.shared);
        }
        {
            let mut list = self.shared.entries.lock();
            let index = insertion_index(&list, position);
            list.splice(index..index, entries.iter().cloned());
        }
        self.shared.schedule_rebuild();
    }

    /// Reorder the overlay to `new_entries`, then place any entry **not**
    /// mentioned on top of them, preserving that group's relative order.
    ///
    /// Flutter's `OverlayState.rearrange` (`overlay.dart:813-846`) with neither
    /// `above:` nor `below:` — the only form `Navigator._flushHistoryUpdates`
    /// uses (`navigator.dart:4612`), where `newEntries` names every entry anyway.
    /// Entries in `new_entries` that the overlay does not hold are inserted, as
    /// Flutter documents (`:798`).
    ///
    /// Two of Flutter's guards are ported: the empty early-return (`:830`), and
    /// the `listEquals` short-circuit (`:833`) that makes a no-op reorder cost
    /// **no rebuild** — pinned by `overlay_rearrange_to_the_same_order_is_a_noop`.
    ///
    /// **Deferred:** the `above:` / `below:` placement of the unmentioned group.
    /// Nothing needs it yet; `Navigator` never passes either.
    pub(crate) fn rearrange(&self, new_entries: &[OverlayEntry]) {
        if new_entries.is_empty() {
            return;
        }

        for entry in new_entries {
            entry.attach(&self.shared);
        }

        {
            let mut list = self.shared.entries.lock();

            if list.len() == new_entries.len()
                && list
                    .iter()
                    .zip(new_entries)
                    .all(|(old, new)| old.is_same(new))
            {
                return; // listEquals short-circuit: no mutation, no rebuild.
            }

            // Entries the overlay holds that `new_entries` does not name, in
            // their existing relative order. Flutter keeps these as a group and,
            // with no `above`/`below`, leaves them on top (`:798-811`, `:845`).
            let unmentioned: Vec<OverlayEntry> = list
                .iter()
                .filter(|held| !new_entries.iter().any(|new| new.is_same(held)))
                .cloned()
                .collect();

            list.clear();
            list.extend(new_entries.iter().cloned());
            list.extend(unmentioned);
        }

        self.shared.schedule_rebuild();
    }
}

impl fmt::Debug for OverlayHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OverlayHandle")
            .field("entries", &self.len())
            .field("mounted", &self.is_mounted())
            .finish()
    }
}

/// Flutter's `_insertionIndex` (`overlay.dart:660-669`).
///
/// An `Above`/`Below` naming an entry the overlay does not hold falls back to
/// `Top`. Flutter would return `-1` from `indexOf` and then either insert at
/// `-1` (a runtime error) or at `0`; neither is a contract worth porting, and
/// [`PANIC-POLICY`](../../../../docs/PANIC-POLICY.md) reserves panics for
/// framework invariants, not caller mistakes.
fn insertion_index(entries: &[OverlayEntry], position: &InsertPosition) -> usize {
    let find = |needle: &OverlayEntry| entries.iter().position(|held| held.is_same(needle));
    match position {
        InsertPosition::Top => entries.len(),
        InsertPosition::Below(below) => find(below).unwrap_or(entries.len()),
        InsertPosition::Above(above) => find(above).map_or(entries.len(), |index| index + 1),
    }
}

/// Which entries [`OverlayState::build`] puts in the tree, and how many of them
/// the [`Theater`] holds offstage.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct OnstagePlan {
    /// Indices into the entry list, bottom → top. A covered entry without
    /// `maintain_state` is absent.
    pub(crate) build: Vec<usize>,
    /// How many leading entries of `build` are covered by an opaque entry.
    pub(crate) skip_count: usize,
}

/// `OverlayState.build`'s onstage loop, as pure data (`overlay.dart:888-918`).
///
/// Flutter walks `_entries.reversed` — top first — adding children until it
/// passes an `opaque` entry, then adding only the `maintainState` ones below it.
/// It reverses once at the end, which is why the covered entries land at the
/// front of the list and `skipCount` counts a *prefix*.
pub(crate) fn onstage_plan(entries: &[OverlayEntry]) -> OnstagePlan {
    let mut build = Vec::new();
    let mut onstage = true;
    let mut onstage_count = 0usize;

    for (index, entry) in entries.iter().enumerate().rev() {
        if onstage {
            onstage_count += 1;
            build.push(index);
            if entry.opaque() {
                onstage = false;
            }
        } else if entry.maintain_state() {
            // Flutter also passes `tickerEnabled: false` here; FLUI has no
            // per-subtree ticker gate. See `entry`'s module docs.
            build.push(index);
        }
    }

    let skip_count = build.len() - onstage_count;
    build.reverse();
    OnstagePlan { build, skip_count }
}

// ============================================================================
// THE OVERLAY VIEW
// ============================================================================

/// A stack of entries, each an independently-rebuildable layer.
///
/// The entry list lives in the [`OverlayHandle`] the caller supplies, so it
/// survives this view being rebuilt and can be mutated from outside the tree.
/// Flutter's `Overlay.initialEntries` (`overlay.dart:655-658`, inserted in
/// `initState`) has no analogue: insert into the handle before mounting instead.
#[derive(Clone)]
pub struct Overlay {
    handle: OverlayHandle,
}

impl Overlay {
    /// An overlay backed by `handle`.
    pub(crate) fn new(handle: OverlayHandle) -> Self {
        Self { handle }
    }

    /// The nearest ancestor [`Overlay`]'s handle, registering a dependency so
    /// this element rebuilds if a *different* overlay identity ever replaces
    /// the one found here — a FLUI-native divergence from the oracle; see
    /// [`maybe_of`](Self::maybe_of)'s doc for why.
    ///
    /// # Panics
    ///
    /// Panics if there is no `Overlay` ancestor. Use
    /// [`maybe_of`](Self::maybe_of) for a non-panicking variant.
    ///
    /// Flutter parity (API shape, not the dependency behavior below):
    /// `Overlay.of(context)` (`.flutter/packages/flutter/lib/src/widgets/overlay.dart`,
    /// tag `3.44.0`).
    #[must_use]
    pub fn of(ctx: &dyn BuildContext) -> OverlayHandle {
        Self::maybe_of(ctx).expect(
            "Overlay::of called with no Overlay ancestor in the tree — wrap the \
             subtree in a Navigator (which mounts one) or an Overlay directly, \
             or use Overlay::maybe_of with a caller-chosen fallback",
        )
    }

    /// Look up the nearest ancestor [`Overlay`]'s handle, registering a
    /// dependency. Returns `None` if there is no `Overlay` ancestor.
    ///
    /// # Depend, not get — a FLUI-native divergence, not oracle parity
    ///
    /// Resolves via [`BuildContextExt::depend_on`], not the lookup-only `get`.
    /// **This is not what the oracle does**: Flutter 3.44's `Overlay.maybeOf`
    /// calls the private `_RenderTheaterMarker.maybeOf` with
    /// `createDependency: false` explicitly (`overlay.dart`) — `_RenderTheaterMarker`'s
    /// own `maybeOf` helper defaults that parameter to `true`, but `Overlay.maybeOf`
    /// overrides it to `false`, and `Overlay.of` routes through `maybeOf`. So
    /// neither oracle entry point registers a dependency at all; a
    /// dependency-free `get` would in fact be the *loyal* port.
    ///
    /// `depend_on` is used anyway, deliberately: it is what makes
    /// `Overlay::maybe_of` re-fire from `did_change_dependencies` if a
    /// *different* overlay identity ever replaces the resolved one. That
    /// re-resolution is load-bearing here in a way the oracle never needs it
    /// to be. Flutter's `_DragAvatar.update` can call `Overlay.of(context)`
    /// fresh, on demand, because Dart closures keep `context` alive for free.
    /// FLUI's `MultiDragHandle` (what `DragSession`, the thing that would
    /// need the overlay mid-drag, implements) is `Send + Sync` and holds no
    /// `BuildContext` at all — the handle has to be resolved and cached
    /// *ahead of time*, in a lifecycle hook, for a gesture callback with no
    /// `BuildContext` to read later (see `draggable.rs`'s `DraggableState`).
    /// `depend_on` is what keeps that cached value honest if the ancestor
    /// ever changes underneath it; `get`, resolved once and never
    /// re-checked, would silently go stale. This differs from
    /// `ScaffoldScope::maybe_of` (`flui-material`), which uses `get` for the
    /// same reason the oracle would here too: nothing there needs to survive
    /// past the immediate lookup into a context-free callback.
    ///
    /// Resolves an `OverlayScope` marker (crate-internal) mounted **per entry** (wrapping
    /// that entry's built child, not once per `Overlay`) — the 3.44.0 oracle's
    /// own shift from `findAncestorStateOfType<OverlayState>` to resolving a
    /// private `_RenderTheaterMarker` `InheritedWidget` each
    /// `_OverlayEntryWidgetState` mounts around its entry's child. A nested
    /// `Overlay`'s own entries therefore see the nearest enclosing overlay,
    /// falling out of the ordinary inherited-map nearest-wins shadowing with
    /// no extra code here.
    ///
    /// Flutter parity: `Overlay.maybeOf(context)`.
    #[must_use]
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<OverlayHandle> {
        ctx.depend_on::<OverlayScope, _>(|scope| scope.data().clone())
    }
}

impl fmt::Debug for Overlay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Overlay")
            .field("entries", &self.handle.len())
            .finish()
    }
}

impl View for Overlay {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl StatefulView for Overlay {
    type State = OverlayState;

    fn create_state(&self) -> Self::State {
        OverlayState {
            shared: Arc::clone(&self.handle.shared),
        }
    }
}

/// Persistent state for [`Overlay`].
///
/// Holds the shared entry list. The list is `Arc`-shared rather than owned
/// outright because `ViewState::build` takes `&self` and no caller can ever
/// obtain `&mut OverlayState`.
///
/// `pub` only because [`StatefulView::State`] must be at least as visible as
/// [`Overlay`] itself — its field stays private, and nothing outside this
/// module constructs or names it.
pub struct OverlayState {
    shared: Arc<OverlayShared>,
}

impl fmt::Debug for OverlayState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OverlayState")
            .field("entries", &self.shared.entries.lock().len())
            .finish_non_exhaustive()
    }
}

impl ViewState<Overlay> for OverlayState {
    /// Publish the rebuild capability so [`OverlayHandle`] mutations, which run
    /// outside any frame phase, can schedule this element.
    ///
    /// `init_state` is the correct hook and the only permitted one: port-check
    /// trigger #22 rejects acquiring a `RebuildHandle` from `build`/layout/paint.
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        *self.shared.rebuild.lock() = Some(ctx.rebuild_handle());
    }

    /// Bottom → top: `entries[i]` paints below `entries[i + 1]`, because
    /// [`Theater`] paints its children in order.
    ///
    /// A line-for-line port of `OverlayState.build` (`overlay.dart:886-918`).
    /// The loop runs **top-first** over `_entries.reversed`, so `children` comes
    /// out top→bottom and is reversed once at the end; `skip_count` therefore
    /// counts the covered `maintain_state` entries, which are the leading ones.
    fn build(&self, _view: &Overlay, _ctx: &dyn BuildContext) -> impl IntoView {
        let entries = self.shared.entries.lock();
        let plan = onstage_plan(&entries);
        // The handle every entry's `OverlayScope` marker provides to
        // `Overlay::of`/`maybe_of` — the same `Arc` this `OverlayState` was
        // constructed from, so `OverlayHandle::is_same` matches it.
        let handle = OverlayHandle {
            shared: Arc::clone(&self.shared),
        };
        let children: Vec<BoxedView> = plan
            .build
            .iter()
            .map(|&index| OverlayEntryView::new(entries[index].clone(), handle.clone()).boxed())
            .collect();
        Theater::new(children, plan.skip_count)
    }

    /// Drop the rebuild capability, making every surviving [`OverlayHandle`]
    /// inert. Flutter gets this from `_markDirty`'s `if (mounted)` guard
    /// (`overlay.dart:849`).
    fn dispose(&mut self) {
        *self.shared.rebuild.lock() = None;
    }
}

// ============================================================================
// THE PER-ENTRY VIEW
// ============================================================================

/// The child the [`Overlay`] builds for each entry.
///
/// Flutter's `_OverlayEntryWidget` (`overlay.dart:297`), which is likewise
/// `Stateful` — and for the same primary reason: it is the thing
/// `markNeedsBuild` rebuilds on its own, without touching the `Overlay`.
///
/// Keyed by [`OverlayEntryId`] so a `rearrange` reorder is a permutation the
/// keyed reconciler recognises, preserving each layer's subtree state. Flutter
/// spends a `GlobalKey` on this (`overlay.dart:214`); a plain [`ValueKey`] is
/// enough, because the moves are always among siblings of one parent.
#[derive(Clone)]
struct OverlayEntryView {
    entry: OverlayEntry,
    /// The enclosing [`Overlay`]'s handle, provided to this entry's built
    /// child through an [`OverlayScope`] marker so `Overlay::of`/`maybe_of`
    /// can resolve it.
    overlay: OverlayHandle,
    key: ValueKey<u64>,
}

impl OverlayEntryView {
    fn new(entry: OverlayEntry, overlay: OverlayHandle) -> Self {
        let key = ValueKey::new(entry.id().get());
        Self {
            entry,
            overlay,
            key,
        }
    }
}

impl View for OverlayEntryView {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }

    /// Written by hand rather than derived: `#[derive(StatefulView)]` emits its
    /// own `impl View`, and Rust forbids a second one, so a keyed view must own
    /// the whole impl (`flui-macros` documents this).
    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

impl StatefulView for OverlayEntryView {
    type State = OverlayEntryViewState;

    fn create_state(&self) -> Self::State {
        OverlayEntryViewState {
            entry: self.entry.clone(),
        }
    }
}

/// Persistent state for one overlay layer.
pub(crate) struct OverlayEntryViewState {
    entry: OverlayEntry,
}

impl ViewState<OverlayEntryView> for OverlayEntryViewState {
    /// Hand this element's rebuild capability to the entry, so
    /// [`OverlayEntry::mark_needs_build`] rebuilds this layer alone.
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        self.entry.publish_rebuild(ctx.rebuild_handle());
    }

    /// Build from `view`, not `self`: the element may have been reconciled onto a
    /// fresh `OverlayEntryView`. Both name the same entry (the key guarantees it),
    /// but reading the current view is the contract.
    ///
    /// Wraps the entry's built child in an [`OverlayScope`] marker — the
    /// per-entry mount point `Overlay::of`/`maybe_of` resolve against
    /// (ADR-0036), matching the 3.44.0 oracle's `_OverlayEntryWidgetState`,
    /// which wraps each entry's child in its own `_RenderTheaterMarker`.
    fn build(&self, view: &OverlayEntryView, ctx: &dyn BuildContext) -> impl IntoView {
        OverlayScope::new(view.overlay.clone(), (view.entry.builder())(ctx))
    }

    /// The keyed reconciler must never hand this element a *different* entry: the
    /// `ValueKey<OverlayEntryId>` makes an entry's view matchable only against
    /// itself. If this fires, `OverlayEntryView::key` was lost and every layer's
    /// published `RebuildHandle` now points at the wrong element.
    fn did_update_view(&mut self, old: &OverlayEntryView, new: &OverlayEntryView) {
        debug_assert!(
            old.entry.is_same(&new.entry),
            "BUG: an OverlayEntryView element was reconciled onto a different \
             OverlayEntry — the ValueKey<OverlayEntryId> should have prevented this"
        );
    }

    /// Revoke the capability, so a `mark_needs_build` after unmount is inert.
    fn dispose(&mut self) {
        self.entry.clear_rebuild();
    }
}

// ============================================================================
// THE LOOKUP MARKER
// ============================================================================

/// Marks the nearest enclosing [`Overlay`] for `Overlay::of`/`maybe_of`
/// lookups. Mounted **per entry**, wrapping that entry's built child — never
/// once per `Overlay` — by [`OverlayEntryViewState::build`].
///
/// This is FLUI's analogue of the 3.44.0 oracle's private
/// `_RenderTheaterMarker`: `_OverlayEntryWidgetState.build` wraps each
/// entry's child in one, and `Overlay.maybeOf` resolves it via
/// `dependOnInheritedWidgetOfExactType`. Earlier Flutter releases used
/// `context.findAncestorStateOfType<OverlayState>()` instead — a lookup with
/// no dependency and no per-entry granularity. `OverlayScope` stays
/// `pub(crate)`, matching its oracle counterpart's own privacy: nothing
/// outside `overlay` ever names it directly — [`Overlay::of`]/[`Overlay::maybe_of`]
/// are the only door.
#[derive(Clone)]
pub(crate) struct OverlayScope {
    handle: OverlayHandle,
    child: BoxedView,
}

impl OverlayScope {
    /// Wrap `child` in a scope that provides `handle` — the enclosing
    /// `Overlay`'s handle — to `Overlay::of`/`maybe_of` lookups in `child`'s
    /// subtree.
    fn new(handle: OverlayHandle, child: impl IntoView) -> Self {
        Self {
            handle,
            child: BoxedView(Box::new(child.into_view())),
        }
    }
}

impl fmt::Debug for OverlayScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OverlayScope").finish_non_exhaustive()
    }
}

impl InheritedView for OverlayScope {
    type Data = OverlayHandle;

    fn data(&self) -> &Self::Data {
        &self.handle
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    /// An `OverlayEntryView` element is reconciled in place across ordinary
    /// rebuilds of the *same* mounted entry, and its `overlay` field never
    /// changes for that entry's lifetime — so in production this compares
    /// the same handle to itself and is always `false`. It is still handle
    /// **identity**, not structural/derived equality, because the contract
    /// this type exists to satisfy is `InheritedView`'s in general, not just
    /// the one call site that happens to exercise it today.
    fn update_should_notify(&self, old: &Self) -> bool {
        !self.handle.is_same(&old.handle)
    }
}

impl_inherited_view!(OverlayScope);
