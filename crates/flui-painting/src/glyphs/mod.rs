//! The raster side of text (ADR-0092 §5): the key that names one glyph
//! bitmap, the registry that keeps the faces keys name alive, the rasterizer
//! that draws them, and what an atlas reads off a paragraph.
//!
//! The engine draws text from these types and a
//! [`ShapedParagraph`](crate::display_list::ShapedParagraph), and nothing
//! else: no shaper type crosses. A [`GlyphKey`] names its face by font blob
//! id and face index, never by a process table; a [`FontRegistry`] holds the
//! blob for as long as a key may name it.
//!
//! - `key` — [`GlyphKey`] and its parts.
//! - `registry` — [`FontRegistry`]: faces and interned variation instances.
//! - `swash` — [`SwashRasterizer`], swash's scaler with the sources, format
//!   and offsets cosmic-text's rasterizer drew with (`parley_oracle` holds
//!   the recorded bitmaps).
//!
//! Everything here is owned and used through `&mut`: no `static`, no lock.

mod key;
mod registry;
mod swash;

pub use crate::error::RegisterFaceError;
pub use key::{FaceKey, GlyphKey, SubpixelBin, Synthesis, VariationId};
pub use registry::{FontBytes, FontRegistry, RunKey};
pub use swash::SwashRasterizer;
pub(crate) use swash::fake_bold_width;

use crate::styling::Color;

/// One glyph of a paragraph, placed in device pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlacedGlyph {
    /// What to rasterise.
    pub key: GlyphKey,
    /// The device column of the glyph's raster origin; the bitmap's `left`
    /// bearing is added to it.
    pub x: i32,
    /// The device row of the glyph's baseline; the bitmap's `top` bearing is
    /// subtracted from it.
    pub y: i32,
    /// A span colour that overrides the paragraph's own, when the span was
    /// styled with one.
    pub color: Option<Color>,
}

/// How a [`GlyphImage`]'s texels are to be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlyphContent {
    /// One byte per texel: coverage, to be tinted by the glyph's colour.
    Mask,
    /// Four bytes per texel: straight-alpha RGBA in the paragraph's colour
    /// space (an emoji, a colour bitmap face), drawn as-is.
    Color,
}

impl GlyphContent {
    /// Bytes per texel in [`GlyphImage::data`].
    #[must_use]
    pub const fn bytes_per_texel(self) -> u32 {
        match self {
            Self::Mask => 1,
            Self::Color => 4,
        }
    }
}

/// A rasterised glyph: the bitmap and where it sits relative to the glyph's
/// origin.
///
/// `width`/`height` may be zero (a space, a control glyph); such an image
/// draws nothing and carries no data. [`try_new`](Self::try_new) validates the
/// complete row-major buffer before admitting an image; immutable accessors
/// preserve that byte-layout invariant for every rasterizer and atlas consumer.
/// The `compile_fail::trybuild_ui` integration test pins the diagnostics for
/// literal construction, dimension mutation and mutation through [`Self::data`],
/// alongside a passing constructor-and-getter caller.
///
/// A consumer cannot bypass buffer admission with a struct literal:
///
/// ```compile_fail
/// use flui_painting::{GlyphContent, GlyphImage};
/// let image = GlyphImage {
///     left: 0, top: 0, width: 2, height: 2,
///     content: GlyphContent::Mask, data: vec![255],
/// };
/// ```
///
/// Admitted dimensions and the borrowed pixel buffer cannot be mutated:
///
/// ```compile_fail
/// use flui_painting::{GlyphContent, GlyphImage};
/// let mut image = GlyphImage::try_new(0, 0, 1, 1, GlyphContent::Mask, vec![255]).unwrap();
/// image.width = 2;
/// ```
///
/// ```compile_fail
/// use flui_painting::{GlyphContent, GlyphImage};
/// let mut image = GlyphImage::try_new(0, 0, 1, 1, GlyphContent::Mask, vec![255]).unwrap();
/// image.data()[0] = 0;
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlyphImage {
    /// Horizontal bearing: the bitmap's left edge relative to the origin's
    /// column, in device pixels.
    left: i32,
    /// Vertical bearing: the bitmap's top edge ABOVE the baseline row, in
    /// device pixels (a positive value moves the bitmap up).
    top: i32,
    /// Bitmap width in texels.
    width: u32,
    /// Bitmap height in texels.
    height: u32,
    /// How [`Self::data`] is laid out.
    content: GlyphContent,
    /// Row-major texels, `width * height * content.bytes_per_texel()` bytes.
    data: Vec<u8>,
}

