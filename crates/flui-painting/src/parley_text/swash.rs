//! [`SwashRasterizer`]: [`ParleyGlyphKey`]s drawn by swash's scaler.

use swash::scale::image::Content;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::zeno::{Angle, Format, Transform, Vector};

use super::key::{ParleyGlyphKey, Synthesis};
use super::registry::FontRegistry;
use crate::text_layout::{GlyphContent, GlyphImage, GlyphRasterizer};

/// Rasterizes [`ParleyGlyphKey`]s through swash, the scaler the cosmic-text
/// path drives today, with the same sources, format and offsets. Owns its
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

/// Skia's fake-bold outline growth at `size` px: the stroke width
/// `SkScalerContext` adds, `size × ratio`, with the ratio interpolated
/// linearly from 1/24 at 9 px to 1/32 at 36 px and clamped outside
/// (`kStdFakeBoldInterpKeys`/`kStdFakeBoldInterpValues`). Painting
/// ARCHITECTURE, mapping decision 10.
pub(super) fn fake_bold_width(size: f32) -> f32 {
    const KEYS: [f32; 2] = [9.0, 36.0];
    const RATIOS: [f32; 2] = [1.0 / 24.0, 1.0 / 32.0];
    let t = ((size - KEYS[0]) / (KEYS[1] - KEYS[0])).clamp(0.0, 1.0);
    size * (t * (RATIOS[1] - RATIOS[0]) + RATIOS[0])
}

impl GlyphRasterizer for SwashRasterizer {
    type Key = ParleyGlyphKey;

