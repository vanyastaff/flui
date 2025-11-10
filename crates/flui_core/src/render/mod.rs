//! Render system - Unified architecture
//!
//! # Architecture (v0.1.0)
//!
//! - `Render` trait: Unified trait for all render objects
//! - `Children` enum: Unified child representation (None/Single/Multi)
//! - `LayoutContext` / `PaintContext`: Context structs for operations
//! - `Arity`: Runtime child count validation
//! - `ParentData`: Metadata system (stored in RenderElement)
//!
//! # Pattern
//!
//! ```text
//! View (trait) → Element (enum) → RenderNode → Render trait
//!                                      ↓
//!                                  LayoutContext / PaintContext
//! ```
//!
//! # Implementation Guide
//!
//! To create a render object, implement the unified `Render` trait:
//!
//! ```rust,ignore
//! impl Render for MyRenderObject {
//!     fn layout(&mut self, ctx: &LayoutContext) -> Size { /* ... */ }
//!     fn paint(&self, ctx: &PaintContext) -> BoxedLayer { /* ... */ }
//!     fn arity(&self) -> Arity { Arity::Variable }  // or Exact(n)
//! }
//! ```

// Core modules
pub mod arity;
pub mod cache;
pub mod children;
pub mod context;
pub mod parent_data;
pub mod render_flags;
pub mod render_pipeline;
pub mod render_state;
pub mod render_unified;





// ========== Public API ==========

// New unified API (v0.1.0+)
/// Unified Render trait - replaces LeafRender, SingleRender, MultiRender
pub use render_unified::Render;

/// Children enum - unified child representation
pub use children::Children;

/// Arity - child count specification
pub use arity::Arity;

/// Context structs for layout and paint
pub use context::{LayoutContext, PaintContext};

// Core types
/// Parent data and metadata
pub use parent_data::{
    BoxParentData, ContainerBoxParentData, ContainerParentData, ParentData, ParentDataWithOffset,
};

/// Supporting types
pub use cache::{LayoutCache, LayoutCacheKey, LayoutResult};
pub use render_flags::{AtomicRenderFlags, RenderFlags};
pub use render_pipeline::RenderPipeline;
pub use render_state::RenderState;





