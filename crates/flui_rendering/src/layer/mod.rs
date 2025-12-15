//! Layer system for compositing render tree output.
//!
//! Layers form a tree that represents the composited output of the render tree.
//! Each layer can have its own backing store (texture) and transformation.
//!
//! # Layer Types
//!
//! - [`Layer`]: Base trait for all layers
//! - [`ContainerLayer`]: Layer that can contain child layers
//! - [`OffsetLayer`]: Layer with a translation offset
//! - [`ClipRectLayer`]: Layer that clips to a rectangle
//! - [`ClipRRectLayer`]: Layer that clips to a rounded rectangle
//! - [`ClipPathLayer`]: Layer that clips to a path
//! - [`OpacityLayer`]: Layer that applies opacity
//! - [`TransformLayer`]: Layer that applies a transformation
//! - [`PictureLayer`]: Leaf layer containing recorded drawing commands
//!
//! # Flutter Equivalence
//!
//! This corresponds to Flutter's layer system in `rendering/layer.dart`.

mod base;
mod clip;
mod container;
mod effects;
mod picture;
mod scene;

pub use base::{EngineLayer, Layer, LayerHandle, LayerId, SceneBuilder, SceneOperation};
pub use clip::{Clip, ClipPathLayer, ClipRRectLayer, ClipRectLayer};
pub use container::{ContainerLayer, OffsetLayer};
pub use effects::{
    BackdropFilterLayer, BlendMode, ColorFilter, ColorFilterLayer, ImageFilter, OpacityLayer,
    Shader, ShaderMaskLayer, TransformLayer,
};
pub use picture::{Picture, PictureId, PictureLayer};
pub use scene::{Scene, SceneId, SceneStatistics};
