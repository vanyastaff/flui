//! Painting types for Flui.
//!
//! This module contains low-level painting primitives used for rendering,
//! including blend modes, image handling, clipping, canvas primitives, and shaders.

pub mod blend_mode;
pub mod canvas;
pub mod clipping;
pub mod effects;
pub mod image;
pub mod paint;
pub mod path;
pub mod shader;

// Re-exports for convenience
pub use blend_mode::BlendMode;
pub use canvas::{
    BlurStyle, FilterQuality, PaintingStyle, PathFillType, PathOperation, PointMode, StrokeCap,
    StrokeJoin, TextureId, TileMode, VertexMode,
};
pub use clipping::{
    AutomaticNotchedShape, CircularNotchedRectangle, Clip, ClipBehavior, ClipOp, NotchedShape,
};
pub use effects::{
    BlurMode, BlurQuality, ColorAdjustment, ColorMatrix, ImageFilter, PathPaintMode, StrokeOptions,
};
pub use image::{BoxFit, ColorFilter, FittedSizes, Image, ImageConfiguration, ImageRepeat};
pub use paint::{Paint, PaintBuilder, PaintStyle};
pub use path::{Path, PathCommand};
pub use shader::{ImageShader, MaskFilter, Shader, ShaderSpec};
