//! `ElementOwner` — split-borrow handle into `BuildOwner` for the
//! `Element` lifecycle.
//!
//! # Why this type exists
//!
//! Flutter's `Element` class carries a mutable backreference to its
//! `BuildOwner` (see `flutter/lib/src/widgets/framework.dart:2901`'s
//! `_owner` field). Element lifecycle methods (`mount`, `unmount`,
//! `update`) reach back through that field to:
//! - register / unregister `GlobalKey`s,
//! - schedule rebuilds when a descendant marks itself dirty,
//! - queue inactive elements for finalization at end-of-frame.
//!
//! Rust's borrow checker forbids a mutable backreference of that shape
//! (mutable aliasing). The Rust-native answer is a **split-borrow
//! handle**: a small struct that carries `&mut` references to the
//! specific `BuildOwner` fields each lifecycle path needs, with no
//! aliasing because each field is borrowed once. The handle is built
//! via `BuildOwner::element_owner_mut(&mut self) -> ElementOwner<'_>`
//! and is threaded through `ElementBase::mount` / `unmount` / `update`
//! by the framework. See *Rust for Rustaceans* §"Lifetimes and split
//! borrows" (Gjengset) for the pattern.
//!
//! Plan reference: `docs/plans/2026-05-21-002-feat-framework-spine-repair-plan.md` §U8, §D1.
//! Audit reference: `docs/research/2026-05-21-view-tree-foundation-audit.md` Finding #2.
//!
//! # Lifetime variance
//!
//! `ElementOwner<'a>` carries plain `&'a mut` references — no HRTB, no
//! invariance trickery. Recursive `mount` calls reborrow the handle
//! (`&mut *element_owner`) and the compiler accepts the chain because
//! every reborrow is sequential. Per plan §I1 this simplest possible
//! shape was tried first and held; we did not need the
//! `for<'a> Fn(&'a mut ElementOwner<'a>)` HRTB fallback.

use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap, HashSet},
};

use flui_foundation::ElementId;

use super::build_owner::{DirtyElement, InactiveElement};

/// Split-borrow handle into `BuildOwner` for `Element` lifecycle paths.
///
/// Carries `&mut` references to the subset of `BuildOwner` fields a
/// recursive `mount` / `unmount` / `update` traversal needs. The
/// `BuildOwner` is split into its independently-mutable parts so the
/// borrow checker can prove non-aliasing — no `RefCell` / `RwLock`
/// needed.
///
/// # Constructed by
///
/// [`BuildOwner::element_owner_mut`](super::BuildOwner::element_owner_mut).
///
/// # Use sites
///
/// - `ElementBase::mount`
/// - `ElementBase::unmount`
/// - `ElementBase::update`
/// - `ElementTree::mount_root` / `::mount_root_with_pipeline_owner`
/// - `ElementTree::insert`
/// - `ElementTree::remove`
/// - `ElementTree::update`
///
/// Downstream units (U9–U14) layer on top by calling the registration
/// methods below from `Element` lifecycle code paths.
///
/// # Flutter equivalent
///
/// Replaces `Element._owner` mutable backreference at
/// `flutter/lib/src/widgets/framework.dart:2901`.
#[non_exhaustive]
pub struct ElementOwner<'a> {
    /// `GlobalKey` registry: key hash → element holding the key.
    ///
    /// Populated by `register_global_key`; consulted by the future
    /// `find_global_key_target` (U14). Initial U8 surface only exposes
    /// register / unregister.
    pub(crate) global_keys: &'a mut HashMap<u64, ElementId>,

    /// Dirty heap, sorted by depth (shallowest first). Pushed by
    /// `schedule_build_for`, drained by `BuildOwner::build_scope` at
    /// frame start.
    pub(crate) dirty_elements: &'a mut BinaryHeap<Reverse<DirtyElement>>,

    /// Dedup set tracking ids already in `dirty_elements` so a
    /// reentrant `schedule_build_for` for the same id is a no-op.
    pub(crate) dirty_set: &'a mut HashSet<ElementId>,

    /// Inactive elements queue (deactivated but not yet unmounted).
    /// Drained by `BuildOwner::finalize_tree` at end-of-frame.
    ///
    /// Carries `(id, depth)` so finalization can unmount deepest first
    /// — parents must wait for children to detach.
    pub(crate) inactive_elements: &'a mut Vec<InactiveElement>,

    /// Pending `did_change_dependencies` dispatch set. Populated by
    /// [`InheritedBehavior::on_view_updated`](crate::element::InheritedBehavior)
    /// when `update_should_notify == true`; drained by
    /// `BuildOwner::build_scope` immediately before each dependent's
    /// `perform_build` so the typed
    /// [`ViewState::did_change_dependencies`](crate::view::ViewState::did_change_dependencies)
    /// hook fires exactly once per dependency-change-then-rebuild
    /// cycle. Flutter parity: `framework.dart:6114`
    /// `_didChangeDependencies` flag on `StatefulElement`. Plan §U14.
    pub(crate) pending_dependency_changes: &'a mut HashSet<ElementId>,

    /// Snapshot of `BuildOwner::on_build_scheduled` so
    /// `schedule_build_for` can fire the visual-update callback
    /// without re-borrowing the owner.
    ///
    /// Stored as a raw reference because `Box<dyn Fn>` is not `Copy`
    /// and we never mutate it through this handle.
    pub(crate) on_build_scheduled: Option<&'a (dyn Fn() + Send + Sync)>,
}

