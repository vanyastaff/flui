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
//! Audit reference: `docs/research/2026-05-21-view-tree-foundation-audit.md` Finding #2.
//!
//! # Lifetime variance
//!
//! `ElementOwner<'a>` carries plain `&'a mut` references — no HRTB, no
//! invariance trickery. Recursive `mount` calls reborrow the handle
//! (`&mut *element_owner`) and the compiler accepts the chain because
//! every reborrow is sequential. This simplest possible
//! shape was tried first and held; we did not need the
//! `for<'a> Fn(&'a mut ElementOwner<'a>)` HRTB fallback.

use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap, HashSet},
    rc::Rc,
    sync::Arc,
};

use flui_foundation::{ElementId, RenderId, ViewKey};
use flui_interaction::FocusManager;
use parking_lot::Mutex;

use flui_objects::BuildDuringLayoutCell;

use super::RebuildReason;
use super::build_owner::{DirtyElement, ExternalBuildScheduler, InactiveElement};
use super::global_key_registry::GlobalKeyRegistry;
use super::global_key_reservations::GlobalKeyReservations;
use super::global_key_scope::{self, GlobalKeyScope, OwnerTag};
use super::inherited_dependencies::{InheritedDependencies, ProviderIds};
use super::layout_builder::{LayoutBuilderEntry, LayoutBuilderRegistry};
use crate::element::child_manager::{ChildManager, ChildManagerRegistry};
use flui_foundation::RebuildReasons;

