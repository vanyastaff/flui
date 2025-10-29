//! Render system - New enum-based architecture
//!
//! # New API (Recommended)
//!
//! - `Render` enum: Unified render object type
//! - `LeafRender`, `SingleRender`, `MultiRender` traits: Simple, object-safe traits
//!
//! # Legacy API
//!
//! The old Render trait with Arity generics is still available but deprecated.

// New architecture (recommended)
pub mod render_enum;
pub mod render_traits;
pub mod render_adapter;

// Legacy modules
pub mod arity;
pub mod cache;
pub mod dyn_render_object;
pub mod layout_cx;
pub mod paint_cx;
pub mod parent_data;
pub mod render_context;
pub mod render_flags;
pub mod render_object;
pub mod render_pipeline;
pub mod render_state;


// ========== New API ==========

/// Unified render object enum
pub use render_enum::Render;

/// Object-safe render traits
pub use render_traits::{LeafRender, MultiRender, SingleRender};

/// Adapters for backward compatibility
pub use render_adapter::{LeafAdapter, SingleAdapter, MultiAdapter};

// ========== Legacy API ==========

pub use arity::{Arity, LeafArity, MultiArity, SingleArity};
pub use cache::{LayoutCache, LayoutCacheKey, LayoutResult};
pub use dyn_render_object::{BoxedRender, DynRender};
pub use layout_cx::{LayoutCx, MultiChild, SingleChild};
pub use paint_cx::{MultiChildPaint, PaintCx, SingleChildPaint};
pub use parent_data::{
    BoxParentData, ContainerBoxParentData, ContainerParentData, ParentData, ParentDataWithOffset,
};
pub use render_context::RenderContext;
pub use render_flags::{AtomicRenderFlags, RenderFlags};
pub use render_object::Render;
pub use render_pipeline::RenderPipeline;
pub use render_state::RenderState;