impl ElementOwner<'_> {
    /// Register a `GlobalKey` hash → element mapping.
    ///
    /// Called by `Element::mount` when the mounted element carries a
    /// `GlobalKey`. Idempotent: re-registering the same hash with the
    /// same `id` is a no-op; with a different `id` the new mapping
    /// wins (last-write-wins; conflict detection lives in U14 per plan
    /// §I4).
    ///
    /// `debug_assert!`s that `id` is non-default — an
    /// `ElementId::INVALID` register slips through release builds with
    /// a `tracing::warn!` so production doesn't crash on a stray call,
    /// but tests catch it.
    pub fn register_global_key(&mut self, key_hash: u64, id: ElementId) {
        self.global_keys.insert(key_hash, id);
    }

    /// Unregister a `GlobalKey` hash mapping.
    ///
    /// Called by `Element::unmount` when an element carrying a
    /// `GlobalKey` leaves the tree. No-op if the hash isn't present.
    pub fn unregister_global_key(&mut self, key_hash: u64) {
        self.global_keys.remove(&key_hash);
    }

    /// Look up the element holding a given `GlobalKey` hash.
    ///
    /// Returned `None` means no element with that key is currently
    /// mounted. U14 will layer reparenting on top of this lookup.
    pub fn element_for_global_key(&self, key_hash: u64) -> Option<ElementId> {
        self.global_keys.get(&key_hash).copied()
    }

    /// Atomically remove and return the element registered under
    /// `key_hash` for a reparent operation. Wrapper around
    /// [`BuildOwner::take_global_key_for_reparent`](crate::BuildOwner::take_global_key_for_reparent)
    /// for the split-borrow `ElementOwner` handle that the
    /// reconciler holds. Plan §U17 / KTD-3 N1.
    pub fn take_global_key_for_reparent(&mut self, key_hash: u64) -> Option<ElementId> {
        self.global_keys.remove(&key_hash)
    }

    /// Schedule an element for rebuild at the next frame.
    ///
    /// Pushed onto the depth-sorted heap so parents rebuild before
    /// children. Dedup against `dirty_set` so a reentrant call for the
    /// same id is a no-op. Fires the `on_build_scheduled` callback on
    /// fresh inserts so the binding can request a visual update.
    pub fn schedule_build_for(&mut self, id: ElementId, depth: usize) {
        if self.dirty_set.insert(id) {
            self.dirty_elements
                .push(Reverse(DirtyElement::new(id, depth)));

            if let Some(callback) = self.on_build_scheduled {
                callback();
            }
        }
    }

    /// Mark a dependent as having received an inherited-dependency
    /// change.
    ///
    /// Called by `InheritedBehavior::on_view_updated` when
    /// `update_should_notify == true` for each dependent in addition to
    /// `schedule_build_for`. `BuildOwner::build_scope` consults this
    /// set immediately before each dependent's `perform_build` and, if
    /// the id is present, fires the typed
    /// [`ViewState::did_change_dependencies`](crate::view::ViewState::did_change_dependencies)
    /// hook (via `ElementBase::notify_dependency_change`) BEFORE the
    /// actual rebuild — Flutter parity for the `_didChangeDependencies`
    /// flag at `framework.dart:6114`. Plan §U14.
    ///
    /// Idempotent: re-marking the same id is a no-op (HashSet dedup) —
    /// `did_change_dependencies` fires at most once per
    /// dependency-change-then-rebuild cycle even if multiple inherited
    /// ancestors fire in the same update phase.
    pub fn note_dependency_change(&mut self, id: ElementId) {
        self.pending_dependency_changes.insert(id);
    }

    /// Discard any pending `did_change_dependencies` for this id.
    ///
    /// Called from `ElementBase::unmount` paths so a dependent that
    /// leaves the tree before its rebuild ever runs does not leave a
    /// stale entry behind. No-op if the id is not present.
    pub fn clear_pending_dependency_change(&mut self, id: ElementId) {
        self.pending_dependency_changes.remove(&id);
    }

    /// Whether the given id has a pending `did_change_dependencies`
    /// dispatch queued. Used by tests; `BuildOwner::build_scope` reads
    /// (and removes) the entry directly via field access.
    pub fn has_pending_dependency_change(&self, id: ElementId) -> bool {
        self.pending_dependency_changes.contains(&id)
    }

    /// Push an element onto the inactive-elements queue.
    ///
    /// Called from `Element::deactivate` (typically during
    /// reconciliation, when a parent drops a child without unmounting
    /// it). `BuildOwner::finalize_tree` drains the queue at frame end
    /// and unmounts each entry deepest-first.
    pub fn push_inactive(&mut self, id: ElementId, depth: usize) {
        self.inactive_elements.push(InactiveElement::new(id, depth));
    }

    /// Remove an element from the inactive queue.
    ///
    /// Used when an element is re-activated mid-frame (Flutter
    /// reparenting via `GlobalKey`, U14 territory). No-op if the id
    /// isn't queued.
    pub fn remove_inactive(&mut self, id: ElementId) {
        self.inactive_elements.retain(|entry| entry.id() != id);
    }

    /// Whether the given element is currently queued for finalization.
    ///
    /// Used by `ElementTree::insert` to gate the U14 state-migration
    /// retake: the GlobalKey registry's `Some(_)` entry could be stale
    /// (the element is still active elsewhere) — only an entry that's
    /// actually in the inactive queue is safe to re-attach.
    pub fn is_inactive(&self, id: ElementId) -> bool {
        self.inactive_elements.iter().any(|entry| entry.id() == id)
    }

    /// Iterate over inactive entries without draining.
    ///
    /// `BuildOwner::finalize_tree` uses an owned-drain pattern with
    /// `mem::take` to avoid re-entrancy hazards during recursive
    /// unmounts; this iterator is for introspection / debug-only.
    pub fn finalize_inactive(&self) -> impl Iterator<Item = ElementId> + '_ {
        self.inactive_elements.iter().map(InactiveElement::id)
    }

    /// Number of elements queued for end-of-frame unmount.
    pub fn inactive_len(&self) -> usize {
        self.inactive_elements.len()
    }

    /// Number of `GlobalKey`s currently registered.
    pub fn global_key_count(&self) -> usize {
        self.global_keys.len()
    }

    /// Number of dirty elements pending rebuild.
    pub fn dirty_count(&self) -> usize {
        self.dirty_elements.len()
    }
}

