//! Render system - RenderObject with typed arity constraints
//!
//! This module implements the core rendering architecture from idea.md Chapters 2-4.

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

pub use arity::{Arity, LeafArity, MultiArity, SingleArity};

// Re-exports
pub use cache::{LayoutCache, LayoutCacheKey, LayoutResult};
pub use dyn_render_object::{BoxedRenderObject, DynRenderObject};
pub use layout_cx::{LayoutCx, MultiChild, SingleChild};
pub use paint_cx::{MultiChildPaint, PaintCx, SingleChildPaint};
pub use parent_data::{
    BoxParentData, ContainerBoxParentData, ContainerParentData, ParentData, ParentDataWithOffset,
};
pub use render_context::RenderContext;
pub use render_flags::{AtomicRenderFlags, RenderFlags};
pub use render_object::RenderObject;
pub use render_pipeline::RenderPipeline;
pub use render_state::RenderState;
