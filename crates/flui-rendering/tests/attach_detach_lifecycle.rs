//! ADR-0013 milestone — the `attach`/`detach` tree-lifecycle hook.
//!
//! Proves the pipeline actually fires `RenderObject::attach`/`detach` at
//! insert/remove, that the handed-over `RenderInvalidationHandle` is bound to the
//! right node and drives a REAL re-layout on the very next frame, and
//! that a handle captured before removal degrades to a silent no-op
//! afterward — the generational-staleness guarantee `RenderInvalidationHandle`
//! documents. Reparenting in this codebase has no dedicated API (no
//! `move_child`/`adopt_child`); it is remove-then-insert, so that case is
//! exercised the same way a real reparent would hit it.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use flui_rendering::pipeline::{PipelineOwner, RenderInvalidationHandle};
use flui_rendering::prelude::*;
use flui_rendering::traits::RenderSliver;
use flui_tree::Leaf;
use flui_types::geometry::px;

use crate::common::{BoxedRenderObject, BoxedSliverObject};

// ────────────────────────────────────────────────────────────────────────
// Probe: a leaf RenderBox that records attach/detach/perform_layout calls
// ────────────────────────────────────────────────────────────────────────

/// Shared bookkeeping a [`LifecycleProbe`] writes into on
/// `attach`/`detach`/`perform_layout`, read back by the test after the
/// pipeline call returns.
#[derive(Clone, Default, Debug)]
struct LifecycleLog {
    attach_count: Arc<AtomicUsize>,
    detach_count: Arc<AtomicUsize>,
    layout_count: Arc<AtomicUsize>,
    captured_handle: Arc<Mutex<Option<RenderInvalidationHandle>>>,
}

impl LifecycleLog {
    fn attach_count(&self) -> usize {
        self.attach_count.load(Ordering::SeqCst)
    }

    fn detach_count(&self) -> usize {
        self.detach_count.load(Ordering::SeqCst)
    }

    fn layout_count(&self) -> usize {
        self.layout_count.load(Ordering::SeqCst)
    }

    /// The most recently captured handle. Panics if `attach` never fired —
    /// every test here calls it only after asserting `attach_count() > 0`.
    fn captured_handle(&self) -> RenderInvalidationHandle {
        self.captured_handle
            .lock()
            .expect("lock poisoned")
            .clone()
            .expect("attach must have captured a handle before this call")
    }
}

/// A leaf `RenderBox` whose only job is to prove the tree-lifecycle hook
/// fires and hands over a working handle — real render objects (e.g. the
/// future `RenderAnimatedSize`) hold a `Listenable` here instead of a log.
#[derive(Debug)]
struct LifecycleProbe {
    log: LifecycleLog,
    size: Size,
}

impl flui_foundation::Diagnosticable for LifecycleProbe {}

impl RenderBox for LifecycleProbe {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
        self.log.layout_count.fetch_add(1, Ordering::SeqCst);
        self.size
    }

    fn attach(&mut self, handle: RenderInvalidationHandle) {
        self.log.attach_count.fetch_add(1, Ordering::SeqCst);
        *self.log.captured_handle.lock().expect("lock poisoned") = Some(handle);
    }

    fn detach(&mut self) {
        self.log.detach_count.fetch_add(1, Ordering::SeqCst);
    }
}

fn probe(log: LifecycleLog) -> BoxedRenderObject {
    Box::new(LifecycleProbe {
        log,
        size: Size::new(px(40.0), px(40.0)),
    }) as BoxedRenderObject
}

// ────────────────────────────────────────────────────────────────────────
// Probe: a leaf RenderSliver that records attach/detach/perform_layout
// calls — the Sliver-protocol counterpart of `LifecycleProbe`, proving the
// ADR-0013 lifecycle hook fires for Sliver children too (it did not,
// before this fix: no insertion path called `attach` for a Sliver child).
// ────────────────────────────────────────────────────────────────────────

/// A leaf `RenderSliver` whose only job is to prove the tree-lifecycle hook
/// fires and hands over a working handle — mirrors [`LifecycleProbe`] but
/// returns [`SliverGeometry`] instead of [`Size`].
#[derive(Debug)]
struct LifecycleProbeSliver {
    log: LifecycleLog,
}

impl flui_foundation::Diagnosticable for LifecycleProbeSliver {}

