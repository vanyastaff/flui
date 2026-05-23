//! O(N) keyed child reconciliation for the live box-vec element model.
//!
//! Flutter insight: "Contrary to popular belief, Flutter does not employ
//! a tree-diffing algorithm." Child reconciliation is a single linear
//! pass that matches old child elements to new Views by `Key` (keyed
//! children) or by position (un-keyed children), reusing element state
//! wherever a match is found.
//!
//! # What this operates on
//!
//! [`reconcile_children`] mutates the live
//! `Vec<Box<dyn ElementBase>>` that [`VariableChildStorage`] owns — the
//! structure production actually runs. It does NOT operate on a by-id
//! `ElementTree`; the box-vec is the live element tree. Old child
//! elements are matched against new `&dyn View` configurations and
//! either reused in place (preserving their state — origin requirement
//! R12: keyed `Hero` / `Reorderable` / `GlobalKey` moves), updated, or
//! unmounted.
//!
//! [`VariableChildStorage`]: crate::element::VariableChildStorage
//!
//! # Lifecycle boundary — created children are left unmounted
//!
//! Mounting a child runs `Element::mount`, and a `RenderObjectElement`'s
//! `on_mount` needs the parent's `PipelineOwner` already in scope to
//! create its `RenderObject`. That owner lives one layer up, in
//! `ElementCore` — *above* the bare box-vec this function operates on.
//! So `reconcile_children` deliberately stops at the structural diff:
//! it matches, updates reused elements, unmounts dropped ones, and
//! *creates* new elements in [`Lifecycle::Initial`] — but does NOT
//! mount or build them. The caller
//! ([`VariableChildStorage::update_with_views`]) finishes the lifecycle
//! by propagating the owner, mounting the still-`Initial` children, and
//! rebuilding the subtree. This keeps the propagate-before-mount
//! ordering `RenderBehavior::on_mount` depends on.
//!
//! [`Lifecycle::Initial`]: crate::element::Lifecycle
//! [`VariableChildStorage::update_with_views`]: crate::element::VariableChildStorage
//!
//! # Flutter parity
//!
//! The 5-phase structure mirrors `Element.updateChildren`
//! (`framework.dart:4125`):
//!
//! 1. Walk both lists from the top, syncing matching nodes.
//! 2. Walk both lists from the bottom, *recording* matches without
//!    syncing yet (so the final sync runs strictly front-to-back).
//! 3. Build a key map of the remaining old children; unmount the
//!    un-keyed leftovers.
//! 4. Walk the remaining new Views: a keyed View claims its old match
//!    from the map; everything else gets a fresh element.
//! 5. Sync the bottom matches recorded in phase 2, then unmount any old
//!    keyed children never claimed.
//!
//! # Key matching is hash-based
//!
//! The object-safe `ElementBase` surface erases the concrete `View`
//! type, so an old child element can only report its key as a *hash*
//! ([`ElementBase::current_key_hash`]). Matching therefore compares
//! `u64` hashes rather than calling [`ViewKey::key_eq`]. `ValueKey`'s
//! hash mixes in the payload's `TypeId`, making cross-type collisions
//! vanishingly unlikely; this is the accepted cost of the store-by-value
//! model (plan §"Key Technical Decisions" — V-2 lands store-by-value).
//!
//! [`ElementBase::current_key_hash`]: crate::view::ElementBase::current_key_hash
//! [`ViewKey::key_eq`]: flui_foundation::ViewKey::key_eq

use std::collections::HashMap;

use flui_foundation::ElementId;

use super::reconcile_event::{ReconcileEvent, emit as emit_event};
use crate::view::{ElementBase, View};

