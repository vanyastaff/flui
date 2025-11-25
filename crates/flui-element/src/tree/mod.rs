//! Tree module - ElementTree and flui-tree trait implementations
//!
//! This module provides the ElementTree data structure and implements
//! the abstract tree traits from flui-tree.

mod element_tree;
mod tree_traits;

pub use element_tree::ElementTree;

// Re-export tree traits for convenience
pub use flui_tree::{TreeNav, TreeRead, TreeWrite, TreeWriteNav};
