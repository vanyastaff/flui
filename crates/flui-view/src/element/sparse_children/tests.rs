use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use flui_foundation::ViewKey;
use flui_objects::RenderSizedBox;
use flui_rendering::parent_data::SliverMultiBoxAdaptorParentData;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_rendering::prelude::{BoxLayoutContext, BoxParentData, RenderBox, Size};
use flui_tree::Leaf;
use flui_types::geometry::px;

use super::SparseChildren;
use crate::GlobalKey;
use crate::owner::RecoveredAt;
use crate::view::{RenderView, View};
use crate::{BuildOwner, ElementTree};

/// A minimal render-bearing leaf view used as both host and child in these
/// tests — mirrors the `SizedBoxView` in `view/render.rs` tests.
#[derive(Clone)]
struct LeafBox {
    side: f32,
}

impl RenderView for LeafBox {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(&self, _ctx: &crate::RenderObjectContext<'_>) -> Self::RenderObject {
        RenderSizedBox::new(Some(px(self.side)), Some(px(self.side)))
    }

    fn update_render_object(
        &self,
        _ctx: &crate::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.set_size(Some(px(self.side)), Some(px(self.side)))
    }
}

impl View for LeafBox {
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::render_variable(self)
    }
}

/// Like [`LeafBox`] but carries a [`GlobalKey`] so `tree.remove` soft-removes
/// it into the inactive queue instead of freeing the slab entry immediately.
/// Used to test the globally-keyed eviction → `finalize_tree` → slab-free path.
#[derive(Clone)]
struct GlobalKeyedLeafBox {
    side: f32,
    key: GlobalKey<Self>,
    detach_count: Arc<AtomicUsize>,
}

#[derive(Debug)]
struct DetachCountingBox {
    side: f32,
    detach_count: Arc<AtomicUsize>,
}

impl flui_foundation::Diagnosticable for DetachCountingBox {}

impl RenderBox for DetachCountingBox {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
        Size::new(px(self.side), px(self.side))
    }

    fn detach(&mut self) {
        self.detach_count.fetch_add(1, Ordering::SeqCst);
    }
}

impl RenderView for GlobalKeyedLeafBox {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = DetachCountingBox;

    fn create_render_object(&self, _ctx: &crate::RenderObjectContext<'_>) -> Self::RenderObject {
        DetachCountingBox {
            side: self.side,
            detach_count: Arc::clone(&self.detach_count),
        }
    }

    fn update_render_object(
        &self,
        _ctx: &crate::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        render_object.side = self.side;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }
}