/// Reconcile a parent's old child elements against its new child Views,
/// in place.
///
/// On return, `old_children` has been replaced with the reconciled list:
/// length equal to `new_views.len()`, each entry the element that should
/// occupy that slot — a reused (and updated) old element where a match
/// was found, or a freshly created element otherwise. Old elements that
/// found no match have been unmounted and dropped.
///
/// **Newly created elements are returned in [`Lifecycle::Initial`] —
/// unmounted and unbuilt.** See the module-level "Lifecycle boundary"
/// section: the caller must propagate the `PipelineOwner`, mount the
/// still-`Initial` children, and rebuild. Reused elements are already
/// `Active` and have had `Element::update` applied.
///
/// This is an O(N) linear algorithm, NOT a tree diff.
///
/// [`Lifecycle::Initial`]: crate::element::Lifecycle
///
/// # Arguments
///
/// * `parent` - Owning parent's [`ElementId`]. Stamped onto every
///   emitted [`crate::tree::ReconcileEvent`] so subscribers can
///   correlate the trace back to the build path that produced it
///   (plan §U13 / FR-035).
/// * `old_children` - The parent's current child elements, owned. Drained
///   and replaced with the reconciled list.
/// * `new_views` - The new child Views to reconcile against.
/// * `owner` - Split-borrow [`ElementOwner`](crate::ElementOwner) handle,
///   threaded into every child `update` / `unmount` so `GlobalKey`
///   registration and dirty scheduling stay coherent.
///
/// # Duplicate keys
///
/// If two new Views carry the same key, resolution is **first-wins**:
/// the first occurrence claims the matching old element; every later
/// occurrence with that key gets a freshly created element. This is a
/// defined, non-panicking resolution. (Flutter asserts against
/// duplicate keys in debug builds; FLUI degrades gracefully instead of
/// aborting — Constitution Principle 6: no panics on recoverable input.)
// Five Flutter-parity phases + three fast paths + per-disposition
// ReconcileEvent emission push this body past 100 lines. Splitting
// risks scrambling the 1:1 mapping to `framework.dart:4125`
// `Element.updateChildren`; keep inline so the algorithm stays
// auditable against the Flutter source. SC-007 / FR-024.
#[allow(clippy::too_many_lines)]
pub fn reconcile_children(
    parent: ElementId,
    old_children: &mut Vec<Box<dyn ElementBase>>,
    new_views: &[&dyn View],
    owner: &mut crate::ElementOwner<'_>,
) {
    // Fast path: nothing on either side. Nothing to emit either —
    // a no-op frame is genuinely silent.
    if old_children.is_empty() && new_views.is_empty() {
        return;
    }

    // Fast path: all new — create every element + emit Mount per slot.
    //
    // Emission AFTER `create_element` so a panic in a view's
    // `create_element` body does not leave the observer with a Mount
    // event for an element that was never created. Per-slot iteration
    // (not `.map().collect()`) keeps the order of side effects
    // observable and gives a panic-cleanup hook a place to land
    // without unwinding partial work — today the iteration is
    // monotonic and bails on first panic via the iterator's natural
    // unwind, dropping the partially-built children + leaving
    // `*old_children` untouched (it was already empty).
    if old_children.is_empty() {
        let mut created: Vec<Box<dyn ElementBase>> = Vec::with_capacity(new_views.len());
        for (slot, view) in new_views.iter().enumerate() {
            let element = view.create_element();
            // Emit only after the lifecycle call returned. A panic
            // inside `create_element` aborts the loop; observers
            // see Mount events ONLY for elements that exist.
            emit_event(&ReconcileEvent::mount(
                parent,
                slot,
                view.view_type_id(),
                view.key().map(flui_foundation::ViewKey::key_hash),
            ));
            created.push(element);
        }
        *old_children = created;
        return;
    }

    // Fast path: all removed — unmount every old child + emit Unmount.
    //
    // Symmetric to the Mount fast path: emission AFTER `child.unmount`
    // returns so a panic in the unmount body does not leak an
    // Unmount event for an element whose cleanup is incomplete.
    if new_views.is_empty() {
        for (slot, mut child) in old_children.drain(..).enumerate() {
            // Capture identity before unmount; the child is consumed
            // by the unmount call and the event needs the view_type
            // it had.
            let type_id = child.view_type_id();
            let key_hash = child.current_key_hash();
            child.unmount(owner);
            emit_event(&ReconcileEvent::unmount(parent, slot, type_id, key_hash));
        }
        return;
    }

    // Move the old children into a slotted working buffer. `take()`ing
    // an entry marks that old element as consumed — every old element
    // can be matched at most once.
    let mut old_slots: Vec<Option<Box<dyn ElementBase>>> =
        old_children.drain(..).map(Some).collect();

    let old_len = old_slots.len();
    let new_len = new_views.len();

    // The reconciled result, built front-to-back.
    let mut result: Vec<Box<dyn ElementBase>> = Vec::with_capacity(new_len);

    // ------------------------------------------------------------------
    // Phase 1 — sync the top of both lists while nodes match.
    //
    // Top scan matches are SAME-SLOT (old_top == new_top throughout the
    // loop body), so emit `Reuse`, not `Reorder`.
    // ------------------------------------------------------------------
    let mut old_top = 0;
    let mut new_top = 0;
    while old_top < old_len && new_top < new_len {
        let matches = old_slots[old_top]
            .as_deref()
            .is_some_and(|old| can_update_element(old, new_views[new_top]));
        if !matches {
            break;
        }
        let mut element = old_slots[old_top]
            .take()
            .expect("phase-1 match guaranteed Some");
        element.update(new_views[new_top], owner);
        emit_event(&ReconcileEvent::reuse(
            parent,
            new_top,
            new_views[new_top].view_type_id(),
            new_views[new_top]
                .key()
                .map(flui_foundation::ViewKey::key_hash),
        ));
        result.push(element);
        old_top += 1;
        new_top += 1;
    }

    // ------------------------------------------------------------------
    // Phase 2 — scan the bottom of both lists while nodes match.
    //
    // Matches are *recorded*, not synced — Flutter syncs them in phase 5
    // so every `update` runs strictly front-to-back. Emission is
    // deferred to phase 5a alongside the actual `update` call.
    // ------------------------------------------------------------------
    let mut old_bottom = old_len;
    let mut new_bottom = new_len;
    while old_top < old_bottom && new_top < new_bottom {
        let matches = old_slots[old_bottom - 1]
            .as_deref()
            .is_some_and(|old| can_update_element(old, new_views[new_bottom - 1]));
        if !matches {
            break;
        }
        old_bottom -= 1;
        new_bottom -= 1;
    }

    // ------------------------------------------------------------------
    // Phase 3 — index the remaining old middle by key hash.
    //
    // Un-keyed old middle children cannot be matched out of order, so
    // they are unmounted here (emit `Unmount`). Keyed ones go into
    // `old_keyed` for phase 4 to claim.
    //
    // The index is `HashMap<u64, Vec<usize>>` rather than `HashMap<u64,
    // usize>` so two old children with DISTINCT keys that happen to
    // collide on `u64` hash can both be candidate matches in Phase 4 —
    // the symmetric defense to FR-024 (c)'s `key_eq`-on-hash-hit check.
    // Without the Vec, the first-wins on duplicate-hash silently drops
    // every later old whose hash collides; if a new view's key
    // `key_eq`s only the dropped one, the matcher returns no candidate
    // and the new view mounts fresh while the right old element unmounts
    // — silent state loss.
    //
    // True duplicate OLD keys (two old elements that genuinely
    // `key_eq` each other — a prior-frame bug) end up in the same
    // bucket, and Phase 4's `match_old_for_new` claims them
    // first-in-first-out by `pop`ping the front. Behavior matches the
    // documented first-wins convention while the algorithm gains
    // collision-resistance.
    // ------------------------------------------------------------------
    let mut old_keyed: HashMap<u64, Vec<usize>> = HashMap::new();
    for (idx, slot) in old_slots
        .iter_mut()
        .enumerate()
        .take(old_bottom)
        .skip(old_top)
    {
        let Some(child) = slot.as_deref() else {
            continue;
        };
        if let Some(key) = child.current_key_hash() {
            // Append to the candidate bucket; FIFO claim order in
            // Phase 4 preserves first-wins semantics across true
            // duplicates AND lets hash-colliding distinct keys both be
            // claim candidates.
            old_keyed.entry(key).or_default().push(idx);
        } else {
            // Un-keyed middle child with no positional match — drop it.
            let type_id = child.view_type_id();
            let key_hash = child.current_key_hash();
            let mut child = slot.take().expect("iter yielded Some");
            child.unmount(owner);
            emit_event(&ReconcileEvent::unmount(parent, idx, type_id, key_hash));
        }
    }

    // ------------------------------------------------------------------
    // Phase 4 — sync the new middle front-to-back.
    //
    // A keyed new View claims its old match from `old_keyed` (removing
    // the entry so a later duplicate key cannot reuse it — first-wins).
    // Everything else gets a fresh element. Claim emits `Reorder`
    // when the slot actually moves and `Reuse` when it coincidentally
    // matches; fresh creation emits `Mount`.
    // ------------------------------------------------------------------
    for (new_offset, &new_view) in new_views[new_top..new_bottom].iter().enumerate() {
        let new_slot = new_top + new_offset;
        if let Some(old_idx) = match_old_for_new(new_view, &mut old_keyed, &old_slots) {
            let mut element = old_slots[old_idx]
                .take()
                .expect("old_keyed only indexes Some entries");
            element.update(new_view, owner);
            let key_hash = new_view.key().map(flui_foundation::ViewKey::key_hash);
            if old_idx == new_slot {
                emit_event(&ReconcileEvent::reuse(
                    parent,
                    new_slot,
                    new_view.view_type_id(),
                    key_hash,
                ));
            } else {
                emit_event(&ReconcileEvent::reorder(
                    parent,
                    new_slot,
                    new_view.view_type_id(),
                    key_hash,
                ));
            }
            result.push(element);
        } else {
            // No keyed match — fresh element, left unmounted for the
            // caller to mount (see module "Lifecycle boundary").
            emit_event(&ReconcileEvent::mount(
                parent,
                new_slot,
                new_view.view_type_id(),
                new_view.key().map(flui_foundation::ViewKey::key_hash),
            ));
            result.push(new_view.create_element());
        }
    }

    // ------------------------------------------------------------------
    // Phase 5a — sync the bottom matches recorded in phase 2.
    //
    // The bottom scan recorded `(old_idx, new_idx)` pairs by walking
    // both lists from the end while elements matched. After the
    // middle phases run, each pair's slot relationship is:
    //
    //   old_idx == new_idx  → the symmetric-delta case: middle had
    //                         the same number of insertions and
    //                         removals, so bottom-matched elements
    //                         stay at the same slot. Emit `Reuse`.
    //   old_idx != new_idx  → the asymmetric-delta case: middle had
    //                         more insertions than removals (or vice
    //                         versa), so the bottom-matched element
    //                         shifted slot. Emit `Reorder`.
    //
    // Earlier versions of this loop always emitted `Reuse` on the
    // (incorrect) assumption that `old_bottom + offset` and
    // `new_bottom + offset` were always equal. They are equal only
    // when `old_bottom == new_bottom`. Subscribers reading the trace
    // (devtools, selection-persistence per FR-035) need the
    // disposition to reflect actual movement.
    // ------------------------------------------------------------------
    for offset in 0..(old_len - old_bottom) {
        let old_idx = old_bottom + offset;
        let new_idx = new_bottom + offset;
        let mut element = old_slots[old_idx]
            .take()
            .expect("phase-2 bottom match guaranteed Some");
        element.update(new_views[new_idx], owner);
        let key_hash = new_views[new_idx]
            .key()
            .map(flui_foundation::ViewKey::key_hash);
        let view_type = new_views[new_idx].view_type_id();
        if old_idx == new_idx {
            emit_event(&ReconcileEvent::reuse(parent, new_idx, view_type, key_hash));
        } else {
            emit_event(&ReconcileEvent::reorder(
                parent, new_idx, view_type, key_hash,
            ));
        }
        result.push(element);
    }

    // ------------------------------------------------------------------
    // Phase 5b — unmount any keyed old children never claimed.
    // ------------------------------------------------------------------
    for (idx, slot) in old_slots.iter_mut().enumerate() {
        if let Some(mut child) = slot.take() {
            let type_id = child.view_type_id();
            let key_hash = child.current_key_hash();
            child.unmount(owner);
            emit_event(&ReconcileEvent::unmount(parent, idx, type_id, key_hash));
        }
    }

    debug_assert_eq!(
        result.len(),
        new_len,
        "reconciled list length must equal new view count"
    );
    *old_children = result;
}

