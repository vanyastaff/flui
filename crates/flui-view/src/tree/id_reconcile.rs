//! Id-based keyed child reconciliation over the slab-resident
//! [`ElementTree`] — the **production** child reconciler.
//!
//! # Relationship to the box-graph reconciler
//!
//! This is the reconciler the production build path runs after the E3
//! atomic box→arena swap: [`BuildOwner::build_scope`](crate::BuildOwner)
//! feeds each dirty element's freshly-built child views to
//! [`reconcile_children_by_id`], which permutes the parent's
//! [`ElementNode::child_ids`](super::ElementNode) list, reusing /
//! inserting / removing real slab nodes through the [`ElementTree`]
//! accessors. The single element graph is the slab.
//!
//! The original box-vec `reconcile_children` (permuting a caller-owned
//! `Vec<Box<dyn ElementBase>>`) is now dead in production and retained
//! only as a `cfg(test)` / `feature = "test-utils"`-gated keyed-match +
//! `ReconcileEvent` emission reference; this module mirrors its matching
//! semantics over the slab.
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
//! # Matching semantics (Flutter-faithful, mirrors the box reconciler)
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

use super::element_tree::ElementTree;
use crate::view::{ElementBase, View};

/// Reconcile the slab-resident children of `parent_id` against
/// `new_views`, then write the resulting child-id list back onto the
/// parent node.
///
/// On return, `parent_id`'s [`child_ids`](super::ElementNode::child_ids)
/// holds exactly `new_views.len()` ids in new-view order: a reused (and
/// `update`d) old child where type + key matched, or a freshly
/// [`inserted`](ElementTree::insert) child otherwise. Old children that
/// found no match have been [`removed`](ElementTree::remove). Every
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
/// the same collision-resistance cost the box reconciler pays. Both `n`
/// and `m` are bounded by the parent's fan-out, not the whole tree.
pub(crate) fn reconcile_children_by_id(
    tree: &mut ElementTree,
    parent_id: ElementId,
    new_views: &[Box<dyn View>],
    owner: &mut crate::ElementOwner<'_>,
) {
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
    // `None` once a new view claims it. Mirrors the box reconciler's
    // `old_slots: Vec<Option<Box<dyn ElementBase>>>`.
    let mut old_slots: Vec<Option<ElementId>> = old_ids.iter().copied().map(Some).collect();

    // The reconciled id list, built front-to-back in new-view order.
    let mut result: Vec<ElementId> = Vec::with_capacity(new_len);

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
        update_child(tree, old_id, new_views[new_top].as_ref(), owner);
        old_slots[old_top] = None;
        result.push(old_id);
        old_top += 1;
        new_top += 1;
    }

    // ── Phase 2: scan the bottom of both lists while children match.
    // Matches are RECORDED (the bounds shrink); the actual `update` runs
    // in phase 5a so every update is applied strictly front-to-back,
    // matching the box reconciler's ordering guarantee.
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
    // The bucket is a `Vec<ElementId>` (not a single id) so two old
    // children with DISTINCT keys that collide on `u64` hash both stay
    // claim candidates — the symmetric defense the box reconciler also
    // carries. Phase 4 disambiguates via the semantic `key_eq` inside
    // `can_update_by_id`.
    let mut old_keyed: HashMap<u64, Vec<ElementId>> = HashMap::new();
    for slot in old_slots.iter_mut().take(old_bottom).skip(old_top) {
        let Some(old_id) = *slot else {
            continue;
        };
        if let Some(hash) = key_hash_of(tree, old_id) {
            // Keyed: defer the claim to phase 4. FIFO bucket order
            // preserves first-wins across true duplicate keys.
            old_keyed.entry(hash).or_default().push(old_id);
        } else {
            // Keyless middle child with no positional match: remove.
            *slot = None;
            remove_child(tree, old_id, owner);
        }
    }

    // ── Phase 4: sync the new middle front-to-back. A keyed new view
    // claims its old match from `old_keyed`; everything else inserts a
    // fresh child. Claimed olds are cleared from `old_slots` so phase 5b
    // does not also remove them.
    for new_offset in 0..(new_bottom - new_top) {
        let new_slot = new_top + new_offset;
        let new_view = new_views[new_slot].as_ref();
        if let Some(old_id) = claim_old_for_new(tree, new_view, &mut old_keyed) {
            update_child(tree, old_id, new_view, owner);
            clear_slot(&mut old_slots, old_id);
            result.push(old_id);
        } else {
            let new_id = tree.insert(new_view, parent_id, new_slot, owner);
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
        update_child(tree, old_id, new_views[new_idx].as_ref(), owner);
        result.push(old_id);
    }

    // ── Phase 5b: remove any old child never claimed.
    for slot in &mut old_slots {
        if let Some(old_id) = slot.take() {
            remove_child(tree, old_id, owner);
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
            owner.schedule_build_for(id, depth);
        }
    }

    // ── Step 3: re-borrow the parent exactly once to store the result.
    // The parent may have been soft-removed mid-reconcile (a keyed child
    // removal cannot affect the parent, but a stale parent is handled
    // defensively): if it no longer resolves we drop the result rather
    // than panic.
    if let Some(parent_node) = tree.get_mut(parent_id) {
        parent_node.set_child_ids(result);
    }
}

