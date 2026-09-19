//! What a rasteriser reads off a shaped paragraph: placed glyphs keyed for an
//! atlas, and the bitmap one key rasterises to.
//!
//! The engine draws text from these two types and nothing else — the shaped
//! buffer never leaves this crate. A [`GlyphKey`] is opaque: the engine's
//! atlas hashes it and hands it back to [`SharedFontSystem::rasterize`]; what
//! it encodes (face, glyph index, size, weight, subpixel bin, synthesis
//! flags) is this crate's business.
//!
//! [`SharedFontSystem::rasterize`]: super::SharedFontSystem::rasterize

use flui_types::styling::Color;

/// Identifies one rasterised glyph bitmap.
///
/// Two glyphs with equal keys rasterise to identical bitmaps, so an atlas
/// keyed on this shares them. A key stays valid for the life of the process:
/// the font database is append-only ([`SharedFontSystem::register_font`]),
/// so the face it names is never removed.
///
/// [`SharedFontSystem::register_font`]: super::SharedFontSystem::register_font
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GlyphKey(pub(super) cosmic_text::CacheKey);

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
/// draws nothing and carries no data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlyphImage {
    /// Horizontal bearing: the bitmap's left edge relative to the origin's
    /// column, in device pixels.
    pub left: i32,
    /// Vertical bearing: the bitmap's top edge ABOVE the baseline row, in
    /// device pixels (a positive value moves the bitmap up).
    pub top: i32,
    /// Bitmap width in texels.
    pub width: u32,
    /// Bitmap height in texels.
    pub height: u32,
    /// How [`Self::data`] is laid out.
    pub content: GlyphContent,
    /// Row-major texels, `width * height * content.bytes_per_texel()` bytes.
    pub data: Vec<u8>,
}