impl View for GlobalKeyedLeafBox {
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::render_variable(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

/// Mount a render-bearing host root wired to a fresh `PipelineOwner`, and
/// return everything the tests drive `SparseChildren` against.
fn host_tree() -> (
    ElementTree,
    BuildOwner,
    PipelineCell,
    flui_foundation::ElementId,
) {
    let pipeline = PipelineCell::new(PipelineOwner::new());
    let mut build_owner = BuildOwner::new();
    let mut tree = ElementTree::new();
    let host = tree.mount_root_with_pipeline_owner(
        &LeafBox { side: 10.0 },
        Some(pipeline.clone()),
        &mut build_owner.element_owner_mut(),
    );
    (tree, build_owner, pipeline, host)
}

/// Read back the stamped logical index from a child's render node.
fn stamped_index(
    tree: &ElementTree,
    pipeline: &PipelineCell,
    child: flui_foundation::ElementId,
) -> Option<usize> {
    let render_id = tree.get(child)?.element().render_id()?;
    pipeline.with(|owner| {
        let node = owner.render_tree().get(render_id)?;
        node.parent_data()?
            .downcast_ref::<SliverMultiBoxAdaptorParentData>()
            .map(|pd| pd.index)
    })
}

#[test]
fn ensure_mounts_child_under_host_and_stamps_logical_index() {
    let (mut tree, mut build_owner, pipeline, host) = host_tree();
    let host_render = tree.get(host).unwrap().element().render_id().unwrap();
    let mut children = SparseChildren::new();

    let child = children.ensure(
        5,
        &LeafBox { side: 4.0 },
        host,
        &mut tree,
        &mut build_owner.element_owner_mut(),
        &pipeline,
    );

    assert_eq!(children.get(5), Some(child), "map records the built child");
    assert_eq!(children.len(), 1);

    // The child's render node attached under the host's render node.
    let child_render = tree.get(child).unwrap().element().render_id().unwrap();
    pipeline.with(|owner| {
        assert_eq!(
            owner.render_tree().parent(child_render),
            Some(host_render),
            "the lazy child's render node attaches under the host",
        );
    });

    // And carries the logical index in its parent data.
    assert_eq!(stamped_index(&tree, &pipeline, child), Some(5));
}

#[test]
fn ensure_is_idempotent_for_a_built_index() {
    let (mut tree, mut build_owner, pipeline, host) = host_tree();
    let mut children = SparseChildren::new();

    let first = children.ensure(
        2,
        &LeafBox { side: 4.0 },
        host,
        &mut tree,
        &mut build_owner.element_owner_mut(),
        &pipeline,
    );
    let second = children.ensure(
        2,
        &LeafBox { side: 9.0 },
        host,
        &mut tree,
        &mut build_owner.element_owner_mut(),
        &pipeline,
    );

    assert_eq!(first, second, "a built index is not rebuilt");
    assert_eq!(children.len(), 1);
}

#[test]
fn evict_unmounts_child_and_removes_its_render_node() {
    let (mut tree, mut build_owner, pipeline, host) = host_tree();
    let mut children = SparseChildren::new();

    let child = children.ensure(
        3,
        &LeafBox { side: 4.0 },
        host,
        &mut tree,
        &mut build_owner.element_owner_mut(),
        &pipeline,
    );
    let child_render = tree.get(child).unwrap().element().render_id().unwrap();

    let removed = children.evict(3, &mut tree, &mut build_owner.element_owner_mut());

    assert!(removed, "evict reports the child was removed");
    assert_eq!(children.get(3), None);
    assert!(children.is_empty());
    // The element is gone from the tree…
    assert!(tree.get(child).is_none(), "child element unmounted");
    // …and so is its render node.
    pipeline.with(|owner| {
        assert!(
            owner.render_tree().get(child_render).is_none(),
            "the lazy child's render node is removed on evict",
        );
    });
}

#[test]
fn evict_absent_index_is_a_no_op() {
    let (mut tree, mut build_owner, _pipeline, _host) = host_tree();
    let mut children = SparseChildren::new();
    assert!(!children.evict(7, &mut tree, &mut build_owner.element_owner_mut()));
}

#[test]
fn retain_band_drops_out_of_band_children_only() {
    let (mut tree, mut build_owner, pipeline, host) = host_tree();
    let mut children = SparseChildren::new();

    for logical_index in 0..5 {
        children.ensure(
            logical_index,
            &LeafBox { side: 4.0 },
            host,
            &mut tree,
            &mut build_owner.element_owner_mut(),
            &pipeline,
        );
    }
    assert_eq!(children.len(), 5);

    // Keep only the band [2, 4): indices 2 and 3 survive.
    children.retain_band(2, 4, &mut tree, &mut build_owner.element_owner_mut());

    let surviving: Vec<usize> = children.logical_indices().copied().collect();
    assert_eq!(surviving, vec![2, 3], "only in-band children survive");
}

/// `ensure` must push the freshly-mounted child onto the dirty heap so
/// the second `build_scope` in `service_child_requests` can expand its
/// subtree (e.g. Padding(Text)). Without `schedule_build_for` the heap is
/// empty and child subtrees never grow past the top-level node.
#[test]
fn ensure_schedules_child_for_build() {
    let (mut tree, mut build_owner, pipeline, host) = host_tree();

    // Record how many elements are already scheduled by the root mount.
    let count_before = build_owner.dirty_count();

    let mut children = SparseChildren::new();
    children.ensure(
        0,
        &LeafBox { side: 4.0 },
        host,
        &mut tree,
        &mut build_owner.element_owner_mut(),
        &pipeline,
    );

    // After `ensure`, the child must be on the dirty heap so the next
    // `build_scope` can expand its own subtree.
    assert!(
        build_owner.dirty_count() > count_before,
        "ensure must schedule the freshly-mounted child for build — \
             without schedule_build_for, service_child_requests runs build_scope \
             over an empty heap and child subtrees never expand",
    );
}

/// `evict` must remove the child's *entire* descendant subtree, not
/// only the top-level element. A single-node `tree.remove` leaks every
/// descendant element (and their render nodes), which the slab retains as
/// orphans forever.
///
/// The test simulates a two-level view tree by:
/// 1. `ensure`-mounting a top-level lazy child.
/// 2. `tree.insert`-ing a grandchild and wiring it into the child's
///    `child_ids` via `set_child_ids` — exactly what the reconciler does
///    when it resolves a composite child view (e.g. Padding wrapping Text).
/// 3. Evicting and asserting both nodes are gone.
#[test]
fn evict_subtree_cleans_descendants() {
    let (mut tree, mut build_owner, pipeline, host) = host_tree();
    let mut children = SparseChildren::new();

    // Mount a top-level lazy child (the view-tree root of one list item).
    let child = children.ensure(
        0,
        &LeafBox { side: 4.0 },
        host,
        &mut tree,
        &mut build_owner.element_owner_mut(),
        &pipeline,
    );

    // Insert a grandchild under `child` to simulate a composite view
    // subtree (e.g. Container → Padding → Text). `tree.insert` creates
    // the slab entry and runs `on_mount`, but does NOT automatically write
    // into `child.child_ids` — that only happens during reconciliation.
    // Wire it up explicitly so `remove_subtree`'s DFS finds it.
    let grandchild = tree.insert(
        &LeafBox { side: 2.0 },
        child,
        0,
        &mut build_owner.element_owner_mut(),
    );
    // Simulate the reconciler's `set_child_ids` call so the subtree-DFS
    // in `remove_subtree` can reach `grandchild` through `child.child_ids`.
    tree.get_mut(child).unwrap().set_child_ids(vec![grandchild]);

    // Both nodes live in the tree before eviction.
    assert!(tree.get(child).is_some(), "child present before evict");
    assert!(
        tree.get(grandchild).is_some(),
        "grandchild present before evict"
    );

    // Capture render IDs before eviction to verify render-tree cleanup.
    let child_render_id = tree.get(child).and_then(|n| n.element().render_id());
    let grandchild_render_id = tree.get(grandchild).and_then(|n| n.element().render_id());

    // Both render nodes must exist (pipeline is threaded through the parent
    // element into `tree.insert` via `PipelineCell` propagation).
    assert!(
        child_render_id.is_some(),
        "child element must have a render node before evict"
    );
    assert!(
        grandchild_render_id.is_some(),
        "grandchild element must have a render node before evict"
    );

    // Evict the list item — the whole subtree must disappear.
    let removed = children.evict(0, &mut tree, &mut build_owner.element_owner_mut());

    assert!(removed, "evict reports the child was present");
    assert!(
        tree.get(child).is_none(),
        "top-level lazy child must be removed on evict",
    );
    assert!(
        tree.get(grandchild).is_none(),
        "descendant element must also be removed — single-node remove \
             would leak this grandchild as an orphaned slab entry",
    );

    // Render nodes must also be gone after subtree eviction.
    pipeline.with(|owner| {
        if let Some(rid) = child_render_id {
            assert!(
                owner.render_tree().get(rid).is_none(),
                "child render node must be removed on subtree evict",
            );
        }
        if let Some(rid) = grandchild_render_id {
            assert!(
                owner.render_tree().get(rid).is_none(),
                "grandchild render node must also be removed on subtree evict — \
                     single-node remove leaks descendant render nodes",
            );
        }
    });
}

/// A globally-keyed lazy child pushed to the inactive queue by eviction
/// must be slab-freed by `finalize_tree` — not left dangling.
///
/// A globally-keyed element is soft-removed by `tree.remove` (called inside
/// `remove_subtree`): the slab entry stays alive, the element is placed into
/// `BuildOwner::inactive_elements`, and `has_inactive_elements()` returns
/// `true`. Only `finalize_tree` drains that queue and calls `remove_finalized`
/// which actually frees the slab slot. Without `finalize_tree` the element
/// would remain in the slab indefinitely.
///
/// The test uses a leaf view so the globally-keyed root has no descendants —
/// the non-keyed descendant-leak concern for composite subtrees is a separate,
/// orthogonal investigation.
/// A GlobalKey moving between two lazy hosts must relocate the existing
/// element, not panic.
///
/// TWO preconditions block it, and only the first is fixed:
///
/// 1. `ensure` called `ElementTree::insert` with no reconcile guard, so
///    `retake_active_global_key`'s `is_reconciling_parent` check failed.
///    Fixed — `ensure` now declares `host` for the duration of the insert.
/// 2. **Still open.** `retake_active_global_key` then verifies the
///    candidate is in `from_parent.child_ids`. A lazy host never populates
///    `child_ids` — resident children live in the `SparseChildren` map —
///    so that reverse-edge check fails, `try_retake_global_key` yields
///    `GlobalKeyRetake::Rejected`, and `insert`'s `Rejected` arm panics.
///
/// Closing (2) is a design call, not a patch: either the membership check
/// stops treating `child_ids` as authoritative, or the lazy path starts
/// maintaining it. The latter has wider consequences — every walk that
/// iterates `child_ids` (`collect_render_frontier`, `deactivate_subtree`,
/// ancestry recompute) currently skips sparse children too.
#[test]
#[ignore = "known regression: a lazy host does not maintain child_ids, so \
                retake_active_global_key's reverse-edge membership check rejects \
                the relocation and insert's Rejected arm panics — see the test's \
                own doc comment"]
fn a_global_key_moving_between_lazy_hosts_relocates_instead_of_panicking() {
    let (mut tree, mut build_owner, pipeline, host_a) = host_tree();
    let host_b = tree.insert(
        &LeafBox { side: 10.0 },
        host_a,
        1,
        &mut build_owner.element_owner_mut(),
    );

    let keyed_item = GlobalKeyedLeafBox {
        side: 4.0,
        key: GlobalKey::<GlobalKeyedLeafBox>::new(),
        detach_count: Arc::new(AtomicUsize::new(0)),
    };

    let mut list_a = SparseChildren::new();
    let first = list_a.ensure(
        0,
        &keyed_item,
        host_a,
        &mut tree,
        &mut build_owner.element_owner_mut(),
        &pipeline,
    );

    // The same key surfacing under a different lazy host — a keyed item
    // scrolled from one list into another.
    let mut list_b = SparseChildren::new();
    let moved = list_b.ensure(
        0,
        &keyed_item,
        host_b,
        &mut tree,
        &mut build_owner.element_owner_mut(),
        &pipeline,
    );

    assert_eq!(
        moved, first,
        "the keyed child must relocate, preserving element identity, not mount a duplicate"
    );
    assert_eq!(
        tree.get(moved).and_then(crate::ElementNode::parent),
        Some(host_b),
        "the relocated child must be reparented onto the new host"
    );
}

/// A `GlobalKey`'d leaf whose `update_render_object` panics once armed.
///
/// The retake path applies the incoming view to the element it pulled
/// back (`element_mut().update(view, owner)`), so a retake runs this user
/// code with the relocation already committed — a second panic site
/// behind the same `insert` call as `create_render_object`.
#[derive(Clone)]
struct GlobalKeyedPanicsOnUpdate {
    key: GlobalKey<Self>,
    armed: Arc<std::sync::atomic::AtomicBool>,
}

impl RenderView for GlobalKeyedPanicsOnUpdate {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = RenderSizedBox;
    fn create_render_object(&self, _ctx: &crate::RenderObjectContext<'_>) -> Self::RenderObject {
        RenderSizedBox::new(Some(px(4.0)), Some(px(4.0)))
    }
    fn update_render_object(
        &self,
        _ctx: &crate::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        assert!(
            !self.armed.load(std::sync::atomic::Ordering::SeqCst),
            "retake update_render_object boom"
        );
        flui_rendering::RenderUpdateImpact::NONE
    }
}

impl View for GlobalKeyedPanicsOnUpdate {
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::render_variable(self)
    }
    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

/// Records every mount/unmount an owner announces, so a test can check
/// that the two stay causally ordered.
#[derive(Default)]
struct MountLedger {
    mounted: parking_lot::Mutex<Vec<flui_foundation::ElementId>>,
    unmounted: parking_lot::Mutex<Vec<flui_foundation::ElementId>>,
}

impl flui_foundation::observe::TreeObserver for MountLedger {
    fn element_mounted(&self, event: &flui_foundation::observe::ElementMounted) {
        self.mounted.lock().push(event.element);
    }
    fn element_unmounted(&self, event: &flui_foundation::observe::ElementUnmounted) {
        self.unmounted.lock().push(event.element);
    }
}

/// A mount that panicked was never announced, so its cleanup must not
/// announce an unmount either.
///
/// `insert` emits `Mount` only after `mount` returns, so an observer never
/// sees the abandoned node appear; reporting its removal would hand that
/// observer an id out of nowhere and break the causal ordering ADR-0040
/// promises. `discard_unannounced` is the silent counterpart of
/// `remove_finalized` for exactly this node.
#[test]
fn an_abandoned_mount_announces_neither_a_mount_nor_an_unmount() {
    let (mut tree, mut build_owner, pipeline, host) = host_tree();
    let ledger = Arc::new(MountLedger::default());
    build_owner
        .set_tree_observer(Arc::clone(&ledger) as Arc<dyn flui_foundation::observe::TreeObserver>);

    let mut children = SparseChildren::new();
    let recovered = children.ensure(
        0,
        &PanicsOnCreateRenderObject,
        host,
        &mut tree,
        &mut build_owner.element_owner_mut(),
        &pipeline,
    );

    let unmounted = ledger.unmounted.lock().clone();
    assert!(
        unmounted.is_empty(),
        "the abandoned node was never announced as mounted, so nothing may \
             be announced as unmounted: got {unmounted:?}"
    );
    let mounted = ledger.mounted.lock().clone();
    assert_eq!(
        mounted,
        vec![recovered],
        "only the recovery child is announced"
    );
}

/// A render view whose `create_render_object` panics.
#[derive(Clone)]
struct PanicsOnCreateRenderObject;

impl RenderView for PanicsOnCreateRenderObject {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = RenderSizedBox;
    fn create_render_object(&self, _ctx: &crate::RenderObjectContext<'_>) -> Self::RenderObject {
        panic!("create_render_object boom")
    }
    fn update_render_object(
        &self,
        _ctx: &crate::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }
}

impl View for PanicsOnCreateRenderObject {
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::render_variable(self)
    }
}

/// A panic in the *retake* half of a mount is undone too.
///
/// The mount boundary's report distinguishes a freshly minted node from a
/// retaken one precisely for this: an evicted `GlobalKey`'d child pulled
/// back from the inactive queue is already reparented and reactivated by
/// the time its `update` runs, so recovering by "remove the node this
/// insert minted" would remove nothing and leave a broken, half-updated
/// element parented here with the error view beside it.
#[test]
fn a_panicking_retake_removes_the_reactivated_element_instead_of_stranding_it() {
    let (mut tree, mut build_owner, pipeline, host) = host_tree();
    let item = GlobalKeyedPanicsOnUpdate {
        key: GlobalKey::<GlobalKeyedPanicsOnUpdate>::new(),
        armed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };

    let mut children = SparseChildren::new();
    let first = children.ensure(
        0,
        &item,
        host,
        &mut tree,
        &mut build_owner.element_owner_mut(),
        &pipeline,
    );

    // Evict: the `GlobalKey` makes this a soft removal, so the element
    // waits in the inactive queue for a retake instead of being freed.
    children.evict(0, &mut tree, &mut build_owner.element_owner_mut());
    assert!(
        tree.get(first).is_some(),
        "a globally-keyed eviction is a soft removal — the slab entry survives"
    );

    item.armed.store(true, std::sync::atomic::Ordering::SeqCst);
    let recovered = children.ensure(
        0,
        &item,
        host,
        &mut tree,
        &mut build_owner.element_owner_mut(),
        &pipeline,
    );

    assert_ne!(
        recovered, first,
        "the reactivated element was removed, not handed back broken"
    );
    assert!(
        tree.get(first).is_none(),
        "the half-updated element must not stay parented under the host"
    );
    assert_eq!(
        tree.get(recovered)
            .expect("a live element")
            .element()
            .view_type_id(),
        std::any::TypeId::of::<crate::view::ErrorView>(),
        "the index carries the error view"
    );

    let recovered_panics = build_owner.take_recovered_panics();
    assert_eq!(
        recovered_panics.len(),
        1,
        "exactly one panic must be recorded for the retake's own update"
    );
    assert_eq!(recovered_panics[0].hook, crate::LifecycleHook::Update);
}

/// A `GlobalKey`'d stateful item whose `ViewState::activate` panics once
/// armed — the retake half of a mount that runs *before* the retake's own
/// `update`, so this exercises the containment window's earlier half
/// (the `Retaken` report is written before `activate_subtree` runs, so
/// the undo always has a relocated element to act on).
#[derive(Clone)]
struct GlobalKeyedPanicsOnActivate {
    key: GlobalKey<Self>,
    armed: Arc<std::sync::atomic::AtomicBool>,
}

struct GlobalKeyedPanicsOnActivateState {
    armed: Arc<std::sync::atomic::AtomicBool>,
}

impl crate::StatefulView for GlobalKeyedPanicsOnActivate {
    type State = GlobalKeyedPanicsOnActivateState;

