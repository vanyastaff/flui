//! End-to-end proof of the ADR-0040 tree-observation seam: a real tree
//! driven through the production `build_scope` path against
//! `flui-devtools`' `InspectorCounters` — the two halves the seam exists to
//! connect, linked here because this crate is the one that can see both.
//!
//! The oracle is the ADR §4 invariant: `ElementUnmounted` fires exactly
//! once per `Element::unmount`, `ElementMounted` exactly once per fresh
//! mint — so mounts and unmounts balance against live-tree size.

use std::sync::Arc;

use flui_devtools::inspector::InspectorCounters;
use flui_foundation::observe::TreeObserver;
use flui_foundation::{ElementId, ValueKey, ViewKey};
use flui_objects::RenderSizedBox;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_view::{BuildOwner, ElementTree, RebuildReason, RenderView, View, ViewExt};

#[derive(Clone)]
struct KeyedLeafBox {
    key: ValueKey<u32>,
}

impl KeyedLeafBox {
    fn new(tag: u32) -> Self {
        Self {
            key: ValueKey::new(tag),
        }
    }
}

impl RenderView for KeyedLeafBox {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderSizedBox::shrink()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }
}

impl View for KeyedLeafBox {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

#[derive(Clone)]
struct MultiBox {
    children: Vec<flui_view::BoxedView>,
}

impl MultiBox {
    fn keyed(order: &[u32]) -> Self {
        Self {
            children: order
                .iter()
                .copied()
                .map(|tag| KeyedLeafBox::new(tag).boxed())
                .collect(),
        }
    }
}

impl RenderView for MultiBox {
    type Protocol = flui_rendering::protocol::BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderSizedBox::shrink()
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        flui_rendering::RenderUpdateImpact::NONE
    }

    fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn View)) {
        for child in &self.children {
            visitor(child);
        }
    }
}

impl View for MultiBox {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}

fn live_children(tree: &ElementTree, parent: ElementId) -> Vec<ElementId> {
    let mut children: Vec<_> = tree
        .iter_nodes()
        .filter(|(_, node)| node.parent() == Some(parent))
        .map(|(id, node)| (node.slot(), id))
        .collect();
    children.sort_by_key(|(slot, _)| *slot);
    children.into_iter().map(|(_, id)| id).collect()
}

fn mount_root_with_pipeline(
    tree: &mut ElementTree,
    view: &dyn View,
    owner: &mut BuildOwner,
) -> ElementId {
    tree.mount_root_with_pipeline_owner(
        view,
        Some(PipelineCell::new(PipelineOwner::new())),
        &mut owner.element_owner_mut(),
    )
}

/// Drives mount → first builds → keyed reorder → shrink (eager unmounts) →
/// clear, asserting exact counts at each step. Any emission site silently
/// removed fails a specific assertion here (red→green verified during
/// development by disabling sites).
#[test]
fn inspector_counts_mounts_moves_rebuilds_and_unmounts_exactly() {
    let counters = Arc::new(InspectorCounters::new());
    let mut owner = BuildOwner::new();
    owner.set_tree_observer(Arc::clone(&counters) as Arc<dyn TreeObserver>);
    let mut tree = ElementTree::new();

    // Mount the root; its three keyed children mount during the first
    // build_scope (phase-2 reconcile of the root's build output).
    let root_id = mount_root_with_pipeline(&mut tree, &MultiBox::keyed(&[1, 2, 3]), &mut owner);
    let after_root = counters.snapshot();
    assert_eq!(
        after_root.mounts, 1,
        "root mint must emit exactly one mount"
    );
    assert_eq!(after_root.rebuilds, 0, "no build ran yet");

    owner.schedule_build_for(root_id, 0, RebuildReason::InitialMount);
    owner.build_scope(&mut tree);

    let after_first_build = counters.snapshot();
    assert_eq!(
        after_first_build.mounts, 4,
        "root + three children minted exactly once each",
    );
    assert!(
        after_first_build.rebuilds >= 1,
        "the root's first build must emit a rebuilt event",
    );
    assert_eq!(
        after_first_build.rebuilds_for(RebuildReason::InitialMount),
        after_first_build.rebuilds,
        "every completed build so far was a first build",
    );
    assert_eq!(after_first_build.moves, 0);
    assert_eq!(after_first_build.unmounts, 0);

    // Keyed rotation: ids follow keys, three reorder moves, nothing
    // mounts or unmounts.
    let before = live_children(&tree, root_id);
    assert_eq!(before.len(), 3);
    tree.update(
        root_id,
        &MultiBox::keyed(&[3, 1, 2]),
        &mut owner.element_owner_mut(),
    );
    owner.schedule_build_for(root_id, 0, RebuildReason::ParentUpdate);
    owner.build_scope(&mut tree);

    let after_reorder = counters.snapshot();
    assert_eq!(
        after_reorder.moves, 3,
        "a full keyed rotation moves every child exactly once",
    );
    assert_eq!(after_reorder.mounts, 4, "reorder must not mount");
    assert_eq!(after_reorder.unmounts, 0, "reorder must not unmount");
    assert!(
        after_reorder.rebuilds_for(RebuildReason::ParentUpdate) >= 1,
        "the reorder build must carry the ParentUpdate cause",
    );

    // Shrink to one child: two un-keyed... (keyed, but not GlobalKey-keyed)
    // children take the EAGER unmount path inside the reconcile — the
    // majority path the finalize-only design would have missed.
    tree.update(
        root_id,
        &MultiBox::keyed(&[3]),
        &mut owner.element_owner_mut(),
    );
    owner.schedule_build_for(root_id, 0, RebuildReason::ParentUpdate);
    owner.build_scope(&mut tree);
    owner.finalize_tree(&mut tree);

    let after_shrink = counters.snapshot();
    assert_eq!(
        after_shrink.unmounts, 2,
        "each removed child unmounts exactly once (eager inline path)",
    );

    // Balance invariant (ADR-0040 §4): mounts − unmounts == live tree size
    // once nothing is parked in the inactive queue.
    assert_eq!(
        after_shrink.mounts - after_shrink.unmounts,
        tree.len() as u64,
        "mounts and unmounts must balance against the live tree",
    );

    // Install/clear symmetry: the consumer gets its end-of-stream signal.
    owner.clear_tree_observer();
    assert!(
        counters.snapshot().is_final(),
        "clear_tree_observer must fire detached()",
    );
}

