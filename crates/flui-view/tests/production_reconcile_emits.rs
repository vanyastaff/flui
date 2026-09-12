//! Production-path keyed reconciliation locks.
//!
//! These tests exercise the public `ElementTree` + `BuildOwner::build_scope`
//! path, not a direct call to `tree::id_reconcile::reconcile_children_by_id`.
//! The production chain is:
//!
//! `mount_root/update` → `BuildOwner::build_scope` → `ElementBase::build_into_views`
//! → `tree::id_reconcile::reconcile_children_by_id`.
//!
//! # What this test locks
//!
//! - `set_self_id` is still stamped before mount, so the mounted element can
//!   later rebuild through the production path without losing its own
//!   `ElementId`.
//! - A real variable-arity render view can reorder keyed children through
//!   `build_scope`; element ids follow keys, no remount occurs, and the
//!   `flui::reconcile` trace stream reports the real parent id.

#![cfg(feature = "test-utils")]

use std::{any::TypeId, cell::Cell, rc::Rc};

use flui_foundation::{ElementId, ValueKey, ViewKey};
use flui_objects::RenderSizedBox;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_rendering::protocol::BoxProtocol;
use flui_view::{
    BoxedView, BuildOwner, ElementTree, ErrorView, GlobalKey, RenderView, View, ViewExt,
    tree::ReconcileEventKind,
};
use serial_test::serial;

use crate::dense_reconcile_containment::{
    DENSE_CHILD_COUNT, DenseGlobalKeyUpdatePanicSubtree, DensePanicsOnCreate, DenseRetakeRoot,
    DenseRetakeRootState, DenseRow, mount_dense_root,
};
use crate::dense_update_containment::{
    DenseDidUpdateView, DenseRenderUpdateLeaf, PHASE_ONE_FAILED_SLOT, keyed_render_children,
    phase_one_children, update_dense_parent,
};
use crate::reconcile_capture::capture;

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
    type Protocol = BoxProtocol;
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
struct GlobalLeafBox {
    key: GlobalKey<()>,
}

impl GlobalLeafBox {
    fn new(key: GlobalKey<()>) -> Self {
        Self { key }
    }
}

impl RenderView for GlobalLeafBox {
    type Protocol = BoxProtocol;
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

impl View for GlobalLeafBox {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&self.key)
    }
}

#[derive(Clone)]
struct MultiBox {
    key: Option<ValueKey<u32>>,
    children: Vec<flui_view::BoxedView>,
}

impl MultiBox {
    fn keyed(order: &[u32]) -> Self {
        Self {
            key: None,
            children: order
                .iter()
                .copied()
                .map(|tag| KeyedLeafBox::new(tag).boxed())
                .collect(),
        }
    }

    fn host(tag: u32, children: Vec<flui_view::BoxedView>) -> Self {
        Self {
            key: Some(ValueKey::new(tag)),
            children,
        }
    }
}

impl RenderView for MultiBox {
    type Protocol = BoxProtocol;
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

    fn key(&self) -> Option<&dyn ViewKey> {
        self.key.as_ref().map(|key| key as &dyn ViewKey)
    }
}

fn direct_children_in_slot_order(tree: &ElementTree, parent: ElementId) -> Vec<ElementId> {
    let mut children: Vec<_> = tree
        .iter_nodes()
        .filter(|(_, node)| node.parent() == Some(parent))
        .map(|(id, node)| (node.slot(), id))
        .collect();
    children.sort_by_key(|(slot, _)| *slot);
    children.into_iter().map(|(_, id)| id).collect()
}