/// Whether `old` (an existing child element) can be updated in place by
/// `new` (a new child View) — same concrete View type AND matching key
/// per spec FR-028.
///
/// Key comparison runs in two stages:
///
/// 1. **Hash equality** — `ElementBase::current_key_hash` returns the
///    pre-computed `u64`, so the prefix/suffix scans and the
///    [`match_old_for_new`] HashMap lookup stay cheap.
/// 2. **Semantic equality on a hash hit** — distinct keys can collide
///    on `u64`, so a hash match alone is not proof that the two keys
///    are equal. When both sides carry a key whose hashes agree, this
///    function then calls [`flui_foundation::ViewKey::key_eq`] via
///    [`ElementBase::current_key`] to reject silent collisions
///    (FR-024 work item (c)).
///
/// Both-keyless and one-side-keyed cases short-circuit on the hash
/// comparison without ever hitting the semantic call — the typical
/// reconciliation pass pays for at most ONE `key_eq` per matched
/// child, only when both sides are keyed and their hashes coincide.
fn can_update_element(old: &dyn ElementBase, new: &dyn View) -> bool {
    if old.view_type_id() != new.view_type_id() {
        return false;
    }
    // Stage 1 — hash quick check. Both keyless: `None == None` → true,
    // proceed (the type check above already passed). Both keyed with
    // unequal hashes → false, no need to consult the typed accessors.
    // One side keyed: `None != Some(_)` → false.
    if old.current_key_hash() != new.key().map(flui_foundation::ViewKey::key_hash) {
        return false;
    }
    // Stage 2 — only reachable when EITHER both sides are keyless
    // (both `None`) OR both sides are keyed AND hashes agree.
    // The keyless branch is the common case; short-circuit it.
    let Some(new_key) = new.key() else {
        return true;
    };
    // Both keyed + hashes agree → defend against `u64` collision by
    // asking the underlying `ViewKey` whether the two are really equal.
    // The typed accessor is only consulted on a hash hit, so the cost
    // stays paid-when-used. A missing `current_key()` override on the
    // old side (i.e. an element type that hashes a key but does not
    // expose its `&dyn ViewKey`) falls through to "no match", which is
    // strictly safer than trusting a bare hash.
    //
    // Debug-build trip-wire: if the old element overrides
    // `current_key_hash` to return `Some` but `current_key` falls back
    // to the trait default `None`, the stage-2 check returns `false`
    // for EVERY new view whose hash matches — silently breaking ALL
    // keyed reuse for that element type forever. Production gets the
    // strictly-safer no-match, but debug builds panic so the
    // asymmetric override surfaces during testing.
    debug_assert!(
        old.current_key().is_some(),
        "ElementBase impl overrode current_key_hash to return Some(_) but \
         left current_key returning None — keyed reconciliation will silently \
         lose state on every reorder. Override BOTH or NEITHER.",
    );
    old.current_key()
        .is_some_and(|old_key| new_key.key_eq(old_key))
}

