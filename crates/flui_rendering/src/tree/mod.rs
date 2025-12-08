//! Tree module - RenderTree and related types
//!
//! This module provides the RenderTree data structure for storing RenderObjects
//! in a separate tree from Elements, following Flutter's architecture.
//!
//! # Modules
//!
//! - [`render_tree`] - The RenderTree data structure with RenderNode
//! - [`tree_traits`] - TreeRead/TreeWrite/TreeNav implementations
//! - [`access`] - Traits for render-specific tree access (RenderTreeAccess)
//! - [`dirty`] - Dirty tracking for layout and paint phases
//! - [`iter`] - Render-specific iterators
//! - [`collector`] - Arity-aware render children collection

mod render_tree;

// Render-specific traits and utilities
pub mod access;
pub mod collector;
pub mod dirty;
pub mod iter;
pub mod visitable;

// Export RenderTree and RenderNode (no longer export RenderId - use flui_foundation::RenderId)
pub use render_tree::{RenderNode, RenderTree};

// Re-export RenderId from flui_foundation for convenience
pub use flui_foundation::RenderId;

// Re-export key types from access module
pub use access::{RenderChildAccessor, RenderTreeAccess, RenderTreeAccessExt, RenderTreeExt};

// Re-export dirty tracking
pub use dirty::{AtomicDirtyFlags, DirtyTracking, DirtyTrackingExt};

// Re-export iterators
pub use iter::{
    RenderAncestors, RenderChildren, RenderChildrenWithIndex, RenderDescendants, RenderLeaves,
    RenderPath, RenderSiblings, RenderSubtree, RenderSubtreeItem,
};

// Re-export collector
pub use collector::RenderChildrenCollector;

// Re-export visitable traits
pub use visitable::{HitTestVisitable, LayoutVisitable, PaintVisitable};

// Re-export lifecycle from crate root
pub use crate::core::RenderLifecycle;