impl RenderSliver for LifecycleProbeSliver {
    type Arity = Leaf;
    type ParentData = SliverParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>,
    ) -> SliverGeometry {
        self.log.layout_count.fetch_add(1, Ordering::SeqCst);
        let paint = 20.0_f32.min(ctx.constraints().remaining_paint_extent);
        SliverGeometry {
            scroll_extent: 20.0,
            paint_extent: paint,
            layout_extent: paint,
            max_paint_extent: 20.0,
            hit_test_extent: paint,
            visible: paint > 0.0,
            ..SliverGeometry::ZERO
        }
    }

    fn attach(&mut self, handle: RenderInvalidationHandle) {
        self.log.attach_count.fetch_add(1, Ordering::SeqCst);
        *self.log.captured_handle.lock().expect("lock poisoned") = Some(handle);
    }

    fn detach(&mut self) {
        self.log.detach_count.fetch_add(1, Ordering::SeqCst);
    }
}

fn sliver_probe(log: LifecycleLog) -> BoxedSliverObject {
    Box::new(LifecycleProbeSliver { log }) as BoxedSliverObject
}

/// Mounts a probe as the pipeline's root with tight constraints, ready to
/// drive `run_frame`.
fn rooted_fixture() -> (PipelineOwner, flui_foundation::RenderId, LifecycleLog) {
    let mut owner = PipelineOwner::new();
    let log = LifecycleLog::default();
    let id = owner.insert(probe(log.clone()));
    owner.set_root_id(Some(id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(40.0), px(40.0)))));
    (owner, id, log)
}

fn frame(owner: PipelineOwner) -> PipelineOwner {
    let (owner, result) = owner.run_frame();
    result.expect("frame must not error");
    owner
}

// ────────────────────────────────────────────────────────────────────────
// attach on insert
// ────────────────────────────────────────────────────────────────────────

#[test]
fn insert_fires_exactly_one_attach_with_a_handle_bound_to_the_new_id() {
    let mut owner = PipelineOwner::new();
    let log = LifecycleLog::default();

    let id = owner.insert(probe(log.clone()));

    assert_eq!(
        log.attach_count(),
        1,
        "insert must call attach exactly once"
    );
    let handle = log.captured_handle();
    assert_eq!(
        handle.id(),
        id,
        "the handed-over handle must be bound to the freshly-inserted node"
    );
}

// ────────────────────────────────────────────────────────────────────────
// mark_needs_layout from the captured handle reaches perform_layout
// ────────────────────────────────────────────────────────────────────────

#[test]
fn captured_handle_mark_needs_layout_relayouts_that_node_next_frame() {
    let (owner, _id, log) = rooted_fixture();

    let owner = frame(owner);
    assert_eq!(
        log.layout_count(),
        1,
        "the first frame lays the node out once"
    );

    let owner = frame(owner);
    assert_eq!(
        log.layout_count(),
        1,
        "a clean tree must not re-layout on an idle frame"
    );

    let handle = log.captured_handle();
    handle
        .mark_needs_layout()
        .expect("owner is alive; the send must succeed");

    let owner = frame(owner);
    assert_eq!(
        log.layout_count(),
        2,
        "mark_needs_layout on the captured handle must reach perform_layout \
         on the very next frame"
    );

    let _ = frame(owner);
    assert_eq!(
        log.layout_count(),
        2,
        "one request produces one relayout, then the tree idles again"
    );
}

// ────────────────────────────────────────────────────────────────────────
// detach on remove
// ────────────────────────────────────────────────────────────────────────

#[test]
fn remove_fires_exactly_one_detach() {
    let (owner, id, log) = rooted_fixture();
    let mut owner = frame(owner);
    assert_eq!(log.detach_count(), 0, "detach must not fire before removal");

    owner.remove_render_object(id);

    assert_eq!(
        log.detach_count(),
        1,
        "remove must call detach exactly once"
    );
}

