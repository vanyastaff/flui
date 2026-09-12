use std::{cell::Cell, rc::Rc};

use super::*;
use crate::view::{IntoView, View, ViewExt};
use crate::{
    BuildContext, BuildOwner, ErrorView, GlobalKey, LifecycleHook, RecoveredAt, RenderView,
    StatelessView,
};
use flui_foundation::{ValueKey, ViewKey};
use flui_objects::RenderSizedBox;
use flui_rendering::{
    RenderUpdateImpact,
    pipeline::{PipelineCell, PipelineOwner},
    protocol::BoxProtocol,
};

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
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::stateless(self)
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
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::stateless(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

/// A hostile key that hashes EVERY instance to the same `u64` but
/// compares by inner `tag` — exercises the production collision
/// defense: two distinct `ColliderKey`s land in one hash bucket, and
/// only the semantic `key_eq` (consulted on a hash hit) tells them
/// apart.
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
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::stateless(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

#[derive(Clone)]
struct RenderTestParent;

impl RenderView for RenderTestParent {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(&self, _ctx: &crate::RenderObjectContext<'_>) -> Self::RenderObject {
        RenderSizedBox::shrink()
    }

    fn update_render_object(
        &self,
        _ctx: &crate::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> RenderUpdateImpact {
        RenderUpdateImpact::NONE
    }
}

impl View for RenderTestParent {
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::render_variable(self)
    }
}

#[derive(Clone)]
struct PanickingGlobalRenderView {
    key: GlobalKey<Self>,
    is_armed: Rc<Cell<bool>>,
}

impl RenderView for PanickingGlobalRenderView {
    type Protocol = BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(&self, _ctx: &crate::RenderObjectContext<'_>) -> Self::RenderObject {
        RenderSizedBox::shrink()
    }

    fn update_render_object(
        &self,
        _ctx: &crate::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> RenderUpdateImpact {
        assert!(!self.is_armed.get(), "injected keyed update panic");
        RenderUpdateImpact::NONE
    }
}

impl View for PanickingGlobalRenderView {
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::render_variable(self)
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

#[test]
fn failed_global_key_update_leaves_no_reservation_for_finalized_resident() {
    let pipeline = PipelineCell::new(PipelineOwner::new());
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let root = tree.mount_root_with_pipeline_owner(
        &RenderTestParent,
        Some(pipeline),
        &mut owner.element_owner_mut(),
    );
    let key = GlobalKey::<PanickingGlobalRenderView>::new();
    let is_armed = Rc::new(Cell::new(false));
    let resident_view = PanickingGlobalRenderView {
        key: key.clone(),
        is_armed: is_armed.clone(),
    };
    let initial: Vec<Box<dyn View>> = vec![Box::new(resident_view.clone())];
    reconcile_children_by_id(&mut tree, root, &initial, &mut owner.element_owner_mut());
    let resident = tree.get(root).expect("render parent resolves").child_ids()[0];
    assert_eq!(owner.element_for_global_key(&key), Some(resident));
    assert!(
        !owner.global_key_reservations.is_empty(),
        "the fresh keyed mount must prove the ledger fixture is live"
    );
    owner.global_key_reservations.clear();
    assert!(owner.global_key_reservations.is_empty());
    is_armed.set(true);

    let update: Vec<Box<dyn View>> = vec![Box::new(resident_view)];
    reconcile_children_by_id(&mut tree, root, &update, &mut owner.element_owner_mut());

    let substitute = tree
        .get(root)
        .expect("render parent remains live")
        .child_ids()[0];
    assert_ne!(substitute, resident);
    assert!(
        tree.get(resident).is_none(),
        "the failed resident is finalized"
    );
    assert_eq!(
        tree.get(substitute)
            .expect("the recovery substitute resolves")
            .element()
            .view_type_id(),
        std::any::TypeId::of::<ErrorView>()
    );
    assert_eq!(owner.element_for_global_key(&key), None);
    assert!(
        owner.global_key_reservations.is_empty(),
        "a substitute result must not reserve the finalized resident before frame verification"
    );

    let mut recovered = owner.take_recovered_panics();
    assert_eq!(recovered.len(), 1);
    let panic = recovered.remove(0);
    assert_eq!(panic.hook, LifecycleHook::Update);
    assert!(matches!(
        panic.at,
        RecoveredAt::Substituted {
            element: Some(recorded_resident),
            substitute: recorded_substitute,
            parent: recorded_parent,
            slot: 0,
        } if recorded_resident == resident
            && recorded_substitute == substitute
            && recorded_parent == root
    ));
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

/// Deep-tree stack-safety: the eager removal path's subtree
/// collection must survive an element chain far deeper than the
/// fixed OS stack would allow with plain recursion. The element tree
/// nests several times deeper than the render tree (every render
/// object is wrapped in multiple composition views), so it hits the
/// 1 MiB Windows main-thread stack earlier — same failure class
/// PR #177 closed in flui-rendering. The collection frame is small,
/// so the depth is 20 000 (the small-frame sizing the
/// flui-rendering compositing-bits test established; 2 500 survived
/// unprotected there by luck).
///
/// The chain is built one generation at a time through the
/// production reconciler (it inserts a parent's DIRECT children
/// only), then torn down by reconciling the root to zero children —
/// the keyless top takes the eager path, which collects and frees
/// the whole 20 000-node subtree in one call.
///
/// Ignored under miri: the interpreter cannot finish a 20 000-level
/// walk in reasonable time; `eager_remove_tears_down_whole_subtree`
/// exercises the same path natively at shallow depth.
#[test]
#[cfg_attr(miri, ignore = "20k-node walk too slow for the interpreter")]
fn eager_remove_survives_deep_chain() {
    const DEPTH: usize = 20_000;

    let (mut tree, mut owner, root) = fixture();

    let mut parent = root;
    for _ in 0..DEPTH {
        reconcile_children_by_id(
            &mut tree,
            parent,
            &plain_views(&[1]),
            &mut owner.element_owner_mut(),
        );
        parent = tree.get(parent).expect("parent resolves").child_ids()[0];
    }
    assert_eq!(tree.len(), DEPTH + 1, "root + 20 000 chain nodes");

    // Drop the chain top from the root's children → the eager
    // removal must collect and free all 20 000 descendants without
    // exhausting the stack.
    reconcile_children_by_id(&mut tree, root, &[], &mut owner.element_owner_mut());

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

/// Duplicate keys in the NEW list: the first occurrence claims the
/// matching old element, every later duplicate mints a fresh one
/// (first-wins), and the call never panics. Ports the unique
/// error-path case from the retired box-reconciler corpus onto the
/// slab.
#[test]
fn duplicate_keys_in_new_list_first_wins() {
    let (mut tree, mut owner, root) = fixture();
    reconcile_children_by_id(
        &mut tree,
        root,
        &keyed_views(&[1, 2]),
        &mut owner.element_owner_mut(),
    );
    let before = tree.get(root).unwrap().child_ids().to_vec();
    let (id1, id2) = (before[0], before[1]);

    // New list repeats key 1. Defined behavior: the first key=1 reuses
    // the original element; the second key=1 is a fresh element; key=2
    // is reused.
    reconcile_children_by_id(
        &mut tree,
        root,
        &keyed_views(&[1, 1, 2]),
        &mut owner.element_owner_mut(),
    );
    let after = tree.get(root).unwrap().child_ids().to_vec();

    assert_eq!(after.len(), 3);
    assert_eq!(after[0], id1, "first key=1 reuses the original element");
    assert_ne!(
        after[1], id1,
        "second key=1 must be a fresh element (first-wins)"
    );
    assert_ne!(after[1], id2, "second key=1 is not the key=2 element");
    assert_eq!(after[2], id2, "key=2 is reused");
    for id in &after {
        assert!(tree.get(*id).is_some(), "every surviving child id resolves");
    }
}

/// `flui::reconcile` emission coverage on the LIVE slab path
/// (catalog #3). The production reconciler
/// emits one typed [`ReconcileEvent`](super::ReconcileEvent) per
/// child disposition so devtools / selection-persistence subscribers
/// reconstruct each frame's outcome WITHOUT a tree diff. Before this
/// wiring `reconcile_children_by_id` emitted ZERO events, so every
/// test here fails its multiset assertion (a real red→green guard,
/// not a tautology).
///
/// Per the collector's process-global tracing-callsite caveat, these
/// install a per-thread dispatcher and are `#[serial]`-gated so a
/// concurrent dispatcher swap cannot make a freshly installed
/// collector miss events.
mod emission {
    use flui_foundation::ElementId;
    use serial_test::serial;
    use tracing::dispatcher::Dispatch;
    use tracing_subscriber::Registry;
    use tracing_subscriber::layer::SubscriberExt;

    use super::super::reconcile_children_by_id;
    use super::{KeyedView, fixture, keyed_views, plain_views};
    use std::sync::OnceLock;

    use crate::BuildOwner;
    use crate::tree::ElementTree;
    use crate::tree::ReconcileEventKind;
    use crate::tree::test_utils::{CollectedEvent, ReconcileEventCollector};
    use crate::view::View;

    /// Process-global guard so the keep-alive subscriber installs once.
    static GLOBAL_SUBSCRIBER: OnceLock<()> = OnceLock::new();

    /// Install an *interested* process-global default subscriber ONCE
    /// for this test binary.
    ///
    /// The `flui::reconcile` callsite is shared by every reconcile,
    /// including the many `ElementTree::insert` calls (an emit site) in
    /// non-collector tests that run in PARALLEL. If one hits the callsite
    /// while the only default is the no-op global, tracing caches
    /// `Interest::never` and the callsite goes dead — later per-thread
    /// collectors are then bypassed (tests pass in isolation but fail in
    /// the full suite). `tracing::callsite::rebuild_interest_cache()` is
    /// NOT sufficient: it re-evaluates against the GLOBAL default (still
    /// the no-op), so under the suite's parallel emit pressure the
    /// callsite re-poisons. Installing an interested global default (a
    /// bare `Registry`, no layer → records nothing) keeps the callsite
    /// permanently armed; per-event dispatch still routes to the CURRENT
    /// thread's `with_default` collector, so each test's events stay
    /// isolated. Confined to this test binary (a separate process from
    /// every `tests/*.rs`), and nothing else here installs a global
    /// default, so it cannot interfere.
    fn ensure_global_subscriber() {
        GLOBAL_SUBSCRIBER.get_or_init(|| {
            // Ignore Err: only need *an* interested global default present.
            let _ = tracing::subscriber::set_global_default(Registry::default());
        });
    }

    /// Capture the `flui::reconcile` events `body` emits on this thread.
    fn capture<F: FnOnce()>(body: F) -> Vec<CollectedEvent> {
        ensure_global_subscriber();
        let collector = ReconcileEventCollector::new();
        let subscriber = Registry::default().with(collector.layer());
        // Disarm `tracing`'s process-global callsite-interest cache first: it is
        // computed on whichever thread reaches a callsite FIRST, so without this a
        // sibling test can have it cached as `never` and silently empty this capture.
        // See `flui_testing::log_capture`.
        flui_testing::log_capture::disarm_interest_cache();
        tracing::dispatcher::with_default(&Dispatch::new(subscriber), body);
        collector.events()
    }

    /// Assert the captured events carry exactly the expected
    /// `(kind, slot)` dispositions as a MULTISET. Both sides are
    /// sorted before comparison so a test does not depend on the
    /// HashMap-iteration order of the keyed-middle phase's multiset
    /// contract — `expected` is written in natural emission order
    /// at the call site.
    fn assert_dispositions(events: &[CollectedEvent], expected: &[(ReconcileEventKind, u64)]) {
        let sort_key = |(kind, slot): &(ReconcileEventKind, u64)| (*kind as u8, *slot);
        let mut actual: Vec<(ReconcileEventKind, u64)> =
            events.iter().map(|e| (e.kind, e.slot)).collect();
        actual.sort_by_key(sort_key);
        let mut want = expected.to_vec();
        want.sort_by_key(sort_key);
        assert_eq!(
            actual, want,
            "reconcile disposition multiset mismatch\n  expected: {want:?}\n  actual:   {actual:?}\n  full events: {events:?}",
        );
    }

    /// Seed `parent` with `views` via direct slab inserts, bypassing
    /// the reconciler so NO `flui::reconcile` event fires during
    /// setup. This matters: tracing's callsite-interest cache is
    /// process-global, so if the production emit callsite is first
    /// exercised OUTSIDE a collector scope it can latch "no interest"
    /// and the first captured reconcile then observes zero events.
    /// Building the prior state with raw inserts keeps every emit
    /// inside a `capture` — the same discipline the reconciler test corpus uses
    /// (it mounts its initial tree directly, never via a warmup
    /// reconcile).
    fn seed(
        tree: &mut ElementTree,
        owner: &mut BuildOwner,
        parent: ElementId,
        views: &[Box<dyn View>],
    ) {
        let mut ids = Vec::with_capacity(views.len());
        for (slot, view) in views.iter().enumerate() {
            ids.push(tree.insert(view.as_ref(), parent, slot, &mut owner.element_owner_mut()));
        }
        tree.get_mut(parent)
            .expect("seeded parent resolves")
            .set_child_ids(ids);
    }

    /// An empty parent gaining N children emits one `Mount` per slot,
    /// each carrying the reconciled parent id.
    #[test]
    #[serial]
    fn emits_mount_for_each_inserted_child() {
        let (mut tree, mut owner, root) = fixture();
        let views = keyed_views(&[1, 2, 3]);
        let events = capture(|| {
            reconcile_children_by_id(&mut tree, root, &views, &mut owner.element_owner_mut());
        });

        assert_dispositions(
            &events,
            &[
                (ReconcileEventKind::Mount, 0),
                (ReconcileEventKind::Mount, 1),
                (ReconcileEventKind::Mount, 2),
            ],
        );
        for event in &events {
            assert_eq!(
                event.parent,
                root.as_u64(),
                "every event must carry the reconciled parent id; got {event:?}",
            );
        }
    }

    /// Re-reconciling the same shape reuses every child in place →
    /// one `Reuse` per slot, no `Mount`/`Unmount`.
    #[test]
    #[serial]
    fn emits_reuse_for_unchanged_children() {
        let (mut tree, mut owner, root) = fixture();
        seed(&mut tree, &mut owner, root, &keyed_views(&[1, 2, 3]));

        let events = capture(|| {
            reconcile_children_by_id(
                &mut tree,
                root,
                &keyed_views(&[1, 2, 3]),
                &mut owner.element_owner_mut(),
            );
        });

        assert_dispositions(
            &events,
            &[
                (ReconcileEventKind::Reuse, 0),
                (ReconcileEventKind::Reuse, 1),
                (ReconcileEventKind::Reuse, 2),
            ],
        );
    }

    /// A keyed reorder keeps the prefix match in place (`Reuse`) and
    /// moves the rest (`Reorder`) — the element follows its key, so
    /// the disposition reflects real movement.
    #[test]
    #[serial]
    fn emits_reuse_and_reorder_on_keyed_move() {
        let (mut tree, mut owner, root) = fixture();
        seed(&mut tree, &mut owner, root, &keyed_views(&[1, 2, 3]));

        // [1,2,3] -> [1,3,2]: key 1 stays (Reuse@0); keys 3 and 2 are
        // pulled to new slots by the keyed-middle walk (Reorder@1,@2).
        let events = capture(|| {
            reconcile_children_by_id(
                &mut tree,
                root,
                &keyed_views(&[1, 3, 2]),
                &mut owner.element_owner_mut(),
            );
        });

        assert_dispositions(
            &events,
            &[
                (ReconcileEventKind::Reuse, 0),
                (ReconcileEventKind::Reorder, 1),
                (ReconcileEventKind::Reorder, 2),
            ],
        );
    }

    /// Dropping the last keyed child reuses the survivors and emits a
    /// single `Unmount` at the dropped child's old slot.
    #[test]
    #[serial]
    fn emits_unmount_for_dropped_child() {
        let (mut tree, mut owner, root) = fixture();
        seed(&mut tree, &mut owner, root, &keyed_views(&[1, 2, 3]));

        let events = capture(|| {
            reconcile_children_by_id(
                &mut tree,
                root,
                &keyed_views(&[1, 2]),
                &mut owner.element_owner_mut(),
            );
        });

        assert_dispositions(
            &events,
            &[
                (ReconcileEventKind::Reuse, 0),
                (ReconcileEventKind::Reuse, 1),
                (ReconcileEventKind::Unmount, 2),
            ],
        );
    }

    /// A view-type change at a slot replaces the element: the old
    /// (keyless) child unmounts and a fresh one mounts, both at the
    /// same slot but distinct dispositions.
    #[test]
    #[serial]
    fn emits_unmount_then_mount_on_type_change() {
        let (mut tree, mut owner, root) = fixture();
        seed(&mut tree, &mut owner, root, &plain_views(&[1]));

        // A keyless `TestView` slot replaced by a keyed `KeyedView`:
        // different concrete types, so no reuse.
        let new_views: Vec<Box<dyn View>> = vec![Box::new(KeyedView::new(9))];
        let events = capture(|| {
            reconcile_children_by_id(&mut tree, root, &new_views, &mut owner.element_owner_mut());
        });

        assert_dispositions(
            &events,
            &[
                (ReconcileEventKind::Unmount, 0),
                (ReconcileEventKind::Mount, 0),
            ],
        );
    }

    /// The S_3 permutation corpus (FR-024(b)) on the slab:
    /// for each of the 6 permutations of keyed `[1, 2, 3]`, the
    /// disposition multiset matches the keyed-reconcile contract AND
    /// every key's element moves to its permuted slot (never rebuilt).
    /// Ports the retired box-reconciler's exhaustive permutation
    /// corpus onto the production reconciler — multiset equality
    /// because Phase-4 HashMap iteration order is not stable.
    #[test]
    #[serial]
    fn all_six_permutations_preserve_identity_and_emit_expected() {
        use ReconcileEventKind::{Reorder, Reuse};

        // Element type inferred from the first tuple's suffixes
        // (`u32` keys, `u64` slots) — an explicit annotation would
        // trip clippy::type_complexity for no readability gain.
        let cases = [
            ([1u32, 2, 3], [(Reuse, 0u64), (Reuse, 1), (Reuse, 2)]),
            ([1, 3, 2], [(Reuse, 0), (Reorder, 1), (Reorder, 2)]),
            ([2, 1, 3], [(Reorder, 0), (Reorder, 1), (Reuse, 2)]),
            ([2, 3, 1], [(Reorder, 0), (Reorder, 1), (Reorder, 2)]),
            ([3, 1, 2], [(Reorder, 0), (Reorder, 1), (Reorder, 2)]),
            ([3, 2, 1], [(Reorder, 0), (Reuse, 1), (Reorder, 2)]),
        ];

        for (perm, expected) in cases {
            let (mut tree, mut owner, root) = fixture();
            seed(&mut tree, &mut owner, root, &keyed_views(&[1, 2, 3]));
            let before = tree.get(root).unwrap().child_ids().to_vec();
            // Seed order is key-ascending, so key `k` lives at index `k - 1`.
            let id_of = |k: u32| before[(k - 1) as usize];

            let new_views = keyed_views(&perm);
            let events = capture(|| {
                reconcile_children_by_id(
                    &mut tree,
                    root,
                    &new_views,
                    &mut owner.element_owner_mut(),
                );
            });

            assert_dispositions(&events, &expected);

            let after = tree.get(root).unwrap().child_ids().to_vec();
            for (slot, &key) in perm.iter().enumerate() {
                assert_eq!(
                    after[slot],
                    id_of(key),
                    "perm {perm:?}: slot {slot} must hold the original key={key} element, not a rebuild",
                );
            }
        }
    }
}
