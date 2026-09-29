//! Painting types for Flui.
//!
//! This module contains low-level painting primitives used for rendering,
//! including blend modes, image handling, clipping, canvas primitives, and
//! shaders.

#![expect(unused)] // Painting API for future implementation

pub mod blend_mode;
pub mod canvas;
pub mod clipping;
pub mod effects;
pub mod image;
pub mod path;
pub mod shader;
pub mod style;

// Re-exports for convenience
pub use crate::Alignment;
pub use blend_mode::BlendMode;
pub use canvas::{
    BlurStyle, FilterQuality, PaintingStyle, PathFillType, PathOperation, PointMode, StrokeCap,
    StrokeJoin, TextureId, TileMode, VertexMode,
};
pub use clipping::{Clip, ClipBehavior, ClipOp};
pub use effects::{
    BlurMode, BlurQuality, ColorAdjustment, ColorMatrix, ImageFilter, PathPaintMode, StrokeOptions,
};
pub use image::{
    BoxFit, ColorFilter, FittedSizes, Image, ImageConfiguration, ImageDataError, ImageRepeat,
};
pub use path::{Path, PathCommand};
pub use shader::{ImageShader, MaskFilter, Shader};
pub use style::{DashPattern, Paint, PaintBuilder, PaintStyle};