/// Locks the contract: `ElementTree::insert` /
/// `mount_root_with_pipeline_owner` MUST stamp `set_self_id` BEFORE
/// `mount`. The production variable-arity test below proves the stamp is
/// then visible when `BuildOwner::build_scope` emits reconciliation events
/// for that element's children. This smaller smoke lock keeps the raw mount
/// and insert paths honest.
#[test]
fn set_self_id_fires_on_insert_no_panic() {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();

    // Mount root. ElementTree::mount_root_with_pipeline_owner calls
    // `element.set_self_id(id)` immediately after slab insertion
    // (per the wiring in element_tree.rs:223).
    let root_view = KeyedLeafBox::new(1);
    let root_id = tree.mount_root(&root_view, &mut owner.element_owner_mut());
    assert_eq!(root_id, ElementId::new(1), "root must occupy slab[0]");

    // Insert a child — exercises ElementTree::insert which also
    // calls set_self_id before mount.
    let child_view = KeyedLeafBox::new(2);
    let child_id = tree.insert(&child_view, root_id, 0, &mut owner.element_owner_mut());
    assert_eq!(child_id, ElementId::new(2), "child must occupy slab[1]");

    // If the debug_assert ever fires (perform_build before
    // set_self_id), THIS TEST would panic. The fact that mount +
    // insert returned cleanly is the lock that the assert's
    // precondition (set_self_id called before lifecycle work observes
    // the element id) holds for the production mount path in debug builds.
}

#[test]
#[serial]
fn variable_arity_reorder_through_build_scope_preserves_ids_and_emits_parent_id() {
    let pipeline_owner = PipelineCell::new(PipelineOwner::new());
    let mut owner = BuildOwner::new();
    let mut tree = ElementTree::new();

    let root_v1 = MultiBox::keyed(&[1, 2, 3]);
    let root_id = tree.mount_root_with_pipeline_owner(
        &root_v1,
        Some(pipeline_owner),
        &mut owner.element_owner_mut(),
    );

    owner.schedule_build_for(root_id, 0, flui_view::RebuildReason::InitialMount);
    owner.build_scope(&mut tree);
    let before = direct_children_in_slot_order(&tree, root_id);
    assert_eq!(before.len(), 3);
    let (id1, id2, id3) = (before[0], before[1], before[2]);

    let root_v2 = MultiBox::keyed(&[3, 1, 2]);
    tree.update(root_id, &root_v2, &mut owner.element_owner_mut());
    owner.schedule_build_for(root_id, 0, flui_view::RebuildReason::ParentUpdate);

    let events = capture(|| {
        owner.build_scope(&mut tree);
    });

    let after = direct_children_in_slot_order(&tree, root_id);
    assert_eq!(
        after,
        vec![id3, id1, id2],
        "build_scope must route variable children through keyed reconcile; ids follow keys",
    );
    assert_eq!(
        tree.len(),
        4,
        "root plus three children: reorder must not mount or unmount",
    );

    let dispositions: Vec<_> = events.iter().map(|event| event.kind).collect();
    assert_eq!(
        dispositions,
        vec![
            ReconcileEventKind::Reorder,
            ReconcileEventKind::Reorder,
            ReconcileEventKind::Reorder,
        ],
        "a full keyed rotation reports movement for every child; got {events:?}",
    );
    for event in &events {
        assert_eq!(
            event.parent,
            root_id.as_u64(),
            "production trace event must carry the rebuilding variable parent id",
        );
    }
}

