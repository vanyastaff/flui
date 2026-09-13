//! Id-based keyed child reconciliation over the slab-resident
//! [`ElementTree`] — the **production** child reconciler.
//!
//! # The production reconciler
//!
//! This is the reconciler the production build path runs after the E3
//! atomic box→arena swap: [`BuildOwner::build_scope`](crate::BuildOwner)
//! feeds each dirty element's freshly-built child views to
//! [`reconcile_children_by_id`], which permutes the parent's
//! [`ElementNode::child_ids`](super::ElementNode) list, reusing /
//! inserting / removing real slab nodes through the [`ElementTree`]
//! accessors. The single element graph is the slab. (It replaced an
//! earlier box-vec reconciler that permuted a caller-owned
//! `Vec<Box<dyn ElementBase>>`; that one has been removed.)
//!
//! It also emits one typed [`ReconcileEvent`] per child disposition on
//! the live path, so the `flui::reconcile` (FR-035) stability boundary
//! is meaningful for normal reconciliation, not just a reference impl.
//!
//! # Emitted dispositions
//!
//! Emitted directly by this module: `Reuse` (top scan, same slot),
//! `Unmount` (keyless-middle drop, unclaimed-keyed drop, or bounded update
//! recovery, at the child's OLD slot), `Reorder`/`Reuse` (keyed claim, by
//! old-slot vs new-slot), and `Reuse`/`Reorder` for the bottom slice (by
//! `old_bottom == new_bottom`).
//!
//! The insert path delegates its event to
//! [`ElementTree::mount_or_substitute`](super::ElementTree::mount_or_substitute):
//! it emits `Mount` for a fresh element or substitute, or `Reparent` when it
//! successfully retakes a GlobalKey element — never both, so the reconciler
//! must NOT also emit a disposition for an insert.
//!
//! # The borrow discipline this module proves out
//!
//! The render tree learned the hard way that holding a `&mut` (or `&`)
//! into an arena across a *second* mutation of that arena is
//! Stacked/Tree-Borrows undefined behaviour: the first borrow's tag is
//! invalidated by the second access, and a later use through the first
//! borrow is UB even though the slots are distinct. The fix there was an
//! "extract-then-apply" shape; this module is built so that shape is the
//! *only* shape the function can take:
//!
//! 1. The parent's current child ids are cloned into an **owned**
//!    `Vec<ElementId>` and the parent borrow is dropped before any child
//!    mutation runs.
//! 2. Every read of an old child node ([`ElementTree::get`]) and every
//!    mutation ([`ElementTree::get_mut`] / [`ElementTree::insert`] /
//!    [`ElementTree::remove`]) takes a **fresh** borrow of `tree` that
//!    ends before the next statement. No borrow into the slab is ever
//!    alive across another slab access.
//! 3. The parent is re-borrowed exactly once at the very end to store
//!    the new child-id list.
//!
//! There is **no `unsafe`, no raw pointer**, and no long-lived slab
//! reference anywhere in this file. The keyed-permutation matching is
//! reconstructed from owned snapshots (key hashes + view type ids) taken
//! up front, so the matching loop never holds a slab borrow either.
//!
//! # Matching semantics (Flutter-faithful)
//!
//! A new view reuses an old child when they share a concrete view type
//! **and** the same key (both keyless, or both keyed and equal via
//! [`ViewKey::key_eq`](flui_foundation::ViewKey::key_eq)). Keyed children
//! may move slots and keep their element (and thus their slab id and
//! state); keyless children only match positionally. Child order in the
//! result equals new-view order. This is the same predicate the box
//! reconciler applies through `can_update_element`; here it is expressed
//! over the type-erased [`ElementBase`] accessors plus the typed
//! [`View`] surface.

use std::collections::HashMap;

use flui_foundation::ElementId;

use super::element_tree::{ElementTree, ProvisionalOrder};
use super::reconcile_event::{ReconcileEvent, emit as emit_event};
use crate::view::{ElementBase, View};