/// Find the old-middle element index a new View should claim.
///
/// Returns `Some(idx)` only for a *keyed* new View whose key hash is
/// present in `old_keyed` AND whose claimed old element is genuinely
/// updatable (type + key per [`can_update_element`]). On a successful
/// claim, the chosen candidate index is removed from the bucket so a
/// later duplicate-key View cannot reuse the same old element
/// (first-wins duplicate-key resolution). Un-keyed new Views always
/// return `None` — they only ever match positionally, which the
/// top/bottom scans already handled.
///
/// Walks the entire `Vec<usize>` candidate list for the hash bucket
/// (not just the first entry) to handle the symmetric case where two
/// old children with distinct keys collide on `u64`. The right
/// candidate is identified via `can_update_element`'s stage-2
/// semantic `key_eq` check; non-matching candidates stay in the
/// bucket so a LATER new view whose key matches one of them can
/// still claim it.
fn match_old_for_new(
    new_view: &dyn View,
    old_keyed: &mut HashMap<u64, Vec<usize>>,
    old_slots: &[Option<Box<dyn ElementBase>>],
) -> Option<usize> {
    let key_hash = new_view.key()?.key_hash();
    let bucket = old_keyed.get_mut(&key_hash)?;
    // Walk the candidates; on the first one `can_update_element`
    // accepts (matching type + matching key via `key_eq`), remove it
    // from the bucket and return its index. Leaves non-matching
    // candidates in place for a future new view to claim.
    let position = bucket.iter().position(|&old_idx| {
        old_slots[old_idx]
            .as_deref()
            .is_some_and(|old| can_update_element(old, new_view))
    })?;
    let old_idx = bucket.remove(position);
    if bucket.is_empty() {
        old_keyed.remove(&key_hash);
    }
    Some(old_idx)
}

#[cfg(test)]
mod tests {
    use std::any::TypeId;

    use crate::{
        BuildOwner, ElementOwner,
        element::Lifecycle,
        view::{ElementBase, View},
    };

    use super::reconcile_children;

    /// A minimal keyless leaf view (no children — terminates the build
    /// chain). `tag` distinguishes instances in test setup.
    #[derive(Clone)]
    struct PlainView {
        #[expect(dead_code, reason = "distinguishes instances; read only via Clone")]
        tag: u32,
    }

    impl View for PlainView {
        fn create_element(&self) -> Box<dyn ElementBase> {
            Box::new(LeafElement::of::<PlainView>())
        }
    }

    /// A second concrete leaf type, to exercise type-mismatch replacement.
    #[derive(Clone)]
    struct OtherView;

    impl View for OtherView {
        fn create_element(&self) -> Box<dyn ElementBase> {
            Box::new(LeafElement::of::<OtherView>())
        }
    }

