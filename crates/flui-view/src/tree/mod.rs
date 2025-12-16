//! Element tree storage and management.
//!
//! This module provides:
//! - [`ElementTree`] - Slab-based storage for Elements
//! - [`reconciliation`] - O(N) linear child reconciliation

mod element_tree;
mod reconciliation;

pub use element_tree::{ElementNode, ElementTree};
pub use reconciliation::reconcile_children;