#[test]
#[serial]
fn active_global_key_move_through_build_scope_updates_render_parent_links() {
    let pipeline_owner = PipelineCell::new(PipelineOwner::new());
    let mut owner = BuildOwner::new();
    let mut tree = ElementTree::new();

    let global = GlobalKey::<()>::new();
    let root_v1 = MultiBox::host(
        0,
        vec![
            MultiBox::host(1, vec![GlobalLeafBox::new(global.clone()).boxed()]).boxed(),
            MultiBox::host(2, Vec::new()).boxed(),
        ],
    );
    let root_id = tree.mount_root_with_pipeline_owner(
        &root_v1,
        Some(pipeline_owner.clone()),
        &mut owner.element_owner_mut(),
    );

    owner.schedule_build_for(root_id, 0, flui_view::RebuildReason::InitialMount);
    owner.build_scope(&mut tree);

    let parents = direct_children_in_slot_order(&tree, root_id);
    assert_eq!(parents.len(), 2);
    let (parent_a, parent_b) = (parents[0], parents[1]);
    let moved_before = direct_children_in_slot_order(&tree, parent_a);
    assert_eq!(moved_before.len(), 1);
    let moved_id = moved_before[0];

    let parent_a_render = tree
        .get(parent_a)
        .and_then(|node| node.element().render_id())
        .expect("parent A has a render object");
    let parent_b_render = tree
        .get(parent_b)
        .and_then(|node| node.element().render_id())
        .expect("parent B has a render object");
    let moved_render = tree
        .get(moved_id)
        .and_then(|node| node.element().render_id())
        .expect("moved child has a render object");
    let root_render = tree
        .get(root_id)
        .and_then(|node| node.element().render_id())
        .expect("root has a render object");

    pipeline_owner.with(|pipeline| {
        let render_tree = pipeline.render_tree();
        assert_eq!(
            render_tree.get(parent_a_render).unwrap().children(),
            &[moved_render],
            "precondition: parent A owns the moved render child before reparent",
        );
    });

    pipeline_owner.with_mut(|pipeline| {
        pipeline.set_semantics_enabled(true);
        pipeline.clear_all_dirty_nodes();
        for render_id in [root_render, parent_a_render, parent_b_render, moved_render] {
            let node = pipeline
                .render_tree()
                .get(render_id)
                .expect("mounted render ID must resolve before the move");
            node.clear_needs_layout();
            node.clear_needs_paint();
            node.clear_needs_compositing_bits_update();
        }
    });

    let parent_b_v2 = MultiBox::host(2, vec![GlobalLeafBox::new(global.clone()).boxed()]);
    tree.update(parent_b, &parent_b_v2, &mut owner.element_owner_mut());
    owner.schedule_build_for(
        parent_b,
        tree.get(parent_b).unwrap().depth(),
        flui_view::RebuildReason::ParentUpdate,
    );

    let events = capture(|| {
        owner.build_scope(&mut tree);
    });

    assert!(
        direct_children_in_slot_order(&tree, parent_a).is_empty(),
        "old parent must forget the moved active GlobalKey child",
    );
    assert_eq!(
        direct_children_in_slot_order(&tree, parent_b),
        vec![moved_id],
        "new parent must own the moved GlobalKey child",
    );

    let reparent_events: Vec<_> = events
        .iter()
        .filter(|event| event.kind == ReconcileEventKind::Reparent)
        .collect();
    assert_eq!(
        reparent_events.len(),
        1,
        "exactly one active Reparent event expected; got {events:?}",
    );
    assert_eq!(reparent_events[0].parent, parent_b.as_u64());
    assert_eq!(reparent_events[0].from_parent, Some(parent_a.as_u64()));

    pipeline_owner.with(|pipeline| {
        let render_tree = pipeline.render_tree();
        assert!(
            !render_tree
                .get(parent_a_render)
                .unwrap()
                .children()
                .contains(&moved_render),
            "old render parent must no longer list the moved render child",
        );
        assert_eq!(
            render_tree.get(parent_b_render).unwrap().children(),
            &[moved_render],
            "new render parent must list the moved render child",
        );
        assert_eq!(
            render_tree.get(moved_render).unwrap().parent(),
            Some(parent_b_render),
            "moved render child parent pointer must follow the element reparent",
        );

        assert!(render_tree.get(parent_a_render).unwrap().needs_layout());
        assert!(render_tree.get(parent_b_render).unwrap().needs_layout());
        assert!(
            render_tree
                .get(parent_a_render)
                .unwrap()
                .needs_compositing_bits_update()
        );
        assert!(
            render_tree
                .get(parent_b_render)
                .unwrap()
                .needs_compositing_bits_update()
        );
        assert_eq!(
            pipeline
                .nodes_needing_layout()
                .iter()
                .map(|dirty| dirty.id)
                .collect::<Vec<_>>(),
            vec![root_render],
            "both changed parents propagate layout to their shared relayout boundary",
        );
        assert_eq!(
            pipeline
                .nodes_needing_compositing_bits_update()
                .iter()
                .map(|dirty| dirty.id)
                .collect::<Vec<_>>(),
            vec![root_render],
            "both membership changes share the root compositing walk",
        );
        let semantics_ids = pipeline
            .nodes_needing_semantics()
            .iter()
            .map(|dirty| dirty.id)
            .collect::<Vec<_>>();
        assert_eq!(semantics_ids.len(), 3, "{semantics_ids:?}");
        assert!(semantics_ids.contains(&parent_a_render));
        assert!(semantics_ids.contains(&parent_b_render));
        assert!(semantics_ids.contains(&moved_render));
        assert_eq!(
            pipeline
                .nodes_needing_paint()
                .iter()
                .map(|dirty| dirty.id)
                .collect::<Vec<_>>(),
            vec![root_render],
            "a fresh attachment interval canonically repaints through its boundary",
        );
    });
}