/// Reconcile the slab-resident children of `parent_id` against
/// `new_views`, then write the resulting child-id list back onto the
/// parent node.
///
/// On return, `parent_id`'s [`child_ids`](super::ElementNode::child_ids)
/// holds exactly `new_views.len()` ids in new-view order: a reused (and
/// `update`d) old child where type + key matched, or a freshly mounted child
/// (possibly a recovery substitute) otherwise. Old children that found no
/// match have been [`removed`](ElementTree::remove). Every
/// surviving child's [`slot`](super::ElementNode::slot) is refreshed to
/// its final index, so the node metadata stays coherent after a reorder.
///
/// # Borrow discipline
///
/// This function never holds a reference into `tree` across a second
/// `tree` access — see the module docs. The old child-id list is cloned
/// to an owned `Vec` before any mutation; each child mutation takes a
/// fresh `&mut tree`; the parent is re-borrowed only at the end. This is
/// the property the eventual atomic swap depends on, made structurally
/// unavoidable here.
///
/// # Arguments
///
/// * `tree` - the element arena; mutated in place.
/// * `parent_id` - the parent whose children are reconciled. If it does
///   not resolve (stale / absent id) the call is a no-op.
/// * `new_views` - the new child views, in slot order.
/// * `owner` - split-borrow [`ElementOwner`](crate::ElementOwner) handle
///   threaded into every child `insert` / `update` / `remove`.
///
/// # Complexity
///
/// `O(n + m)` average over `n` old children and `m` new views (one
/// HashMap-indexed pass plus the prefix/suffix scans). Worst case rises
/// to `O(n * m)` only when every new keyed view's hash collides into one
/// bucket whose candidates mostly fail the semantic `key_eq` check —
/// the cost of collision-resistant keyed matching. Both `n`
/// and `m` are bounded by the parent's fan-out, not the whole tree.
// Five Flutter-parity phases + per-disposition ReconcileEvent emission push the
// body past 100 lines; splitting would scramble the 1:1 mapping to the keyed
// reconcile algorithm (Flutter's `Element.updateChildren`).
pub(crate) fn reconcile_children_by_id(
    tree: &mut ElementTree,
    parent_id: ElementId,
    new_views: &[Box<dyn View>],
    owner: &mut crate::ElementOwner<'_>,
) {
    let _reconcile_guard = tree.begin_reconcile(parent_id);

    // ── Step 1: extract. Clone the parent's current child ids into an
    // OWNED vec, then DROP the parent borrow. From here on no reference
    // into the slab outlives a single statement.
    let Some(parent_node) = tree.get(parent_id) else {
        // Stale or absent parent: nothing to reconcile. (A no-op, not a
        // panic — a caller may legitimately race a removed parent.)
        return;
    };
    let old_ids: Vec<ElementId> = parent_node.child_ids().to_vec();
    // `parent_node` borrow ends here (not used again before re-borrow).

    // ── Step 2: snapshot the old children's match keys. Each `tree.get`
    // is a fresh borrow that ends at the end of its statement; we copy
    // out only owned data (the key hash) so the matching loop below
    // holds no slab borrow. Ids whose slot no longer resolves are
    // dropped from consideration (defensive against a stale entry).
    let old_len = old_ids.len();
    let new_len = new_views.len();

    // `Some(id)` while the slot is still a claim candidate; `take()`n to
    // `None` once a new view claims it — a working buffer of per-slot
    // claim candidates.
    let mut old_slots: Vec<Option<ElementId>> = old_ids.iter().copied().map(Some).collect();

    // The reconciled id list, built front-to-back in new-view order.
    let mut result: Vec<ElementId> = Vec::with_capacity(new_len);
    // Scheduling cause for every surviving child. Kept beside the owned result
    // during reconciliation so fresh mounts and in-place updates remain
    // distinguishable when the final depth-aware scheduling pass runs.
    let mut scheduling_reasons: HashMap<ElementId, crate::RebuildReason> =
        HashMap::with_capacity(new_len);

    // ── Phase 1: sync the top of both lists while children match.
    // Same-slot reuse: update the old child in place, keep its id.
    let mut old_top = 0;
    let mut new_top = 0;
    while old_top < old_len && new_top < new_len {
        let Some(old_id) = old_slots[old_top] else {
            break;
        };
        if !can_update_by_id(tree, old_id, new_views[new_top].as_ref()) {
            break;
        }
        let now = update_child(
            tree,
            parent_id,
            old_id,
            new_views[new_top].as_ref(),
            new_top,
            owner,
        );
        if now == old_id {
            scheduling_reasons.insert(now, crate::RebuildReason::ParentUpdate);
            // Top scan is same-slot (old_top == new_top throughout): the
            // surviving child neither moved nor was recreated — a `Reuse`
            // disposition. A recovery substitute emitted its own `Mount`.
            emit_event(&ReconcileEvent::reuse(
                parent_id,
                new_top,
                new_views[new_top].view_type_id(),
                new_views[new_top]
                    .key()
                    .map(flui_foundation::ViewKey::key_hash),
            ));
        } else {
            scheduling_reasons.insert(now, crate::RebuildReason::InitialMount);
        }
        old_slots[old_top] = None;
        result.push(now);
        old_top += 1;
        new_top += 1;
    }

    // ── Phase 2: scan the bottom of both lists while children match.
    // Matches are RECORDED (the bounds shrink); the actual `update` runs
    // in phase 5a so every update is applied strictly front-to-back
    // (the Flutter `updateChildren` ordering guarantee).
    let mut old_bottom = old_len;
    let mut new_bottom = new_len;
    while old_top < old_bottom && new_top < new_bottom {
        let Some(old_id) = old_slots[old_bottom - 1] else {
            break;
        };
        if !can_update_by_id(tree, old_id, new_views[new_bottom - 1].as_ref()) {
            break;
        }
        old_bottom -= 1;
        new_bottom -= 1;
    }

    // ── Phase 3: index the remaining old middle by key hash; remove any
    // keyless old middle child (it can only match positionally, which
    // the top/bottom scans already exhausted).
    //
    // The bucket holds OLD-SLOT INDICES (a `Vec<usize>`, not a single
    // index) so two old children with DISTINCT keys that collide on `u64`
    // hash both stay claim candidates — the symmetric FR-024(c) collision
    // defense. Phase 4 disambiguates via the semantic `key_eq` inside
    // `can_update_by_id`, and clears a claim by index in O(1) (indexing
    // the bucket by slot, not id, is what keeps phase 4 linear rather than
    // O(n*m) — no per-claim scan of `old_slots`).
    let mut old_keyed: HashMap<u64, Vec<usize>> = HashMap::new();
    for (idx, slot) in old_slots
        .iter_mut()
        .enumerate()
        .take(old_bottom)
        .skip(old_top)
    {
        let Some(old_id) = *slot else {
            continue;
        };
        if let Some(hash) = key_hash_of(tree, old_id) {
            // Keyed: defer the claim to phase 4. FIFO bucket order
            // preserves first-wins across true duplicate keys.
            old_keyed.entry(hash).or_default().push(idx);
        } else {
            // Keyless middle child with no positional match: remove.
            // Capture the view type BEFORE the slab frees the slot so the
            // `Unmount` disposition carries a stable identifier; the slot
            // is its OLD index (`idx`), the position it is leaving.
            let view_type = view_type_of(tree, old_id);
            *slot = None;
            remove_child(tree, parent_id, old_id, owner);
            if let Some(view_type) = view_type {
                emit_event(&ReconcileEvent::unmount(parent_id, idx, view_type, None));
            }
        }
    }

    // ── Phase 4: sync the new middle front-to-back. A keyed new view
    // claims its old match from `old_keyed`; everything else inserts a
    // fresh child. Claimed olds are cleared from `old_slots` so phase 5b
    // does not also remove them.
    for new_offset in 0..(new_bottom - new_top) {
        let new_slot = new_top + new_offset;
        let new_view = new_views[new_slot].as_ref();
        if let Some(old_idx) = claim_old_for_new(tree, new_view, &mut old_keyed, &old_slots) {
            // Clear the claim by index in O(1) and take its id; `take`
            // leaves `None`, so phase 5b will not also remove this reused
            // child.
            let old_id = old_slots[old_idx]
                .take()
                .expect("claim_old_for_new only returns indices of Some, keyed slots");
            // The claimed child's `slot` is still its OLD index (the
            // tail-of-pass re-stamp has not run yet): equal to the new
            // slot means it stayed put (`Reuse`); otherwise the keyed
            // match pulled it across slots (`Reorder`).
            let stayed = slot_of(tree, old_id) == Some(new_slot);
            let now = update_child(tree, parent_id, old_id, new_view, new_slot, owner);
            if now == old_id {
                scheduling_reasons.insert(now, crate::RebuildReason::ParentUpdate);
                let key_hash = new_view.key().map(flui_foundation::ViewKey::key_hash);
                let view_type = new_view.view_type_id();
                emit_event(&if stayed {
                    ReconcileEvent::reuse(parent_id, new_slot, view_type, key_hash)
                } else {
                    ReconcileEvent::reorder(parent_id, new_slot, view_type, key_hash)
                });
                if !stayed {
                    // ADR-0040: a keyed match pulled the surviving child
                    // across slots. A substitute emitted its own `Mount` and
                    // has no prior identity to move.
                    crate::owner::emit_observation(owner.tree_observer, |o| {
                        o.element_moved(&flui_foundation::observe::ElementMoved::new(
                            old_id, parent_id, new_slot,
                        ));
                    });
                }
            } else {
                scheduling_reasons.insert(now, crate::RebuildReason::InitialMount);
            }
            result.push(now);
        } else {
            // `mount_or_substitute` emits the disposition itself — `Mount`
            // for a fresh element or substitute, or `Reparent` when it
            // successfully retakes a GlobalKey element. Emitting here too
            // would double-fire on either path.
            let new_id = tree.mount_or_substitute(
                new_view,
                parent_id,
                new_slot,
                owner,
                ProvisionalOrder::during_reconcile(&result, &old_slots),
                "mounting child during reconcile",
            );
            scheduling_reasons.insert(new_id, crate::RebuildReason::InitialMount);
            result.push(new_id);
        }
    }

    // ── Phase 5a: apply the bottom matches recorded in phase 2, now
    // strictly front-to-back, keeping each reused child's id.
    for offset in 0..(old_len - old_bottom) {
        let old_id = old_slots[old_bottom + offset]
            .take()
            .expect("phase-2 bottom-scan recorded this slot as a match; it cannot be None");
        let new_idx = new_bottom + offset;
        let now = update_child(
            tree,
            parent_id,
            old_id,
            new_views[new_idx].as_ref(),
            new_idx,
            owner,
        );
        // The bottom slice stays at the tail of both lists; it shifts
        // slot only when the middle changed size, i.e. when the deltas
        // are asymmetric (`old_bottom != new_bottom`) — then `Reorder`,
        // else `Reuse`. (`old_idx == new_idx` reduces to this equality
        // because both indices are `bottom + offset`.)
        if now == old_id {
            scheduling_reasons.insert(now, crate::RebuildReason::ParentUpdate);
            let key_hash = new_views[new_idx]
                .key()
                .map(flui_foundation::ViewKey::key_hash);
            let view_type = new_views[new_idx].view_type_id();
            emit_event(&if old_bottom == new_bottom {
                ReconcileEvent::reuse(parent_id, new_idx, view_type, key_hash)
            } else {
                ReconcileEvent::reorder(parent_id, new_idx, view_type, key_hash)
            });
            if old_bottom != new_bottom {
                // ADR-0040: the tail slice shifted because the middle resized.
                crate::owner::emit_observation(owner.tree_observer, |o| {
                    o.element_moved(&flui_foundation::observe::ElementMoved::new(
                        old_id, parent_id, new_idx,
                    ));
                });
            }
        } else {
            scheduling_reasons.insert(now, crate::RebuildReason::InitialMount);
        }
        result.push(now);
    }

    // ── Phase 5b: remove any old child never claimed.
    for (idx, slot) in old_slots.iter_mut().enumerate() {
        if let Some(old_id) = slot.take() {
            // Capture identity before the slab frees the slot; the event
            // slot is the child's OLD index (`idx`).
            let view_type = view_type_of(tree, old_id);
            let key_hash = key_hash_of(tree, old_id);
            remove_child(tree, parent_id, old_id, owner);
            if let Some(view_type) = view_type {
                emit_event(&ReconcileEvent::unmount(
                    parent_id, idx, view_type, key_hash,
                ));
            }
        }
    }

    debug_assert_eq!(
        result.len(),
        new_len,
        "id-reconcile must produce exactly one child id per new view; \
         a phase dropped or duplicated a slot"
    );

    // ── Stamp each surviving child's `slot` field to its final position
    // and schedule it for build.
    //
    // A reused child keeps the slot it was first inserted at unless we
    // refresh it here; after a reorder that would leave `node.slot()`
    // disagreeing with the child's actual index in `child_ids`.
    //
    // Scheduling (E3 — atomic box→arena swap): in the slab/drain model
    // each child rebuilds as its OWN `build_scope` drain entry, not via a
    // recursive `perform_build` call from this parent. So every child that
    // still needs a build is pushed onto the dirty heap here — a freshly
    // inserted child (dirty by construction) and a reused child whose
    // `update` re-set its dirty flag both qualify; a reused child left
    // clean by an idempotent update is skipped (`schedule_build_for`
    // dedups, and `build_scope`'s `can_build` guard would no-op it
    // anyway). The child's own depth (`parent_depth + 1`, stamped by
    // `insert`) orders the heap so parents drain before children.
    //
    // Each `set_child_slot` / read takes a fresh, immediately-dropped
    // `&mut tree` borrow, so the extract-then-apply discipline still
    // holds.
    for (slot, &id) in result.iter().enumerate() {
        set_child_slot(tree, id, slot);
        if let Some(node) = tree.get(id)
            && node.element().is_dirty()
        {
            let depth = node.depth();
            let reason = scheduling_reasons
                .get(&id)
                .copied()
                .expect("BUG: every reconciled child must have a scheduling cause");
            owner.schedule_build_for(id, depth, reason);
        }
    }

    // ── Step 3: re-borrow the parent exactly once to store the result.
    // The parent may have been soft-removed mid-reconcile (a keyed child
    // removal cannot affect the parent, but a stale parent is handled
    // defensively): if it no longer resolves we drop the result rather
    // than panic.
    let render_children_changed = old_ids != result;
    if let Some(parent_node) = tree.get_mut(parent_id) {
        parent_node.set_child_ids(result);
    }
    if render_children_changed {
        // A retake installs its destination-local provisional order before
        // update/observer callbacks. Later keyed claims may still permute the
        // suffix, so request one authoritative full-tree synchronization after
        // the entire build drain instead of scanning globally per parent.
        tree.mark_render_reorder_needed();
    }
    if render_children_changed && let Some(parent_node) = tree.get(parent_id) {
        let element = parent_node.element();
        if let Some(render_id) = element.render_id() {
            let pipeline = element.pipeline_owner().expect(
                "BUG: active render element must have a PipelineOwner when children reorder",
            );
            pipeline.with_mut(|owner| owner.note_render_children_reordered(render_id));
        }
    }
}

