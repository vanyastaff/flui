//! `GlobalKey` registration at mount time and the retake of an existing element that owns the key.

use flui_foundation::{ElementId, ViewKey};

use crate::owner::LifecycleHook;
use crate::view::View;

use super::super::containment::{ChildHookPanic, contain_window};
use super::render_children::{
    attach_render_relocation, detach_render_relocation, preflight_render_relocation,
};
use super::{ElementNode, ElementTree, InsertedChild};

// ============================================================================
// GlobalKey helpers
// ============================================================================

/// Borrow a view's `GlobalKey`, if it has one. Returns `None` for
/// un-keyed views and for keyed views whose `ViewKey::is_global_key()` is
/// `false` (e.g. `ValueKey`, `UniqueKey`, `ObjectKey`).
///
/// Centralises the "is this a global key?" check so the mount /
/// soft-remove / retake paths all read it the same way. The key is
/// returned by reference rather than as a hash because every consumer
/// resolves by identity — a hash would only be an index into a bucket.
pub(super) fn global_key_of(view: &dyn View) -> Option<&dyn ViewKey> {
    let key = view.key()?;
    key.is_global_key().then_some(key)
}

/// Register the `(key → id)` mapping on the owner.
///
/// Reaching this with the SAME key already registered to a DIFFERENT
/// element means one key is live on two elements at once inside one tree —
/// a real duplicate, not the hash accident the old hash-keyed registry
/// could also produce here (two distinct keys that collide now hold two
/// independent entries and never meet). Debug builds panic;
/// release builds trace and fall through to
/// last-write-wins so an application does not crash on it, and the frame
/// boundary's reservation check reports the duplicate as data.
pub(super) fn register_global_key_with_collision_check(
    owner: &mut crate::ElementOwner<'_>,
    key: &dyn ViewKey,
    id: ElementId,
) {
    if let Some(existing) = owner.element_for_global_key(key)
        && existing != id
    {
        tracing::error!(
            key = ?key,
            existing = ?existing,
            new = ?id,
            "duplicate GlobalKey: replacing an existing registration in the same tree"
        );
        #[cfg(debug_assertions)]
        {
            panic!(
                "duplicate GlobalKey: {key:?} is already registered to {existing:?} \
                 but new mount wants {id:?}"
            );
        }
    }
    owner.register_global_key(key, id);
}

/// State-migration entry point. If `key` resolves to an existing element,
/// reuse that element instead of mounting a fresh one:
///
/// - inactive candidate: pop it out of the inactive queue and re-attach;
/// - active candidate under a different parent: forget it from that parent,
///   deactivate/activate it, then attach it at the new `(parent, slot)`.
///
/// Returns the migrated `ElementId` on success (or the [`ChildHookPanic`]
/// its own containment window caught), or `None` when no retakeable element
/// exists (caller falls back to creating a fresh element).
pub(super) enum GlobalKeyRetake {
    Absent,
    Retaken(ElementId),
    RetakePanicked(ChildHookPanic),
    Rejected,
}

pub(super) struct RetakeDestination<'a> {
    pub(super) parent: ElementId,
    pub(super) slot: usize,
    pub(super) reconciled_prefix: &'a [ElementId],
    pub(super) unclaimed_old_slots: &'a [Option<ElementId>],
}

pub(super) fn try_retake_global_key(
    tree: &mut ElementTree,
    owner: &mut crate::ElementOwner<'_>,
    key: &dyn ViewKey,
    view: &dyn View,
    destination: RetakeDestination<'_>,
) -> GlobalKeyRetake {
    let Some(candidate_id) = owner.element_for_global_key(key) else {
        return GlobalKeyRetake::Absent;
    };
    if !can_retake_global_key_candidate(tree, candidate_id, view) {
        return GlobalKeyRetake::Rejected;
    }

    let retaken = if owner.is_inactive(candidate_id) {
        retake_inactive_global_key(tree, owner, key, view, candidate_id, destination)
    } else {
        retake_active_global_key(tree, owner, key, view, candidate_id, destination)
    };
    match retaken {
        None => GlobalKeyRetake::Rejected,
        Some(Ok(id)) => GlobalKeyRetake::Retaken(id),
        Some(Err(panic)) => GlobalKeyRetake::RetakePanicked(panic),
    }
}

fn can_retake_global_key_candidate(
    tree: &ElementTree,
    candidate_id: ElementId,
    view: &dyn View,
) -> bool {
    let Some(node) = tree.get(candidate_id) else {
        return false;
    };
    let element = node.element();
    if element.view_type_id() != view.view_type_id() {
        return false;
    }
    let Some(new_key) = view.key() else {
        return false;
    };
    element
        .current_key()
        .is_some_and(|old_key| new_key.key_eq(old_key))
}