    fn create_state(&self) -> Self::State {
        GlobalKeyedPanicsOnActivateState {
            armed: Arc::clone(&self.armed),
        }
    }
}

impl crate::ViewState<GlobalKeyedPanicsOnActivate> for GlobalKeyedPanicsOnActivateState {
    fn build(
        &self,
        _view: &GlobalKeyedPanicsOnActivate,
        _ctx: &dyn crate::BuildContext,
    ) -> impl crate::IntoView {
        LeafBox { side: 4.0 }
    }

    fn activate(&mut self) {
        assert!(
            !self.armed.load(std::sync::atomic::Ordering::SeqCst),
            "retake activate boom"
        );
    }
}

impl View for GlobalKeyedPanicsOnActivate {
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::stateful(self)
    }
    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

/// A panic in `activate_subtree` (not the retake's `update`) is undone
/// too. Before the `Retaken` write moved ahead of `activate_subtree`, it
/// ran AFTER it instead, so an `activate` panic left the relocated
/// element live and reparented but reported nowhere: the undo found
/// nothing armed and removed nothing, stranding it.
#[test]
fn a_panicking_activate_removes_the_reactivated_element_instead_of_stranding_it() {
    let (mut tree, mut build_owner, pipeline, host) = host_tree();
    let pre_mount_count = tree.len();
    let item = GlobalKeyedPanicsOnActivate {
        key: GlobalKey::<GlobalKeyedPanicsOnActivate>::new(),
        armed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };

    let mut children = SparseChildren::new();
    let first = children.ensure(
        0,
        &item,
        host,
        &mut tree,
        &mut build_owner.element_owner_mut(),
        &pipeline,
    );
    // Drive the first build so `init_state` actually runs before the
    // retake — `StatefulBehavior::on_activate` is
    // gated on a completed `init_state` (matching Flutter's guaranteed
    // `initState` -> `activate` ordering), so an item that was only
    // mounted, never built, never runs its `activate` callback (and
    // this fixture's induced panic would never fire).
    build_owner.build_scope(&mut tree);

    // Evict: the `GlobalKey` makes this a soft removal, so the element
    // waits in the inactive queue for a retake instead of being freed.
    children.evict(0, &mut tree, &mut build_owner.element_owner_mut());
    assert!(
        tree.get(first).is_some(),
        "a globally-keyed eviction is a soft removal — the slab entry survives"
    );

    item.armed.store(true, std::sync::atomic::Ordering::SeqCst);
    // Re-ensured at a DIFFERENT index within the same frame — the retake
    // still resolves by GlobalKey, not by index.
    let recovered = children.ensure(
        1,
        &item,
        host,
        &mut tree,
        &mut build_owner.element_owner_mut(),
        &pipeline,
    );

    assert_ne!(
        recovered, first,
        "the reactivated element was removed, not handed back broken"
    );
    assert!(
        tree.get(first).is_none(),
        "the half-activated element must not stay parented under the host"
    );
    assert!(
        build_owner.element_for_global_key(&item.key).is_none(),
        "the GlobalKey registration must not still resolve to the removed element"
    );
    assert_eq!(
        tree.get(recovered)
            .expect("a live element")
            .element()
            .view_type_id(),
        std::any::TypeId::of::<crate::view::ErrorView>(),
        "the index carries the error view"
    );
    assert_eq!(
        tree.len(),
        pre_mount_count + 1,
        "only the substitute error view remains — the stranded original is gone"
    );

    let recovered_panics = build_owner.take_recovered_panics();
    assert_eq!(
        recovered_panics.len(),
        1,
        "exactly one panic must be recorded, not double-counted across the retake \
             and its substitute mount"
    );
    assert_eq!(recovered_panics[0].hook, crate::LifecycleHook::Activate);
    // `StatefulBehavior::on_activate` catches its OWN panic and records
    // it under its own accurate identity — `first`,
    // the retaken candidate itself, since `activate` panicked on the
    // candidate's own state, not a descendant's — then re-raises so
    // the retake window still observes the unwind. Without that
    // behavior-level catch the retake window one level up would be the
    // only thing to record it, and could only name the retake as a
    // `Substituted { element: Some(first), .. }` (a coarser identity
    // than the element whose hook actually panicked; see
    // `a_panicking_activate_records_the_actual_panicking_descendant_not_the_retake_root`
    // for the case where those two differ).
    assert!(matches!(
        recovered_panics[0].at,
        RecoveredAt::Element {
            element,
            parent: None,
            ..
        } if element == first
    ));
}

/// A `GlobalKey`'d Stateful host whose OWN `activate` never panics — its
/// single build child does. Proves attribution names the DESCENDANT
/// whose hook actually panicked, not the retake candidate, when the two
/// differ.
#[derive(Clone)]
struct GlobalKeyedHostOverActivatePanicChild {
    key: GlobalKey<Self>,
    child_armed: Arc<std::sync::atomic::AtomicBool>,
}

struct GlobalKeyedHostOverActivatePanicChildState {
    child_armed: Arc<std::sync::atomic::AtomicBool>,
}

impl crate::StatefulView for GlobalKeyedHostOverActivatePanicChild {
    type State = GlobalKeyedHostOverActivatePanicChildState;

