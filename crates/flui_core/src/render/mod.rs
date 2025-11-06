//! Render system - Enum-based architecture
//!
//! # Architecture
//!
//! - `RenderNode` enum: Unified render tree node (stores in Element tree)
//! - `LeafRender`, `SingleRender`, `MultiRender` traits: Specialized render traits
//!
//! # Pattern
//!
//! ```text
//! View (trait) → Element (enum) → RenderNode (enum)
//!                                      ↓
//!                                  LeafRender/SingleRender/MultiRender traits
//! ```
//!
//! # Implementation Guide
//!
//! To create a render object, implement one of the specialized traits:
//!
//! - `LeafRender`: For renders with no children (e.g., text, image)
//! - `SingleRender`: For renders with exactly one child (e.g., opacity, transform)
//! - `MultiRender`: For renders with multiple children (e.g., flex, stack)

// Core modules
pub mod cache;
pub mod parent_data;
pub mod render_flags;
pub mod render_node;
pub mod render_pipeline;
pub mod render_state;
pub mod render_traits;

// ========== Public API ==========

/// Unified render tree node enum
pub use render_node::RenderNode;

/// Object-safe render traits (specialized by child count)
///
/// These are the main traits for implementing render objects:
/// - `LeafRender`: For renders with no children
/// - `SingleRender`: For renders with exactly one child
/// - `MultiRender`: For renders with multiple children
pub use render_traits::{LeafRender, MultiRender, SingleRender};

/// Parent data and metadata
pub use parent_data::{
    BoxParentData, ContainerBoxParentData, ContainerParentData, ParentData, ParentDataWithOffset,
};

/// Supporting types
pub use cache::{LayoutCache, LayoutCacheKey, LayoutResult};
pub use render_flags::{AtomicRenderFlags, RenderFlags};
pub use render_pipeline::RenderPipeline;
pub use render_state::RenderState;
