//! Text styling types.

use std::num::NonZeroU16;

use crate::styling::{Color, Shadow};
use crate::typography::{FontFeature, FontVariation, TextDecoration};

/// The thickness of glyphs, on the CSS Fonts 4 scale of 1 to 1000.
///
/// A static font answers with its nearest face, as CSS font matching picks
/// it; a variable font draws the weight itself, so 350 or 550 reach its
/// `wght` axis. [`FontWeight::W100`] through [`FontWeight::W900`] name the
/// standard steps.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(try_from = "u16", into = "u16")
)]
pub struct FontWeight(NonZeroU16);

// The zero niche keeps `Option<FontWeight>`, which every `TextStyle` holds, at
// two bytes: a plain `u16` would grow each style and its clones.
const _: () = assert!(size_of::<Option<FontWeight>>() == size_of::<u16>());

/// A font weight outside the CSS range of 1 to 1000.
#[derive(Copy, Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("font weight {0} is outside 1..=1000")]
pub struct InvalidFontWeight(pub u16);

impl FontWeight {
    /// Thin (100).
    pub const W100: Self = Self::in_range(100);
    /// Extra-light (200).
    pub const W200: Self = Self::in_range(200);
    /// Light (300).
    pub const W300: Self = Self::in_range(300);
    /// Normal (400).
    pub const W400: Self = Self::in_range(400);
    /// Medium (500).
    pub const W500: Self = Self::in_range(500);
    /// Semi-bold (600).
    pub const W600: Self = Self::in_range(600);
    /// Bold (700).
    pub const W700: Self = Self::in_range(700);
    /// Extra-bold (800).
    pub const W800: Self = Self::in_range(800);
    /// Black (900).
    pub const W900: Self = Self::in_range(900);

    /// Normal font weight (400).
    pub const NORMAL: Self = Self::W400;

    /// Bold font weight (700).
    pub const BOLD: Self = Self::W700;

    /// The lightest weight CSS admits.
    pub const MIN: Self = Self::in_range(1);

    /// The heaviest weight CSS admits.
    pub const MAX: Self = Self::in_range(1000);

    /// The weight `value`, which CSS admits from 1 to 1000.
    ///
    /// # Errors
    ///
    /// [`InvalidFontWeight`] when `value` is 0 or above 1000.
    pub const fn try_new(value: u16) -> Result<Self, InvalidFontWeight> {
        if value >= Self::MIN.value() && value <= Self::MAX.value() {
            Ok(Self::in_range(value))
        } else {
            Err(InvalidFontWeight(value))
        }
    }

    /// `value`, which the caller has checked lies in 1..=1000. A zero in a
    /// `const` fails to compile.
    const fn in_range(value: u16) -> Self {
        Self(NonZeroU16::MIN.saturating_add(value - 1))
    }

    /// The numeric weight, 1 to 1000.
    #[must_use]
    #[inline]
    pub const fn value(self) -> u16 {
        self.0.get()
    }

    /// The weight nearest `value`, held inside 1..=1000: what an
    /// interpolation that overshoots its ends draws. `None` for NaN, which
    /// names no weight; the caller keeps the nearer end.
    #[must_use]
    pub(crate) fn saturating(value: f64) -> Option<Self> {
        if value.is_nan() {
            return None;
        }
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "clamped into 1..=1000 first, so the cast is exact"
        )]
        let weight = value.round().clamp(1.0, 1000.0) as u16;
        Some(Self::in_range(weight))
    }
}

impl Default for FontWeight {
    #[inline]
    fn default() -> Self {
        Self::NORMAL
    }
}

