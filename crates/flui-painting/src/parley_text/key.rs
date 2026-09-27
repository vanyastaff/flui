//! The glyph key: a face named by its font blob, not by a process-global
//! database id (ADR-0092 §5).

use std::num::NonZeroU32;

/// One face: a font blob's id and the face index inside it (a TTC holds
/// several).
///
/// `blob_id` must name the same bytes for as long as a registry holds the
/// face.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FaceKey {
    /// The blob's id.
    pub blob_id: u64,
    /// The face index inside the blob.
    pub index: u32,
}

/// An interned set of normalized variation coordinates; minted by, and
/// meaningful only to, the [`FontRegistry`](super::FontRegistry) that
/// interned it. `None` in a key is the default instance.
///
/// The id carries its registry's identity, so another registry does not
/// resolve it to whichever instance it interned at the same index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VariationId {
    registry: u64,
    index: NonZeroU32,
}

impl VariationId {
    /// The id for the `index`-th instance (0-based) interned by `registry`.
    pub(super) fn from_index(registry: u64, index: usize) -> Option<Self> {
        u32::try_from(index)
            .ok()
            .and_then(|i| i.checked_add(1))
            .and_then(NonZeroU32::new)
            .map(|index| Self { registry, index })
    }

    /// The identity of the registry that minted this id.
    pub(super) const fn registry(self) -> u64 {
        self.registry
    }

    /// The 0-based index this id was minted from.
    pub(super) fn index(self) -> usize {
        // A `u32` always fits `usize` on the targets this crate builds for.
        self.index.get() as usize - 1
    }
}

/// Horizontal quarter-pixel bin: cosmic-text's rule, so both paths draw the
/// same bitmap for the same glyph at the same position.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SubpixelBin {
    /// Offset 0.0.
    #[default]
    Zero,
    /// Offset 0.25.
    One,
    /// Offset 0.5.
    Two,
    /// Offset 0.75.
    Three,
}

impl SubpixelBin {
    /// Whole device pixel and bin; NaN maps to `(0, Zero)`.
    ///
    /// Truncates toward zero and then bins the fraction, with the same edges
    /// as `cosmic_text::SubpixelBin::new`.
    #[must_use]
    pub fn split(position: f32) -> (i32, Self) {
        if position.is_nan() {
            return (0, Self::Zero);
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a device coordinate fits i32; truncation toward zero is the bin rule"
        )]
        let trunc = position as i32;
        #[expect(
            clippy::cast_precision_loss,
            reason = "the truncated coordinate came from an f32"
        )]
        let fract = position - trunc as f32;
        if position.is_sign_negative() {
            if fract > -0.125 {
                (trunc, Self::Zero)
            } else if fract > -0.375 {
                (trunc - 1, Self::Three)
            } else if fract > -0.625 {
                (trunc - 1, Self::Two)
            } else if fract > -0.875 {
                (trunc - 1, Self::One)
            } else {
                (trunc - 1, Self::Zero)
            }
        } else if fract < 0.125 {
            (trunc, Self::Zero)
        } else if fract < 0.375 {
            (trunc, Self::One)
        } else if fract < 0.625 {
            (trunc, Self::Two)
        } else if fract < 0.875 {
            (trunc, Self::Three)
        } else {
            (trunc + 1, Self::Zero)
        }
    }

    /// 0.0, 0.25, 0.5 or 0.75.
    #[must_use]
    pub const fn offset(self) -> f32 {
        match self {
            Self::Zero => 0.0,
            Self::One => 0.25,
            Self::Two => 0.5,
            Self::Three => 0.75,
        }
    }
}

/// Synthetic styling applied at raster time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Synthesis {
    /// Synthetic bold (painting ARCHITECTURE, mapping decision 10).
    pub embolden: bool,
    /// Synthetic oblique angle in whole degrees; `0` for none. A key skewed
    /// past [`Self::MAX_SKEW_DEGREES`] either way is not rasterized.
    pub skew_degrees: i8,
}

impl Synthesis {
    /// The steepest skew a rasterizer draws. Near 90° the shear's tangent
    /// grows without bound, and so would the bitmap; cosmic-text's fake
    /// italic is 14°.
    pub const MAX_SKEW_DEGREES: u8 = 45;
}

/// Identifies one rasterized bitmap (ADR-0092 §5). Names no shaper and no
/// process table.
///
/// Two equal keys rasterize to equal bitmaps. There is no vertical bin: the
/// cosmic-text path truncates a glyph's row before binning, so its vertical
/// bin is always zero, and this key keeps that.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ParleyGlyphKey {
    face: FaceKey,
    glyph_id: u16,
    size_bits: u32,
    variation: Option<VariationId>,
    x_bin: SubpixelBin,
    hinted: bool,
    synthesis: Synthesis,
}

