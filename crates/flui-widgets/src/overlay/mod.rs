//! [`Overlay`] — an insertion-ordered stack of independently-managed layers.
//!
//! ADR-0019 U1, the first prerequisite for `Navigator`. **Private to
//! `flui-widgets`**: nothing here is exported from the crate root or the prelude
//! until ADR-0019 U4's parity + sign-off gate.
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
//! # No new render object
//!
//! Flutter's `_RenderTheater` (`overlay.dart:1194`) is *not* a `RenderStack`; it
//! reimplements the stack algorithm so it can skip the first `skipCount`
//! children. But `skipCount` exists **only** to serve `opaque`-skipping: entries
//! below the topmost `opaque` entry are dropped from the widget tree unless they
//! set `maintainState`, and the ones kept are the ones skipped in layout, paint,
//! hit-test and semantics (`:888-918`, `:1344-1355`, `:1427-1428`).
//!
//! With `skipCount == 0` and `alwaysSizeToContent == false`, `_RenderTheater`
//! lays every child out with `BoxConstraints.tight(size)` where
//! `size = constraints.biggest` — which its own source comments as "Equivalent to
//! BoxConstraints used by RenderStack for StackFit.expand" (`:1478-1484`). So
//! this `Overlay` builds a plain [`Stack`] with [`StackFit::Expand`], exactly as
//! ADR-0019 §2.2 predicted, and adds no render object.
//!
//! # `opaque` / `maintainState` are deferred, not claimed
//!
//! Every entry is built, laid out and painted, every frame.
//!
//! - **Cost:** `O(number of entries)` wasted layout + paint once a full-screen
//!   layer covers the ones beneath it.
//! - **Behavioral consequence:** Flutter's `maintainState == false` — a covered
//!   entry's state is *destroyed* and rebuilt fresh when it is uncovered — is
//!   unobservable here, because nothing is ever unbuilt. FLUI preserves strictly
//!   *more* state than Flutter, never less, so no correctness contract is broken;
//!   but parity for `opaque`/`maintainState` is **not** claimed and must not be
//!   reported as done. `overlay_deferred_opaque_builds_every_entry` pins the
//!   current behavior so the day someone implements skipping, it goes red.
//! - **Input isolation must not lean on this.** With no skipping, covered layers
//!   remain hit-testable; a modal layer has to absorb pointers itself
//!   (`AbsorbPointer`). ADR-0019 §2.4.
//! - **Upgrade path:** a dedicated `RenderTheater` carrying `skip_count`, plus a
//!   port of `OverlayState.build`'s onstage loop.
//!
//! # Threading and locks
//!
//! [`OverlayHandle`] is the ADR-0019 §3.2 answer to "how does something outside
//! the tree mutate the overlay?": an owned, `'static`, cloneable capability, not
//! a `GlobalKey` lookup. Mutation takes a private `Mutex` and then schedules a
//! rebuild through the [`RebuildHandle`] the state published at `init_state`.
//! No element-tree borrow is held, no second lock is taken under a first, and no
//! `GlobalKey` registry is consulted. `Navigator` (U3) will reach its overlay the
//! same way.
//!
//! [`RebuildHandle`]: flui_view::RebuildHandle

// `Navigator` (ADR-0019 U3) is the intended consumer; until it lands, nothing in
// `flui-widgets` calls this module, and nothing outside can (it is private until
// the U4 gate). Same posture as ADR-0018's `FutureBuilder` between U4 and U6 —
// and, like it, this attribute must be **deleted** when the consumer arrives, so
// genuinely-dead code cannot hide behind it.
#![allow(dead_code)]

mod entry;

#[cfg(test)]
mod tests;

use std::fmt;
use std::sync::Arc;

pub(crate) use entry::{OverlayEntry, OverlayEntryId};
use flui_foundation::ViewKey;
use flui_types::layout::StackFit;
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_view::{BoxedView, RebuildHandle, ValueKey};
use parking_lot::Mutex;

use crate::Stack;

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
/// outside the tree (from a `Navigator`'s route flush, in U2), so the list cannot
/// live behind `&mut OverlayState` — nothing can obtain one. See ADR-0019 §3.2.
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

    /// Retain the entries matching `keep`. Used by [`OverlayEntry::remove`].
    pub(crate) fn retain_entries(&self, keep: impl FnMut(&OverlayEntry) -> bool) {
        self.entries.lock().retain(keep);
    }
}

/// An owned, `'static` capability to mutate an [`Overlay`]'s entry list.
///
/// Create one, hand it to [`Overlay::new`], and keep a clone: every clone names
/// the same overlay. Mutating before mount is legal — the first build reads
/// whatever the list holds — and mutating after unmount is a silent no-op.
///
/// This replaces Flutter's `GlobalKey<OverlayState>` (`navigator.dart:3746`),
/// which `Navigator` uses purely to call `rearrange`. ADR-0019 §3.2 records why
/// the `GlobalKey` route is not merely unnecessary but hazardous here.
#[derive(Clone)]
pub(crate) struct OverlayHandle {
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
pub(crate) struct Overlay {
    handle: OverlayHandle,
}

impl Overlay {
    /// An overlay backed by `handle`.
    pub(crate) fn new(handle: OverlayHandle) -> Self {
        Self { handle }
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
/// obtain `&mut OverlayState` — see ADR-0019 §3.2.
pub(crate) struct OverlayState {
    shared: Arc<OverlayShared>,
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
    /// [`Stack`] paints its children in order and [`RenderStack`] is
    /// last-child-on-top — the same contract as `_RenderTheater.paint`
    /// (`overlay.dart:1157-1161`).
    ///
    /// [`RenderStack`]: flui_objects::RenderStack
    fn build(&self, _view: &Overlay, _ctx: &dyn BuildContext) -> impl IntoView {
        let children: Vec<BoxedView> = self
            .shared
            .entries
            .lock()
            .iter()
            .cloned()
            .map(|entry| OverlayEntryView::new(entry).boxed())
            .collect();

        // `StackFit::Expand` reproduces `_RenderTheater.performLayout`'s
        // `BoxConstraints.tight(constraints.biggest)` for every child
        // (`overlay.dart:1478-1484`).
        Stack::new(children).fit(StackFit::Expand)
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
    key: ValueKey<u64>,
}

impl OverlayEntryView {
    fn new(entry: OverlayEntry) -> Self {
        let key = ValueKey::new(entry.id().get());
        Self { entry, key }
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
    fn build(&self, view: &OverlayEntryView, ctx: &dyn BuildContext) -> impl IntoView {
        (view.entry.builder())(ctx)
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
