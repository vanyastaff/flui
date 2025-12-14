//! Pipeline management for render tree.
//!
//! The pipeline coordinates the rendering phases:
//! 1. Layout - compute sizes and positions
//! 2. Compositing bits - determine layer requirements
//! 3. Paint - generate display lists
//! 4. Semantics - build accessibility tree

mod owner;
mod painting_context;

pub use owner::PipelineOwner;
pub use painting_context::{Canvas, Paint, PaintStyle, PaintingContext};