    fn create_state(&self) -> Self::State {
        GlobalKeyedHostOverActivatePanicChildState {
            child_armed: Arc::clone(&self.child_armed),
        }
    }
}

impl crate::ViewState<GlobalKeyedHostOverActivatePanicChild>
    for GlobalKeyedHostOverActivatePanicChildState
{
    fn build(
        &self,
        _view: &GlobalKeyedHostOverActivatePanicChild,
        _ctx: &dyn crate::BuildContext,
    ) -> impl crate::IntoView {
        ActivatePanicChild {
            armed: Arc::clone(&self.child_armed),
        }
    }

    // No `activate` override: this host's own `activate` never panics —
    // only its build child's does.
}

impl View for GlobalKeyedHostOverActivatePanicChild {
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::stateful(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

/// An unkeyed Stateful descendant whose `activate` panics once armed.
#[derive(Clone)]
struct ActivatePanicChild {
    armed: Arc<std::sync::atomic::AtomicBool>,
}

struct ActivatePanicChildState {
    armed: Arc<std::sync::atomic::AtomicBool>,
}

impl crate::StatefulView for ActivatePanicChild {
    type State = ActivatePanicChildState;

    fn create_state(&self) -> Self::State {
        ActivatePanicChildState {
            armed: Arc::clone(&self.armed),
        }
    }
}

impl crate::ViewState<ActivatePanicChild> for ActivatePanicChildState {
    fn build(
        &self,
        _view: &ActivatePanicChild,
        _ctx: &dyn crate::BuildContext,
    ) -> impl crate::IntoView {
        LeafBox { side: 4.0 }
    }

    fn activate(&mut self) {
        assert!(
            !self.armed.load(std::sync::atomic::Ordering::SeqCst),
            "descendant activate boom"
        );
    }
}

impl View for ActivatePanicChild {
    fn create_element(&self) -> crate::element::ElementKind {
        crate::element::ElementKind::stateful(self)
    }
}

/// Direct public activation has no recovery boundary: its panic propagates,
/// remains unrecorded, and cannot leave the retake-only handoff armed.
#[test]
fn direct_public_activate_panic_is_not_reported_as_recovered() {
    let (mut tree, mut build_owner, _pipeline, host) = host_tree();
    let armed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let activating = tree.insert(
        &ActivatePanicChild {
            armed: Arc::clone(&armed),
        },
        host,
        0,
        &mut build_owner.element_owner_mut(),
    );
    let depth = tree.get(activating).expect("activating child").depth();
    build_owner.schedule_build_for(activating, depth, crate::RebuildReason::InitialMount);
    build_owner.build_scope(&mut tree);
    tree.deactivate(activating, &mut build_owner.element_owner_mut());

    armed.store(true, std::sync::atomic::Ordering::SeqCst);
    let escaped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        tree.activate(activating, &mut build_owner.element_owner_mut());
    }));
    assert!(
        escaped.is_err(),
        "direct public activation remains unbounded"
    );
    assert_eq!(
        build_owner.hook_panic_recorded.get(),
        None,
        "direct activation must leave the retake-only handoff disarmed"
    );
    assert!(
        build_owner.take_recovered_panics().is_empty(),
        "an unbounded panic must not be reported as recovered"
    );
}