/// Whether the slab child `old_id` can be updated in place by `new` —
/// same concrete view type AND matching key.
///
/// Reads the old
/// side from the slab through a **fresh, immediately-dropped**
/// [`ElementTree::get`] borrow rather than from a `&dyn ElementBase`
/// held by the caller. A stale / absent `old_id` is treated as
/// not-updatable (returns `false`), never a panic.
///
/// Key comparison is two-stage:
/// 1. hash equality (cheap, via [`ElementBase::current_key_hash`]);
/// 2. on a hash hit where both sides are keyed, a semantic
///    [`ViewKey::key_eq`](flui_foundation::ViewKey::key_eq) check via
///    [`ElementBase::current_key`] to reject `u64` collisions.
pub(crate) fn can_update_by_id(tree: &ElementTree, old_id: ElementId, new: &dyn View) -> bool {
    let Some(node) = tree.get(old_id) else {
        return false;
    };
    let old: &dyn ElementBase = node.element();

    if old.view_type_id() != new.view_type_id() {
        return false;
    }
    // Stage 1: hash quick check. Both keyless (`None == None`) proceeds;
    // one-side-keyed (`None != Some`) rejects; both-keyed-unequal-hash
    // rejects without consulting the typed accessors.
    if old.current_key_hash() != new.key().map(flui_foundation::ViewKey::key_hash) {
        return false;
    }
    // Stage 2: reachable only when both keyless or both keyed with equal
    // hash. The keyless case (common) short-circuits.
    let Some(new_key) = new.key() else {
        return true;
    };
    // Both keyed + hashes agree: defend against a `u64` collision by
    // asking the underlying key whether the two are really equal. A
    // missing `current_key` override (an element that hashes a key but
    // does not expose it) falls through to "no match" — strictly safer
    // than trusting a bare hash.
    debug_assert!(
        old.current_key().is_some(),
        "ElementBase overrode current_key_hash to Some(_) but left current_key None: \
         keyed id-reconcile would silently lose state on every reorder. Override BOTH or NEITHER.",
    );
    old.current_key()
        .is_some_and(|old_key| new_key.key_eq(old_key))
}