impl TryFrom<u16> for FontWeight {
    type Error = InvalidFontWeight;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

impl From<FontWeight> for u16 {
    fn from(weight: FontWeight) -> Self {
        weight.value()
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// Whether glyphs are drawn upright or slanted.
#[non_exhaustive]
pub enum FontStyle {
    /// Upright (non-italic) font style.
    #[default]
    Normal,
    /// Italic font style.
    Italic,
}

/// Defines a strut: a minimum line-height scaffold that vertical text
/// metrics are laid out against, independent of the actual glyphs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StrutStyle {
    /// Font family name.
    pub font_family: Option<String>,
    /// Font families to fall back to.
    pub font_family_fallback: Vec<String>,
    /// Font size.
    pub font_size: Option<f64>,
    /// Line height multiplier.
    pub height: Option<f64>,
    /// Leading distribution.
    pub leading: Option<f64>,
    /// Font weight.
    pub font_weight: Option<FontWeight>,
    /// Font style.
    pub font_style: Option<FontStyle>,
    /// Whether to force strut height.
    pub force_strut_height: bool,
}

impl StrutStyle {
    /// Creates a new strut style.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the font family.
    #[inline]
    pub fn with_font_family(mut self, font_family: impl Into<String>) -> Self {
        self.font_family = Some(font_family.into());
        self
    }

    /// Sets the font size.
    #[inline]
    pub fn with_font_size(mut self, font_size: f64) -> Self {
        self.font_size = Some(font_size);
        self
    }

    /// Sets the line height.
    #[inline]
    pub fn with_height(mut self, height: f64) -> Self {
        self.height = Some(height);
        self
    }

    /// Sets whether to force strut height.
    #[inline]
    pub fn with_force_strut_height(mut self, force: bool) -> Self {
        self.force_strut_height = force;
        self
    }
}

#[derive(Default, Clone, Debug, PartialEq)]
/// Visual and layout styling to apply to a span of text.
///
/// All fields are optional; unset fields inherit from an enclosing style
/// via `merge`. Use `layout_affecting_eq` to compare only the fields that
/// influence glyph shaping and layout.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextStyle {
    /// The colour the glyphs paint in.
    pub color: Option<Color>,
    /// The colour filled behind the span: one rectangle per line box of its
    /// text, painted under its shadows and glyphs (ADR-0309).
    pub background_color: Option<Color>,
    /// Font size.
    pub font_size: Option<f64>,
    /// Numeric growth intent; absent values inherit independently of size.
    #[cfg_attr(feature = "serde", serde(default))]
    pub sizing: Option<flui_foundation::TextSizingIntent>,
    /// Font weight.
    pub font_weight: Option<FontWeight>,
    /// Font style.
    pub font_style: Option<FontStyle>,
    /// Letter spacing.
    pub letter_spacing: Option<f64>,
    /// Word spacing.
    pub word_spacing: Option<f64>,
    /// Line height multiplier.
    pub height: Option<f64>,
    /// Font family name.
    pub font_family: Option<String>,
    /// Font families to fall back to.
    pub font_family_fallback: Vec<String>,
    /// Font features.
    pub font_features: Vec<FontFeature>,
    /// Font variations.
    pub font_variations: Vec<FontVariation>,
    /// The shadows the span's glyphs cast, painted in list order under the
    /// glyphs: each is the glyphs' silhouette in the shadow's colour, moved
    /// by its offset and blurred by its blur radius (ADR-0309). `None`
    /// inherits the enclosing style's; an empty list casts none.
    pub shadows: Option<Vec<Shadow>>,
    /// The lines drawn along the span's text, where its font puts them
    /// (ADR-0310). `None` inherits the enclosing style's;
    /// [`TextDecoration::NONE`] draws none.
    #[cfg_attr(feature = "serde", serde(default))]
    pub decoration: Option<TextDecoration>,
}

impl TextStyle {
    /// Creates a new text style.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the two styles shape the same glyph geometry: the same font
    /// selection (family, fallback, weight, style, features, variations),
    /// font size and sizing, spacing and line height.
    ///
    /// Colour is not compared here. A run carries its span's colour, so
    /// [`TextPainter::set_text`](crate::TextPainter::set_text) compares the
    /// span colours itself, and only a root recolour keeps the shaped
    /// layout. Every field is named below, so a field added to `TextStyle`
    /// does not compile until it is classified here.
    #[must_use]
    pub fn layout_affecting_eq(&self, other: &Self) -> bool {
        let Self {
            font_size,
            sizing,
            font_weight,
            font_style,
            letter_spacing,
            word_spacing,
            height,
            font_family,
            font_family_fallback,
            font_features,
            font_variations,
            // The glyph colour is a run's brush, not geometry; `set_text`
            // compares the span colours the runs carry. Another attribute
            // baked into a run's brush must join that comparison.
            color: _,
            // Paint only: `SpanPaint::of` reads them over the kept layout.
            background_color: _,
            shadows: _,
            decoration: _,
        } = self;
        *font_size == other.font_size
            && *sizing == other.sizing
            && *font_weight == other.font_weight
            && *font_style == other.font_style
            && *letter_spacing == other.letter_spacing
            && *word_spacing == other.word_spacing
            && *height == other.height
            && *font_family == other.font_family
            && *font_family_fallback == other.font_family_fallback
            && *font_features == other.font_features
            && *font_variations == other.font_variations
    }

