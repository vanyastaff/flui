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

use std::sync::{Arc, OnceLock};

use flui_foundation::{ElementId, ValueKey, ViewKey};
use flui_objects::RenderSizedBox;
use flui_rendering::pipeline::PipelineOwner;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{
    BuildOwner, ElementTree, GlobalKey, RenderView, View, ViewExt,
    tree::ReconcileEventKind,
    tree::test_utils::{CollectedEvent, ReconcileEventCollector},
};
use parking_lot::RwLock;
use serial_test::serial;
use tracing::dispatcher::Dispatch;
use tracing_subscriber::Registry;
use tracing_subscriber::layer::SubscriberExt;

static GLOBAL_SUBSCRIBER: OnceLock<()> = OnceLock::new();

fn ensure_global_subscriber() {
    GLOBAL_SUBSCRIBER.get_or_init(|| {
        let _ = tracing::subscriber::set_global_default(Registry::default());
    });
}

fn capture<F: FnOnce()>(body: F) -> Vec<CollectedEvent> {
    ensure_global_subscriber();
    let collector = ReconcileEventCollector::new();
    let subscriber = Registry::default().with(collector.layer());
    tracing::dispatcher::with_default(&Dispatch::new(subscriber), body);
    collector.events()
}

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
    ) {
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
    ) {
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
    ) {
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
    let mut owner = BuildOwner::new();
    let mut tree = ElementTree::new();

    let root_v1 = MultiBox::keyed(&[1, 2, 3]);
    let root_id = tree.mount_root(&root_v1, &mut owner.element_owner_mut());

    owner.schedule_build_for(root_id, 0);
    owner.build_scope(&mut tree);
    let before = direct_children_in_slot_order(&tree, root_id);
    assert_eq!(before.len(), 3);
    let (id1, id2, id3) = (before[0], before[1], before[2]);

    let root_v2 = MultiBox::keyed(&[3, 1, 2]);
    tree.update(root_id, &root_v2, &mut owner.element_owner_mut());
    owner.schedule_build_for(root_id, 0);

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
    let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));
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
        Some(Arc::clone(&pipeline_owner)),
        &mut owner.element_owner_mut(),
    );

    owner.schedule_build_for(root_id, 0);
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

    {
        let pipeline = pipeline_owner.read();
        let render_tree = pipeline.render_tree();
        assert_eq!(
            render_tree.get(parent_a_render).unwrap().children(),
            &[moved_render],
            "precondition: parent A owns the moved render child before reparent",
        );
    }

    let parent_b_v2 = MultiBox::host(2, vec![GlobalLeafBox::new(global.clone()).boxed()]);
    tree.update(parent_b, &parent_b_v2, &mut owner.element_owner_mut());
    owner.schedule_build_for(parent_b, tree.get(parent_b).unwrap().depth());

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

    let pipeline = pipeline_owner.read();
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
}