/// A recorded activation unwind that escapes the public dense insertion
/// path must disarm its transient handoff before the caller catches it.
///
/// CALIBRATION: removing the immediate `take_hook_panic_recorded` from the
/// active-retake `Activate` catch leaves the handoff armed, so the private
/// `None` assertion below fails before the later bounded recovery begins.
#[test]
fn public_dense_retake_disarms_recording_before_a_later_bounded_retake() {
    let (mut tree, mut build_owner, pipeline, host) = host_tree();
    let donor = tree.insert(
        &LeafBox { side: 8.0 },
        host,
        0,
        &mut build_owner.element_owner_mut(),
    );
    let destination = tree.insert(
        &LeafBox { side: 8.0 },
        host,
        1,
        &mut build_owner.element_owner_mut(),
    );
    tree.get_mut(host)
        .expect("host")
        .set_child_ids(vec![donor, destination]);

    let dense_item = GlobalKeyedPanicsOnActivate {
        key: GlobalKey::new(),
        armed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    let dense_candidate = tree.insert(&dense_item, donor, 0, &mut build_owner.element_owner_mut());
    tree.get_mut(donor)
        .expect("donor")
        .set_child_ids(vec![dense_candidate]);
    let depth = tree.get(dense_candidate).expect("dense candidate").depth();
    build_owner.schedule_build_for(dense_candidate, depth, crate::RebuildReason::InitialMount);
    build_owner.build_scope(&mut tree);

    dense_item
        .armed
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let escaped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _reconcile_guard = tree.begin_reconcile(destination);
        tree.insert(
            &dense_item,
            destination,
            0,
            &mut build_owner.element_owner_mut(),
        );
    }));
    assert!(
        escaped.is_err(),
        "the public dense insertion path resumes the retake panic"
    );
    assert_eq!(
        build_owner.hook_panic_recorded.get(),
        None,
        "the immediate retake catch must disarm the unwind-local handoff"
    );
    let activation_records = build_owner.take_recovered_panics();
    assert_eq!(activation_records.len(), 1, "the dense escape records once");
    assert_eq!(activation_records[0].hook, crate::LifecycleHook::Activate);
    assert!(matches!(
        activation_records[0].at,
        RecoveredAt::Element {
            element,
            parent: None,
            ..
        } if element == dense_candidate
    ));

    let bounded_item = GlobalKeyedPanicsOnActivate {
        key: GlobalKey::new(),
        armed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    let mut sparse = SparseChildren::new();
    let bounded_candidate = sparse.ensure(
        0,
        &bounded_item,
        host,
        &mut tree,
        &mut build_owner.element_owner_mut(),
        &pipeline,
    );
    build_owner.build_scope(&mut tree);
    sparse.evict(0, &mut tree, &mut build_owner.element_owner_mut());
    bounded_item
        .armed
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let substitute = sparse.ensure(
        1,
        &bounded_item,
        host,
        &mut tree,
        &mut build_owner.element_owner_mut(),
        &pipeline,
    );

    let later_records = build_owner.take_recovered_panics();
    assert_eq!(
        later_records.len(),
        1,
        "the later bounded retake records once without suppression or duplication"
    );
    assert_eq!(later_records[0].hook, crate::LifecycleHook::Activate);
    assert!(matches!(
        later_records[0].at,
        RecoveredAt::Element {
            element,
            parent: None,
            ..
        } if element == bounded_candidate
    ));
    assert_ne!(substitute, bounded_candidate);
    assert_eq!(
        tree.get(substitute)
            .expect("bounded substitute")
            .element()
            .view_type_id(),
        std::any::TypeId::of::<crate::view::ErrorView>()
    );
    assert_eq!(build_owner.hook_panic_recorded.get(), None);
}

