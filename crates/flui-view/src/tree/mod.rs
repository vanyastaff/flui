//! Element tree storage and management.
//!
//! This module provides:
//! - [`ElementTree`] - Slab-based storage for Elements
//! - `reconcile_children_by_id` (in the crate-private `id_reconcile`
//!   module) - O(N) linear child reconciliation over the slab-resident
//!   element graph (the production reconciler after the E3 box→arena swap)
//! - [`ReconcileEvent`] - structured trace stream for the keyed
//!   reconciler (FR-035)

mod element_tree;
pub(crate) mod id_reconcile;
pub mod reconcile_event;

// `test_utils` carries the `ReconcileEventCollector` Layer fixture
// (FR-035). Gated to `cfg(test)` for in-crate test use
// and to `feature = "test-utils"` for downstream test crates that
// install the collector to assert on the trace stream (the
// 6-permutation corpus + GlobalKey reparenting tests).
#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;

pub use element_tree::{ElementNode, ElementTree};
pub use reconcile_event::{RECONCILE_TARGET, ReconcileEvent, ReconcileEventKind};
