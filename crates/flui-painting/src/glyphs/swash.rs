//! [`SwashRasterizer`]: [`GlyphKey`]s drawn by swash's scaler.

use swash::scale::image::Content;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::zeno::{Angle, Format, Transform, Vector};

use super::key::{GlyphKey, Synthesis};
use super::registry::FontRegistry;
use super::{GlyphContent, GlyphImage, GlyphRasterizer};

/// Rasterizes [`GlyphKey`]s through swash, with the sources, format and
/// offsets cosmic-text's rasterizer used, so a glyph draws the bitmap it
/// drew there. Owns its
/// registry and scaler; `Send`, no lock.
#[derive(Default)]
pub struct SwashRasterizer {
    fonts: FontRegistry,
    context: ScaleContext,
}

impl SwashRasterizer {
    /// A rasterizer with no faces yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A rasterizer that owns copied font bytes, for unloadable scene sources.
    #[must_use]
    pub fn with_owned_fonts() -> Self {
        Self::with_fonts(FontRegistry::with_owned_sources())
    }

    /// A rasterizer over `fonts`.
    #[must_use]
    pub fn with_fonts(fonts: FontRegistry) -> Self {
        Self {
            fonts,
            context: ScaleContext::new(),
        }
    }

    /// The faces and variation instances this rasterizer can draw.
    #[must_use]
    pub fn fonts(&self) -> &FontRegistry {
        &self.fonts
    }

    /// The registry, to add faces and intern variation instances.
    pub fn fonts_mut(&mut self) -> &mut FontRegistry {
        &mut self.fonts
    }
}

impl std::fmt::Debug for SwashRasterizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SwashRasterizer")
            .field("fonts", &self.fonts)
            .finish_non_exhaustive()
    }
}

/// The fake-bold outline growth at `size` px, in total: `size × ratio`, with
/// the ratio interpolated linearly from 1/24 at 9 px to 1/32 at 36 px and
/// clamped outside. A FLUI choice; painting
/// ARCHITECTURE, mapping decision 10.
pub(crate) fn fake_bold_width(size: f32) -> f32 {
    const KEYS: [f32; 2] = [9.0, 36.0];
    const RATIOS: [f32; 2] = [1.0 / 24.0, 1.0 / 32.0];
    let t = ((size - KEYS[0]) / (KEYS[1] - KEYS[0])).clamp(0.0, 1.0);
    size * (t * (RATIOS[1] - RATIOS[0]) + RATIOS[0])
}

impl GlyphRasterizer for SwashRasterizer {
    type Key = GlyphKey;

    fn rasterize(&mut self, key: GlyphKey) -> Option<GlyphImage> {
        let size = key.size();
        if !size.is_finite() || size <= 0.0 {
            return None;
        }
        let synthesis = key.synthesis();
        if synthesis.skew_degrees.unsigned_abs() > Synthesis::MAX_SKEW_DEGREES {
            return None;
        }
        let face = self.fonts.face(key.face())?;
        let coords: &[i16] = match key.variation() {
            None => &[],
            // An id this registry did not mint is not drawn as the default
            // instance: that would cache a wrong bitmap under the key.
            Some(id) => self.fonts.variation(id)?,
        };
        let mut scaler = self
            .context
            .builder(face.font_ref())
            .size(size)
            .hint(key.hinted())
            .normalized_coords(coords.iter())
            .build();
        // swash moves each outline point by the strength on each side, so the
        // outline grows by twice it in total.
        let embolden = if synthesis.embolden {
            fake_bold_width(size) / 2.0
        } else {
            0.0
        };
        let skew = (synthesis.skew_degrees != 0).then(|| {
            Transform::skew(
                Angle::from_degrees(f32::from(synthesis.skew_degrees)),
                Angle::from_degrees(0.0),
            )
        });
        // The sources, format and offset cosmic-text 0.19's `swash_image`
        // uses, so the two paths draw identical bitmaps.
        let image = Render::new(&[
            Source::ColorOutline(0),
            Source::ColorBitmap(StrikeWith::BestFit),
            Source::Outline,
        ])
        .format(Format::Alpha)
        .offset(Vector::new(key.x_bin().offset(), 0.0))
        .transform(skew)
        .embolden(embolden)
        .render(&mut scaler, key.glyph_id())?;
        let content = match image.content {
            Content::Color => GlyphContent::Color,
            // The scaler is asked for `Format::Alpha`, so no subpixel mask
            // comes back; the arm keeps the match total.
            Content::Mask | Content::SubpixelMask => GlyphContent::Mask,
        };
        Some(GlyphImage {
            left: image.placement.left,
            top: image.placement.top,
            width: image.placement.width,
            height: image.placement.height,
            content,
            data: image.data,
        })
    }
}
