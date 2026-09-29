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
#![expect(
    clippy::unwrap_used,
    reason = "a panic is the failure report in a test scenario"
)]

use std::any::TypeId;

use flui_foundation::{ElementId, ValueKey, ViewKey};
use flui_objects::RenderSizedBox;
use flui_rendering::pipeline::{PipelineCell, PipelineOwner};
use flui_rendering::protocol::BoxProtocol;
use flui_view::{
    BoxedView, BuildOwner, ElementTree, ErrorView, GlobalKey, RenderView, View, ViewExt,
    tree::ReconcileEventKind,
};
use serial_test::serial;

use crate::dense_reconcile_containment::DensePanicsOnCreate;
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

#[serial]
pub(crate) fn active_global_key_move_through_build_scope_updates_render_parent_links() {
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

#[serial]
pub(crate) fn failed_dense_mount_production_reconcile_emits_only_final_slots() {
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
