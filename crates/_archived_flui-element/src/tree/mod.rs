//! Tree module - ElementTree and flui-tree trait implementations
//!
//! This module provides the ElementTree data structure and implements
//! the abstract tree traits from flui-tree.

mod element_tree;
mod tree_traits;

// Element-specific tree traits (moved from flui-tree)
pub mod diff;
pub mod inherited;
pub mod lifecycle_traits;
pub mod reconciliation;

pub use element_tree::ElementTree;

// Re-export element-specific traits
pub use diff::TreeDiff;
pub use inherited::{InheritedData, InheritedRegistry, InheritedScope};
pub use lifecycle_traits::Lifecycle;
pub use reconciliation::{CanUpdate, Reconciler};

// Re-export tree traits for convenience
pub use flui_tree::{TreeNav, TreeRead, TreeWrite, TreeWriteNav};
