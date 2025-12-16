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

// Re-export Clip from flui_types
pub use flui_types::painting::Clip;

pub use clip_context::ClipContext;
pub use owner::{DirtyNode, PipelineOwner};
pub use painting_context::{Canvas, Paint, PaintStyle, PaintingContext, Picture};

// Re-export additional types from flui_types::painting for convenience
pub use flui_types::painting::{BlendMode, ClipOp, FilterQuality, ImageFilter, PointMode, Shader};

// Re-export canvas types from flui_types
pub use flui_types::painting::{BlurStyle, StrokeCap, StrokeJoin, TileMode};

// Re-export layer types from flui-layer
pub use flui_layer::{Layer, LayerId, LayerTree, OffsetLayer, SceneBuilder};