/// Borrowed live-tree access carried by [`ElementOwner`] while a
/// `build_scope` drain runs an element's `build()` (PR-K).
///
/// Lets a behavior construct a live
/// [`BuildCtx`](crate::context::BuildCtx) — reading the real tree and
/// buffering inherited dependents — WITHOUT changing the object-safe
/// `build_into_views` signature: the handle rides on the `ElementOwner`
/// that is already threaded into every build. `dep_sink` collects the
/// dependents recorded during the read-only build; `build_scope` drains it
/// onto the provider nodes once it holds `&mut tree` again.
///
/// Both references are `Copy`, so reading [`ElementOwner::build_view`] lifts
/// them out by value without keeping the owner borrowed.
#[derive(Clone, Copy)]
pub(crate) struct BuildHandle<'a> {
    pub(crate) tree: &'a crate::tree::ElementTree,
    pub(crate) dep_sink: &'a parking_lot::Mutex<Vec<crate::context::DependentRecord>>,
}

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
/// - `BuildOwner::build_scope` (carrying the live `BuildHandle`)
///
/// Downstream units layer on top by calling the registration methods below
/// from `Element` lifecycle code paths.
///
/// # Flutter equivalent
///
/// Replaces `Element._owner` mutable backreference at
/// `flutter/lib/src/widgets/framework.dart:2901`.
#[non_exhaustive]
pub struct ElementOwner<'a> {
    /// `GlobalKey` registry: key → element holding the key, indexed by
    /// `ViewKey::key_hash` and decided by `ViewKey::key_eq` so a hash
    /// collision between two distinct keys never conflates them.
    ///
    /// Populated by `register_global_key`; read by the retake machinery in
    /// `crate::tree::element_tree`.
    pub(crate) global_keys: &'a mut GlobalKeyRegistry,

    /// This frame's `GlobalKey` declarations (`parent -> child -> key`),
    /// verified at the frame boundary by `BuildOwner::finalize_tree`.
    ///
    /// Recorded by `reserve_global_key`, which every path that attaches or
    /// re-declares a keyed child funnels through.
    pub(crate) global_key_reservations: &'a mut GlobalKeyReservations,

    /// Dirty heap, sorted by depth (shallowest first). Pushed by
    /// `schedule_build_for`, drained by `BuildOwner::build_scope` at
    /// frame start.
    pub(crate) dirty_elements: &'a mut BinaryHeap<Reverse<DirtyElement>>,

    /// Dirty elements held in root/isolated build-scope buckets when this
    /// split-borrow was created. Lifecycle operations can add to the main heap
    /// but do not mutate those private buckets, so this snapshot keeps
    /// diagnostics authoritative without exposing queue internals.
    pub(crate) partitioned_dirty_count: usize,

    /// Accumulated causes for ids already in `dirty_elements`.
    pub(crate) dirty_reasons: &'a mut HashMap<ElementId, RebuildReasons>,

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
    /// `_didChangeDependencies` flag on `StatefulElement`.
    pub(crate) pending_dependency_changes: &'a mut HashSet<ElementId>,

    /// Sparse reverse index for inherited dependency ownership.
    pub(crate) inherited_dependencies: &'a mut InheritedDependencies,

    /// Snapshot of `BuildOwner::on_build_scheduled` so
    /// `schedule_build_for` can fire the visual-update callback
    /// without re-borrowing the owner.
    ///
    /// Stored as a raw reference because `Box<dyn Fn>` is not `Copy`
    /// and we never mutate it through this handle.
    pub(crate) on_build_scheduled: Option<&'a (dyn Fn() + Send + Sync)>,

    /// Reference to `BuildOwner::external_inbox`, so an element can capture a
    /// clone at mount (via [`Self::external_scheduler`]) for its mark-dirty
    /// callback to push onto from outside a frame.
    pub(crate) external_inbox: &'a Arc<Mutex<HashMap<ElementId, RebuildReasons>>>,

    /// The realm's tree-observer slot (ADR-0040), threaded to the tree
    /// primitives so mount/move/unmount facts are emitted at their funnels.
    /// `&mut` is load-bearing: the panic-detach policy clears the slot from
    /// inside the emission helper.
    pub(crate) tree_observer: &'a mut Option<Arc<dyn flui_foundation::observe::TreeObserver>>,

    /// Reference to `BuildOwner::on_build_scheduled` as the shareable `Arc`
    /// (the [`Self::on_build_scheduled`] field above is the `&dyn Fn` view used
    /// for the in-frame fire). An [`ExternalBuildScheduler`] clones this so a
    /// listener tick can request a frame from outside the build.
    pub(crate) external_request_frame: Option<&'a Arc<dyn Fn() + Send + Sync>>,

    /// Live-tree access during a `build_scope` drain (PR-K). `Some` only
    /// while an element's `build()` runs; `None` on every other lifecycle
    /// path (mount / unmount / update / reconcile). The behavior reads it
    /// to build a live [`BuildCtx`](crate::context::BuildCtx).
    pub(crate) build_view: Option<BuildHandle<'a>>,

    /// Registry of live lazy-sliver `ChildManager`s, keyed by sliver `RenderId`.
    ///
    /// Carried as a reference to the `Arc` (same pattern as `external_inbox`)
    /// so `SliverAdaptorBehavior::on_mount` / `on_unmount` can mutate the
    /// registry without re-borrowing `BuildOwner`. `service_child_requests` on
    /// `BuildOwner` reads it after all `ElementOwner` borrows drop.
    pub(crate) child_manager_registry: &'a ChildManagerRegistry,

    /// Split-borrow view of `BuildOwner::layout_builder_registry`,
    /// so a build-during-layout element can register its
    /// `(RenderId -> ElementId + LayoutConstraintsCell)` entry at mount and drop
    /// it at unmount — the same shape as `child_manager_registry`.
    pub(crate) layout_builder_registry: &'a LayoutBuilderRegistry,

    /// The exact focus manager owned by the surrounding `BuildOwner`.
    ///
    /// Cloned into each live `BuildCtx`; it is never looked up through
    /// process-global or thread-local state.
    pub(crate) focus_manager: &'a Rc<FocusManager>,

    /// The binding's async task driver, or `None` when no
    /// binding installed one. Cloned into the live `BuildCtx` so a
    /// `ViewState::init_state` can spawn a subscription.
    pub(crate) async_driver: &'a Option<flui_scheduler::AsyncDriver>,
    /// The binding's post-frame capability, threaded into every
    /// `BuildCtx` so a `ViewState` can acquire it from a lifecycle hook.
    pub(crate) post_frame_handle: &'a Option<flui_scheduler::PostFrameHandle>,

    /// The binding's owner-local post-frame capability, threaded into every
    /// `BuildCtx` the same way `post_frame_handle` is.
    pub(crate) local_post_frame_handle: &'a Option<flui_scheduler::LocalPostFrameHandle>,

    /// The binding's IME/text-input attach-detach capability, threaded into
    /// every `BuildCtx` the same way `post_frame_handle` is.
    pub(crate) text_input_handle: &'a Option<flui_interaction::TextInputHandle>,

    /// The binding's owner-local interaction dispatch capability (ADR-0027),
    /// threaded into render-object lifecycle contexts.
    pub(crate) interaction_dispatch: &'a Option<flui_interaction::InteractionDispatchHandle>,

    /// This owner's identity for [`GlobalKeyScope`] claim tagging (ADR-0043).
    /// `Copy`, so every split-borrow construction site just copies it — no
    /// second borrow of `BuildOwner` needed.
    pub(crate) owner_tag: OwnerTag,

    /// Split-borrow view of `BuildOwner::global_key_scope` (ADR-0043), so
    /// `register_global_key`/`unregister_global_key` can claim/release
    /// through the shared scope without a full `&mut BuildOwner` borrow.
    pub(crate) global_key_scope: &'a mut Option<GlobalKeyScope>,
}

