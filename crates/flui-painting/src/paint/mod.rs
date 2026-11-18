//! The paint vocabulary: how a canvas command fills, strokes, blends, clips
//! and samples (`Paint`, `Shader`, `BlendMode`, `Path`, `Image`, filters).

mod blend_mode;
mod canvas;
mod clipping;
mod effects;
mod image;
#[cfg(feature = "decode")]
mod image_decode;
mod path;
mod shader;
mod shapes;
mod style;

pub use blend_mode::{BlendMode, CoverageClass};
pub use canvas::{
    FilterQuality, PathFillType, PointMode, StrokeCap, StrokeJoin, TextureId, TileMode,
};
pub use clipping::{Clip, ClipOp};
pub use effects::{
    ColorAdjustment, ColorMatrix, ImageFilter, ImageSampling, ImageTransform, InvalidImageTransform,
};
pub use image::{ColorFilter, Image, ImageDataError, ImageRepeat, MAX_ENCODED_IMAGE_BYTES};
#[cfg(feature = "decode")]
pub use image_decode::{ImageDecodeError, MAX_DECODED_IMAGE_BYTES};
pub use path::{Path, PathCommand};
pub use shader::{ImageShader, Shader};
pub use shapes::{CornerRounding, Morph, RoundedPolygon, ShapeError, material_shapes};
pub use style::{DashPattern, Paint, PaintBuilder, PaintStyle};
