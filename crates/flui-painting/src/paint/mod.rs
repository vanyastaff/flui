//! Painting types for Flui.
//!
//! This module contains low-level painting primitives used for rendering,
//! including blend modes, image handling, clipping, canvas primitives, and
//! shaders.

pub mod blend_mode;
pub mod canvas;
pub mod clipping;
pub mod effects;
pub mod image;
#[cfg(feature = "decode")]
mod image_decode;
pub mod path;
pub mod shader;
pub mod shapes;
pub mod style;

// Re-exports for convenience
pub use crate::Alignment;
pub use blend_mode::{BlendMode, CoverageClass};
pub use canvas::{
    FilterQuality, PathFillType, PointMode, StrokeCap, StrokeJoin, TextureId, TileMode,
};
pub use clipping::{Clip, ClipOp};
pub use effects::{
    ColorAdjustment, ColorMatrix, ImageFilter, ImageSampling, ImageTransform, InvalidImageTransform,
};
pub use image::{
    BoxFit, ColorFilter, FittedSizes, Image, ImageDataError, ImageRepeat, MAX_ENCODED_IMAGE_BYTES,
};
#[cfg(feature = "decode")]
pub use image_decode::{ImageDecodeError, MAX_DECODED_IMAGE_BYTES};
pub use path::{Path, PathCommand};
pub use shader::{ImageShader, Shader};
pub use shapes::{CornerRounding, Morph, RoundedPolygon, ShapeError};
pub use style::{DashPattern, Paint, PaintBuilder, PaintStyle};