fn provisional_child_order(
    reconciled_prefix: &[ElementId],
    candidate_id: ElementId,
    unclaimed_old_slots: &[Option<ElementId>],
) -> Vec<ElementId> {
    let mut order = Vec::with_capacity(reconciled_prefix.len() + unclaimed_old_slots.len() + 1);
    let mut seen = flui_foundation::SlotSet::new();
    for element_id in reconciled_prefix
        .iter()
        .copied()
        .chain(std::iter::once(candidate_id))
        .chain(unclaimed_old_slots.iter().flatten().copied())
    {
        if seen.insert(element_id) {
            order.push(element_id);
        }
    }
    order
}

fn retake_inactive_global_key(
    tree: &mut ElementTree,
    owner: &mut crate::ElementOwner<'_>,
    key: &dyn ViewKey,
    view: &dyn View,
    candidate_id: ElementId,
    destination: RetakeDestination<'_>,
) -> Option<Result<ElementId, ChildHookPanic>> {
    let new_parent = destination.parent;
    let new_slot = destination.slot;
    let mut relocation = preflight_render_relocation(tree, candidate_id, new_parent)?;
    // A candidate with render nodes can only be reattached through the token
    // its soft removal minted. Reject *before* dequeueing: `remove_inactive`
    // is the point of no return, and bailing after it would strand the
    // element — no finalization record, and its render subtree detached with
    // nothing left holding the right to reattach or release it.
    if !relocation.frontier.is_empty()
        && relocation.pipeline.is_some()
        && !owner.inactive_holds_detached_render_subtrees(candidate_id)
    {
        return None;
    }
    // Clone caller-controlled key data before dequeueing the inactive entry.
    // `remove_inactive` is the first point of no return for this retake.
    let stored_key = view.key().map(ViewKey::clone_key);
    relocation.detached_render_subtrees = owner.remove_inactive(candidate_id);

    let parent_depth = tree.get(new_parent).map_or(0, ElementNode::depth);
    let child_parent_render_id = tree
        .get(new_parent)
        .and_then(|node| node.element().child_render_id());
    let child_sliver_slot = tree
        .get(new_parent)
        .and_then(|node| node.element().child_sliver_slot(new_slot));

    // Route through the staleness-checked accessor. The candidate came from the
    // live GlobalKey registry and was soft-removed (slot kept, generation NOT
    // bumped), so its generation still matches and this resolves.
    {
        let node = tree.get_mut(candidate_id)?;
        node.parent = Some(new_parent);
        node.slot = new_slot;
        node.set_depth(parent_depth + 1);
        node.element_mut()
            .set_parent_render_id(child_parent_render_id);
        node.element_mut().set_sliver_slot(child_sliver_slot);
    }

    // The relocation is committed; the element is live and parented here.
    // Report it now — BEFORE either containment window below, which run
    // `activate_subtree` and (separately) this retake's own `update`, both
    // panic sites — so a caller bounding those windows (see
    // `ChildHookPanic`'s doc) knows to remove THIS element rather than
    // looking for a node this call never minted.
    let inserted = InsertedChild::Retaken(candidate_id);
    let provisional = provisional_child_order(
        destination.reconciled_prefix,
        candidate_id,
        destination.unclaimed_old_slots,
    );

    // Containment window — Activate: reactivate the whole subtree alone. Deactivation released
    // every inherited edge; the owner schedules `did_change_dependencies`
    // for nodes that had dependencies so their next build registers
    // against the new ancestry. A descendant's own `ViewState::activate`
    // can panic here — a behavior that catches its own (see
    // `StatefulBehavior::on_activate`) stages it under its own identity
    // and re-raises, so this still observes the unwind.
    owner.arm_lifecycle_panic_handoff();
    if let Err(payload) = contain_window(|| tree.activate_subtree(candidate_id, owner)) {
        let staged = owner.take_staged_lifecycle_panic();
        return Some(Err(ChildHookPanic {
            inserted: Some(inserted),
            hook: LifecycleHook::Activate,
            staged,
            payload,
        }));
    }
    owner.take_staged_lifecycle_panic();

    // Framework code between the two windows, never user code — a `BUG:`
    // here propagates uncontained (see `ChildHookPanic`'s doc). The
    // subtree moved under a new parent, so its inherited scopes (built
    // against the OLD ancestor chain) and descendant depths are stale.
    // Recompute both top-down against `new_parent`.
    tree.recompute_subtree_ancestry(candidate_id, owner.child_manager_registry);
    tree.synchronize_destination_render_children_for_relocation(new_parent, &provisional);
    for &(element_id, _) in &relocation.frontier {
        tree.reset_ancestor_parent_data(element_id);
    }
    attach_render_relocation(&mut relocation);

    // Containment window — Update: the retake's own `update`.
    let node = tree
        .get_mut(candidate_id)
        .expect("BUG: a retaken candidate must still be live for its own update");
    let update_result = {
        let element = node.element_mut();
        contain_window(|| element.update(view, owner))
    };

    match update_result {
        Ok(()) => {
            node.set_key(stored_key);
            tracing::debug!(
                candidate = ?candidate_id,
                new_parent = ?new_parent,
                new_slot,
                "ElementTree::insert retook inactive element for GlobalKey state migration"
            );

            // Emit ReconcileEvent::Reparent. The element
            // came from the inactive queue (Lifecycle::Inactive → Active), so
            // `from_parent: None` per ADV-1 branch case 1 — there is no prior
            // *active* parent at the moment of reparent; the donor parent
            // already cleared its slot when it pushed the element into the
            // inactive queue.
            super::super::reconcile_event::emit(&super::super::reconcile_event::ReconcileEvent {
                kind: super::super::reconcile_event::ReconcileEventKind::Reparent,
                parent: new_parent,
                child_key: Some(key.key_hash()),
                slot: new_slot,
                view_type_id: view.view_type_id(),
                from_parent: None,
            });

            // ADR-0040: identity and state survived — a move, never a second mount.
            crate::owner::emit_observation(owner.tree_observer, |o| {
                o.element_moved(&flui_foundation::observe::ElementMoved::new(
                    candidate_id,
                    new_parent,
                    new_slot,
                ));
            });

            Some(Ok(candidate_id))
        }
        Err(payload) => Some(Err(ChildHookPanic {
            inserted: Some(inserted),
            hook: LifecycleHook::Update,
            staged: None,
            payload,
        })),
    }
}

