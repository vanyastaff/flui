//! Text styling types.

use crate::styling::{Color, Shadow};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// The thickness of glyphs used to draw text, on the standard 100–900 scale.
pub enum FontWeight {
    /// Thin (100)
    W100,
    /// Extra-light (200)
    W200,
    /// Light (300)
    W300,
    /// Normal (400)
    W400,
    /// Medium (500)
    W500,
    /// Semi-bold (600)
    W600,
    /// Bold (700)
    W700,
    /// Extra-bold (800)
    W800,
    /// Black (900)
    W900,
}

impl FontWeight {
    /// Normal font weight (400).
    pub const NORMAL: Self = Self::W400;

    /// Bold font weight (700).
    pub const BOLD: Self = Self::W700;

    /// Returns the numeric weight value (100 for `W100` through 900 for `W900`).
    #[must_use]
    #[inline]
    pub const fn value(&self) -> u16 {
        match self {
            Self::W100 => 100,
            Self::W200 => 200,
            Self::W300 => 300,
            Self::W400 => 400,
            Self::W500 => 500,
            Self::W600 => 600,
            Self::W700 => 700,
            Self::W800 => 800,
            Self::W900 => 900,
        }
    }

    /// Returns `true` if this weight renders as bold (600 or heavier).
    #[must_use]
    #[inline]
    pub const fn is_bold(&self) -> bool {
        self.value() >= 600
    }

    /// Converts a CSS numeric weight to the nearest `FontWeight` variant.
    ///
    /// Values round to the closest hundred and clamp to `W100`..=`W900`.
    /// An exact half goes the way CSS Fonts 4's font matching algorithm
    /// searches when a weight is missing: lighter below 400, heavier from
    /// 400 up, so 350 is `W300` and 450 is `W500`. A plain `round()` would
    /// send 350 to `W400`, away from the direction a browser would pick for
    /// a CSS weight.
    #[must_use]
    #[inline]
    pub const fn from_css(value: i32) -> Self {
        match value {
            i32::MIN..=150 => Self::W100,
            151..=250 => Self::W200,
            251..=350 => Self::W300,
            351..=449 => Self::W400,
            450..=549 => Self::W500,
            550..=649 => Self::W600,
            650..=749 => Self::W700,
            750..=849 => Self::W800,
            _ => Self::W900,
        }
    }
}

impl Default for FontWeight {
    #[inline]
    fn default() -> Self {
        Self::NORMAL
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// Whether glyphs are drawn upright or slanted.
pub enum FontStyle {
    /// Upright (non-italic) font style.
    #[default]
    Normal,
    /// Italic font style.
    Italic,
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// An OpenType feature setting (e.g. `"smcp"` for small caps) applied
/// during text shaping.
///
/// Shaping ignores settings whose tag is not four printable ASCII bytes or
/// whose value is outside `0..=65535`. Other valid settings still apply.
pub struct FontFeature {
    /// OpenType feature tag (4 characters).
    pub feature: String,
    /// Feature value (typically 0 or 1).
    pub value: i32,
}

impl FontFeature {
    /// Creates a new font feature.
    #[inline]
    pub fn new(feature: impl Into<String>, value: i32) -> Self {
        Self {
            feature: feature.into(),
            value,
        }
    }

    /// Creates an enabled font feature.
    #[inline]
    pub fn enable(feature: impl Into<String>) -> Self {
        Self::new(feature, 1)
    }

    /// Creates a disabled font feature.
    #[inline]
    pub fn disable(feature: impl Into<String>) -> Self {
        Self::new(feature, 0)
    }
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// A variable-font axis setting (e.g. `"wght"` for weight) applied
/// during text shaping.
///
/// Shaping ignores settings whose tag is not four printable ASCII bytes or
/// whose value is not finite in the shaper's `f32` coordinate space. Unknown
/// valid axis tags are left to the font's shaping implementation.
pub struct FontVariation {
    /// Variation axis tag (4 characters).
    pub axis: String,
    /// Variation axis value.
    pub value: f64,
}

impl FontVariation {
    /// Creates a new font variation.
    #[inline]
    pub fn new(axis: impl Into<String>, value: f64) -> Self {
        Self {
            axis: axis.into(),
            value,
        }
    }
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
    pub shadows: Option<Vec<Shadow<f64>>>,
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
    pub fn with_shadow(mut self, shadow: Shadow<f64>) -> Self {
        self.shadows.get_or_insert_with(Vec::new).push(shadow);
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
        }
    }
}