/// Whether the slab child `old_id` can be updated in place by `new` —
/// same concrete view type AND matching key.
///
/// Mirrors the box reconciler's `can_update_element`, but reads the old
/// side from the slab through a **fresh, immediately-dropped**
/// [`ElementTree::get`] borrow rather than from a `&dyn ElementBase`
/// held by the caller. A stale / absent `old_id` is treated as
/// not-updatable (returns `false`), never a panic.
///
/// Key comparison is two-stage, exactly as in the box reconciler:
/// 1. hash equality (cheap, via [`ElementBase::current_key_hash`]);
/// 2. on a hash hit where both sides are keyed, a semantic
///    [`ViewKey::key_eq`](flui_foundation::ViewKey::key_eq) check via
///    [`ElementBase::current_key`] to reject `u64` collisions.
fn can_update_by_id(tree: &ElementTree, old_id: ElementId, new: &dyn View) -> bool {
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

/// Claim the old-middle child a keyed `new_view` should reuse, removing
/// it from `old_keyed` so a later duplicate-key view cannot reclaim it
/// (first-wins).
///
/// Walks the whole hash bucket (distinct keys can collide on `u64`) and
/// returns the first candidate that [`can_update_by_id`] accepts —
/// non-matching candidates stay in the bucket for a later view. Returns
/// `None` for a keyless new view (those only match positionally, already
/// handled by the top/bottom scans) or when no candidate matches.
fn claim_old_for_new(
    tree: &ElementTree,
    new_view: &dyn View,
    old_keyed: &mut HashMap<u64, Vec<ElementId>>,
) -> Option<ElementId> {
    let key_hash = new_view.key()?.key_hash();
    let bucket = old_keyed.get_mut(&key_hash)?;
    let position = bucket
        .iter()
        .position(|&old_id| can_update_by_id(tree, old_id, new_view))?;
    let old_id = bucket.remove(position);
    if bucket.is_empty() {
        old_keyed.remove(&key_hash);
    }
    Some(old_id)
}

/// Clear `old_id` from the `old_slots` working buffer once it has been
/// claimed, so phase 5b does not remove an element that is being reused.
///
/// Linear scan over the (parent-fan-out-bounded) slot list. The id is
/// present at most once — old children are unique in a child list.
fn clear_slot(old_slots: &mut [Option<ElementId>], old_id: ElementId) {
    if let Some(slot) = old_slots.iter_mut().find(|slot| **slot == Some(old_id)) {
        *slot = None;
    }
}

/// Apply `new` to the reused slab child `id` through a fresh `&mut tree`
/// borrow that ends with this call.
///
/// [`ElementTree::update`] is itself a no-op for a stale / absent id, so
/// this is safe to call unconditionally inside the phases.
fn update_child(
    tree: &mut ElementTree,
    id: ElementId,
    new: &dyn View,
    owner: &mut crate::ElementOwner<'_>,
) {
    // `ElementTree::update` re-clones the node's stored key from the new
    // view, keeping the keyed-match field in lock-step — mirrors the box
    // reconciler's per-update key re-clone.
    tree.update(id, new, owner);
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
/// Two cases, branched on the top node's keyed-ness via `remove`'s return:
/// - **Keyed top** → `remove` soft-removes it into the inactive queue and
///   leaves the subtree intact in the slab.
///   [`BuildOwner::finalize_tree`](crate::BuildOwner::finalize_tree) later
///   re-collects that subtree and tears it down deepest-first, preserving
///   the same-frame GlobalKey retake window. Nothing more to do here.
/// - **Unkeyed top** → `remove` eager-unmounts + frees only the top,
///   returning the node. Its descendants are now orphaned, so they are
///   freed here deepest-first via
///   [`ElementTree::remove_finalized`](crate::tree::ElementTree::remove_finalized),
///   mirroring `finalize_tree`'s reverse-pre-order drain so no parent slot
///   is freed before its children.
///
/// A keyed *descendant* of an unkeyed top is freed (not soft-removed) — it
/// loses its retake window because its ancestor is already gone. That
/// cross-parent reparent is the GlobalKey-reparent case deferred to E3.x;
/// E3's contract is only that every descendant unmounts exactly once.
///
/// Stale / absent ids are a no-op inside `remove` / `remove_finalized`.
fn remove_child(tree: &mut ElementTree, id: ElementId, owner: &mut crate::ElementOwner<'_>) {
    // Snapshot the subtree pre-order (parent before children) BEFORE
    // touching the top, while every `child_ids` list is still intact.
    // Owned `Vec` → no slab borrow is held across the removals
    // (extract-then-apply).
    let mut subtree = Vec::new();
    collect_subtree_preorder(tree, id, &mut subtree);

    // Remove the top. `Some` ⇒ eager (unkeyed) free; `None` ⇒ soft-removed
    // (keyed) and parked for `finalize_tree`.
    let removed_eagerly = tree.remove(id, owner).is_some();

    if removed_eagerly {
        // Free the orphaned descendants deepest-first. `subtree[0]` is the
        // top (already removed); reverse of pre-order visits every parent
        // after all of its descendants.
        for &descendant in subtree[1..].iter().rev() {
            tree.remove_finalized(descendant, owner);
        }
    }
}

/// Collect `id` and its whole subtree in pre-order (parent before
/// children) through fresh, immediately-dropped `&ElementTree` borrows.
///
/// The caller removes the collected ids in REVERSE (deepest-first) so no
/// parent slot is freed before its children — mirrors
/// [`BuildOwner::finalize_tree`](crate::BuildOwner::finalize_tree) +
/// `collect_elements_to_unmount`.
///
/// Complexity: O(n) over the subtree size n (each node visited once); the
/// recursion depth equals the subtree height.
fn collect_subtree_preorder(tree: &ElementTree, id: ElementId, out: &mut Vec<ElementId>) {
    out.push(id);
    // Owned snapshot so no slab borrow is held across the recursive call.
    let children: Vec<ElementId> = match tree.get(id) {
        Some(node) => node.child_ids().to_vec(),
        None => return,
    };
    for child in children {
        collect_subtree_preorder(tree, child, out);
    }
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
mod tests {
    use super::*;
    use crate::view::{IntoView, View, ViewExt};
    use crate::{BuildContext, BuildOwner, StatelessElement, StatelessView};
    use flui_foundation::{ValueKey, ViewKey};

    /// A keyless leaf-ish stateless test view. `tag` distinguishes
    /// instances; the self-returning `build` is never driven here (the
    /// id-reconciler does not call `perform_build`), so it cannot
    /// recurse.
    #[derive(Clone)]
    struct TestView {
        #[expect(
            dead_code,
            reason = "carried only so distinct instances differ under Clone"
        )]
        tag: u32,
    }

    impl TestView {
        fn new(tag: u32) -> Self {
            Self { tag }
        }
    }

    impl StatelessView for TestView {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            self.clone().boxed()
        }
    }

    impl View for TestView {
        fn create_element(&self) -> Box<dyn ElementBase> {
            use crate::element::StatelessBehavior;
            Box::new(StatelessElement::new(self, StatelessBehavior))
        }
    }

    /// A keyed stateless test view carrying a `ValueKey<u32>`. Reuses the
    /// same `StatelessElement` machinery; `key()` is overridden so the
    /// reconciler can match by key.
    #[derive(Clone)]
    struct KeyedView {
        key: ValueKey<u32>,
    }

    impl KeyedView {
        fn new(key: u32) -> Self {
            Self {
                key: ValueKey::new(key),
            }
        }
    }

    impl StatelessView for KeyedView {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            self.clone().boxed()
        }
    }

    impl View for KeyedView {
        fn create_element(&self) -> Box<dyn ElementBase> {
            use crate::element::StatelessBehavior;
            Box::new(StatelessElement::new(self, StatelessBehavior))
        }

        fn key(&self) -> Option<&dyn ViewKey> {
            Some(&self.key)
        }
    }

    /// A hostile key that hashes EVERY instance to the same `u64` but
    /// compares by inner `tag` — exercises the production collision
    /// defense: two distinct `ColliderKey`s land in one hash bucket, and
    /// only the semantic `key_eq` (consulted on a hash hit) tells them
    /// apart. Mirrors the box reconciler's `ColliderKey` reference.
    #[derive(Clone)]
    struct ColliderKey {
        tag: u64,
    }

    impl ViewKey for ColliderKey {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn key_eq(&self, other: &dyn ViewKey) -> bool {
            other
                .as_any()
                .downcast_ref::<Self>()
                .is_some_and(|o| self.tag == o.tag)
        }
        fn key_hash(&self) -> u64 {
            // Deliberate collision — every ColliderKey hashes to 0xDEAD.
            0xDEAD
        }
        fn clone_key(&self) -> Box<dyn ViewKey> {
            Box::new(self.clone())
        }
        fn debug_fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "ColliderKey({})", self.tag)
        }
    }

    /// A keyed stateless view carrying a [`ColliderKey`].
    #[derive(Clone)]
    struct ColliderView {
        key: ColliderKey,
    }

    impl ColliderView {
        fn new(tag: u64) -> Self {
            Self {
                key: ColliderKey { tag },
            }
        }
    }

    impl StatelessView for ColliderView {
        fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
            self.clone().boxed()
        }
    }

    impl View for ColliderView {
        fn create_element(&self) -> Box<dyn ElementBase> {
            use crate::element::StatelessBehavior;
            Box::new(StatelessElement::new(self, StatelessBehavior))
        }

        fn key(&self) -> Option<&dyn ViewKey> {
            Some(&self.key)
        }
    }

    /// Build a `Vec<Box<dyn View>>` of keyless `TestView`s with the given
    /// tags.
    fn plain_views(tags: &[u32]) -> Vec<Box<dyn View>> {
        tags.iter()
            .map(|&t| Box::new(TestView::new(t)) as Box<dyn View>)
            .collect()
    }

    /// Build a `Vec<Box<dyn View>>` of keyed `KeyedView`s with the given
    /// keys.
    fn keyed_views(keys: &[u32]) -> Vec<Box<dyn View>> {
        keys.iter()
            .map(|&k| Box::new(KeyedView::new(k)) as Box<dyn View>)
            .collect()
    }

    /// Mount a fresh keyless root and return `(tree, owner, root_id)`.
    fn fixture() -> (ElementTree, BuildOwner, ElementId) {
        let mut tree = ElementTree::new();
        let mut owner = BuildOwner::new();
        let root_id = tree.mount_root(&TestView::new(0), &mut owner.element_owner_mut());
        (tree, owner, root_id)
    }

    /// Empty parent → N children inserted; every stored child id
    /// resolves and parent/slot wiring is correct.
    #[test]
    fn empty_parent_inserts_all_children() {
        let (mut tree, mut owner, root) = fixture();
        let views = plain_views(&[1, 2, 3]);

        reconcile_children_by_id(&mut tree, root, &views, &mut owner.element_owner_mut());

        let child_ids = tree.get(root).expect("root resolves").child_ids().to_vec();
        assert_eq!(child_ids.len(), 3, "three children must be inserted");
        for (slot, id) in child_ids.iter().enumerate() {
            let node = tree
                .get(*id)
                .expect("each child id must resolve in the slab");
            assert_eq!(node.parent(), Some(root), "child parent must be the root");
            assert_eq!(node.slot(), slot, "child slot must match its position");
        }
        // root + 3 children.
        assert_eq!(tree.len(), 4);
    }

    /// Update-in-place: reconciling the SAME view shape again reuses
    /// every id — no inserts, no removes, no slab growth.
    #[test]
    fn same_views_reuse_ids_no_insert_or_remove() {
        let (mut tree, mut owner, root) = fixture();

        reconcile_children_by_id(
            &mut tree,
            root,
            &plain_views(&[1, 2, 3]),
            &mut owner.element_owner_mut(),
        );
        let first = tree.get(root).unwrap().child_ids().to_vec();
        let len_after_first = tree.len();

        // Second pass with the same shape (fresh view instances, same
        // types/keys) must reuse the same ids in the same order.
        reconcile_children_by_id(
            &mut tree,
            root,
            &plain_views(&[10, 20, 30]),
            &mut owner.element_owner_mut(),
        );
        let second = tree.get(root).unwrap().child_ids().to_vec();

        assert_eq!(
            first, second,
            "same-shape reconcile must reuse the same ids"
        );
        assert_eq!(
            tree.len(),
            len_after_first,
            "no slab node may be inserted or removed on a same-shape reconcile",
        );
    }

    /// Keyed reorder: permuting keyed children makes the stored ids
    /// follow their keys (the element — and thus its state — moves with
    /// its key, it is not absorbed by the sibling in the old position).
    #[test]
    fn keyed_reorder_ids_follow_keys() {
        let (mut tree, mut owner, root) = fixture();

        reconcile_children_by_id(
            &mut tree,
            root,
            &keyed_views(&[1, 2, 3]),
            &mut owner.element_owner_mut(),
        );
        let before = tree.get(root).unwrap().child_ids().to_vec();
        assert_eq!(before.len(), 3);
        let (id1, id2, id3) = (before[0], before[1], before[2]);

        // Reorder keys [1,2,3] -> [3,1,2].
        reconcile_children_by_id(
            &mut tree,
            root,
            &keyed_views(&[3, 1, 2]),
            &mut owner.element_owner_mut(),
        );
        let after = tree.get(root).unwrap().child_ids().to_vec();

        assert_eq!(
            after,
            vec![id3, id1, id2],
            "each keyed child id must move to the slot its key now occupies",
        );
        // No element was created or destroyed: same three ids, same slab size.
        assert_eq!(tree.len(), 4, "reorder must not insert or remove any node");
        for id in [id1, id2, id3] {
            assert!(
                tree.get(id).is_some(),
                "every reordered id must still resolve"
            );
        }
    }

    /// Shrink: N children → fewer. The dropped ids are removed from the
    /// slab and no longer resolve.
    #[test]
    fn shrink_removes_stale_ids() {
        let (mut tree, mut owner, root) = fixture();

        reconcile_children_by_id(
            &mut tree,
            root,
            &keyed_views(&[1, 2, 3, 4]),
            &mut owner.element_owner_mut(),
        );
        let before = tree.get(root).unwrap().child_ids().to_vec();
        assert_eq!(before.len(), 4);

        // Keep keys 1 and 3; drop 2 and 4.
        reconcile_children_by_id(
            &mut tree,
            root,
            &keyed_views(&[1, 3]),
            &mut owner.element_owner_mut(),
        );
        let after = tree.get(root).unwrap().child_ids().to_vec();

        assert_eq!(
            after,
            vec![before[0], before[2]],
            "survivors keep their ids"
        );
        // The two dropped children must be gone from the slab.
        assert!(
            tree.get(before[1]).is_none(),
            "dropped key-2 id must no longer resolve",
        );
        assert!(
            tree.get(before[3]).is_none(),
            "dropped key-4 id must no longer resolve",
        );
        assert_eq!(tree.len(), 3, "root + 2 survivors remain in the slab");
    }

    /// Grow: fewer children → N. Survivors keep ids; new slots get fresh
    /// resolvable ids.
    #[test]
    fn grow_inserts_new_children() {
        let (mut tree, mut owner, root) = fixture();

        reconcile_children_by_id(
            &mut tree,
            root,
            &keyed_views(&[1, 2]),
            &mut owner.element_owner_mut(),
        );
        let before = tree.get(root).unwrap().child_ids().to_vec();
        assert_eq!(before.len(), 2);

        // Grow to keys [1, 2, 3, 4]: 1 and 2 reuse, 3 and 4 are new.
        reconcile_children_by_id(
            &mut tree,
            root,
            &keyed_views(&[1, 2, 3, 4]),
            &mut owner.element_owner_mut(),
        );
        let after = tree.get(root).unwrap().child_ids().to_vec();

        assert_eq!(after.len(), 4);
        assert_eq!(&after[..2], &before[..], "existing keys reuse their ids");
        assert_ne!(after[2], before[0]);
        assert_ne!(after[2], before[1]);
        for id in &after {
            assert!(
                tree.get(*id).is_some(),
                "every child id must resolve after grow"
            );
        }
        assert_eq!(tree.len(), 5, "root + 4 children");
    }

    /// Type-mismatch replacement: a keyless slot whose view type changes
    /// is removed and a fresh element of the new type is inserted (not
    /// reused).
    #[test]
    fn type_mismatch_replaces_child() {
        let (mut tree, mut owner, root) = fixture();

        // Start with one keyless TestView child.
        reconcile_children_by_id(
            &mut tree,
            root,
            &plain_views(&[1]),
            &mut owner.element_owner_mut(),
        );
        let old_id = tree.get(root).unwrap().child_ids()[0];

        // Replace with a single KeyedView (different concrete type).
        reconcile_children_by_id(
            &mut tree,
            root,
            &keyed_views(&[9]),
            &mut owner.element_owner_mut(),
        );
        let new_id = tree.get(root).unwrap().child_ids()[0];

        assert_ne!(new_id, old_id, "type change must mint a fresh element");
        assert!(
            tree.get(old_id).is_none(),
            "replaced element must be removed"
        );
        assert!(
            tree.get(new_id).is_some(),
            "replacement element must resolve"
        );
    }

    /// The double-borrow stressor: read the parent's child-id vec, mutate
    /// many children (insert + update + remove in one pass), then write
    /// the new list back — all without aliasing. Under Miri this proves
    /// no slab borrow is held across another slab mutation. The
    /// assertions confirm the resulting structure is exactly right.
    #[test]
    fn double_borrow_stressor_interleaved_mutations() {
        let (mut tree, mut owner, root) = fixture();

        // Seed: keyed [1, 2, 3, 4, 5].
        reconcile_children_by_id(
            &mut tree,
            root,
            &keyed_views(&[1, 2, 3, 4, 5]),
            &mut owner.element_owner_mut(),
        );
        let seed = tree.get(root).unwrap().child_ids().to_vec();
        assert_eq!(seed.len(), 5);

        // One pass that simultaneously: removes key 2 and key 4, reorders
        // the survivors (3 before 1), keeps 5, and inserts new keys 6, 7.
        // New order: [3, 1, 5, 6, 7].
        reconcile_children_by_id(
            &mut tree,
            root,
            &keyed_views(&[3, 1, 5, 6, 7]),
            &mut owner.element_owner_mut(),
        );
        let after = tree.get(root).unwrap().child_ids().to_vec();

        assert_eq!(after.len(), 5);
        // Survivors keep their original ids, in the new order.
        assert_eq!(after[0], seed[2], "key 3 reused at slot 0");
        assert_eq!(after[1], seed[0], "key 1 reused at slot 1");
        assert_eq!(after[2], seed[4], "key 5 reused at slot 2");
        // Removed keys 2 and 4 no longer resolve.
        assert!(tree.get(seed[1]).is_none(), "removed key 2 must be gone");
        assert!(tree.get(seed[3]).is_none(), "removed key 4 must be gone");
        // New keys 6 and 7 are fresh, resolvable, and distinct from seeds.
        for new_slot in [after[3], after[4]] {
            assert!(tree.get(new_slot).is_some(), "inserted child must resolve");
            assert!(!seed.contains(&new_slot), "inserted child id must be fresh");
        }
        // Every stored child id resolves and points back at the root.
        for (slot, id) in after.iter().enumerate() {
            let node = tree.get(*id).expect("child id resolves");
            assert_eq!(node.parent(), Some(root));
            assert_eq!(node.slot(), slot, "slot wiring follows new order");
        }
        // root + 5 live children (2 removed, 2 inserted, net 5).
        assert_eq!(tree.len(), 6);
    }

    /// Reconciling to an empty view list removes every child and leaves
    /// an empty child-id list.
    #[test]
    fn reconcile_to_empty_removes_all() {
        let (mut tree, mut owner, root) = fixture();

        reconcile_children_by_id(
            &mut tree,
            root,
            &plain_views(&[1, 2, 3]),
            &mut owner.element_owner_mut(),
        );
        let seeded = tree.get(root).unwrap().child_ids().to_vec();
        assert_eq!(seeded.len(), 3);

        reconcile_children_by_id(&mut tree, root, &[], &mut owner.element_owner_mut());
        let after = tree.get(root).unwrap().child_ids().to_vec();

        assert!(after.is_empty(), "empty view list clears the child-id list");
        for id in seeded {
            assert!(tree.get(id).is_none(), "every old child must be removed");
        }
        assert_eq!(tree.len(), 1, "only the root remains");
    }

    /// E3 regression: dropping an (un)keyed child whose top takes the
    /// eager removal path tears down its ENTIRE subtree, not just the top.
    ///
    /// A bare `tree.remove(top)` frees only the top slot and orphans every
    /// descendant — leaked in the slab, `on_unmount` never run, `parent`
    /// edge dangling at a freed slot. The teardown walk in `remove_child`
    /// closes that. `tree.len() == 1` afterwards is the leak assertion:
    /// the buggy single-node remove would leave the chain resident
    /// (`len == 4`) with `a1` / `a1a` still resolving.
    #[test]
    fn eager_remove_tears_down_whole_subtree() {
        let (mut tree, mut owner, root) = fixture();

        // Build root → a → a1 → a1a one level at a time: the reconciler
        // inserts a parent's DIRECT children only (it schedules, it does
        // not recurse), so each generation is seeded explicitly.
        reconcile_children_by_id(
            &mut tree,
            root,
            &plain_views(&[1]),
            &mut owner.element_owner_mut(),
        );
        let a = tree.get(root).unwrap().child_ids()[0];
        reconcile_children_by_id(
            &mut tree,
            a,
            &plain_views(&[2]),
            &mut owner.element_owner_mut(),
        );
        let a1 = tree.get(a).unwrap().child_ids()[0];
        reconcile_children_by_id(
            &mut tree,
            a1,
            &plain_views(&[3]),
            &mut owner.element_owner_mut(),
        );
        let a1a = tree.get(a1).unwrap().child_ids()[0];
        assert_eq!(tree.len(), 4, "root + a + a1 + a1a");

        // Drop `a` from the root's children → `remove_child(a)` must free
        // a, a1 and a1a together.
        reconcile_children_by_id(&mut tree, root, &[], &mut owner.element_owner_mut());

        assert!(tree.get(a).is_none(), "removed subtree top is gone");
        assert!(tree.get(a1).is_none(), "mid descendant is not orphaned");
        assert!(tree.get(a1a).is_none(), "leaf descendant is not orphaned");
        assert_eq!(tree.len(), 1, "only the root remains — no slab leak");
    }

    /// Production FR-024(c) collision defense, false-positive case: a new
    /// keyed view whose key HASH collides with the old child but whose
    /// `key_eq` disagrees must NOT reuse the old element. `can_update_by_id`
    /// rejects the hash hit (top scan AND the Phase-4 bucket walk return
    /// no match), so the new view mints a fresh slab id and the old child
    /// is removed.
    #[test]
    fn keyed_hash_collision_falls_through_to_fresh_id() {
        let (mut tree, mut owner, root) = fixture();

        reconcile_children_by_id(
            &mut tree,
            root,
            &[Box::new(ColliderView::new(1)) as Box<dyn View>],
            &mut owner.element_owner_mut(),
        );
        let old_id = tree.get(root).unwrap().child_ids()[0];

        // Same hash (0xDEAD), different tag → the semantic `key_eq` rejects.
        reconcile_children_by_id(
            &mut tree,
            root,
            &[Box::new(ColliderView::new(2)) as Box<dyn View>],
            &mut owner.element_owner_mut(),
        );
        let new_id = tree.get(root).unwrap().child_ids()[0];

        assert_ne!(
            new_id, old_id,
            "a hash collision must not fool the reconciler into reusing the old \
             element — can_update_by_id's key_eq stage rejects it and a fresh id is minted",
        );
    }

    /// Production FR-024(c) collision defense, symmetric case: two old
    /// children whose distinct keys collide on hash, plus a new view that
    /// `key_eq`s the SECOND. The Phase-4 bucket walk
    /// ([`match_old_for_new`]) must walk both colliding candidates and
    /// reuse the one `key_eq` accepts — not the first in the bucket, and
    /// not a fresh element. A trailing keyless view keeps the match in the
    /// middle so the bottom scan cannot shortcut the bucket walk.
    #[test]
    fn keyed_hash_collision_bucket_walk_reuses_correct_old() {
        let (mut tree, mut owner, root) = fixture();

        reconcile_children_by_id(
            &mut tree,
            root,
            &[
                Box::new(ColliderView::new(1)) as Box<dyn View>,
                Box::new(ColliderView::new(2)) as Box<dyn View>,
            ],
            &mut owner.element_owner_mut(),
        );
        let ids = tree.get(root).unwrap().child_ids().to_vec();
        let (id_c1, id_c2) = (ids[0], ids[1]);

        // [c1, c2] → [c2', keyless]: c2' matches the SECOND old through the
        // bucket walk; the trailing keyless view blocks a bottom-scan match
        // so the claim is forced through Phase 4.
        reconcile_children_by_id(
            &mut tree,
            root,
            &[
                Box::new(ColliderView::new(2)) as Box<dyn View>,
                Box::new(TestView::new(9)) as Box<dyn View>,
            ],
            &mut owner.element_owner_mut(),
        );
        let after = tree.get(root).unwrap().child_ids().to_vec();

        assert_eq!(
            after[0], id_c2,
            "the bucket walk must reuse the tag=2 old (matched by key_eq), not the \
             hash-colliding tag=1 old and not a fresh element",
        );
        assert!(
            !after.contains(&id_c1),
            "the unmatched tag=1 collider must be removed, not silently reused",
        );
    }

    /// A stale / absent parent id is a no-op, not a panic.
    #[test]
    fn stale_parent_is_noop() {
        let (mut tree, mut owner, root) = fixture();
        // Remove the root, then reconcile against its now-stale id.
        tree.remove(root, &mut owner.element_owner_mut());
        reconcile_children_by_id(
            &mut tree,
            root,
            &plain_views(&[1, 2]),
            &mut owner.element_owner_mut(),
        );
        // No children were inserted; the slab is empty.
        assert_eq!(tree.len(), 0, "stale-parent reconcile must insert nothing");
    }
}