/// The record names the DESCENDANT whose `activate` actually panicked,
/// not the retake root — `StatefulBehavior::on_activate` catches its
/// own panic under its own identity before the retake window one level
/// up ever sees the unwind.
///
/// RED before `StatefulBehavior::on_activate` gained its own catch: the
/// retake window was the only thing to record the panic, and could only
/// name the retake candidate itself — `Substituted { element:
/// Some(root_first), .. }` — because it has no way to see which element
/// deep in the reactivated subtree actually panicked.
#[test]
fn a_panicking_activate_records_the_actual_panicking_descendant_not_the_retake_root() {
    let (mut tree, mut build_owner, pipeline, host) = host_tree();
    let child_armed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let item = GlobalKeyedHostOverActivatePanicChild {
        key: GlobalKey::<GlobalKeyedHostOverActivatePanicChild>::new(),
        child_armed: Arc::clone(&child_armed),
    };

    let mut children = SparseChildren::new();
    let root_first = children.ensure(
        0,
        &item,
        host,
        &mut tree,
        &mut build_owner.element_owner_mut(),
        &pipeline,
    );
    // Drive the first build so the descendant `ActivatePanicChild`
    // element actually exists (and its own `init_state` runs) before
    // the retake — `on_activate` is gated on a completed `init_state`.
    build_owner.build_scope(&mut tree);

    let descendant_id = tree
        .iter_nodes()
        .find_map(|(id, node)| (node.parent() == Some(root_first)).then_some(id))
        .expect("the host's build mounted exactly one child (ActivatePanicChild)");

    children.evict(0, &mut tree, &mut build_owner.element_owner_mut());
    assert!(
        tree.get(root_first).is_some(),
        "a globally-keyed eviction is a soft removal — the slab entry survives"
    );

    child_armed.store(true, std::sync::atomic::Ordering::SeqCst);
    let recovered = children.ensure(
        1,
        &item,
        host,
        &mut tree,
        &mut build_owner.element_owner_mut(),
        &pipeline,
    );

    assert_ne!(
        recovered, root_first,
        "the reactivated subtree was removed, not handed back broken"
    );
    assert!(
        tree.get(root_first).is_none(),
        "the half-activated subtree must not stay parented under the host"
    );
    assert_eq!(
        tree.get(recovered)
            .expect("a live element")
            .element()
            .view_type_id(),
        std::any::TypeId::of::<crate::view::ErrorView>(),
        "the index carries the error view"
    );

    let recovered_panics = build_owner.take_recovered_panics();
    assert_eq!(
        recovered_panics.len(),
        1,
        "exactly one panic must be recorded, not double-counted across the descendant's \
             own catch and the retake window"
    );
    assert_eq!(recovered_panics[0].hook, crate::LifecycleHook::Activate);
    assert!(
        matches!(
            recovered_panics[0].at,
            RecoveredAt::Element {
                element,
                parent: None,
                ..
            } if element == descendant_id
        ),
        "the record names the actual panicking descendant, not the retake root: {:?}",
        recovered_panics[0].at
    );
}

