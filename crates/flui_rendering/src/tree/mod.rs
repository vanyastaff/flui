//! Tree module - RenderTree and related types
//!
//! This module provides the RenderTree data structure for storing RenderObjects
//! in a separate tree from Elements, following Flutter's architecture.

mod render_tree;

pub use render_tree::{ConcreteRenderNode, RenderId, RenderNode, RenderTree};

// Re-export lifecycle from crate root
pub use crate::core::RenderLifecycle;