/// Why a rasterized glyph's dimensions, format and bytes cannot form an image.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum GlyphImageError {
    /// The expected byte count cannot be represented in `usize`.
    #[error("glyph image byte count overflows usize")]
    SizeOverflow,
    /// The supplied buffer does not match the complete row-major image.
    #[error("glyph image needs {expected} bytes, got {actual}")]
    InvalidDataLength {
        /// Checked `width * height * content.bytes_per_texel()` byte count.
        expected: usize,
        /// Number of supplied bytes.
        actual: usize,
    },
}

impl GlyphImage {
    /// Admit a complete row-major mask or straight-alpha RGBA glyph bitmap.
    ///
    /// Either zero dimension denotes an empty glyph and requires an empty
    /// buffer, even when the other dimension is very large. Bearings are device
    /// pixel offsets and may span the entire `i32` range.
    ///
    /// # Errors
    ///
    /// [`GlyphImageError::SizeOverflow`] if the expected byte count does not fit
    /// `usize`; [`GlyphImageError::InvalidDataLength`] if the buffer has a
    /// different length. No image is admitted in either case.
    pub fn try_new(
        left: i32,
        top: i32,
        width: u32,
        height: u32,
        content: GlyphContent,
        data: Vec<u8>,
    ) -> Result<Self, GlyphImageError> {
        let expected =
            glyph_byte_len(width, height, content).ok_or(GlyphImageError::SizeOverflow)?;
        if data.len() != expected {
            return Err(GlyphImageError::InvalidDataLength {
                expected,
                actual: data.len(),
            });
        }
        Ok(Self {
            left,
            top,
            width,
            height,
            content,
            data,
        })
    }

    /// Horizontal bearing in device pixels, relative to the glyph origin.
    #[must_use]
    pub const fn left(&self) -> i32 {
        self.left
    }

    /// Vertical bearing in device pixels above the baseline.
    #[must_use]
    pub const fn top(&self) -> i32 {
        self.top
    }

    /// Bitmap width in texels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Bitmap height in texels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// The format of this image's row-major texels.
    #[must_use]
    pub const fn content(&self) -> GlyphContent {
        self.content
    }

    /// The complete immutable row-major texel buffer.
    #[must_use]
    pub fn data(&self) -> &[u8] {
        &self.data
    }
}

/// Checked expected size, with empty glyphs admitted before any conversion or
/// multiplication of their unused dimension.
fn glyph_byte_len(width: u32, height: u32, content: GlyphContent) -> Option<usize> {
    if width == 0 || height == 0 {
        return Some(0);
    }
    usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?
        .checked_mul(usize::try_from(content.bytes_per_texel()).ok()?)
}

/// Turns a glyph key into the bitmap an atlas uploads (ADR-0067, ADR-0092 §5).
///
/// The engine's atlas hashes `Key`, and on a miss asks for the bitmap. A
/// rasterizer is owned by whoever owns the atlas and is taken by `&mut`, so
/// rasterization shares no lock with shaping unless an implementation brings
/// one.
///
/// # Contract
///
/// - **Deterministic:** equal keys rasterize to equal images, however often
///   and in whatever order. The atlas re-rasterizes live keys when a page
///   grows and uploads into the slot the first image sized; an image of
///   another size or content is not uploaded.
/// - Images preserve the validated byte layout enforced by [`GlyphImage::try_new`].
/// - An empty glyph (a space) is `Some` with zero `width` or `height`.
/// - `None`: this rasterizer cannot draw the key (an unknown face or
///   variation, a size that is not finite and positive, synthesis outside
///   the rasterizer's bounds such as a skew steeper than
///   `Synthesis::MAX_SKEW_DEGREES`). The atlas does not place the glyph and
///   asks again on its next use.
pub trait GlyphRasterizer {
    /// Identifies one bitmap.
    type Key: Copy + Eq + core::hash::Hash + core::fmt::Debug;

    /// The bitmap for `key`; see the trait's contract.
    fn rasterize(&mut self, key: Self::Key) -> Option<GlyphImage>;
}