#[test]
fn detach_and_reattach_mint_a_new_invalidation_interval() {
    let (owner, id, log) = rooted_fixture();
    let mut owner = frame(owner);
    let first_interval = log.captured_handle();

    first_interval
        .mark_needs_layout()
        .expect("the request may enqueue before detach");
    let token = owner
        .detach_render_subtrees(&[id])
        .expect("the live root detaches");
    first_interval
        .mark_needs_layout()
        .expect("the old channel stays live while the node is detached");
    owner
        .attach_render_subtrees(token)
        .expect("the preserved root reattaches");
    let second_interval = log.captured_handle();
    assert_eq!(log.attach_count(), 2, "reattach starts one fresh interval");
    assert_eq!(
        log.detach_count(),
        1,
        "detach closes the first interval once"
    );

    owner.clear_all_dirty_nodes();
    owner
        .render_tree()
        .get(id)
        .expect("preserved root remains live")
        .clear_needs_layout();
    owner.drain_pending_dirty();
    assert_eq!(
        owner.nodes_needing_layout().len(),
        0,
        "requests queued before and while detached must not dirty the new interval",
    );

    first_interval
        .mark_needs_layout()
        .expect("an old handle may still enqueue after reattach");
    owner.drain_pending_dirty();
    assert_eq!(
        owner.nodes_needing_layout().len(),
        0,
        "a first-interval handle remains inert after the second interval starts",
    );

    second_interval
        .mark_needs_layout()
        .expect("the current interval's handle remains authoritative");
    owner.drain_pending_dirty();
    assert_eq!(owner.nodes_needing_layout().len(), 1);
    let layout_count_before_current_request = log.layout_count();
    let _owner = frame(owner);
    assert_eq!(
        log.layout_count(),
        layout_count_before_current_request + 1,
        "the current interval's request must reach layout",
    );
}

#[test]
fn detached_tokens_are_linear_for_box_and_sliver_intervals() {
    let mut owner = PipelineOwner::new();
    let box_log = LifecycleLog::default();
    let sliver_log = LifecycleLog::default();
    let box_id = owner.insert(probe(box_log.clone()));
    let sliver_id = owner.insert(sliver_probe(sliver_log.clone()));

    assert_eq!(box_log.attach_count(), 1);
    assert_eq!(sliver_log.attach_count(), 1);

    let box_token = owner.detach_render_subtrees(&[box_id]).expect("box detach");
    let sliver_token = owner
        .detach_render_subtrees(&[sliver_id])
        .expect("sliver detach");
    assert!(matches!(
        owner.detach_render_subtrees(&[box_id]),
        Err(flui_rendering::pipeline::DetachRenderSubtreesError::AlreadyDetached { .. })
    ));
    assert_eq!(box_log.detach_count(), 1);
    assert_eq!(sliver_log.detach_count(), 1);

    owner.attach_render_subtrees(box_token).expect("box attach");
    owner
        .attach_render_subtrees(sliver_token)
        .expect("sliver attach");
    assert_eq!(box_log.attach_count(), 2);
    assert_eq!(sliver_log.attach_count(), 2);
}

#[test]
fn subtree_relocation_visits_every_box_and_sliver_node_without_changing_identity_or_edges() {
    let mut owner = PipelineOwner::new();
    let root_log = LifecycleLog::default();
    let sliver_log = LifecycleLog::default();
    let leaf_log = LifecycleLog::default();
    let root_id = owner.insert(probe(root_log.clone()));
    let sliver_id = owner.insert(sliver_probe(sliver_log.clone()));
    let leaf_id = owner.insert(probe(leaf_log.clone()));
    owner.adopt_render_child(root_id, sliver_id);
    owner.adopt_render_child(sliver_id, leaf_id);

    let token = owner
        .detach_render_subtrees(&[root_id])
        .expect("subtree detach");
    assert_eq!(root_log.detach_count(), 1);
    assert_eq!(sliver_log.detach_count(), 1);
    assert_eq!(leaf_log.detach_count(), 1);
    assert_eq!(owner.render_tree().parent(sliver_id), Some(root_id));
    assert_eq!(owner.render_tree().parent(leaf_id), Some(sliver_id));

    owner.attach_render_subtrees(token).expect("subtree attach");
    assert_eq!(root_log.attach_count(), 2);
    assert_eq!(sliver_log.attach_count(), 2);
    assert_eq!(leaf_log.attach_count(), 2);
    assert_eq!(owner.render_tree().parent(sliver_id), Some(root_id));
    assert_eq!(owner.render_tree().parent(leaf_id), Some(sliver_id));
}

