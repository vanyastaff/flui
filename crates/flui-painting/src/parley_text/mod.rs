//! The raster side of the Parley path (ADR-0092 §5): a glyph key that names
//! its face by font blob, the registry that keeps those faces alive, and a
//! swash rasterizer that draws the keys.
//!
//! Nothing on the production path reaches this module yet: shaped runs join
//! the display list, and the engine's atlas switches to [`SwashRasterizer`],
//! in ADR-0092 §10 step 3. It holds no `static` and takes no lock; every
//! object is owned and used through `&mut`.
//!
//! - `key` — [`ParleyGlyphKey`] and its parts.
//! - `registry` — [`FontRegistry`]: faces and interned variation instances.
//! - `swash` — [`SwashRasterizer`], bit-identical to the cosmic-text path's
//!   scaler for the same face, glyph, size and bin.

mod key;
mod registry;
mod swash;

pub use crate::error::RegisterFaceError;
pub use key::{FaceKey, ParleyGlyphKey, SubpixelBin, Synthesis, VariationId};
pub use registry::{FontBytes, FontRegistry};
pub use swash::SwashRasterizer;