/// The key hash of the slab child `old_id`, or `None` if it is keyless
/// (or its id no longer resolves).
///
/// Reads through a fresh, immediately-dropped [`ElementTree::get`]
/// borrow — no slab reference escapes this call.
fn key_hash_of(tree: &ElementTree, old_id: ElementId) -> Option<u64> {
    tree.get(old_id)?.element().current_key_hash()
}

/// The view `TypeId` of the slab child `old_id`, or `None` if its id no
/// longer resolves.
///
/// Read through a fresh, immediately-dropped [`ElementTree::get`] borrow
/// so the captured identity survives the subsequent `remove` of the node
/// — the `Unmount` disposition needs the type the child *had* before the
/// slab freed its slot.
fn view_type_of(tree: &ElementTree, old_id: ElementId) -> Option<std::any::TypeId> {
    Some(tree.get(old_id)?.element().view_type_id())
}

/// The current slot index of the slab child `old_id`, or `None` if its
/// id no longer resolves.
///
/// Used at the phase-4 keyed-claim site to tell a `Reuse` (slot
/// unchanged) from a `Reorder` (slot moved): the value read here is the
/// child's OLD slot, because the tail-of-pass [`set_child_slot`]
/// re-stamping has not run yet.
fn slot_of(tree: &ElementTree, old_id: ElementId) -> Option<usize> {
    Some(tree.get(old_id)?.slot())
}