#[test]
#[serial]
fn inactive_global_key_reinsert_applies_full_new_parent_membership_impact() {
    let pipeline_owner = PipelineCell::new(PipelineOwner::new());
    let mut owner = BuildOwner::new();
    let mut tree = ElementTree::new();
    let global = GlobalKey::<()>::new();
    let root = MultiBox::host(
        0,
        vec![
            MultiBox::host(1, vec![GlobalLeafBox::new(global.clone()).boxed()]).boxed(),
            MultiBox::host(2, Vec::new()).boxed(),
        ],
    );
    let root_id = tree.mount_root_with_pipeline_owner(
        &root,
        Some(pipeline_owner.clone()),
        &mut owner.element_owner_mut(),
    );
    owner.schedule_build_for(root_id, 0, flui_view::RebuildReason::InitialMount);
    owner.build_scope(&mut tree);

    let parents = direct_children_in_slot_order(&tree, root_id);
    let (parent_a, parent_b) = (parents[0], parents[1]);
    let moved_id = direct_children_in_slot_order(&tree, parent_a)[0];
    let root_render = tree.get(root_id).unwrap().element().render_id().unwrap();
    let parent_a_render = tree.get(parent_a).unwrap().element().render_id().unwrap();
    let parent_b_render = tree.get(parent_b).unwrap().element().render_id().unwrap();
    let moved_render = tree.get(moved_id).unwrap().element().render_id().unwrap();

    pipeline_owner.with_mut(|pipeline| pipeline.set_semantics_enabled(true));

    let empty_parent_a = MultiBox::host(1, Vec::new());
    tree.update(parent_a, &empty_parent_a, &mut owner.element_owner_mut());
    owner.schedule_build_for(
        parent_a,
        tree.get(parent_a).unwrap().depth(),
        flui_view::RebuildReason::ParentUpdate,
    );
    owner.build_scope(&mut tree);
    assert!(
        tree.get(moved_id).is_some(),
        "the keyed element remains available for same-frame inactive retake"
    );
    pipeline_owner.with(|pipeline| {
        let render_tree = pipeline.render_tree();
        assert!(render_tree.get(parent_a_render).unwrap().needs_layout());
        assert!(
            render_tree
                .get(parent_a_render)
                .unwrap()
                .needs_compositing_bits_update()
        );
        assert!(
            pipeline
                .nodes_needing_semantics()
                .iter()
                .any(|dirty| dirty.id == parent_a_render)
        );
    });

    pipeline_owner.with_mut(|pipeline| {
        pipeline.clear_all_dirty_nodes();
        for render_id in [root_render, parent_a_render, parent_b_render, moved_render] {
            let node = pipeline.render_tree().get(render_id).unwrap();
            node.clear_needs_layout();
            node.clear_needs_paint();
            node.clear_needs_compositing_bits_update();
        }
    });

    let parent_b_with_child = MultiBox::host(2, vec![GlobalLeafBox::new(global.clone()).boxed()]);
    tree.update(
        parent_b,
        &parent_b_with_child,
        &mut owner.element_owner_mut(),
    );
    owner.schedule_build_for(
        parent_b,
        tree.get(parent_b).unwrap().depth(),
        flui_view::RebuildReason::ParentUpdate,
    );
    owner.build_scope(&mut tree);

    assert_eq!(
        direct_children_in_slot_order(&tree, parent_b),
        vec![moved_id]
    );
    pipeline_owner.with(|pipeline| {
        let render_tree = pipeline.render_tree();
        assert_eq!(
            render_tree.get(moved_render).unwrap().parent(),
            Some(parent_b_render)
        );
        assert!(render_tree.get(parent_b_render).unwrap().needs_layout());
        assert!(
            render_tree
                .get(parent_b_render)
                .unwrap()
                .needs_compositing_bits_update()
        );
        assert_eq!(pipeline.nodes_needing_layout()[0].id, root_render);
        assert_eq!(
            pipeline.nodes_needing_compositing_bits_update()[0].id,
            root_render
        );
        let semantics_ids = pipeline
            .nodes_needing_semantics()
            .iter()
            .map(|dirty| dirty.id)
            .collect::<Vec<_>>();
        assert_eq!(semantics_ids.len(), 2, "{semantics_ids:?}");
        assert!(semantics_ids.contains(&parent_b_render));
        assert!(semantics_ids.contains(&moved_render));
        assert_eq!(
            pipeline
                .nodes_needing_paint()
                .iter()
                .map(|dirty| dirty.id)
                .collect::<Vec<_>>(),
            vec![root_render],
        );
    });
}