impl ParleyGlyphKey {
    /// Hinted, default instance, no synthesis. `size` is in device pixels and
    /// kept exactly (bit equality); `-0.0` is stored as `0.0`.
    #[must_use]
    pub fn new(face: FaceKey, glyph_id: u16, size: f32, x_bin: SubpixelBin) -> Self {
        // `-0.0 + 0.0` is `+0.0`; every other value is unchanged.
        let size = size + 0.0;
        Self {
            face,
            glyph_id,
            size_bits: size.to_bits(),
            variation: None,
            x_bin,
            hinted: true,
            synthesis: Synthesis {
                embolden: false,
                skew_degrees: 0,
            },
        }
    }

    /// This key at the variation instance `variation` (`None`: the default).
    #[must_use]
    pub const fn with_variation(mut self, variation: Option<VariationId>) -> Self {
        self.variation = variation;
        self
    }

    /// This key with `synthesis` applied.
    #[must_use]
    pub const fn with_synthesis(mut self, synthesis: Synthesis) -> Self {
        self.synthesis = synthesis;
        self
    }

    /// This key, hinted or not.
    #[must_use]
    pub const fn with_hinting(mut self, hinted: bool) -> Self {
        self.hinted = hinted;
        self
    }

    /// The face.
    #[must_use]
    pub const fn face(self) -> FaceKey {
        self.face
    }

    /// The glyph index inside the face.
    #[must_use]
    pub const fn glyph_id(self) -> u16 {
        self.glyph_id
    }

    /// The size in device pixels.
    #[must_use]
    pub const fn size(self) -> f32 {
        f32::from_bits(self.size_bits)
    }

    /// The variation instance; `None` is the default.
    #[must_use]
    pub const fn variation(self) -> Option<VariationId> {
        self.variation
    }

    /// The horizontal subpixel bin.
    #[must_use]
    pub const fn x_bin(self) -> SubpixelBin {
        self.x_bin
    }

    /// Whether the outline is hinted.
    #[must_use]
    pub const fn hinted(self) -> bool {
        self.hinted
    }

    /// The synthetic styling.
    #[must_use]
    pub const fn synthesis(self) -> Synthesis {
        self.synthesis
    }
}

#[cfg(test)]
mod tests {
    use super::{FaceKey, ParleyGlyphKey, SubpixelBin};

    fn cosmic(bin: cosmic_text::SubpixelBin) -> SubpixelBin {
        match bin {
            cosmic_text::SubpixelBin::Zero => SubpixelBin::Zero,
            cosmic_text::SubpixelBin::One => SubpixelBin::One,
            cosmic_text::SubpixelBin::Two => SubpixelBin::Two,
            cosmic_text::SubpixelBin::Three => SubpixelBin::Three,
        }
    }

    /// Checked against cosmic-text's own function, not a copied table: the
    /// two paths must pick the same bitmap for the same position.
    #[test]
    fn subpixel_bins_match_cosmic_text() {
        for step in -256..=256i16 {
            let position = f32::from(step) / 64.0;
            let (pixel, bin) = cosmic_text::SubpixelBin::new(position);
            assert_eq!(
                SubpixelBin::split(position),
                (pixel, cosmic(bin)),
                "position {position}"
            );
            assert!(
                (SubpixelBin::split(position).1.offset() - bin.as_float()).abs() < f32::EPSILON,
                "offset of the bin at {position}"
            );
        }
        assert_eq!(SubpixelBin::split(f32::NAN), (0, SubpixelBin::Zero));
        assert_eq!(SubpixelBin::split(-f32::NAN), (0, SubpixelBin::Zero));
    }

    #[test]
    fn negative_zero_size_is_one_key() {
        let face = FaceKey {
            blob_id: 7,
            index: 0,
        };
        let positive = ParleyGlyphKey::new(face, 3, 0.0, SubpixelBin::Zero);
        let negative = ParleyGlyphKey::new(face, 3, -0.0, SubpixelBin::Zero);
        assert_eq!(positive, negative);
        assert_eq!(negative.size().to_bits(), 0.0f32.to_bits());
        assert_ne!(
            ParleyGlyphKey::new(face, 3, 12.0, SubpixelBin::Zero),
            ParleyGlyphKey::new(face, 3, 12.000_001, SubpixelBin::Zero),
            "sizes are kept exactly, not quantised"
        );
    }
}