#[test]
fn multi_frontier_relocation_batches_disjoint_subtrees() {
    let mut owner = PipelineOwner::new();
    let first_root_log = LifecycleLog::default();
    let first_child_log = LifecycleLog::default();
    let second_root_log = LifecycleLog::default();
    let second_child_log = LifecycleLog::default();
    let first_root = owner.insert(probe(first_root_log.clone()));
    let first_child = owner.insert(sliver_probe(first_child_log.clone()));
    let second_root = owner.insert(sliver_probe(second_root_log.clone()));
    let second_child = owner.insert(probe(second_child_log.clone()));
    owner.adopt_render_child(first_root, first_child);
    owner.adopt_render_child(second_root, second_child);

    let token = owner
        .detach_render_subtrees(&[first_root, second_root])
        .expect("batch detach");
    assert_eq!(token.node_count(), 4);
    assert_eq!(token.root_count(), 2);
    for log in [
        &first_root_log,
        &first_child_log,
        &second_root_log,
        &second_child_log,
    ] {
        assert_eq!(log.detach_count(), 1);
    }
    owner.attach_render_subtrees(token).expect("batch attach");
    for log in [
        &first_root_log,
        &first_child_log,
        &second_root_log,
        &second_child_log,
    ] {
        assert_eq!(log.attach_count(), 2);
    }
    let token = owner
        .detach_render_subtrees(&[first_root, second_root])
        .expect("second batch detach");
    assert!(matches!(
        owner.detach_render_subtrees(&[first_root, second_root]),
        Err(flui_rendering::pipeline::DetachRenderSubtreesError::AlreadyDetached { .. })
    ));
    owner
        .release_detached_render_subtrees_for_finalization(token)
        .expect("release detached batch");
}

#[test]
fn batch_lifecycle_rejects_overlap_and_wrong_owner_without_consuming_token() {
    let mut owner = PipelineOwner::new();
    let root_log = LifecycleLog::default();
    let child_log = LifecycleLog::default();
    let root = owner.insert(probe(root_log.clone()));
    let child = owner.insert(probe(child_log.clone()));
    owner.adopt_render_child(root, child);

    assert!(matches!(
        owner.detach_render_subtrees(&[root, child]),
        Err(flui_rendering::pipeline::DetachRenderSubtreesError::OverlappingRoots { .. })
    ));
    assert_eq!(owner.render_tree().parent(child), Some(root));
    assert_eq!(root_log.detach_count(), 0);
    assert_eq!(child_log.detach_count(), 0);

    let token = owner.detach_render_subtrees(&[root]).expect("root detach");
    let mut foreign_owner = PipelineOwner::new();
    let failure = foreign_owner
        .attach_render_subtrees(token)
        .expect_err("foreign owner must reject the token");
    assert!(matches!(
        failure.kind(),
        flui_rendering::pipeline::AttachRenderSubtreesError::WrongOwner
    ));
    let token = failure.into_token();
    owner.clear_all_dirty_nodes();
    owner.attach_render_subtrees(token).expect("owner retry");
    assert_eq!(child_log.attach_count(), 2);
    assert!(
        owner
            .nodes_needing_layout()
            .iter()
            .any(|dirty| dirty.id == root)
    );
    assert!(
        owner
            .nodes_needing_paint()
            .iter()
            .any(|dirty| dirty.id == root)
    );
}

#[test]
fn stale_token_failures_retain_the_token_for_a_terminal_retry() {
    let mut owner = PipelineOwner::new();
    let render_id = owner.insert(probe(LifecycleLog::default()));
    let token = owner
        .detach_render_subtrees(&[render_id])
        .expect("detach token");
    assert_eq!(owner.remove_render_object(render_id), 1);

    let attach_failure = owner
        .attach_render_subtrees(token)
        .expect_err("removed node makes the token stale");
    assert!(matches!(
        attach_failure.kind(),
        flui_rendering::pipeline::AttachRenderSubtreesError::NodeNotFound {
            render_id: missing
        } if *missing == render_id
    ));
    let token = attach_failure.into_token();

    let release_failure = owner
        .release_detached_render_subtrees_for_finalization(token)
        .expect_err("the stale token remains stale on terminal release");
    assert!(matches!(
        release_failure.kind(),
        flui_rendering::pipeline::ReleaseDetachedRenderSubtreesError::NodeNotFound {
            render_id: missing
        } if *missing == render_id
    ));
    assert_eq!(release_failure.into_token().node_count(), 1);
}

// The token is a linear capability: duplicating it would let one detached
// batch be attached and released, or released twice.
static_assertions::assert_not_impl_any!(
    flui_rendering::pipeline::DetachedRenderSubtrees: Clone, Copy
);

#[test]
fn detach_rejects_an_empty_batch() {
    let mut owner = PipelineOwner::new();
    assert!(matches!(
        owner.detach_render_subtrees(&[]),
        Err(flui_rendering::pipeline::DetachRenderSubtreesError::Empty)
    ));
}