const FAILED_SLOT: usize = 7;
const DESTINATION_CHILD_COUNT: usize = 10;

fn dense_stream_children(failed: BoxedView) -> Vec<BoxedView> {
    (0..DESTINATION_CHILD_COUNT)
        .map(|slot| {
            if slot == FAILED_SLOT {
                failed.clone()
            } else {
                KeyedLeafBox::new(slot as u32).boxed()
            }
        })
        .collect()
}

fn expected_destination_mounts() -> Vec<(ReconcileEventKind, u64, String)> {
    (0..DESTINATION_CHILD_COUNT)
        .map(|slot| {
            let view_type_id = if slot == FAILED_SLOT {
                TypeId::of::<ErrorView>()
            } else {
                TypeId::of::<KeyedLeafBox>()
            };
            (
                ReconcileEventKind::Mount,
                slot as u64,
                format!("{view_type_id:?}"),
            )
        })
        .collect()
}

fn assert_destination_emits_only_final_mounts(
    events: &[flui_view::tree::test_utils::CollectedEvent],
    destination: ElementId,
    failed_view_type_id: TypeId,
) {
    let destination_events: Vec<_> = events
        .iter()
        .filter(|event| event.parent == destination.as_u64())
        .map(|event| (event.kind, event.slot, event.view_type_id.clone()))
        .collect();
    assert_eq!(
        destination_events,
        expected_destination_mounts(),
        "the destination stream must describe the ten final slots in order"
    );
    assert!(
        events.iter().all(|event| {
            event.parent != destination.as_u64() || event.kind != ReconcileEventKind::Reparent
        }),
        "a failed retake never emits a successful Reparent"
    );
    assert!(
        events.iter().all(|event| {
            event.parent != destination.as_u64()
                || event.view_type_id != format!("{failed_view_type_id:?}")
        }),
        "the panicking view has no successful disposition in the destination stream"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| {
                event.parent == destination.as_u64()
                    && event.kind == ReconcileEventKind::Mount
                    && event.slot == FAILED_SLOT as u64
                    && event.view_type_id == format!("{:?}", TypeId::of::<ErrorView>())
            })
            .count(),
        1,
        "the failed destination slot emits exactly one substitute Mount"
    );
}

fn mount_stream_retake_hosts(
    child_under_a: BoxedView,
) -> (ElementTree, BuildOwner, ElementId, ElementId) {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let pipeline = PipelineCell::new(PipelineOwner::new());
    let root = MultiBox::host(
        0,
        vec![
            MultiBox::host(1, vec![child_under_a]).boxed(),
            MultiBox::host(2, Vec::new()).boxed(),
        ],
    );
    let root_id =
        tree.mount_root_with_pipeline_owner(&root, Some(pipeline), &mut owner.element_owner_mut());
    owner.schedule_build_for(root_id, 0, flui_view::RebuildReason::InitialMount);
    owner.build_scope(&mut tree);
    let hosts = direct_children_in_slot_order(&tree, root_id);
    assert_eq!(hosts.len(), 2);
    (tree, owner, hosts[0], hosts[1])
}