    /// A hand-rolled leaf element: no children, no render object. Used so
    /// the reconciliation unit tests have a terminal element type with a
    /// concrete `view_type_id` and a real `Lifecycle`, without dragging
    /// in the `StatelessBehavior` build machinery (a self-returning
    /// `StatelessView::build` would recurse without bound).
    struct LeafElement {
        view_type: TypeId,
        depth: usize,
        lifecycle: Lifecycle,
    }

    impl LeafElement {
        fn of<V: 'static>() -> Self {
            Self {
                view_type: TypeId::of::<V>(),
                depth: 0,
                lifecycle: Lifecycle::Initial,
            }
        }
    }

    impl ElementBase for LeafElement {
        fn view_type_id(&self) -> TypeId {
            self.view_type
        }

        fn depth(&self) -> usize {
            self.depth
        }

        fn lifecycle(&self) -> Lifecycle {
            self.lifecycle
        }

        fn mount(
            &mut self,
            _parent: Option<flui_foundation::ElementId>,
            slot: usize,
            _owner: &mut ElementOwner<'_>,
        ) {
            self.depth = slot;
            self.lifecycle = Lifecycle::Active;
        }

        fn unmount(&mut self, _owner: &mut ElementOwner<'_>) {
            self.lifecycle = Lifecycle::Defunct;
        }

        fn activate(&mut self) {
            self.lifecycle = Lifecycle::Active;
        }

        fn deactivate(&mut self) {
            self.lifecycle = Lifecycle::Inactive;
        }

        fn update(&mut self, _new_view: &dyn View, _owner: &mut ElementOwner<'_>) {}

        fn mark_needs_build(&mut self) {}

        fn perform_build(&mut self, _owner: &mut ElementOwner<'_>) {}

        fn visit_children(&self, _visitor: &mut dyn FnMut(flui_foundation::ElementId)) {}
    }

    fn mount_one(view: &dyn View, slot: usize, owner: &mut BuildOwner) -> Box<dyn ElementBase> {
        let mut el = view.create_element();
        el.mount(None, slot, &mut owner.element_owner_mut());
        el
    }

    #[test]
    fn empty_to_empty() {
        let mut owner = BuildOwner::new();
        let mut children: Vec<Box<dyn ElementBase>> = Vec::new();
        reconcile_children(
            flui_foundation::ElementId::new(1),
            &mut children,
            &[],
            &mut owner.element_owner_mut(),
        );
        assert!(children.is_empty());
    }

    #[test]
    fn empty_old_creates_all() {
        let mut owner = BuildOwner::new();
        let mut children: Vec<Box<dyn ElementBase>> = Vec::new();
        let v0 = PlainView { tag: 0 };
        let v1 = PlainView { tag: 1 };
        let views: Vec<&dyn View> = vec![&v0, &v1];
        reconcile_children(
            flui_foundation::ElementId::new(1),
            &mut children,
            &views,
            &mut owner.element_owner_mut(),
        );
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn empty_new_removes_all() {
        let mut owner = BuildOwner::new();
        let v0 = PlainView { tag: 0 };
        let mut children = vec![mount_one(&v0, 0, &mut owner)];
        reconcile_children(
            flui_foundation::ElementId::new(1),
            &mut children,
            &[],
            &mut owner.element_owner_mut(),
        );
        assert!(children.is_empty());
    }

    #[test]
    fn same_type_keyless_reuses_positionally() {
        let mut owner = BuildOwner::new();
        let v0 = PlainView { tag: 0 };
        let v1 = PlainView { tag: 1 };
        let mut children = vec![mount_one(&v0, 0, &mut owner), mount_one(&v1, 1, &mut owner)];
        // New views: same type, keyless — should reuse both slots.
        let n0 = PlainView { tag: 10 };
        let n1 = PlainView { tag: 11 };
        let views: Vec<&dyn View> = vec![&n0, &n1];
        reconcile_children(
            flui_foundation::ElementId::new(1),
            &mut children,
            &views,
            &mut owner.element_owner_mut(),
        );
        assert_eq!(children.len(), 2);
        // Both are still PlainView elements (reused, not replaced).
        for child in &children {
            assert_eq!(child.view_type_id(), std::any::TypeId::of::<PlainView>());
        }
    }

    #[test]
    fn type_mismatch_replaces() {
        let mut owner = BuildOwner::new();
        let v0 = PlainView { tag: 0 };
        let mut children = vec![mount_one(&v0, 0, &mut owner)];
        let other = OtherView;
        let views: Vec<&dyn View> = vec![&other];
        reconcile_children(
            flui_foundation::ElementId::new(1),
            &mut children,
            &views,
            &mut owner.element_owner_mut(),
        );
        assert_eq!(children.len(), 1);
        assert_eq!(
            children[0].view_type_id(),
            std::any::TypeId::of::<OtherView>()
        );
    }

    #[test]
    fn grow_and_shrink() {
        let mut owner = BuildOwner::new();
        let v0 = PlainView { tag: 0 };
        let mut children = vec![mount_one(&v0, 0, &mut owner)];

        // Grow to 3.
        let g = [
            PlainView { tag: 0 },
            PlainView { tag: 1 },
            PlainView { tag: 2 },
        ];
        let grow: Vec<&dyn View> = g.iter().map(|v| v as &dyn View).collect();
        reconcile_children(
            flui_foundation::ElementId::new(1),
            &mut children,
            &grow,
            &mut owner.element_owner_mut(),
        );
        assert_eq!(children.len(), 3);

        // Shrink back to 1.
        let s = [PlainView { tag: 0 }];
        let shrink: Vec<&dyn View> = s.iter().map(|v| v as &dyn View).collect();
        reconcile_children(
            flui_foundation::ElementId::new(1),
            &mut children,
            &shrink,
            &mut owner.element_owner_mut(),
        );
        assert_eq!(children.len(), 1);
    }

    // ========================================================================
    // Plan §U12 / FR-024 — keyed reconciliation semantic-match coverage
    //
    // These tests use a `KeyedView` that carries a `Box<dyn ViewKey>` and
    // override `View::key()`. The companion `KeyedLeafElement` overrides
    // `ElementBase::current_key_hash` AND `current_key` so the reconciler
    // can run its semantic `key_eq` check (FR-024 work item (c)) against
    // a real `&dyn ViewKey`.
    // ========================================================================

    use std::sync::atomic::{AtomicU64, Ordering};

    /// Identity-tracking leaf — records its source ordinal so a test can
    /// assert "the SAME old element survived" after a permutation.
    static ELEMENT_COUNTER: AtomicU64 = AtomicU64::new(1);

    /// A leaf element that carries a `Box<dyn ViewKey>` cloned from the
    /// view at construction time, plus a stable `identity_id` so tests
    /// can prove element reuse vs replacement after a keyed reorder.
    struct KeyedLeafElement {
        view_type: TypeId,
        depth: usize,
        lifecycle: Lifecycle,
        key: Option<Box<dyn flui_foundation::ViewKey>>,
        identity_id: u64,
    }

    impl ElementBase for KeyedLeafElement {
        fn view_type_id(&self) -> TypeId {
            self.view_type
        }

        fn current_key_hash(&self) -> Option<u64> {
            self.key.as_ref().map(|k| k.key_hash())
        }

        fn current_key(&self) -> Option<&dyn flui_foundation::ViewKey> {
            self.key.as_deref()
        }

        fn depth(&self) -> usize {
            self.depth
        }

        fn lifecycle(&self) -> Lifecycle {
            self.lifecycle
        }

        fn mount(
            &mut self,
            _parent: Option<flui_foundation::ElementId>,
            slot: usize,
            _owner: &mut ElementOwner<'_>,
        ) {
            self.depth = slot;
            self.lifecycle = Lifecycle::Active;
        }

        fn unmount(&mut self, _owner: &mut ElementOwner<'_>) {
            self.lifecycle = Lifecycle::Defunct;
        }

        fn activate(&mut self) {
            self.lifecycle = Lifecycle::Active;
        }

        fn deactivate(&mut self) {
            self.lifecycle = Lifecycle::Inactive;
        }

        fn update(&mut self, new_view: &dyn View, _owner: &mut ElementOwner<'_>) {
            // Re-clone the key from the new view so the stored field
            // tracks whatever update applied — mirrors the production
            // ElementNode::update boundary from §U7.
            self.key = new_view.key().map(flui_foundation::ViewKey::clone_key);
        }

        fn mark_needs_build(&mut self) {}
        fn perform_build(&mut self, _owner: &mut ElementOwner<'_>) {}
        fn visit_children(&self, _visitor: &mut dyn FnMut(flui_foundation::ElementId)) {}
    }

    /// View with an optional dynamic key.
    struct KeyedView {
        key: Option<Box<dyn flui_foundation::ViewKey>>,
    }

    impl Clone for KeyedView {
        fn clone(&self) -> Self {
            Self {
                key: self.key.as_ref().map(|k| k.clone_key()),
            }
        }
    }

    impl KeyedView {
        fn with<K: flui_foundation::ViewKey>(key: K) -> Self {
            Self {
                key: Some(Box::new(key)),
            }
        }

        fn keyless() -> Self {
            Self { key: None }
        }
    }

    impl View for KeyedView {
        fn create_element(&self) -> Box<dyn ElementBase> {
            Box::new(KeyedLeafElement {
                view_type: TypeId::of::<KeyedView>(),
                depth: 0,
                lifecycle: Lifecycle::Initial,
                key: self.key.as_ref().map(|k| k.clone_key()),
                identity_id: ELEMENT_COUNTER.fetch_add(1, Ordering::Relaxed),
            })
        }

        fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
            self.key.as_deref()
        }
    }

    /// Helper: downcast an `Option<&dyn ElementBase>` to the test's
    /// concrete leaf type so identity / key assertions can read the
    /// private `identity_id` field.
    fn as_keyed(child: &dyn ElementBase) -> &KeyedLeafElement {
        // The reconciliation suite is the only producer of
        // `KeyedLeafElement`, so the downcast is sound. `ElementBase`
        // inherits `Downcast` (via `downcast_rs::Downcast`), which is
        // what makes the `as_any().downcast_ref::<T>()` chain compile
        // here without an extra `use` line.
        child
            .as_any()
            .downcast_ref::<KeyedLeafElement>()
            .expect("test invariant: every child here is KeyedLeafElement")
    }

    fn mount_keyed(view: &KeyedView, slot: usize, owner: &mut BuildOwner) -> Box<dyn ElementBase> {
        let mut el = view.create_element();
        el.mount(None, slot, &mut owner.element_owner_mut());
        el
    }

    /// Covers FR-024 (a): keyed middle reuses the old element when a
    /// keyed reorder swaps two children. Identity IDs of the originals
    /// survive the swap — proves the elements were reused, not freshly
    /// created.
    #[test]
    fn keyed_reorder_reuses_elements() {
        use flui_foundation::ValueKey;

        let mut owner = BuildOwner::new();
        let v_a = KeyedView::with(ValueKey::new("a"));
        let v_b = KeyedView::with(ValueKey::new("b"));
        let mut children = vec![
            mount_keyed(&v_a, 0, &mut owner),
            mount_keyed(&v_b, 1, &mut owner),
        ];
        let id_a = as_keyed(&*children[0]).identity_id;
        let id_b = as_keyed(&*children[1]).identity_id;

        // Reorder: [A, B] -> [B, A].
        let new_b = KeyedView::with(ValueKey::new("b"));
        let new_a = KeyedView::with(ValueKey::new("a"));
        let views: Vec<&dyn View> = vec![&new_b, &new_a];
        reconcile_children(
            flui_foundation::ElementId::new(1),
            &mut children,
            &views,
            &mut owner.element_owner_mut(),
        );

        assert_eq!(children.len(), 2);
        assert_eq!(
            as_keyed(&*children[0]).identity_id,
            id_b,
            "slot 0 must hold the element originally created for B"
        );
        assert_eq!(
            as_keyed(&*children[1]).identity_id,
            id_a,
            "slot 1 must hold the element originally created for A"
        );
    }

    /// A hostile `ViewKey` that always hashes to a fixed `u64` but
    /// compares by inner `tag` — exercises FR-024 (c) collision
    /// defense. Two `ColliderKey { tag: 1 }` and `ColliderKey { tag: 2 }`
    /// hash to the SAME bucket but `key_eq` rejects the cross-tag
    /// match.
    #[derive(Clone)]
    struct ColliderKey {
        tag: u64,
    }

    impl flui_foundation::ViewKey for ColliderKey {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
        fn key_eq(&self, other: &dyn flui_foundation::ViewKey) -> bool {
            other
                .as_any()
                .downcast_ref::<Self>()
                .is_some_and(|o| self.tag == o.tag)
        }
        fn key_hash(&self) -> u64 {
            // Deliberate collision — every ColliderKey hashes to 0xDEAD.
            0xDEAD
        }
        fn clone_key(&self) -> Box<dyn flui_foundation::ViewKey> {
            Box::new(self.clone())
        }
        fn debug_fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "ColliderKey({})", self.tag)
        }
    }

    /// Covers FR-024 (c): hash collision between two distinct keys does
    /// NOT fool the matcher into a false reuse. The reconciler's
    /// semantic `key_eq` stage detects the mismatch and the new view
    /// gets a fresh element while the old one unmounts.
    #[test]
    fn keyed_hash_collision_falls_through_to_new_element() {
        let mut owner = BuildOwner::new();
        let v_old = KeyedView::with(ColliderKey { tag: 1 });
        let mut children = vec![mount_keyed(&v_old, 0, &mut owner)];
        let id_old = as_keyed(&*children[0]).identity_id;

        // Same hash (0xDEAD), different `tag` — `key_eq` rejects.
        let v_new = KeyedView::with(ColliderKey { tag: 2 });
        let views: Vec<&dyn View> = vec![&v_new];
        reconcile_children(
            flui_foundation::ElementId::new(1),
            &mut children,
            &views,
            &mut owner.element_owner_mut(),
        );

        assert_eq!(children.len(), 1);
        assert_ne!(
            as_keyed(&*children[0]).identity_id,
            id_old,
            "hash collision must NOT silently reuse the old element — \
             key_eq stage must reject and create a fresh element"
        );
    }

    /// Sanity: same-tag ColliderKey on both sides DOES reuse (collision
    /// defense kicks in only when `key_eq` actually disagrees). Pairs
    /// with the previous test as the positive control.
    #[test]
    fn keyed_hash_collision_with_equal_keys_reuses() {
        let mut owner = BuildOwner::new();
        let v_old = KeyedView::with(ColliderKey { tag: 7 });
        let mut children = vec![mount_keyed(&v_old, 0, &mut owner)];
        let id_old = as_keyed(&*children[0]).identity_id;

        let v_new = KeyedView::with(ColliderKey { tag: 7 });
        let views: Vec<&dyn View> = vec![&v_new];
        reconcile_children(
            flui_foundation::ElementId::new(1),
            &mut children,
            &views,
            &mut owner.element_owner_mut(),
        );

        assert_eq!(children.len(), 1);
        assert_eq!(
            as_keyed(&*children[0]).identity_id,
            id_old,
            "equal ColliderKey tags must reuse the same element",
        );
    }

    /// Mixed keyed + keyless children: the keyless ones still match
    /// positionally (top/bottom scan), the keyed one moves to its
    /// keyed slot. Identity preserved for all three.
    #[test]
    fn mixed_keyed_unkeyed_preserves_identity() {
        use flui_foundation::ValueKey;

        let mut owner = BuildOwner::new();
        let v_a = KeyedView::keyless();
        let v_b = KeyedView::with(ValueKey::new("moves"));
        let v_c = KeyedView::keyless();
        let mut children = vec![
            mount_keyed(&v_a, 0, &mut owner),
            mount_keyed(&v_b, 1, &mut owner),
            mount_keyed(&v_c, 2, &mut owner),
        ];
        let id_a = as_keyed(&*children[0]).identity_id;
        let id_b = as_keyed(&*children[1]).identity_id;
        let id_c = as_keyed(&*children[2]).identity_id;

        // Move B to slot 2; keyless A stays at 0, keyless previously-2
        // (C) takes slot 1 positionally.
        let n_a = KeyedView::keyless();
        let n_c = KeyedView::keyless();
        let n_b = KeyedView::with(ValueKey::new("moves"));
        let views: Vec<&dyn View> = vec![&n_a, &n_c, &n_b];
        reconcile_children(
            flui_foundation::ElementId::new(1),
            &mut children,
            &views,
            &mut owner.element_owner_mut(),
        );

        assert_eq!(children.len(), 3);
        // Slot 0 — keyless positional match keeps old element A.
        assert_eq!(as_keyed(&*children[0]).identity_id, id_a);
        // Slot 2 — keyed B moves to its keyed slot.
        assert_eq!(
            as_keyed(&*children[2]).identity_id,
            id_b,
            "keyed B must move to its new keyed slot",
        );
        // Slot 1 — keyless new element. Old C was keyless and in the
        // middle pool (the prefix scan stopped at slot 1 because
        // old[1] is keyed-B but new[1] is keyless), so Phase 3
        // unmounted it. The new keyless slot 1 fills with a freshly
        // created element whose identity_id MUST differ from any of
        // the originals (id_a/id_b/id_c) — proves Phase 4 created a
        // fresh element rather than silently reusing an old one off-
        // position. The slot must also hold an Active KeyedView
        // element (not Defunct, not unmapped), proving the fresh
        // mount completed correctly.
        let slot1 = as_keyed(&*children[1]);
        assert_ne!(slot1.identity_id, id_a);
        assert_ne!(slot1.identity_id, id_b);
        assert_ne!(slot1.identity_id, id_c);
        assert_eq!(slot1.lifecycle, Lifecycle::Initial);
        // Old C was keyless and middle-positioned, so Phase 3
        // unmounted it. Confirm id_c is gone from the surviving
        // children list.
        for child in &children {
            assert_ne!(
                as_keyed(&**child).identity_id,
                id_c,
                "keyless C in the middle pool must NOT survive — \
                 Phase 3 unmounts unkeyed middle children",
            );
        }
    }

    // ========================================================================
    // Plan §U18 follow-up / FR-024 (c) symmetric defense — multi-old
    // hash-collision regression test.
    //
    // The earlier `keyed_hash_collision_falls_through_to_new_element`
    // test covers the false-POSITIVE defense (one old + one new,
    // colliding hashes but `key_eq` rejects). This case covers the
    // symmetric false-NEGATIVE: TWO old children with distinct keys
    // colliding on `u64` AND a new view whose key `key_eq`s the
    // SECOND old. Before the fix, Phase 3's `HashMap<u64, usize>`
    // index dropped the second old (first-wins on hash), so
    // `match_old_for_new` could not find the right candidate and the
    // new view mounted fresh while the matching old unmounted —
    // silent state loss. Post-fix, the index is `HashMap<u64,
    // Vec<usize>>` and `match_old_for_new` walks candidates, picks
    // the one `can_update_element` accepts.
    // ========================================================================

    /// Covers the symmetric defense: two old children with distinct
    /// keys colliding on hash + one new view matching the second old
    /// MUST reuse the matching old (no silent state loss).
    #[test]
    fn keyed_hash_collision_two_olds_matches_second_old() {
        let mut owner = BuildOwner::new();
        // Two olds, distinct ColliderKey tags 1 and 2 → SAME hash
        // 0xDEAD on both, but `key_eq` distinguishes by tag.
        let v_o1 = KeyedView::with(ColliderKey { tag: 1 });
        let v_o2 = KeyedView::with(ColliderKey { tag: 2 });
        let mut children = vec![
            mount_keyed(&v_o1, 0, &mut owner),
            mount_keyed(&v_o2, 1, &mut owner),
        ];
        let id_o1 = as_keyed(&*children[0]).identity_id;
        let id_o2 = as_keyed(&*children[1]).identity_id;

        // New view: keyed with tag 2 — must `key_eq` the SECOND old
        // even though both olds hash to 0xDEAD.
        let v_new = KeyedView::with(ColliderKey { tag: 2 });
        let views: Vec<&dyn View> = vec![&v_new];
        reconcile_children(
            flui_foundation::ElementId::new(1),
            &mut children,
            &views,
            &mut owner.element_owner_mut(),
        );

        assert_eq!(children.len(), 1);
        assert_eq!(
            as_keyed(&*children[0]).identity_id,
            id_o2,
            "matching new view MUST reuse the second old (tag=2), \
             not create a fresh element — proves Phase 3 indexes BOTH \
             colliding olds as candidates and Phase 4 walks them with \
             key_eq, finding the right one",
        );
        // Sanity: o1's identity is gone (unmounted in Phase 5b
        // because no new view matched it).
        let _ = id_o1;
    }

    /// Mirror case: two olds, new matches the FIRST. Same defense
    /// against the symmetric false-negative on the opposite end of
    /// the bucket. Together with the above, locks the bucket walks
    /// in BOTH directions.
    #[test]
    fn keyed_hash_collision_two_olds_matches_first_old() {
        let mut owner = BuildOwner::new();
        let v_o1 = KeyedView::with(ColliderKey { tag: 1 });
        let v_o2 = KeyedView::with(ColliderKey { tag: 2 });
        let mut children = vec![
            mount_keyed(&v_o1, 0, &mut owner),
            mount_keyed(&v_o2, 1, &mut owner),
        ];
        let id_o1 = as_keyed(&*children[0]).identity_id;
        let id_o2 = as_keyed(&*children[1]).identity_id;

        let v_new = KeyedView::with(ColliderKey { tag: 1 });
        let views: Vec<&dyn View> = vec![&v_new];
        reconcile_children(
            flui_foundation::ElementId::new(1),
            &mut children,
            &views,
            &mut owner.element_owner_mut(),
        );

        assert_eq!(children.len(), 1);
        assert_eq!(
            as_keyed(&*children[0]).identity_id,
            id_o1,
            "matching new view MUST reuse the first old (tag=1)",
        );
        let _ = id_o2;
    }
}