/// A mid-run attach must start from an exact structural baseline: the
/// seeded replay reports the live tree as synthetic mounts, and events
/// after the install continue the same stream.
#[test]
fn seeded_replay_gives_a_mid_run_attach_an_exact_baseline() {
    let mut owner = BuildOwner::new();
    let mut tree = ElementTree::new();

    // Build a 1+3 tree with NO observer installed.
    let root_id = mount_root_with_pipeline(&mut tree, &MultiBox::keyed(&[1, 2, 3]), &mut owner);
    owner.schedule_build_for(root_id, 0, RebuildReason::InitialMount);
    owner.build_scope(&mut tree);

    // Mid-run attach: replay-then-install (the WidgetsBinding helper does
    // exactly this under one write guard; here the single thread IS the
    // atomicity).
    let counters = Arc::new(InspectorCounters::new());
    tree.replay_mounts(&*counters);
    owner.set_tree_observer(Arc::clone(&counters) as Arc<dyn TreeObserver>);

    let baseline = counters.snapshot();
    assert_eq!(
        baseline.mounts,
        tree.len() as u64,
        "replay must report every live element exactly once",
    );
    assert_eq!(baseline.rebuilds, 0, "replay synthesizes mounts only");

    // Post-attach events continue the stream: shrink and observe unmounts.
    tree.update(
        root_id,
        &MultiBox::keyed(&[2]),
        &mut owner.element_owner_mut(),
    );
    owner.schedule_build_for(root_id, 0, RebuildReason::ParentUpdate);
    owner.build_scope(&mut tree);

    let after = counters.snapshot();
    assert_eq!(after.unmounts, 2, "post-attach unmounts arrive live");
    assert_eq!(
        after.mounts - after.unmounts,
        tree.len() as u64,
        "baseline + live stream keeps the balance invariant",
    );
}

/// A panicking observer is detached and the frame survives; `detached()`
/// is NOT called on the panicking observer.
#[test]
fn panicking_observer_is_detached_and_the_tree_survives() {
    struct PanicOnMount;
    impl TreeObserver for PanicOnMount {
        fn element_mounted(&self, _event: &flui_foundation::observe::ElementMounted) {
            panic!("observer bug");
        }
    }

    let mut owner = BuildOwner::new();
    owner.set_tree_observer(Arc::new(PanicOnMount));
    let mut tree = ElementTree::new();

    // The mount emission panics inside the observer; the emission helper
    // contains it, detaches the observer, and the mount itself completes.
    let root_id = mount_root_with_pipeline(&mut tree, &MultiBox::keyed(&[1]), &mut owner);
    assert!(tree.get(root_id).is_some(), "the mount itself must survive");
    assert!(
        owner.tree_observer().is_none(),
        "the panicking observer must be detached",
    );

    // The tree keeps working with no observer installed.
    owner.schedule_build_for(root_id, 0, RebuildReason::InitialMount);
    owner.build_scope(&mut tree);
    assert_eq!(tree.len(), 2, "root + one child built normally");
}