fn soft_remove_stream_source(tree: &mut ElementTree, owner: &mut BuildOwner, source: ElementId) {
    tree.update(
        source,
        &MultiBox::host(1, Vec::new()),
        &mut owner.element_owner_mut(),
    );
    owner.schedule_build_for(
        source,
        tree.get(source).expect("source host").depth(),
        flui_view::RebuildReason::ParentUpdate,
    );
    owner.build_scope(tree);
}

fn capture_dense_stream_destination(
    tree: &mut ElementTree,
    owner: &mut BuildOwner,
    destination: ElementId,
    child: BoxedView,
) -> Vec<flui_view::tree::test_utils::CollectedEvent> {
    tree.update(
        destination,
        &MultiBox::host(2, dense_stream_children(child)),
        &mut owner.element_owner_mut(),
    );
    owner.schedule_build_for(
        destination,
        tree.get(destination).expect("destination host").depth(),
        flui_view::RebuildReason::ParentUpdate,
    );
    capture(|| owner.build_scope(tree))
}

#[test]
#[serial]
fn failed_dense_mount_production_reconcile_emits_only_final_slots() {
    let mut tree = ElementTree::new();
    let mut owner = BuildOwner::new();
    let pipeline = PipelineCell::new(PipelineOwner::new());
    let root = MultiBox::host(
        0,
        dense_stream_children(DensePanicsOnCreate::ordinary().boxed()),
    );
    let root_id =
        tree.mount_root_with_pipeline_owner(&root, Some(pipeline), &mut owner.element_owner_mut());
    owner.schedule_build_for(root_id, 0, flui_view::RebuildReason::InitialMount);

    let events = capture(|| owner.build_scope(&mut tree));

    assert_destination_emits_only_final_mounts(
        &events,
        root_id,
        TypeId::of::<DensePanicsOnCreate>(),
    );
}

#[test]
#[serial]
fn failed_update_retake_production_reconcile_emits_substitute_without_reparent() {
    let armed = Rc::new(Cell::new(false));
    let retaken = DenseGlobalKeyUpdatePanicSubtree::new(GlobalKey::new(), armed.clone());
    let (mut tree, mut owner, source, destination) =
        mount_stream_retake_hosts(retaken.clone().boxed());
    soft_remove_stream_source(&mut tree, &mut owner, source);
    armed.set(true);

    let events =
        capture_dense_stream_destination(&mut tree, &mut owner, destination, retaken.boxed());

    assert_destination_emits_only_final_mounts(
        &events,
        destination,
        TypeId::of::<DenseGlobalKeyUpdatePanicSubtree>(),
    );
}

#[test]
#[serial]
fn failed_activate_retake_production_reconcile_emits_substitute_without_reparent() {
    let descendant_armed = Rc::new(Cell::new(false));
    let retaken = DenseRetakeRoot::new(
        GlobalKey::<DenseRetakeRootState>::new(),
        descendant_armed.clone(),
    );
    let (mut tree, mut owner, source, destination) =
        mount_stream_retake_hosts(retaken.clone().boxed());
    soft_remove_stream_source(&mut tree, &mut owner, source);
    descendant_armed.set(true);

    let events =
        capture_dense_stream_destination(&mut tree, &mut owner, destination, retaken.boxed());

    assert_destination_emits_only_final_mounts(
        &events,
        destination,
        TypeId::of::<DenseRetakeRoot>(),
    );
}

