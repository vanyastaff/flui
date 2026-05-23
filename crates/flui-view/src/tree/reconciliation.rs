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
pub fn reconcile_children(
    old_children: &mut Vec<Box<dyn ElementBase>>,
    new_views: &[&dyn View],
    owner: &mut crate::ElementOwner<'_>,
) {
    // Fast path: nothing on either side.
    if old_children.is_empty() && new_views.is_empty() {
        return;
    }

    // Fast path: all new — create every element, no matching needed.
    if old_children.is_empty() {
        *old_children = new_views.iter().map(|v| v.create_element()).collect();
        return;
    }

    // Fast path: all removed — unmount every old child.
    if new_views.is_empty() {
        for mut child in old_children.drain(..) {
            child.unmount(owner);
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
        result.push(element);
        old_top += 1;
        new_top += 1;
    }

    // ------------------------------------------------------------------
    // Phase 2 — scan the bottom of both lists while nodes match.
    //
    // Matches are *recorded*, not synced — Flutter syncs them in phase 5
    // so every `update` runs strictly front-to-back.
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
    // they are unmounted here. Keyed ones go into `old_keyed` for
    // phase 4 to claim. First-wins on duplicate old keys (an old
    // duplicate is itself a prior-frame bug; we keep the first).
    // ------------------------------------------------------------------
    let mut old_keyed: HashMap<u64, usize> = HashMap::new();
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
            // Keyed — phase 4 may claim it. First-wins on a duplicate
            // old key (a duplicate is itself a prior-frame bug).
            old_keyed.entry(key).or_insert(idx);
        } else {
            // Un-keyed middle child with no positional match — drop it.
            let mut child = slot.take().expect("iter yielded Some");
            child.unmount(owner);
        }
    }

    // ------------------------------------------------------------------
    // Phase 4 — sync the new middle front-to-back.
    //
    // A keyed new View claims its old match from `old_keyed` (removing
    // the entry so a later duplicate key cannot reuse it — first-wins).
    // Everything else gets a fresh element.
    // ------------------------------------------------------------------
    for &new_view in &new_views[new_top..new_bottom] {
        if let Some(old_idx) = match_old_for_new(new_view, &mut old_keyed, &old_slots) {
            let mut element = old_slots[old_idx]
                .take()
                .expect("old_keyed only indexes Some entries");
            element.update(new_view, owner);
            result.push(element);
        } else {
            // No keyed match — fresh element, left unmounted for the
            // caller to mount (see module "Lifecycle boundary").
            result.push(new_view.create_element());
        }
    }

    // ------------------------------------------------------------------
    // Phase 5a — sync the bottom matches recorded in phase 2.
    // ------------------------------------------------------------------
    for offset in 0..(old_len - old_bottom) {
        let old_idx = old_bottom + offset;
        let new_idx = new_bottom + offset;
        let mut element = old_slots[old_idx]
            .take()
            .expect("phase-2 bottom match guaranteed Some");
        element.update(new_views[new_idx], owner);
        result.push(element);
    }

    // ------------------------------------------------------------------
    // Phase 5b — unmount any keyed old children never claimed.
    // ------------------------------------------------------------------
    for slot in &mut old_slots {
        if let Some(mut child) = slot.take() {
            child.unmount(owner);
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
/// `new` (a new child View) — same concrete View type AND matching key.
///
/// Key comparison is hash-based: `ElementBase` erases the concrete
/// `View`, so an old element only exposes its key as a `u64` hash. Two
/// children match when both are keyless, or both carry keys whose hashes
/// are equal. This mirrors [`View::can_update`] (which compares keys
/// proper) at the type-erased element boundary — see the module docs on
/// hash-based matching.
fn can_update_element(old: &dyn ElementBase, new: &dyn View) -> bool {
    if old.view_type_id() != new.view_type_id() {
        return false;
    }
    old.current_key_hash() == new.key().map(flui_foundation::ViewKey::key_hash)
}

/// Find the old-middle element index a new View should claim.
///
/// Returns `Some(idx)` only for a *keyed* new View whose key hash is
/// present in `old_keyed` AND whose claimed old element is genuinely
/// updatable (type + key). The entry is removed from `old_keyed` on a
/// successful claim so a later duplicate-key View cannot reuse the same
/// old element (first-wins duplicate-key resolution). Un-keyed new Views
/// always return `None` — they only ever match positionally, which the
/// top/bottom scans already handled.
fn match_old_for_new(
    new_view: &dyn View,
    old_keyed: &mut HashMap<u64, usize>,
    old_slots: &[Option<Box<dyn ElementBase>>],
) -> Option<usize> {
    let key_hash = new_view.key()?.key_hash();
    let &old_idx = old_keyed.get(&key_hash)?;
    // Defensive: the hash matched, but confirm the old element is really
    // updatable (guards against a hash collision across View types).
    let updatable = old_slots[old_idx]
        .as_deref()
        .is_some_and(|old| can_update_element(old, new_view));
    if updatable {
        old_keyed.remove(&key_hash);
        Some(old_idx)
    } else {
        None
    }
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
        reconcile_children(&mut children, &[], &mut owner.element_owner_mut());
        assert!(children.is_empty());
    }

    #[test]
    fn empty_old_creates_all() {
        let mut owner = BuildOwner::new();
        let mut children: Vec<Box<dyn ElementBase>> = Vec::new();
        let v0 = PlainView { tag: 0 };
        let v1 = PlainView { tag: 1 };
        let views: Vec<&dyn View> = vec![&v0, &v1];
        reconcile_children(&mut children, &views, &mut owner.element_owner_mut());
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn empty_new_removes_all() {
        let mut owner = BuildOwner::new();
        let v0 = PlainView { tag: 0 };
        let mut children = vec![mount_one(&v0, 0, &mut owner)];
        reconcile_children(&mut children, &[], &mut owner.element_owner_mut());
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
        reconcile_children(&mut children, &views, &mut owner.element_owner_mut());
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
        reconcile_children(&mut children, &views, &mut owner.element_owner_mut());
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
        reconcile_children(&mut children, &grow, &mut owner.element_owner_mut());
        assert_eq!(children.len(), 3);

        // Shrink back to 1.
        let s = [PlainView { tag: 0 }];
        let shrink: Vec<&dyn View> = s.iter().map(|v| v as &dyn View).collect();
        reconcile_children(&mut children, &shrink, &mut owner.element_owner_mut());
        assert_eq!(children.len(), 1);
    }
}
