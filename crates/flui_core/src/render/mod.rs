//! Render system - RenderObject with typed arity constraints
//!
//! This module implements the core rendering architecture from idea.md Chapters 2-4.

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






// Re-export universal Arity system from parent module
pub use crate::arity::{Arity, LeafArity, SingleArity, MultiArity};

// Re-exports
pub use render_object::RenderObject;
pub use dyn_render_object::{DynRenderObject, BoxedRenderObject};
pub use layout_cx::{LayoutCx, SingleChild, MultiChild};
pub use paint_cx::{PaintCx, SingleChildPaint, MultiChildPaint};
pub use render_context::RenderContext;
pub use render_pipeline::RenderPipeline;
pub use cache::{LayoutCache, LayoutCacheKey, LayoutResult};
pub use render_state::RenderState;
pub use render_flags::{RenderFlags, AtomicRenderFlags};
pub use parent_data::{
    ParentData,
    ParentDataWithOffset,
    BoxParentData,
    ContainerParentData,
    ContainerBoxParentData,
};








