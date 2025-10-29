//! Render system - Enum-based architecture
//!
//! # Architecture
//!
//! - `RenderNode` enum: Unified render tree node (stores in Element tree)
//! - `Render` trait: Core rendering trait (implements layout/paint)
//! - `LeafRender`, `SingleRender`, `MultiRender` traits: Specialized render traits
//!
//! # Pattern
//!
//! ```text
//! Widget (enum) → Element (enum) → RenderNode (enum)
//!                                      ↓
//!                                  Render trait (LeafRender/SingleRender/MultiRender)
//! ```

// Core modules
pub mod arity;
pub mod cache;
pub mod dyn_render_object;
pub mod layout_cx;
pub mod paint_cx;
pub mod parent_data;
pub mod render_adapter;
pub mod render_context;
pub mod render_flags;
pub mod render_node;
pub mod render_object;
pub mod render_pipeline;
pub mod render_state;
pub mod render_traits;

// ========== Public API ==========

/// Unified render tree node enum
pub use render_node::RenderNode;

/// Core render trait (the main trait for implementing render objects)
pub use render_object::Render;

/// Object-safe render traits (specialized by child count)
pub use render_traits::{LeafRender, MultiRender, SingleRender};

/// Adapters for backward compatibility
pub use render_adapter::{LeafAdapter, MultiAdapter, SingleAdapter};

/// Arity types
pub use arity::{Arity, LeafArity, MultiArity, SingleArity};

/// Layout and paint contexts
pub use layout_cx::{LayoutCx, MultiChild, SingleChild};
pub use paint_cx::{MultiChildPaint, PaintCx, SingleChildPaint};

/// Parent data and metadata
pub use parent_data::{
    BoxParentData, ContainerBoxParentData, ContainerParentData, ParentData, ParentDataWithOffset,
};

/// Supporting types
pub use cache::{LayoutCache, LayoutCacheKey, LayoutResult};
pub use dyn_render_object::{BoxedRender, DynRender};
pub use render_context::RenderContext;
pub use render_flags::{AtomicRenderFlags, RenderFlags};
pub use render_pipeline::RenderPipeline;
pub use render_state::RenderState;