fn retake_active_global_key(
    tree: &mut ElementTree,
    owner: &mut crate::ElementOwner<'_>,
    key: &dyn ViewKey,
    view: &dyn View,
    candidate_id: ElementId,
    destination: RetakeDestination<'_>,
) -> Option<Result<ElementId, ChildHookPanic>> {
    let new_parent = destination.parent;
    let new_slot = destination.slot;
    let from_parent = tree.get(candidate_id)?.parent()?;
    if from_parent == new_parent {
        tracing::error!(
            key = ?key,
            ?candidate_id,
            ?new_parent,
            "GlobalKey appears twice under the same active parent"
        );
        #[cfg(debug_assertions)]
        {
            panic!(
                "GlobalKey {key:?} is already active under {new_parent:?}; \
                 duplicate GlobalKey children are not allowed"
            );
        }
        #[cfg(not(debug_assertions))]
        {
            return None;
        }
    }

    // Preflight BEFORE any mutation: a rejected relocation must leave the
    // donor, the registry and the epoch exactly as it found them.
    let mut relocation = preflight_render_relocation(tree, candidate_id, new_parent)?;

    // Reconciliation is required only when this graft actually MOVES render
    // nodes — that is the hazard this gate exists for. A keyed element with
    // no render frontier carries nothing to relocate, and its graft is the
    // ordinary cross-parent declaration whose ambiguity the reservation
    // ledger already resolves: it records both claimants and the frame
    // boundary reports a real duplicate. Refusing that case here would turn
    // a reported duplicate into a silent fresh mount.
    if !relocation.frontier.is_empty() && !tree.is_reconciling_parent(new_parent) {
        return None;
    }
    // Clone caller-controlled key data before displacement bookkeeping or
    // donor/render-tree mutation begins.
    let stored_key = view.key().map(ViewKey::clone_key);

    // Past the preflight the graft is going to happen. It pulls this child
    // out from under a parent that is still live, which is a legitimate
    // reparent only if `from_parent` goes on to rebuild without the child;
    // if it never rebuilds, it ends the frame describing a child it no
    // longer has. Record it so the frame boundary can tell the two apart —
    // `note_parent_rebuild` drops this the moment `from_parent` rebuilds.
    owner.displace_global_key(from_parent, candidate_id, key, new_parent);
    // A lazy sliver that loses a child this way must forget it too, or its
    // next band eviction / delegate refresh would reach into another
    // parent's subtree.
    if let Some(losing_render_id) = tree
        .get(from_parent)
        .and_then(|node| node.element().render_id())
    {
        owner.forget_sparse_child(losing_render_id, candidate_id);
    }

    // A candidate registered under a parent that no longer lists it is
    // tolerated, not refused: the registry is the authority on where a
    // GlobalKey lives, and the frame boundary reports a genuine duplicate
    // through the reservation ledger. Refusing here would convert that
    // report into a fresh mount and lose it.
    if let Some(old_parent) = tree.get_mut(from_parent) {
        if let Some(pos) = old_parent
            .child_ids
            .iter()
            .position(|&child| child == candidate_id)
        {
            old_parent.child_ids.remove(pos);
        } else {
            tracing::warn!(
                ?candidate_id,
                ?from_parent,
                "active GlobalKey candidate was registered under a parent that no longer lists it"
            );
        }
    }
    detach_render_relocation(&mut relocation);

    let (parent_depth, child_parent_render_id, child_sliver_slot) =
        tree.get(new_parent).map_or((0, None, None), |node| {
            (
                node.depth(),
                node.element().child_render_id(),
                node.element().child_sliver_slot(new_slot),
            )
        });

    tree.deactivate_subtree(candidate_id, owner);
    {
        let node = tree.get_mut(candidate_id)?;
        node.parent = Some(new_parent);
        node.slot = new_slot;
        node.set_depth(parent_depth + 1);
        node.element_mut()
            .set_parent_render_id(child_parent_render_id);
        node.element_mut().set_sliver_slot(child_sliver_slot);
    }

    // The relocation is committed; the element is live and parented here.
    // Report it now — BEFORE either containment window below, which run
    // `activate_subtree` and (separately) this retake's own `update`, both
    // panic sites — so a caller bounding those windows (see
    // `ChildHookPanic`'s doc) knows to remove THIS element rather than
    // looking for a node this call never minted.
    let inserted = InsertedChild::Retaken(candidate_id);
    let provisional = provisional_child_order(
        destination.reconciled_prefix,
        candidate_id,
        destination.unclaimed_old_slots,
    );

    // Containment window — Activate: reactivate the whole subtree alone. A descendant's own
    // `ViewState::activate` can panic here — a behavior that catches its
    // own (see `StatefulBehavior::on_activate`) stages it under its own
    // identity and re-raises, so this still observes the unwind.
    owner.arm_lifecycle_panic_handoff();
    if let Err(payload) = contain_window(|| tree.activate_subtree(candidate_id, owner)) {
        let staged = owner.take_staged_lifecycle_panic();
        return Some(Err(ChildHookPanic {
            inserted: Some(inserted),
            hook: LifecycleHook::Activate,
            staged,
            payload,
        }));
    }
    owner.take_staged_lifecycle_panic();

    // Framework code between the two windows, never user code — a `BUG:`
    // here propagates uncontained (see `ChildHookPanic`'s doc).
    // Deactivation above synchronously released dependency edges for the
    // entire subtree. Repair both inherited scopes and recursive depths
    // before the next dirty-heap drain.
    tree.recompute_subtree_ancestry(candidate_id, owner.child_manager_registry);
    tree.synchronize_destination_render_children_for_relocation(new_parent, &provisional);
    for &(element_id, _) in &relocation.frontier {
        tree.reset_ancestor_parent_data(element_id);
    }
    attach_render_relocation(&mut relocation);

    // Containment window — Update: the retake's own `update`.
    let node = tree
        .get_mut(candidate_id)
        .expect("BUG: a retaken candidate must still be live for its own update");
    let update_result = {
        let element = node.element_mut();
        contain_window(|| element.update(view, owner))
    };

    match update_result {
        Ok(()) => {
            node.set_key(stored_key);
            tracing::debug!(
                candidate = ?candidate_id,
                from_parent = ?from_parent,
                new_parent = ?new_parent,
                new_slot,
                "ElementTree::insert moved active GlobalKey element to a new parent"
            );

            super::super::reconcile_event::emit(
                &super::super::reconcile_event::ReconcileEvent::reparent(
                    from_parent,
                    new_parent,
                    new_slot,
                    view.view_type_id(),
                    key.key_hash(),
                ),
            );

            // ADR-0040: cross-parent move of a live element.
            crate::owner::emit_observation(owner.tree_observer, |o| {
                o.element_moved(&flui_foundation::observe::ElementMoved::new(
                    candidate_id,
                    new_parent,
                    new_slot,
                ));
            });

            Some(Ok(candidate_id))
        }
        Err(payload) => Some(Err(ChildHookPanic {
            inserted: Some(inserted),
            hook: LifecycleHook::Update,
            staged: None,
            payload,
        })),
    }
}