fn assert_failed_update_emits_only_final_slot(
    events: &[flui_view::tree::test_utils::CollectedEvent],
    parent: ElementId,
    slot: usize,
    failed_view_type_id: TypeId,
    failed_key_hash: u64,
) {
    let parent_events: Vec<_> = events
        .iter()
        .filter(|event| event.parent == parent.as_u64())
        .collect();
    assert!(
        !parent_events.is_empty(),
        "the production collector must observe the tested parent's reconcile stream"
    );
    let error_mounts: Vec<_> = parent_events
        .iter()
        .filter(|event| {
            event.kind == ReconcileEventKind::Mount
                && event.view_type_id == format!("{:?}", TypeId::of::<ErrorView>())
        })
        .map(|event| (event.slot, event.child_key))
        .collect();
    assert_eq!(
        error_mounts,
        vec![(slot as u64, None)],
        "the parent's complete ErrorView Mount set must be exactly one unkeyed substitute at the final slot"
    );
    assert!(
        parent_events.iter().all(|event| {
            !(event.child_key == Some(failed_key_hash)
                && event.view_type_id == format!("{failed_view_type_id:?}")
                && matches!(
                    event.kind,
                    ReconcileEventKind::Reuse | ReconcileEventKind::Reorder
                ))
        }),
        "the failed resident must not leave a stale Reuse or Reorder disposition"
    );
    assert!(
        parent_events.iter().any(|event| {
            event.slot != slot as u64
                && matches!(
                    event.kind,
                    ReconcileEventKind::Reuse
                        | ReconcileEventKind::Reorder
                        | ReconcileEventKind::Mount
                )
        }),
        "the parent-filtered stream must contain a positive sibling disposition"
    );
}

#[test]
#[serial]
fn failed_phase_one_update_emits_substitute_without_stale_reuse() {
    let armed = Rc::new(Cell::new(false));
    let (mut tree, mut owner, _pipeline, _observer, parent) = mount_dense_root(DenseRow {
        children: phase_one_children(
            PHASE_ONE_FAILED_SLOT,
            DenseDidUpdateView::new(1, armed.clone()).boxed(),
        ),
    });
    owner.build_scope(&mut tree);
    armed.set(true);

    let events = capture(|| {
        update_dense_parent(
            &mut tree,
            &mut owner,
            parent,
            phase_one_children(
                PHASE_ONE_FAILED_SLOT,
                DenseDidUpdateView::new(1, armed).boxed(),
            ),
        );
    });

    assert_failed_update_emits_only_final_slot(
        &events,
        parent,
        PHASE_ONE_FAILED_SLOT,
        TypeId::of::<DenseDidUpdateView>(),
        ValueKey::new(1_u32).key_hash(),
    );
}

#[test]
#[serial]
fn failed_phase_four_update_emits_substitute_without_stale_reorder() {
    let armed = Rc::new(Cell::new(false));
    let old_keys: Vec<_> = (0..DENSE_CHILD_COUNT as u32).collect();
    let (mut tree, mut owner, _pipeline, _observer, parent) = mount_dense_root(DenseRow {
        children: keyed_render_children(&old_keys, None, armed.clone()),
    });
    owner.build_scope(&mut tree);
    armed.set(true);
    let new_keys = [100, 3, 0, 1, 2, 4, 5, 6, 101, 9];

    let events = capture(|| {
        update_dense_parent(
            &mut tree,
            &mut owner,
            parent,
            keyed_render_children(&new_keys, Some(3), armed),
        );
    });

    assert_failed_update_emits_only_final_slot(
        &events,
        parent,
        1,
        TypeId::of::<DenseRenderUpdateLeaf>(),
        ValueKey::new(3_u32).key_hash(),
    );
}

#[test]
#[serial]
fn failed_phase_five_a_update_emits_substitute_without_stale_reorder() {
    let armed = Rc::new(Cell::new(false));
    let old_keys: Vec<_> = (0..DENSE_CHILD_COUNT as u32).collect();
    let (mut tree, mut owner, _pipeline, _observer, parent) = mount_dense_root(DenseRow {
        children: keyed_render_children(&old_keys, None, armed.clone()),
    });
    owner.build_scope(&mut tree);
    armed.set(true);
    let new_keys = [0, 1, 2, 3, 4, 5, 6, 7, 8, 100, 9];

    let events = capture(|| {
        update_dense_parent(
            &mut tree,
            &mut owner,
            parent,
            keyed_render_children(&new_keys, Some(9), armed),
        );
    });

    assert_failed_update_emits_only_final_slot(
        &events,
        parent,
        10,
        TypeId::of::<DenseRenderUpdateLeaf>(),
        ValueKey::new(9_u32).key_hash(),
    );
}