    fn rasterize(&mut self, key: ParleyGlyphKey) -> Option<GlyphImage> {
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use cosmic_text::{CacheKey, CacheKeyFlags, FontSystem, SwashCache, fontdb};

    use super::{SwashRasterizer, fake_bold_width};
    use crate::parley_text::{FaceKey, ParleyGlyphKey, SubpixelBin, Synthesis};
    use crate::text_layout::{GlyphContent, GlyphImage, GlyphRasterizer};

    const ROBOTO: &[u8] = include_bytes!("../../assets/fonts/Roboto-Regular.ttf");
    const FACE: FaceKey = FaceKey {
        blob_id: 1,
        index: 0,
    };

    fn rasterizer() -> SwashRasterizer {
        let mut rasterizer = SwashRasterizer::new();
        rasterizer
            .fonts_mut()
            .register_face(FACE, Arc::new(ROBOTO))
            .expect("Roboto is a face");
        rasterizer
    }

    fn glyph(rasterizer: &SwashRasterizer, c: char) -> u16 {
        let face = rasterizer.fonts().face(FACE).expect("registered");
        face.font_ref().charmap().map(c)
    }

    /// The same glyph through cosmic-text's scaler, on a font system local to
    /// the test (never the process one).
    fn cosmic_image(glyph_id: u16, size: f32, flags: CacheKeyFlags) -> GlyphImage {
        let mut system =
            FontSystem::new_with_locale_and_db("en-US".into(), fontdb::Database::new());
        let ids = system
            .db_mut()
            .load_font_source(fontdb::Source::Binary(Arc::new(ROBOTO.to_vec())));
        let (key, _, _) = CacheKey::new(
            ids[0],
            glyph_id,
            size,
            (0.0, 0.0),
            fontdb::Weight(400),
            flags,
        );
        let image = SwashCache::new()
            .get_image_uncached(&mut system, key)
            .expect("cosmic-text rasterizes");
        GlyphImage {
            left: image.placement.left,
            top: image.placement.top,
            width: image.placement.width,
            height: image.placement.height,
            content: GlyphContent::Mask,
            data: image.data,
        }
    }

    fn ink(image: &GlyphImage) -> u64 {
        image.data.iter().map(|b| u64::from(*b)).sum()
    }

    #[test]
    fn an_unregistered_face_is_not_placed() {
        let mut rasterizer = rasterizer();
        let other = FaceKey {
            blob_id: 2,
            index: 0,
        };
        let key = ParleyGlyphKey::new(other, 5, 16.0, SubpixelBin::Zero);
        assert_eq!(rasterizer.rasterize(key), None);
    }

    /// Fails if an unknown id falls back to the default instance, or resolves
    /// to this registry's own instance at the same index.
    #[test]
    fn an_unknown_variation_is_not_placed() {
        let mut rasterizer = rasterizer();
        let gid = glyph(&rasterizer, 'a');
        // This registry's first instance sits at the index the foreign id
        // names.
        assert!(rasterizer.fonts_mut().intern_variation(&[-2048]).is_some());
        // Minted by another registry: unknown to this one.
        let mut elsewhere = super::FontRegistry::new();
        let foreign = elsewhere.intern_variation(&[1234]);
        assert!(foreign.is_some());
        let key = ParleyGlyphKey::new(FACE, gid, 16.0, SubpixelBin::Zero).with_variation(foreign);
        assert_eq!(rasterizer.rasterize(key), None);
        // The same key at the default instance draws.
        assert!(rasterizer.rasterize(key.with_variation(None)).is_some());
    }

    #[test]
    fn a_size_that_is_not_finite_and_positive_is_not_placed() {
        let mut rasterizer = rasterizer();
        let gid = glyph(&rasterizer, 'a');
        for size in [0.0, -0.0, -12.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let key = ParleyGlyphKey::new(FACE, gid, size, SubpixelBin::Zero);
            assert_eq!(rasterizer.rasterize(key), None, "size {size}");
        }
    }

    /// A near-vertical shear would ask swash for a bitmap millions of pixels
    /// wide; past the bound the key is not drawn, up to it it is.
    #[test]
    fn a_skew_past_the_bound_is_not_placed() {
        let mut rasterizer = rasterizer();
        let gid = glyph(&rasterizer, 'H');
        let skewed = |skew_degrees| {
            ParleyGlyphKey::new(FACE, gid, 20.0, SubpixelBin::Zero).with_synthesis(Synthesis {
                embolden: false,
                skew_degrees,
            })
        };
        for degrees in [46, 89, 90, 127, -46, -90, -128] {
            assert_eq!(
                rasterizer.rasterize(skewed(degrees)),
                None,
                "{degrees} degrees"
            );
        }
        for degrees in [45, -45] {
            let image = rasterizer.rasterize(skewed(degrees)).expect("draws");
            assert!(image.width < 64, "{degrees} degrees: a bounded bitmap");
        }
    }

    #[test]
    fn a_space_rasterizes_to_an_empty_image() {
        let mut rasterizer = rasterizer();
        let gid = glyph(&rasterizer, ' ');
        assert_ne!(gid, 0, "Roboto maps the space");
        let image = rasterizer
            .rasterize(ParleyGlyphKey::new(FACE, gid, 16.0, SubpixelBin::Zero))
            .expect("a space is a glyph");
        assert!(image.width == 0 || image.height == 0);
        assert!(image.data.is_empty());
    }

    #[test]
    fn one_key_rasterizes_to_equal_images_twice() {
        let mut warm = rasterizer();
        let keys: Vec<_> = "Hag"
            .chars()
            .flat_map(|c| {
                let gid = glyph(&warm, c);
                [SubpixelBin::Zero, SubpixelBin::Three]
                    .map(|bin| ParleyGlyphKey::new(FACE, gid, 18.0, bin))
            })
            .collect();
        for key in &keys {
            let first = warm.rasterize(*key).expect("draws");
            let second = warm.rasterize(*key).expect("draws");
            assert_eq!(first, second, "one rasterizer, twice: {key:?}");
            assert!(first.width > 0);
            let fresh_a = rasterizer().rasterize(*key).expect("draws");
            let fresh_b = rasterizer().rasterize(*key).expect("draws");
            assert_eq!(fresh_a, fresh_b, "two fresh rasterizers: {key:?}");
            assert_eq!(first, fresh_a, "warm and fresh agree: {key:?}");
        }
    }

    /// cosmic-text's `FAKE_ITALIC` is a 14° skew; a key skewed 14° draws the
    /// same bitmap.
    #[test]
    fn synthetic_italic_matches_cosmic_text_fake_italic() {
        let mut rasterizer = rasterizer();
        for c in ['l', 'H', 'g'] {
            let gid = glyph(&rasterizer, c);
            let key =
                ParleyGlyphKey::new(FACE, gid, 20.0, SubpixelBin::Zero).with_synthesis(Synthesis {
                    embolden: false,
                    skew_degrees: 14,
                });
            let ours = rasterizer.rasterize(key).expect("draws");
            let upright = rasterizer
                .rasterize(key.with_synthesis(Synthesis::default()))
                .expect("draws");
            assert_ne!(ours, upright, "{c}: the skew changes the bitmap");
            assert_eq!(
                ours,
                cosmic_image(gid, 20.0, CacheKeyFlags::FAKE_ITALIC),
                "{c}: equal to cosmic-text's fake italic"
            );
        }
    }

    /// The outline grows by Skia's fake-bold stroke width: the bitmap widens
    /// by about that much, and carries more ink. At 144 px the stroke is 4.5 px:
    /// a strength applied per side without halving would widen it by 9, and
    /// one taken as the total by 2.25; both fall outside the bound.
    #[test]
    fn synthetic_bold_adds_the_skia_strength() {
        let mut rasterizer = rasterizer();
        let gid = glyph(&rasterizer, 'H');
        for size in [9.0_f32, 20.0, 36.0, 144.0] {
            let regular_key = ParleyGlyphKey::new(FACE, gid, size, SubpixelBin::Zero);
            let bold_key = regular_key.with_synthesis(Synthesis {
                embolden: true,
                skew_degrees: 0,
            });
            let regular = rasterizer.rasterize(regular_key).expect("draws");
            let bold = rasterizer.rasterize(bold_key).expect("draws");
            let expected = fake_bold_width(size);
            let gain = i64::from(bold.width) - i64::from(regular.width);
            #[expect(clippy::cast_possible_truncation, reason = "a few pixels")]
            let (low, high) = (expected.floor() as i64 - 1, expected.ceil() as i64 + 1);
            assert!(
                (low..=high).contains(&gain),
                "{size}px: width gain {gain} outside [{low}, {high}] for a stroke of {expected}"
            );
            assert!(ink(&bold) > ink(&regular), "{size}px: bold has more ink");
        }
        // The interpolation's end points.
        assert!((fake_bold_width(9.0) - 9.0 / 24.0).abs() < 1e-6);
        assert!((fake_bold_width(36.0) - 36.0 / 32.0).abs() < 1e-6);
        assert!((fake_bold_width(4.0) - 4.0 / 24.0).abs() < 1e-6);
        assert!((fake_bold_width(72.0) - 72.0 / 32.0).abs() < 1e-6);
    }

    fn assert_send<T: Send>() {}

    #[test]
    fn the_rasterizer_draws_on_another_thread() {
        assert_send::<SwashRasterizer>();
        let mut rasterizer = rasterizer();
        let gid = glyph(&rasterizer, 'a');
        let key = ParleyGlyphKey::new(FACE, gid, 16.0, SubpixelBin::Two);
        let here = rasterizer.rasterize(key).expect("draws");
        let there = std::thread::scope(|scope| {
            scope
                .spawn(move || rasterizer.rasterize(key))
                .join()
                .expect("the worker does not panic")
        });
        assert_eq!(there, Some(here));
    }
}