    /// Sets the text color.
    #[inline]
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Sets the font size.
    #[inline]
    pub fn with_font_size(mut self, font_size: f64) -> Self {
        self.font_size = Some(font_size);
        self
    }

    /// Sets numeric growth intent without selecting a font or authored size.
    #[must_use]
    pub fn with_sizing(mut self, sizing: flui_foundation::TextSizingIntent) -> Self {
        self.sizing = Some(sizing);
        self
    }

    /// Sets the font weight.
    #[inline]
    pub fn with_font_weight(mut self, font_weight: FontWeight) -> Self {
        self.font_weight = Some(font_weight);
        self
    }

    /// Sets the font style.
    #[inline]
    pub fn with_font_style(mut self, font_style: FontStyle) -> Self {
        self.font_style = Some(font_style);
        self
    }

    /// Sets the font family.
    #[inline]
    pub fn with_font_family(mut self, font_family: impl Into<String>) -> Self {
        self.font_family = Some(font_family.into());
        self
    }

    /// Sets the letter spacing.
    #[inline]
    pub fn with_letter_spacing(mut self, letter_spacing: f64) -> Self {
        self.letter_spacing = Some(letter_spacing);
        self
    }

    /// Sets the word spacing.
    #[inline]
    pub fn with_word_spacing(mut self, word_spacing: f64) -> Self {
        self.word_spacing = Some(word_spacing);
        self
    }

    /// Sets the line height.
    #[inline]
    pub fn with_height(mut self, height: f64) -> Self {
        self.height = Some(height);
        self
    }

    /// Adds a font feature.
    #[inline]
    pub fn with_font_feature(mut self, feature: FontFeature) -> Self {
        self.font_features.push(feature);
        self
    }

    /// Adds a font variation.
    #[inline]
    pub fn with_font_variation(mut self, variation: FontVariation) -> Self {
        self.font_variations.push(variation);
        self
    }

    /// Adds a shadow after the ones the style already casts.
    #[inline]
    pub fn with_shadow(mut self, shadow: Shadow) -> Self {
        self.shadows.get_or_insert_with(Vec::new).push(shadow);
        self
    }

    /// Sets the lines drawn along the text; [`TextDecoration::NONE`] clears
    /// an inherited decoration.
    #[inline]
    pub fn with_decoration(mut self, decoration: TextDecoration) -> Self {
        self.decoration = Some(decoration);
        self
    }

    /// Merges this style with another, with the other taking precedence.
    #[inline]
    pub fn merge(&self, other: &TextStyle) -> Self {
        Self {
            color: other.color.or(self.color),
            background_color: other.background_color.or(self.background_color),
            font_size: other.font_size.or(self.font_size),
            sizing: other.sizing.or(self.sizing),
            font_weight: other.font_weight.or(self.font_weight),
            font_style: other.font_style.or(self.font_style),
            letter_spacing: other.letter_spacing.or(self.letter_spacing),
            word_spacing: other.word_spacing.or(self.word_spacing),
            height: other.height.or(self.height),
            font_family: other
                .font_family
                .clone()
                .or_else(|| self.font_family.clone()),
            font_family_fallback: if other.font_family_fallback.is_empty() {
                self.font_family_fallback.clone()
            } else {
                other.font_family_fallback.clone()
            },
            font_features: if other.font_features.is_empty() {
                self.font_features.clone()
            } else {
                other.font_features.clone()
            },
            font_variations: if other.font_variations.is_empty() {
                self.font_variations.clone()
            } else {
                other.font_variations.clone()
            },
            shadows: other.shadows.clone().or_else(|| self.shadows.clone()),
            decoration: other.decoration.or(self.decoration),
        }
    }
}
