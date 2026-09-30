//! The Parley path (ADR-0092): paragraph shaping on a realm's
//! [`TextContext`](crate::TextContext), and the raster side (§5) — a glyph
//! key that names its face by font blob, the registry that keeps those faces
//! alive, and a swash rasterizer that draws the keys.
//!
//! Layout reaches this module through `TextPainter` under `parley-layout`
//! (ADR-0092 §10 step 3): size, baselines and intrinsics come from
//! [`ParagraphLayout`]. Shaped runs join the display list, with the engine's
//! atlas on [`SwashRasterizer`], in step 4. It holds no `static` and takes
//! no lock; every object is owned and used through `&mut`.
//!
//! - `shape` — [`ParagraphSpec`] in, [`ParagraphLayout`] out, through
//!   `TextContext::shape`.
//! - `key` — [`ParleyGlyphKey`] and its parts.
//! - `registry` — [`FontRegistry`]: faces and interned variation instances.
//! - `swash` — [`SwashRasterizer`], bit-identical to the cosmic-text path's
//!   scaler for the same face, glyph, size and bin.

mod key;
mod registry;
mod shape;
mod swash;

pub use crate::error::RegisterFaceError;
pub use key::{FaceKey, ParleyGlyphKey, SubpixelBin, Synthesis, VariationId};
pub use registry::{FontBytes, FontRegistry};
pub(crate) use shape::SpanBrush;
#[cfg(test)]
pub(crate) use shape::holds_exactly;
pub use shape::{ParagraphLayout, ParagraphSpec};
pub use swash::SwashRasterizer;
