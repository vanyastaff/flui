//! Tree module - ViewTree and related types
//!
//! This module provides the ViewTree data structure for storing ViewObjects
//! in a separate tree from Elements, following Flutter's architecture.

mod view_tree;

pub use view_tree::{ConcreteViewNode, ViewId, ViewNode, ViewTree};

// Re-export lifecycle from crate root
pub use crate::ViewLifecycle;