/// Claim the old-middle child a keyed `new_view` should reuse, returning
/// its OLD-SLOT INDEX and removing that index from `old_keyed` so a later
/// duplicate-key view cannot reclaim it (first-wins). The caller clears
/// the slot by index in O(1) — there is no per-claim scan of `old_slots`.
///
/// Walks the whole hash bucket (distinct keys can collide on `u64`) and
/// returns the index of the first candidate that [`can_update_by_id`]
/// accepts — non-matching candidates stay in the bucket for a later view.
/// Returns `None` for a keyless new view (those only match positionally,
/// already handled by the top/bottom scans) or when no candidate matches.
fn claim_old_for_new(
    tree: &ElementTree,
    new_view: &dyn View,
    old_keyed: &mut HashMap<u64, Vec<usize>>,
    old_slots: &[Option<ElementId>],
) -> Option<usize> {
    let key_hash = new_view.key()?.key_hash();
    let bucket = old_keyed.get_mut(&key_hash)?;
    let position = bucket.iter().position(|&old_idx| {
        old_slots[old_idx].is_some_and(|old_id| can_update_by_id(tree, old_id, new_view))
    })?;
    let old_idx = bucket.remove(position);
    if bucket.is_empty() {
        old_keyed.remove(&key_hash);
    }
    Some(old_idx)
}