#[test]
fn detach_distinguishes_a_duplicate_root_from_an_overlap_and_names_both_roots() {
    let mut owner = PipelineOwner::new();
    let root_log = LifecycleLog::default();
    let child_log = LifecycleLog::default();
    let root = owner.insert(probe(root_log.clone()));
    let child = owner.insert(probe(child_log.clone()));
    owner.adopt_render_child(root, child);

    assert!(
        matches!(
            owner.detach_render_subtrees(&[root, root]),
            Err(flui_rendering::pipeline::DetachRenderSubtreesError::DuplicateRoot {
                root: duplicated
            }) if duplicated == root
        ),
        "the same root twice is a duplicate, not an overlap"
    );

    // Both request orders must name the containment the same way round.
    for roots in [[root, child], [child, root]] {
        assert!(
            matches!(
                owner.detach_render_subtrees(&roots),
                Err(flui_rendering::pipeline::DetachRenderSubtreesError::OverlappingRoots {
                    ancestor,
                    descendant,
                }) if ancestor == root && descendant == child
            ),
            "overlap must report {root:?} as the ancestor of {child:?} for {roots:?}"
        );
    }

    assert_eq!(owner.render_tree().parent(child), Some(root));
    assert_eq!(root_log.detach_count(), 0);
    assert_eq!(child_log.detach_count(), 0);
}

#[test]
fn the_owner_seal_rejects_a_token_whose_render_ids_all_match_the_other_owner() {
    let mut owner = PipelineOwner::new();
    let mut twin_owner = PipelineOwner::new();
    let twin_log = LifecycleLog::default();
    let render_id = owner.insert(probe(LifecycleLog::default()));
    let twin_id = twin_owner.insert(probe(twin_log.clone()));
    assert_eq!(
        render_id, twin_id,
        "this proof needs both owners to mint the same render id"
    );

    // Detaching in both owners leaves the twin's node in exactly the state the
    // foreign token describes, so identity is the ONLY thing left to reject on.
    let token = owner
        .detach_render_subtrees(&[render_id])
        .expect("detach token");
    let twin_token = twin_owner
        .detach_render_subtrees(&[twin_id])
        .expect("twin detach token");

    let attach_failure = twin_owner
        .attach_render_subtrees(token)
        .expect_err("a foreign token must not attach a same-numbered node");
    assert!(matches!(
        attach_failure.kind(),
        flui_rendering::pipeline::AttachRenderSubtreesError::WrongOwner
    ));

    let release_failure = twin_owner
        .release_detached_render_subtrees_for_finalization(attach_failure.into_token())
        .expect_err("a foreign token must not release through the twin either");
    assert!(matches!(
        release_failure.kind(),
        flui_rendering::pipeline::ReleaseDetachedRenderSubtreesError::WrongOwner
    ));
    assert_eq!(
        twin_log.attach_count(),
        1,
        "the twin's node must still be detached"
    );

    owner
        .release_detached_render_subtrees_for_finalization(release_failure.into_token())
        .expect("the token releases through its own owner");
    twin_owner
        .release_detached_render_subtrees_for_finalization(twin_token)
        .expect("the twin's own token still releases");
}

#[test]
fn a_detached_subtree_that_gained_a_child_is_attachable_again_only_once_repaired() {
    let mut owner = PipelineOwner::new();
    let root = owner.insert(probe(LifecycleLog::default()));
    let child = owner.insert(probe(LifecycleLog::default()));
    owner.adopt_render_child(root, child);

    let token = owner.detach_render_subtrees(&[root]).expect("detach token");
    assert_eq!(token.node_count(), 2);

    let interloper = owner.insert(probe(LifecycleLog::default()));
    owner.adopt_render_child(root, interloper);

    let failure = owner
        .attach_render_subtrees(token)
        .expect_err("a grown subtree no longer matches the token");
    assert!(matches!(
        failure.kind(),
        flui_rendering::pipeline::AttachRenderSubtreesError::TopologyChanged
    ));

    let token = failure.into_token();
    assert_eq!(owner.remove_render_object(interloper), 1);
    owner
        .attach_render_subtrees(token)
        .expect("the repaired subtree matches the token again");
}

#[test]
fn reordering_a_detached_subtree_invalidates_the_token_even_at_the_same_node_count() {
    let mut owner = PipelineOwner::new();
    let root = owner.insert(probe(LifecycleLog::default()));
    let first = owner.insert(probe(LifecycleLog::default()));
    let second = owner.insert(probe(LifecycleLog::default()));
    owner.adopt_render_child(root, first);
    owner.adopt_render_child(root, second);

    let token = owner.detach_render_subtrees(&[root]).expect("detach token");
    assert_eq!(token.node_count(), 3);
    assert_eq!(token.root_count(), 1);

    // Park a child elsewhere and re-adopt it, which appends it after its
    // sibling. The node SET is unchanged and the count still matches, so only
    // the recorded per-node order can catch this.
    let bystander = owner.insert(probe(LifecycleLog::default()));
    owner.adopt_render_child(bystander, first);
    owner.adopt_render_child(root, first);
    assert_eq!(
        owner.render_tree().get(root).expect("root").children(),
        [second, first]
    );

    let failure = owner
        .attach_render_subtrees(token)
        .expect_err("a reordered subtree no longer matches the token");
    assert!(matches!(
        failure.kind(),
        flui_rendering::pipeline::AttachRenderSubtreesError::TopologyChanged
    ));

    // Restoring the recorded order makes the same token attach again.
    let token = failure.into_token();
    owner.adopt_render_child(bystander, second);
    owner.adopt_render_child(root, second);
    owner
        .attach_render_subtrees(token)
        .expect("the restored order matches the token again");
}