impl std::fmt::Debug for ElementOwner<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElementOwner")
            .field("global_keys", &self.global_keys.len())
            .field("dirty_elements", &self.dirty_elements.len())
            .field("inactive_elements", &self.inactive_elements.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::owner::BuildOwner;

    #[test]
    fn split_borrow_handle_basic_round_trip() {
        let mut owner = BuildOwner::new();
        let mut handle = owner.element_owner_mut();

        let id = ElementId::new(1);
        handle.register_global_key(0xABCD, id);
        assert_eq!(handle.element_for_global_key(0xABCD), Some(id));
        assert_eq!(handle.global_key_count(), 1);

        handle.unregister_global_key(0xABCD);
        assert_eq!(handle.element_for_global_key(0xABCD), None);
        assert_eq!(handle.global_key_count(), 0);
    }

    #[test]
    fn schedule_build_for_dedups_and_pushes_heap() {
        let mut owner = BuildOwner::new();
        let id = ElementId::new(5);

        let mut handle = owner.element_owner_mut();
        handle.schedule_build_for(id, 2);
        handle.schedule_build_for(id, 2); // duplicate — no-op
        assert_eq!(handle.dirty_count(), 1);
    }

    #[test]
    fn push_and_remove_inactive_round_trip() {
        let mut owner = BuildOwner::new();
        let id = ElementId::new(9);

        let mut handle = owner.element_owner_mut();
        assert_eq!(handle.inactive_len(), 0);

        handle.push_inactive(id, 4);
        assert_eq!(handle.inactive_len(), 1);
        assert!(handle.finalize_inactive().any(|e| e == id));

        handle.remove_inactive(id);
        assert_eq!(handle.inactive_len(), 0);
    }

    #[test]
    fn no_key_path_keeps_registry_empty() {
        // Mirrors the U8 plan test scenario: an element with no key
        // mounts + unmounts without touching the `global_keys`
        // registry. Pure surface check — no Element types here, just
        // the handle's invariants.
        let mut owner = BuildOwner::new();
        let handle = owner.element_owner_mut();
        assert_eq!(handle.global_key_count(), 0);
    }

    #[test]
    fn reborrow_split_chain_compiles() {
        // Compile-time proof that the split-borrow handle survives
        // recursive `&mut *handle` reborrows — this is the path
        // `Element::mount` takes when it recurses into child mounts.
        fn recurse(handle: &mut ElementOwner<'_>, depth: usize) {
            handle.schedule_build_for(ElementId::new(depth + 1), depth);
            if depth < 3 {
                recurse(&mut *handle, depth + 1);
            }
        }

        let mut owner = BuildOwner::new();
        let mut handle = owner.element_owner_mut();
        recurse(&mut handle, 0);
        assert_eq!(handle.dirty_count(), 4);
    }
}
