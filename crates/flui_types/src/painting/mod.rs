//! Painting types for Flui.
//!
//! This module contains low-level painting primitives used for rendering,
//! including blend modes, image handling, clipping, canvas primitives, and shaders.

pub mod blend_mode;
pub mod canvas;
pub mod clipping;
pub mod effects;
pub mod image;
pub mod image_provider;
pub mod path;
pub mod shader;




// Re-exports for convenience
pub use blend_mode::BlendMode;
pub use canvas::{
    BlurStyle, FilterQuality, PaintingStyle, PathFillType, PathOperation, StrokeCap, StrokeJoin,
    TileMode, VertexMode,
};
pub use clipping::{AutomaticNotchedShape, CircularNotchedRectangle, Clip, ClipBehavior, NotchedShape};
pub use effects::{
    BlurQuality, BlurMode, ColorMatrix, PathPaintMode, StrokeOptions,
    ColorFilter as EffectColorFilter,  // Renamed to avoid conflict with image::ColorFilter
};
pub use image::{BoxFit, ColorFilter, FittedSizes, Image, ImageConfiguration, ImageRepeat};
pub use image_provider::{
    AssetImage, FileImage, ImageError, ImageProvider, ImageResult, MemoryImage, NetworkImage,
    ResolvedImageProvider, TransformedImageProvider,
};
pub use shader::{ImageShader, MaskFilter, Shader};



