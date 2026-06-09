//! Element tree storage and management.
//!
//! This module provides:
//! - [`ElementTree`] - Slab-based storage for Elements
//! - [`reconcile_children`] - O(N) linear child reconciliation
//! - [`ReconcileEvent`] - structured trace stream for the keyed
//!   reconciler (plan §U13 / FR-035)

mod element_tree;
mod id_reconcile;
pub mod reconcile_event;
mod reconciliation;

// `test_utils` carries the `ReconcileEventCollector` Layer fixture
// (plan §U14 / FR-035). Gated to `cfg(test)` for in-crate test use
// and to `feature = "test-utils"` for downstream test crates that
// install the collector to assert on the trace stream (plan §U18 /
// §U19 6-permutation corpus + GlobalKey reparenting tests).
#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;

pub use element_tree::{ElementNode, ElementTree};
pub use reconcile_event::{RECONCILE_TARGET, ReconcileEvent, ReconcileEventKind};
pub use reconciliation::reconcile_children;