impl ElementOwner<'_> {
    /// Register a `GlobalKey` → element mapping.
    ///
    /// Called by `Element::mount` when the mounted element carries a
    /// `GlobalKey`. Idempotent for THIS owner: re-registering the same key
    /// with the same `id` is a no-op; with a different `id` the new mapping
    /// wins (last-write-wins). If a [`GlobalKeyScope`] is installed (or this
    /// owner's lazily self-owned private one), this first claims `key`
    /// there: a DIFFERENT owner already holding it is a cross-owner
    /// collision, traced then panicked (ADR-0043) rather than silently
    /// aliased.
    ///
    /// # Panics
    ///
    /// Panics if `key` is already claimed, in the installed
    /// [`GlobalKeyScope`], by a *different* owner (see [`GlobalKeyScope`]'s
    /// contract). Re-registering a key this same owner already holds never
    /// panics. This panic is fatal, not recoverable: it fires from a
    /// post-mount registration site, the same timing the pre-existing
    /// intra-tree duplicate-key panic already has, so this owner's tree is
    /// left with an incomplete mount. A host must not catch it and keep
    /// using this owner's tree.
    pub fn register_global_key(&mut self, key: &dyn ViewKey, id: ElementId) {
        global_key_scope::claim_and_register(
            self.global_key_scope,
            self.owner_tag,
            key,
            id,
            self.global_keys,
        );
    }

    /// Unregister a `GlobalKey` mapping.
    ///
    /// Called by `Element::unmount` when an element carrying a
    /// `GlobalKey` leaves the tree. No-op if the key isn't present locally.
    /// Also releases this owner's scope claim on `key`, if one is
    /// installed — tag-checked, so this is a no-op against a claim a
    /// different owner has since taken (ADR-0043).
    pub fn unregister_global_key(&mut self, key: &dyn ViewKey) {
        global_key_scope::release_and_unregister(
            self.global_key_scope.as_ref(),
            self.owner_tag,
            key,
            self.global_keys,
        );
    }

    /// Look up the element holding a given `GlobalKey`.
    ///
    /// Returned `None` means no element with that key is currently
    /// mounted. Resolution is by key identity: a different key that merely
    /// hashes alike never answers this lookup.
    pub fn element_for_global_key(&self, key: &dyn ViewKey) -> Option<ElementId> {
        self.global_keys.get(key)
    }

    /// Atomically remove and return the element registered under
    /// `key` for a reparent operation. Wrapper around
    /// [`BuildOwner::take_global_key_for_reparent`](crate::BuildOwner::take_global_key_for_reparent)
    /// for the split-borrow `ElementOwner` handle that the
    /// reconciler holds.
    pub fn take_global_key_for_reparent(&mut self, key: &dyn ViewKey) -> Option<ElementId> {
        self.global_keys.remove(key)
    }

    /// Record that `parent` declared a child carrying `key` in this frame.
    ///
    /// Every path that attaches or re-declares a keyed child funnels through
    /// here, so the frame boundary can tell an ordinary `GlobalKey` reparent
    /// (one declaring parent) from a duplicate (two). Verified and cleared
    /// by [`BuildOwner::finalize_tree`](crate::BuildOwner::finalize_tree).
    pub fn reserve_global_key(&mut self, parent: ElementId, child: ElementId, key: &dyn ViewKey) {
        self.global_key_reservations.reserve(parent, child, key);
    }

    /// Drop `parent`'s reservation on `child` — the parent gave the child up
    /// mid-frame, so it is no longer a claimant on whatever key that child
    /// carried.
    pub fn forget_global_key_reservation(&mut self, parent: ElementId, child: ElementId) {
        self.global_key_reservations.forget(parent, child);
    }

    /// Record that `taken_by` grafted `child` — carrying `key` — out of
    /// `losing_parent`, which has not itself declared anything this frame.
    ///
    /// A graft out of a *live* parent is only legitimate if that parent
    /// agrees, which it does by rebuilding without the child. One that never
    /// rebuilds ends the frame describing a child it no longer has, and the
    /// reservation ledger cannot see it because it never reserved. This is
    /// the record that closes that hole.
    pub fn displace_global_key(
        &mut self,
        losing_parent: ElementId,
        child: ElementId,
        key: &dyn ViewKey,
        taken_by: ElementId,
    ) {
        self.global_key_reservations
            .displace(losing_parent, child, key, taken_by);
    }

    /// Schedule an element for rebuild at the next frame.
    ///
    /// Pushed onto the depth-sorted heap so parents rebuild before
    /// children. Reentrant scheduling accumulates the additional cause without
    /// adding a second heap entry. Fires `on_build_scheduled` only for a fresh
    /// entry so the binding requests one visual update per element burst.
    pub fn schedule_build_for(&mut self, id: ElementId, depth: usize, reason: RebuildReason) {
        match self.dirty_reasons.entry(id) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(RebuildReasons::from_reason(reason));
                self.dirty_elements
                    .push(Reverse(DirtyElement::new(id, depth)));

                if let Some(callback) = self.on_build_scheduled {
                    callback();
                }
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                entry.get_mut().insert(reason);
            }
        }
    }

    /// Build an owned [`ExternalBuildScheduler`] for an element to capture at
    /// mount. Its mark-dirty callback — fired by a listenable tick *outside* a
    /// frame, with no owner in scope — uses it to enqueue the element onto the
    /// shared inbox `build_scope` drains, and to request a frame. This is the
    /// arena analogue of Flutter handing each `Element` a `BuildOwner`
    /// backreference for `markNeedsBuild`.
    pub(crate) fn external_scheduler(&self) -> ExternalBuildScheduler {
        ExternalBuildScheduler::from_parts(
            Arc::clone(self.external_inbox),
            self.external_request_frame.map(Arc::clone),
        )
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
    /// flag at `framework.dart:6114`.
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

    /// Remove an active element from all providers and retain the fact that a
    /// later activation must run `did_change_dependencies`.
    pub(crate) fn deactivate_inherited_dependent(&mut self, dependent: ElementId) -> ProviderIds {
        self.inherited_dependencies.deactivate(dependent)
    }

    /// Permanently discard a dependent's reverse-index lifecycle state.
    pub(crate) fn unmount_inherited_dependent(&mut self, dependent: ElementId) -> ProviderIds {
        self.inherited_dependencies.unmount(dependent)
    }

    /// Return whether this reactivated element previously had dependencies.
    pub(crate) fn activate_inherited_dependent(&mut self, dependent: ElementId) -> bool {
        self.inherited_dependencies.activate(dependent)
    }

    /// Remove one provider edge when that provider unmounts.
    pub(crate) fn unregister_inherited_dependency(
        &mut self,
        dependent: ElementId,
        provider: ElementId,
    ) {
        self.inherited_dependencies.unregister(dependent, provider);
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

    /// Push an inactive element together with its exclusive render-relocation token.
    pub(crate) fn push_inactive_with_detached_render_subtrees(
        &mut self,
        id: ElementId,
        depth: usize,
        detached_render_subtrees: flui_rendering::pipeline::DetachedRenderSubtrees,
    ) {
        self.inactive_elements
            .push(InactiveElement::with_detached_render_subtrees(
                id,
                depth,
                detached_render_subtrees,
            ));
    }

    /// Whether the queued entry for `id` still owns a render-relocation token.
    ///
    /// `false` when `id` is not queued at all. A retake asks this *before*
    /// [`Self::remove_inactive`], because dequeueing is the point of no
    /// return: the entry is the element's only finalization record and its
    /// token the only right to reattach the detached render subtree.
    pub(crate) fn inactive_holds_detached_render_subtrees(&self, id: ElementId) -> bool {
        self.inactive_elements
            .iter()
            .find(|entry| entry.id() == id)
            .is_some_and(super::build_owner::InactiveElement::holds_detached_render_subtrees)
    }

    /// Remove an element from the inactive queue.
    ///
    /// Used when an element is re-activated mid-frame (Flutter
    /// reparenting via `GlobalKey`). No-op if the id
    /// isn't queued.
    pub(crate) fn remove_inactive(
        &mut self,
        id: ElementId,
    ) -> Option<flui_rendering::pipeline::DetachedRenderSubtrees> {
        let position = self
            .inactive_elements
            .iter()
            .position(|entry| entry.id() == id)?;
        self.inactive_elements
            .remove(position)
            .take_detached_render_subtrees()
    }

    /// Whether the given element is currently queued for finalization.
    ///
    /// Used by `ElementTree::insert` to gate the state-migration
    /// retake: the GlobalKey registry's `Some(_)` entry could be stale
    /// (the element is still active elsewhere) — only an entry that's
    /// actually in the inactive queue is safe to re-attach.
    /// Whether `id` is in the inactive QUEUE.
    ///
    /// This is queue membership, **not** `Lifecycle`. Callers that queue an
    /// element for possible revival must call `deactivate()` first (see
    /// `ElementTree::remove`), because `retake_inactive_global_key` activates
    /// from `Inactive`. The unmount-only pushes in `sliver_adaptor` leave the
    /// element `Active` on purpose and rely on never being retake candidates.
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

    /// Number of `GlobalKey` declarations recorded so far this frame.
    ///
    /// Diagnostic surface for tests that assert the reservation ledger is
    /// being populated at all; the verification itself reads the ledger by
    /// identity, never by size.
    pub fn global_key_reservation_is_empty(&self) -> bool {
        self.global_key_reservations.is_empty()
    }

    /// Number of dirty elements pending rebuild.
    pub fn dirty_count(&self) -> usize {
        self.dirty_elements.len() + self.partitioned_dirty_count
    }

    // ========================================================================
    // Child-manager registry (lazy sliver backend)
    // ========================================================================

    /// Register a `ChildManager` for the given sliver render id.
    ///
    /// Called from `SliverAdaptorBehavior::on_mount` (F8 — NOT from the
    /// generic `RenderBehavior::on_mount`).  Idempotent: re-registering the
    /// same `render_id` with a new manager replaces the old entry (last-write
    /// wins; a single sliver has exactly one live manager at a time).
    /// Tell the lazy sliver whose render node is `host_render_id` — if one is
    /// registered — that `child` was grafted away by a `GlobalKey` retake.
    ///
    /// The manager `Arc` is cloned out first so the registry lock is not
    /// held across the call. The manager's own lock is taken with
    /// `try_lock`: the only way it can already be held is a retake running
    /// *inside* that same manager's `service` (a child moving within one
    /// list, which the retake path rejects earlier as a duplicate), so a
    /// contended lock here is a bug worth a loud debug failure, never a
    /// hang.
    pub(crate) fn forget_sparse_child(&self, host_render_id: RenderId, child: ElementId) {
        let manager = self
            .child_manager_registry
            .lock()
            .get(&host_render_id)
            .map(Arc::clone);
        let Some(manager) = manager else {
            return;
        };
        if let Some(mut manager) = manager.try_lock() {
            manager.forget_child(child);
        } else {
            debug_assert!(
                false,
                "BUG: forget_sparse_child re-entered a lazy sliver's own service pass"
            );
            tracing::error!(
                ?host_render_id,
                ?child,
                "a GlobalKey retake could not notify the losing lazy sliver (manager busy)"
            );
        }
    }

    pub(crate) fn register_child_manager(
        &mut self,
        render_id: RenderId,
        manager: Arc<Mutex<dyn ChildManager>>,
    ) {
        self.child_manager_registry
            .lock()
            .insert(render_id, manager);
    }

    /// Unregister the `ChildManager` for `render_id`.
    ///
    /// Called from `SliverAdaptorBehavior::on_unmount` so that a stale
    /// registry entry for the dismounted sliver cannot be serviced post-frame.
    /// No-op if the id is not present.
    pub(crate) fn unregister_child_manager(&mut self, render_id: RenderId) {
        self.child_manager_registry.lock().remove(&render_id);
    }

    /// Register a build-during-layout node.
    ///
    /// Called from a layout-builder element's `on_mount` — the only lifecycle
    /// hook handed an `&mut ElementOwner`, and therefore the only normal hook
    /// that can register the render id, element id, and constraints cell as one
    /// atomic mount-time fact. A later [`RebuildHandle`](crate::RebuildHandle)
    /// can schedule the element, but it deliberately cannot mutate this
    /// registry.
    ///
    /// `cell` must be the same `Arc` the render object registered under
    /// `render_id` publishes into. The registry is payload-blind — a
    /// `LayoutBuilder` publishes box constraints, a sliver persistent header
    /// its shrink state — so what matters here is only that both halves hold
    /// the same mailbox.
    pub(crate) fn register_layout_builder(
        &mut self,
        render_id: RenderId,
        element: ElementId,
        cell: Arc<dyn BuildDuringLayoutCell>, // PORT-CHECK-OK-DYN: the registry services heterogeneous build-during-layout nodes (LayoutBuilder's constraints cell, and a sliver persistent header's shrink cell next) and needs only needs_build/has_published/commit; a generic would monomorphise the registry per payload type, so one map could not hold both
    ) {
        self.layout_builder_registry
            .insert(render_id, LayoutBuilderEntry { element, cell });
    }

    /// Unregister the build-during-layout node for `render_id`.
    ///
    /// Must run in `on_unmount`, before the render object is disposed.
    /// `service_layout_builders` prunes entries that outlive their element or
    /// render node anyway, but relying on that is how the sliver adaptor grew
    /// its stale-entry bug.
    pub(crate) fn unregister_layout_builder(&mut self, render_id: RenderId) {
        self.layout_builder_registry.remove(render_id);
    }
}

impl std::fmt::Debug for ElementOwner<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElementOwner")
            .field("global_keys", &self.global_keys.len())
            .field("dirty_count", &self.dirty_count())
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
        let key = crate::GlobalKey::<()>::new();
        handle.register_global_key(&key, id);
        assert_eq!(handle.element_for_global_key(&key), Some(id));
        assert_eq!(handle.global_key_count(), 1);

        handle.unregister_global_key(&key);
        assert_eq!(handle.element_for_global_key(&key), None);
        assert_eq!(handle.global_key_count(), 0);
    }

    #[test]
    fn schedule_build_for_dedups_and_pushes_heap() {
        let mut owner = BuildOwner::new();
        let id = ElementId::new(5);

        let mut handle = owner.element_owner_mut();
        handle.schedule_build_for(id, 2, RebuildReason::StateChange);
        handle.schedule_build_for(id, 2, RebuildReason::AnimationTick);
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

        let _ = handle.remove_inactive(id);
        assert_eq!(handle.inactive_len(), 0);
    }

    #[test]
    fn no_key_path_keeps_registry_empty() {
        // An element with no key
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
            handle.schedule_build_for(ElementId::new(depth + 1), depth, RebuildReason::StateChange);
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
