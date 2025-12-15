//! Pipeline management for render tree.
//!
//! The pipeline coordinates the rendering phases:
//! 1. Layout - compute sizes and positions
//! 2. Compositing bits - determine layer requirements
//! 3. Paint - generate display lists
//! 4. Semantics - build accessibility tree

mod clip_context;
mod owner;
mod painting_context;

pub use crate::layer::Clip;
pub use clip_context::ClipContext;
pub use owner::{DirtyNode, PipelineOwner};
pub use painting_context::{
    BlendMode, BlurStyle, Canvas, ClipOp, DrawCommand, FilterQuality, ImageFilter, MaskFilter,
    Paint, PaintStyle, PaintingContext, PointMode, Shader, StrokeCap, StrokeJoin, TileMode,
};
