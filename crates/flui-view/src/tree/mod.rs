//! Element tree storage and management.
//!
//! This module provides:
//! - [`ElementTree`] - Slab-based storage for Elements
//! - [`reconcile_children`] - O(N) linear child reconciliation
//! - [`ReconcileEvent`] - structured trace stream for the keyed
//!   reconciler (plan §U13 / FR-035)

mod element_tree;
pub mod reconcile_event;
mod reconciliation;

pub use element_tree::{ElementNode, ElementTree};
pub use reconcile_event::{RECONCILE_TARGET, ReconcileEvent, ReconcileEventKind};
pub use reconciliation::reconcile_children;