#[test]
fn relocation_failures_surface_their_kind_through_display_debug_and_source() {
    let mut owner = PipelineOwner::new();
    let mut foreign_owner = PipelineOwner::new();
    let render_id = owner.insert(probe(LifecycleLog::default()));
    let token = owner
        .detach_render_subtrees(&[render_id])
        .expect("detach token");
    assert!(
        format!("{token:?}").contains("node_count"),
        "the token's Debug redacts contents but must still report its shape"
    );

    let attach_failure = foreign_owner
        .attach_render_subtrees(token)
        .expect_err("foreign owner rejects");
    assert_eq!(
        attach_failure.to_string(),
        attach_failure.kind().to_string()
    );
    assert!(std::error::Error::source(&attach_failure).is_some());

    let release_failure = foreign_owner
        .release_detached_render_subtrees_for_finalization(attach_failure.into_token())
        .expect_err("foreign owner rejects the release too");
    assert_eq!(
        release_failure.to_string(),
        release_failure.kind().to_string()
    );
    assert!(std::error::Error::source(&release_failure).is_some());

    owner
        .release_detached_render_subtrees_for_finalization(release_failure.into_token())
        .expect("the owning pipeline still releases it");
}

#[derive(Debug, Clone)]
struct ParentDataDetachProbe(Arc<AtomicUsize>);

