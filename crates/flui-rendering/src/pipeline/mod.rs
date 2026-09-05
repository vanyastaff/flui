//! Pipeline management for render tree.
//!
//! The pipeline coordinates the rendering phases:
//! 1. Layout - compute sizes and positions
//! 2. Compositing bits - determine layer requirements
//! 3. Paint - generate display lists
//! 4. Semantics - build accessibility tree

mod dirty;
pub(crate) mod handle;
pub mod hit_test_probe;
mod notifier;
mod owner;
pub mod phase;
pub(super) mod scheduler;

// Re-export Clip from flui_types
// Re-export layer types from flui-layer
pub use flui_layer::{Layer, LayerId, LayerTree, OffsetLayer, SceneBuilder};
pub use flui_types::painting::Clip;
// Re-export additional types from flui_types::painting for convenience
pub use flui_types::painting::{BlendMode, ClipOp, FilterQuality, ImageFilter, PointMode, Shader};
// Re-export canvas types from flui_types
pub use dirty::{DirtyNode, DirtySets};
pub use flui_types::painting::{BlurStyle, StrokeCap, StrokeJoin, TileMode};
pub use handle::{RenderInvalidationHandle, SendError as DirtySendError};
pub use notifier::VisualUpdateNotifier;
pub use owner::{
    AttachRenderSubtreesError, AttachRenderSubtreesFailure, DetachRenderSubtreesError,
    DetachedRenderSubtrees, PipelineCell, PipelineOwner, ReleaseDetachedRenderSubtreesError,
    ReleaseDetachedRenderSubtreesFailure, WeakPipelineCell,
};
pub use phase::{Compositing, Idle, Layout, PaintPhase, PipelinePhase, Semantics};

// Re-export contexts from context module (canonical location)
pub use crate::context::{Canvas, DisplayList, Paint, PaintStyle};
// `ClipContext` is canonically owned by `flui-painting` (Flutter parity with
// `painting/clip.dart`); re-export through the pipeline path for ergonomic
// in-crate imports.
pub use flui_painting::ClipContext;
