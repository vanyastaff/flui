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

use std::sync::OnceLock;

use flui_foundation::{ElementId, ValueKey, ViewKey};
use flui_objects::RenderSizedBox;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{
    BuildOwner, ElementTree, RenderView, View, ViewExt,
    tree::ReconcileEventKind,
    tree::test_utils::{CollectedEvent, ReconcileEventCollector},
};
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

    fn create_render_object(&self) -> Self::RenderObject {
        RenderSizedBox::shrink()
    }

    fn update_render_object(&self, _render_object: &mut Self::RenderObject) {}
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
    type Protocol = BoxProtocol;
    type RenderObject = RenderSizedBox;

    fn create_render_object(&self) -> Self::RenderObject {
        RenderSizedBox::shrink()
    }

    fn update_render_object(&self, _render_object: &mut Self::RenderObject) {}

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

fn direct_children_in_slot_order(tree: &ElementTree, parent: ElementId) -> Vec<ElementId> {
    let mut children: Vec<_> = tree
        .iter_nodes()
        .filter(|(_, node)| node.parent() == Some(parent))
        .map(|(id, node)| (node.slot(), id))
        .collect();
    children.sort_by_key(|(slot, _)| *slot);
    children.into_iter().map(|(_, id)| id).collect()
}

/// Locks the §U15 contract: `ElementTree::insert` /
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
    // (per the §U15 wiring in element_tree.rs:223).
    let root_view = KeyedLeafBox::new(1);
    let root_id = tree.mount_root(&root_view, &mut owner.element_owner_mut());
    assert_eq!(root_id, ElementId::new(1), "root must occupy slab[0]");

    // Insert a child — exercises ElementTree::insert which also
    // calls set_self_id before mount.
    let child_view = KeyedLeafBox::new(2);
    let child_id = tree.insert(&child_view, root_id, 0, &mut owner.element_owner_mut());
    assert_eq!(child_id, ElementId::new(2), "child must occupy slab[1]");

    // If §U15's debug_assert ever fires (perform_build before
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
