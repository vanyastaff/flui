//! In-tree test utilities for the reconciler.
//!
//! FR-035. Public only under `cfg(any(test, feature =
//! "test-utils"))` so downstream test crates (notably the
//! 6-permutation corpus and the GlobalKey reparenting tests)
//! can re-use the [`ReconcileEventCollector`] without duplicating
//! the `Visit` machinery, while production builds keep the module
//! gone.

pub mod reconcile_event_collector;

pub use reconcile_event_collector::{CollectedEvent, ReconcileEventCollector};

/// Drives the production slab reconciler from integration tests.
pub fn reconcile_children(
    tree: &mut super::ElementTree,
    parent: flui_foundation::ElementId,
    views: &[Box<dyn crate::View>],
    owner: &mut crate::ElementOwner<'_>,
) {
    super::id_reconcile::reconcile_children_by_id(tree, parent, views, owner);
}