impl flui_rendering::parent_data::ParentData for ParentDataDetachProbe {
    fn detach(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn clearing_parent_data_runs_its_detach_hook_before_dropping_storage() {
    let mut owner = PipelineOwner::new();
    let id = owner.insert(probe(LifecycleLog::default()));
    let detach_count = Arc::new(AtomicUsize::new(0));
    let node = owner
        .render_tree_mut()
        .get_mut(id)
        .expect("inserted render node remains live");
    node.set_parent_data(Box::new(ParentDataDetachProbe(Arc::clone(&detach_count))));

    assert!(node.clear_parent_data());
    assert_eq!(detach_count.load(Ordering::SeqCst), 1);
    assert!(node.parent_data().is_none());
    assert!(
        !node.clear_parent_data(),
        "clearing None is an idempotent no-op"
    );
    assert_eq!(detach_count.load(Ordering::SeqCst), 1);
}

#[derive(Debug)]
struct OrderedDetachRender(Arc<Mutex<Vec<&'static str>>>);

impl flui_foundation::Diagnosticable for OrderedDetachRender {}

impl RenderBox for OrderedDetachRender {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, _ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
        Size::ZERO
    }

    fn detach(&mut self) {
        self.0.lock().expect("event log lock").push("render-detach");
    }
}

#[derive(Debug, Clone)]
struct OrderedDetachParentData(Arc<Mutex<Vec<&'static str>>>);

impl flui_rendering::parent_data::ParentData for OrderedDetachParentData {
    fn detach(&mut self) {
        self.0.lock().expect("event log lock").push("parent-data");
    }
}

#[test]
fn subtree_detach_drops_edge_and_parent_data_before_render_detach_hook() {
    let mut owner = PipelineOwner::new();
    let parent = owner.insert(probe(LifecycleLog::default()));
    let events = Arc::new(Mutex::new(Vec::new()));
    let child = owner
        .insert_child_render_object(
            parent,
            Box::new(OrderedDetachRender(Arc::clone(&events))) as BoxedRenderObject,
        )
        .expect("child insert");
    owner
        .render_tree_mut()
        .get_mut(child)
        .expect("child")
        .set_parent_data(Box::new(OrderedDetachParentData(Arc::clone(&events))));

    let _token = owner
        .detach_render_subtrees(&[child])
        .expect("child detach");
    assert_eq!(owner.render_tree().parent(child), None);
    assert!(
        owner
            .render_tree()
            .get(child)
            .unwrap()
            .parent_data()
            .is_none()
    );
    assert_eq!(
        events.lock().expect("event log lock").as_slice(),
        &["parent-data", "render-detach"],
    );
}

#[test]
fn permanent_subtree_removal_detaches_every_parent_data_before_each_render_hook() {
    let mut owner = PipelineOwner::new();
    let events = Arc::new(Mutex::new(Vec::new()));
    let root =
        owner.insert(Box::new(OrderedDetachRender(Arc::clone(&events))) as BoxedRenderObject);
    let child = owner
        .insert_child_render_object(
            root,
            Box::new(OrderedDetachRender(Arc::clone(&events))) as BoxedRenderObject,
        )
        .expect("child insert");
    for render_id in [root, child] {
        owner
            .render_tree_mut()
            .get_mut(render_id)
            .expect("subtree node")
            .set_parent_data(Box::new(OrderedDetachParentData(Arc::clone(&events))));
    }

    assert_eq!(owner.remove_render_object(root), 2);
    assert_eq!(
        events.lock().expect("event log lock").as_slice(),
        &[
            "parent-data",
            "render-detach",
            "parent-data",
            "render-detach",
        ],
    );
}

// ────────────────────────────────────────────────────────────────────────
// Generational staleness: a handle captured before removal goes silent
// ────────────────────────────────────────────────────────────────────────

#[test]
fn mark_needs_layout_on_a_handle_from_a_removed_node_is_a_silent_noop() {
    let (owner, id, log) = rooted_fixture();
    let owner = frame(owner);
    let handle = log.captured_handle();

    let mut owner = owner;
    owner.remove_render_object(id);
    owner.set_root_id(None);

    // The channel cannot know the node died — the send itself still
    // succeeds. The generational guarantee is that drain_pending_dirty
    // drops it silently rather than replaying it against a reused slot.
    handle
        .mark_needs_layout()
        .expect("channel send succeeds even for a dead generation; Ok, not an error");

    let before = log.layout_count();
    let owner = frame(owner);
    assert_eq!(
        log.layout_count(),
        before,
        "a request for a removed node's dead generation must be dropped at \
         drain, never replayed as a real layout"
    );
    drop(owner);
}

// ────────────────────────────────────────────────────────────────────────
// Reparent = remove + insert (no dedicated API in this codebase)
// ────────────────────────────────────────────────────────────────────────

#[test]
fn reparent_via_remove_then_insert_detaches_old_and_attaches_a_fresh_handle() {
    let mut owner = PipelineOwner::new();
    let log = LifecycleLog::default();

    let first_id = owner.insert(probe(log.clone()));
    assert_eq!(log.attach_count(), 1);
    let first_handle = log.captured_handle();
    assert_eq!(first_handle.id(), first_id);

    owner.remove_render_object(first_id);
    assert_eq!(
        log.detach_count(),
        1,
        "the old node's detach must fire before the new node is inserted"
    );

    let second_id = owner.insert(probe(log.clone()));
    assert_eq!(
        log.attach_count(),
        2,
        "reparent (remove + insert) must fire detach then attach with a fresh handle"
    );
    let second_handle = log.captured_handle();
    assert_eq!(
        second_handle.id(),
        second_id,
        "the fresh handle must be bound to the NEW node"
    );
    assert_ne!(
        second_handle.id(),
        first_handle.id(),
        "the fresh handle must not be bound to the stale node"
    );

    // The stale handle from the removed node stays a silent no-op — it
    // must never be confused for the new node's handle.
    first_handle
        .mark_needs_layout()
        .expect("stale handle send still succeeds; drain drops it silently");
}

// ────────────────────────────────────────────────────────────────────────
// Sliver-protocol coverage: no insertion path called `attach` for a Sliver
// child before this fix. `insert_child_render_object` was hard-coded to
// `BoxProtocol`, and `apply_deferred_mutation`'s lazy-child-building path
// (the `Insert` arm in `pipeline/owner/layout.rs`) called the raw
// `RenderTree::insert_sliver_child`/`insert_box_child` directly, bypassing
// `attach_inserted_node` for BOTH protocols. These tests exercise the two
// fixed call sites: the new `insert_sliver_child_render_object` (the
// Sliver-protocol counterpart of `insert_child_render_object`) and
// `apply_deferred_mutation`'s `Insert` arm for each `DeferredRenderObject`
// variant.
// ────────────────────────────────────────────────────────────────────────

#[test]
fn insert_sliver_child_render_object_fires_exactly_one_attach_with_a_handle_bound_to_the_new_id() {
    let mut owner = PipelineOwner::new();
    let root_id = owner.insert(probe(LifecycleLog::default()));

    let log = LifecycleLog::default();
    let child_id = owner
        .insert_sliver_child_render_object(root_id, sliver_probe(log.clone()))
        .expect("root_id was just inserted and is valid");

    assert_eq!(
        log.attach_count(),
        1,
        "insert_sliver_child_render_object must call attach exactly once"
    );
    let handle = log.captured_handle();
    assert_eq!(
        handle.id(),
        child_id,
        "the handed-over handle must be bound to the freshly-inserted sliver child"
    );
}

/// Mounts `parent_id` as a laid-out-ready root, ready to run a layout pass
/// that drains whatever gets deferred onto it.
fn rooted_layout_pipeline() -> (PipelineOwner, flui_foundation::RenderId) {
    let mut owner = PipelineOwner::new();
    let parent_id = owner.insert(probe(LifecycleLog::default()));
    owner.set_root_id(Some(parent_id));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(40.0), px(40.0)))));
    (owner, parent_id)
}