/// Apply `new` to the reused slab child `id` through the bounded update
/// primitive, returning the element id that now occupies `target_slot`.
/// A successful resident update returns `id`; a caught user-hook panic
/// finalizes it and returns the recovery substitute's id.
fn update_child(
    tree: &mut ElementTree,
    parent_id: ElementId,
    id: ElementId,
    new: &dyn View,
    target_slot: usize,
    owner: &mut crate::ElementOwner<'_>,
) -> ElementId {
    let (old_slot, old_view_type_id, old_key_hash) = {
        let resident = tree
            .get(id)
            .expect("BUG: a reconcile update candidate must resolve before its bounded update");
        (
            resident.slot(),
            resident.element().view_type_id(),
            resident.element().current_key_hash(),
        )
    };

    // The bounded primitive's `try_update` re-clones the node's stored key
    // from the new view, keeping the keyed-match field in lock-step — mirrors
    // the box reconciler's per-update key re-clone.
    let now = tree.update_or_substitute(
        id,
        new,
        target_slot,
        owner,
        "updating child during reconcile",
    );

    // Keeping a keyed child is a declaration of that key just as much as
    // mounting one is: a parent that holds its `GlobalKey` child across a
    // rebuild is still claiming the key this frame, and the frame boundary
    // must see that claim or a second parent grafting the same element
    // would look like a lone, legal reparent. Flutter records the same
    // reservation from `Element.updateChild`'s reuse branch
    // (`framework.dart:4086`), which covers update and inflate alike.
    if now == id
        && let Some(key) = new.key()
        && key.is_global_key()
    {
        owner.reserve_global_key(parent_id, id, key);
    }
    if now != id {
        emit_event(&ReconcileEvent::unmount(
            parent_id,
            old_slot,
            old_view_type_id,
            old_key_hash,
        ));
    }
    now
}

