//! Tree module - ViewTree and related types
//!
//! This module provides the ViewTree data structure for storing ViewObjects
//! in a separate tree from Elements, following Flutter's architecture.

mod view_tree;

// View-specific tree utilities (moved from flui-tree)
pub mod snapshot;

pub use view_tree::{ViewNode, ViewTree};

// Re-export ViewId from flui-foundation for convenience
pub use flui_foundation::ViewId;

// Re-export snapshot types
pub use snapshot::{
    AncestorView, DepthLimitedView, FilteredView, SiblingView, SnapshotDiff, SubtreeView,
    TreeSnapshot, TreeViewExt,
};

// Re-export lifecycle from crate root
pub use crate::ViewLifecycle;