/// The debug-only eager duplicate-`GlobalKey` rejection (ADR-0050) must
/// propagate through a lazy host exactly as it does through a dense
/// parent: ensuring the SAME key at a second index under the SAME host,
/// with no evict between the two, is a genuine intra-frame duplicate —
/// not a same-frame retake — so `retake_active_global_key`'s
/// same-active-parent check panics before either per-child containment
/// window opens (see `ChildHookPanic`'s doc: framework code
/// outside those windows is never contained). If a future change
/// widened the substituting primitives' catch to swallow this, an
/// application-level bug that duplicates a key would silently render
/// two `ErrorView`s instead of aborting where Flutter's `assert` does.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "duplicate GlobalKey children are not allowed")]
fn ensuring_the_same_global_key_twice_under_one_lazy_host_without_an_evict_panics() {
    let (mut tree, mut build_owner, pipeline, host) = host_tree();
    let keyed_item = GlobalKeyedLeafBox {
        side: 4.0,
        key: GlobalKey::<GlobalKeyedLeafBox>::new(),
        detach_count: Arc::new(AtomicUsize::new(0)),
    };

    let mut children = SparseChildren::new();
    children.ensure(
        0,
        &keyed_item,
        host,
        &mut tree,
        &mut build_owner.element_owner_mut(),
        &pipeline,
    );
    // No evict between the two — the first is still active under the
    // same host, so this is the duplicate case, not a retake.
    children.ensure(
        1,
        &keyed_item,
        host,
        &mut tree,
        &mut build_owner.element_owner_mut(),
        &pipeline,
    );
}