/// Remove the slab child `id` AND its whole subtree.
///
/// E3 (atomic box→arena swap): the slab is the single element graph, so
/// [`ElementTree::remove`] frees ONLY `id`'s own slot — it does not
/// recurse children. A bare `tree.remove(id)` would therefore orphan
/// every descendant: leaked in the slab with its `on_unmount` (GlobalKey
/// deregistration, dependent cleanup, render-object detach) never run and
/// its `parent` edge left dangling at a freed slot.
///
/// Two cases, branched on the top node's keyed-ness, peeked via
/// `registered_global_key_hash` BEFORE touching anything:
/// - **Keyed top** → soft-removes via `remove` into the inactive queue and
///   returns immediately, leaving the subtree intact in the slab.
///   [`BuildOwner::finalize_tree`](crate::BuildOwner::finalize_tree) later
///   re-collects that subtree and tears it down deepest-first, preserving
///   the same-frame GlobalKey retake window. The subtree snapshot below is
///   never needed for this branch, so checking keyed-ness first avoids
///   paying for that walk on every keyed-top removal.
/// - **Unkeyed top** → its descendants are freed here deepest-first via
///   [`ElementTree::remove_finalized`](crate::tree::ElementTree::remove_finalized)
///   BEFORE the top's own `remove`, mirroring `finalize_tree`'s
///   reverse-pre-order drain so no parent slot is freed before its
///   children — see the ordering rationale below.
///
/// A keyed *descendant* of an unkeyed top is freed (not soft-removed) — it
/// loses its retake window because its ancestor is already gone. The active
/// GlobalKey move path only applies while the keyed element itself remains
/// active and registered; E3's contract here is that every descendant unmounts
/// exactly once.
///
/// Order for an unkeyed top: descendants unmount deepest-first BEFORE the
/// top, not after. An element's own `unmount` (`RenderBehavior::on_unmount`)
/// looks up its render object by id and calls the view-level
/// `did_unmount_render_object` hook ONLY if that lookup still succeeds —
/// which is how `MouseRegion`/`ClipPath`/`Listener` unregister from the
/// owner-local interaction lane on unmount. The top's own render-object
/// removal cascades (`PipelineOwner::remove_render_object` recurses over
/// descendants), so unmounting the top first would delete every descendant's
/// render object before that descendant's own `unmount` ever ran — silently
/// skipping its view-level hook and orphaning its interaction-lane
/// registration for the life of the owner. Freeing descendants first makes
/// each one's own render-object removal (part of its own `unmount`) a no-op
/// by the time the top's cascade would otherwise have reached it.
///
/// Stale / absent ids are a no-op inside `remove` / `remove_finalized`.
fn remove_child(
    tree: &mut ElementTree,
    parent_id: ElementId,
    id: ElementId,
    owner: &mut crate::ElementOwner<'_>,
) {
    // The parent is giving this child up, so whatever `GlobalKey` it
    // declared for it earlier in the frame is withdrawn: a parent that no
    // longer holds a keyed child is not a claimant on that key, and leaving
    // the reservation in place would make the frame boundary report a
    // duplicate against a claim nobody is making. Flutter withdraws the
    // same way in `_debugRemoveGlobalKeyReservationFor`
    // (`framework.dart:3188`).
    owner.forget_global_key_reservation(parent_id, id);

    // One implementation, two callers. This used to carry its own copy of
    // the subtree walk, and the copies drifted: `remove_subtree` learned to
    // stop at a `GlobalKey`'d descendant (issue #838) while this one — the
    // path an ORDINARY dense parent takes — kept freeing them, so the fix was
    // described as tree-wide while covering only the sparse and root cases.
    //
    // `remove_subtree` handles the keyed-top case identically (it soft-removes
    // and returns without walking), so delegating loses nothing.
    tree.remove_subtree(id, owner, super::SubtreeRemoval::DeactivateKeyed);
}

/// Refresh the `slot` metadata of the surviving child `id` to `slot`
/// through a fresh `&mut tree` borrow that ends with this call.
///
/// Keeps `ElementNode::slot()` in lockstep with the child's index in the
/// parent's `child_ids` list after a reorder. A stale / absent id is a
/// no-op (`get_mut` returns `None`).
fn set_child_slot(tree: &mut ElementTree, id: ElementId, slot: usize) {
    if let Some(node) = tree.get_mut(id) {
        node.slot = slot;
    }
}

#[cfg(test)]
#[path = "id_reconcile/tests.rs"]
mod tests;