#[test]
fn deferred_sliver_insert_fires_attach_via_apply_deferred_mutation() {
    let (mut owner, parent_id) = rooted_layout_pipeline();

    let log = LifecycleLog::default();
    owner.defer_insert_sliver(parent_id, sliver_probe(log.clone()), None, None, None);

    let mut layout_owner = owner.into_layout();
    layout_owner.run_layout().expect(
        "layout must not error: parent_id is the root with constraints set, \
         and the deferred insert only needs parent_id to exist",
    );

    assert_eq!(
        log.attach_count(),
        1,
        "apply_deferred_mutation's DeferredRenderObject::Sliver arm \
         (pipeline/owner/layout.rs, the lazy-sliver-child-building path) \
         must call attach exactly once"
    );
}

#[test]
fn deferred_box_insert_fires_attach_via_apply_deferred_mutation() {
    // Collateral fix at the same call site: `apply_deferred_mutation`'s
    // `Insert` arm handles `DeferredRenderObject::Box` and `::Sliver`
    // through one shared code path that calls `attach_inserted_node` once
    // after either variant's tree insertion — so the Box side, which was
    // equally starved of `attach` before this fix, is proven here too.
    let (mut owner, parent_id) = rooted_layout_pipeline();

    let log = LifecycleLog::default();
    owner.defer_insert_box(parent_id, probe(log.clone()), None, None, None);

    let mut layout_owner = owner.into_layout();
    layout_owner
        .run_layout()
        .expect("layout must not error: parent_id is the root with constraints set");

    assert_eq!(
        log.attach_count(),
        1,
        "apply_deferred_mutation's DeferredRenderObject::Box arm must call \
         attach exactly once"
    );
}

#[test]
fn detaching_the_owner_root_vacates_the_root_slot_and_reattaching_restores_it() {
    let mut owner = PipelineOwner::new();
    let root = owner.insert(probe(LifecycleLog::default()));
    let child = owner.insert(probe(LifecycleLog::default()));
    owner.adopt_render_child(root, child);
    owner.set_root_id(Some(root));

    // The owner root has no parent edge, so nothing else clears the slot.
    // Leaving `root_id` on a closed attachment interval would let the paint
    // and semantics walks — which both start from `root_id` without checking
    // that interval — enter a detached object.
    let token = owner
        .detach_render_subtrees(&[root])
        .expect("the owner root detaches");
    assert_eq!(
        owner.root_id(),
        None,
        "detaching the owner root must vacate the root slot"
    );

    owner
        .attach_render_subtrees(token)
        .expect("the owner root reattaches");
    assert_eq!(
        owner.root_id(),
        Some(root),
        "reattaching the batch that vacated the slot must restore it"
    );
    assert_eq!(owner.render_tree().parent(child), Some(root));
}

#[test]
fn detaching_a_non_root_subtree_leaves_the_root_slot_alone() {
    let mut owner = PipelineOwner::new();
    let root = owner.insert(probe(LifecycleLog::default()));
    let child = owner.insert(probe(LifecycleLog::default()));
    owner.adopt_render_child(root, child);
    owner.set_root_id(Some(root));

    let token = owner
        .detach_render_subtrees(&[child])
        .expect("a non-root subtree detaches");
    assert_eq!(
        owner.root_id(),
        Some(root),
        "an unrelated detach must not disturb the root slot"
    );
    owner.attach_render_subtrees(token).expect("reattach");
    assert_eq!(owner.root_id(), Some(root));
}
