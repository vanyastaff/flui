//! Render system - Unified architecture
//!
//! # Architecture
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
pub mod render;
pub mod render_flags;
pub mod render_pipeline;
pub mod render_state;






/// Render trait - single unified trait for all render objects
pub use render::Render;

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