#[test]
fn evicted_globally_keyed_child_freed_by_finalize_tree() {
    let (mut tree, mut build_owner, pipeline, host) = host_tree();
    let element_count_before = tree.len();

    let global_key = GlobalKey::<GlobalKeyedLeafBox>::new();
    let detach_count = Arc::new(AtomicUsize::new(0));
    let keyed_item = GlobalKeyedLeafBox {
        side: 4.0,
        key: global_key.clone(),
        detach_count: Arc::clone(&detach_count),
    };

    let mut children = SparseChildren::new();
    let child_id = children.ensure(
        0,
        &keyed_item,
        host,
        &mut tree,
        &mut build_owner.element_owner_mut(),
        &pipeline,
    );

    assert_eq!(
        tree.len(),
        element_count_before + 1,
        "the globally-keyed child must occupy one slab slot after mount"
    );
    assert!(
        tree.get(child_id).is_some(),
        "child must be accessible in the tree before eviction"
    );

    // Evict: `remove_subtree` → `remove` → soft-removes because the element
    // has a `registered_global_key_hash` (GlobalKey). The slab entry survives.
    children.evict(0, &mut tree, &mut build_owner.element_owner_mut());

    assert_eq!(
        detach_count.load(Ordering::SeqCst),
        1,
        "soft removal must detach the render subtree immediately",
    );

    assert_eq!(
        children.get(0),
        None,
        "evict must clear the SparseChildren map entry"
    );
    // The node is still in the slab (soft-removed), but pushed to inactive.
    assert_eq!(
        tree.len(),
        element_count_before + 1,
        "soft-remove must not free the slab slot immediately"
    );
    assert!(
        build_owner.has_inactive_elements(),
        "a globally-keyed eviction must push the element to the inactive queue, \
             not free it eagerly — this is what distinguishes soft-remove from eager-remove"
    );

    // `finalize_tree` drains the inactive queue and calls `remove_finalized`
    // on each entry, which frees the slab slot.
    build_owner.finalize_tree(&mut tree);

    assert_eq!(
        detach_count.load(Ordering::SeqCst),
        1,
        "finalization must not detach an already-detached render subtree twice",
    );

    assert!(
        !build_owner.has_inactive_elements(),
        "finalize_tree must drain the inactive queue completely"
    );
    assert_eq!(
        tree.len(),
        element_count_before,
        "the globally-keyed element must be slab-freed by finalize_tree"
    );
    assert!(
        tree.get(child_id).is_none(),
        "the element must no longer be accessible in the tree after finalize_tree"
    );
}
