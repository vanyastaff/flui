//! Text styling types.

use crate::Color;

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
    /// Values are bucketed to the closest hundred; out-of-range values
    /// clamp to `W100` or `W900`.
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
/// metrics are laid out against, independent of the actual glyphs
/// (mirrors Flutter's `StrutStyle`).
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
/// Visual and layout styling to apply to a span of text (mirrors
/// Flutter's `TextStyle`).
///
/// All fields are optional; unset fields inherit from an enclosing style
/// via `merge`. Use `layout_affecting_eq` to compare only the fields that
/// influence glyph shaping and layout.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextStyle {
    /// Text color.
    pub color: Option<Color>,
    /// Background color.
    pub background_color: Option<Color>,
    /// Font size.
    pub font_size: Option<f64>,
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
    /// Foreground paint (takes precedence over color).
    pub foreground: Option<Color>,
    /// Background paint (takes precedence over background_color).
    pub background: Option<Color>,
    /// Shadows.
    pub shadows: Vec<TextShadow>,
}

impl TextStyle {
    /// Creates a new text style.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Compares only the fields that affect SHAPING/LAYOUT — font
    /// selection (family/fallback/weight/style/features/variations),
    /// font size, spacing, and line height.
    ///
    /// Two styles that differ only in paint attributes (colors,
    /// foreground/background paints, shadows) produce byte-identical
    /// glyph geometry, so a text engine may keep its shaped layout and
    /// only re-emit draw commands. This is the single source of truth
    /// for that partition: a new `TextStyle` field MUST be classified
    /// here as layout-affecting or paint-only when it is added.
    #[must_use]
    pub fn layout_affecting_eq(&self, other: &Self) -> bool {
        self.font_size == other.font_size
            && self.font_weight == other.font_weight
            && self.font_style == other.font_style
            && self.letter_spacing == other.letter_spacing
            && self.word_spacing == other.word_spacing
            && self.height == other.height
            && self.font_family == other.font_family
            && self.font_family_fallback == other.font_family_fallback
            && self.font_features == other.font_features
            && self.font_variations == other.font_variations
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

    /// Adds a shadow.
    #[inline]
    pub fn with_shadow(mut self, shadow: TextShadow) -> Self {
        self.shadows.push(shadow);
        self
    }

    /// Merges this style with another, with the other taking precedence.
    #[inline]
    pub fn merge(&self, other: &TextStyle) -> Self {
        Self {
            color: other.color.or(self.color),
            background_color: other.background_color.or(self.background_color),
            font_size: other.font_size.or(self.font_size),
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
            foreground: other.foreground.or(self.foreground),
            background: other.background.or(self.background),
            shadows: if other.shadows.is_empty() {
                self.shadows.clone()
            } else {
                other.shadows.clone()
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
/// A single shadow cast by text.
pub struct TextShadow {
    /// Shadow color.
    pub color: Color,
    /// Horizontal offset.
    pub offset_x: f64,
    /// Vertical offset.
    pub offset_y: f64,
    /// Blur radius.
    pub blur_radius: f64,
}

impl TextShadow {
    /// Creates a new text shadow.
    #[inline]
    pub fn new(color: Color, offset_x: f64, offset_y: f64, blur_radius: f64) -> Self {
        Self {
            color,
            offset_x,
            offset_y,
            blur_radius,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typography::TextOverflow;

    const WEIGHTS: [FontWeight; 9] = [
        FontWeight::W100,
        FontWeight::W200,
        FontWeight::W300,
        FontWeight::W400,
        FontWeight::W500,
        FontWeight::W600,
        FontWeight::W700,
        FontWeight::W800,
        FontWeight::W900,
    ];

    #[test]
    fn font_weight_values() {
        for (i, w) in WEIGHTS.into_iter().enumerate() {
            let value = 100 * (i as u16 + 1);
            assert_eq!(w.value(), value, "{w:?}");
            assert_eq!(w.is_bold(), value >= 600, "{w:?}");
            assert_eq!(FontWeight::from_css(i32::from(value)), w, "{w:?}");
        }
        assert_eq!(
            (FontWeight::NORMAL, FontWeight::BOLD),
            (FontWeight::W400, FontWeight::W700)
        );
        assert_eq!(FontWeight::default(), FontWeight::NORMAL);
    }

    /// Each bucket's edges. Exact ties fall down below 400 and up above it
    /// (350 is W300, 550 is W600); out of range clamps to either end.
    #[test]
    fn font_weight_from_css_buckets() {
        use FontWeight::*;
        for (css, w) in [
            (i32::MIN, W100),
            (-5, W100),
            (0, W100),
            (150, W100),
            (151, W200),
            (250, W200),
            (251, W300),
            (350, W300),
            (351, W400),
            (449, W400),
            (450, W500),
            (549, W500),
            (550, W600),
            (649, W600),
            (650, W700),
            (749, W700),
            (750, W800),
            (849, W800),
            (850, W900),
            (i32::MAX, W900),
        ] {
            assert_eq!(FontWeight::from_css(css), w, "{css}");
        }
    }

    #[test]
    fn features_variations_and_defaults() {
        assert_eq!(FontFeature::enable("liga"), FontFeature::new("liga", 1));
        assert_eq!(FontFeature::disable("liga"), FontFeature::new("liga", 0));
        let v = FontVariation::new("wght", 650.0);
        assert_eq!((v.axis.as_str(), v.value), ("wght", 650.0));
        assert_eq!(FontStyle::default(), FontStyle::Normal);
        assert!(matches!(TextOverflow::default(), TextOverflow::Clip));
        let shadow = TextShadow::new(Color::RED, 1.0, 2.0, 3.0);
        assert_eq!(
            (shadow.offset_x, shadow.offset_y, shadow.blur_radius),
            (1.0, 2.0, 3.0)
        );
    }

    #[test]
    fn strut_style_builders() {
        let s = StrutStyle::new()
            .with_font_family("Inter")
            .with_font_size(14.0)
            .with_height(1.5)
            .with_force_strut_height(true);
        assert_eq!(s.font_family.as_deref(), Some("Inter"));
        assert_eq!(
            (s.font_size, s.height, s.force_strut_height),
            (Some(14.0), Some(1.5), true)
        );
        assert_eq!(StrutStyle::new(), StrutStyle::default());
    }

    fn full() -> TextStyle {
        TextStyle::new()
            .with_color(Color::RED)
            .with_font_size(14.0)
            .with_font_weight(FontWeight::BOLD)
            .with_font_style(FontStyle::Italic)
            .with_font_family("Inter")
            .with_letter_spacing(0.5)
            .with_word_spacing(1.0)
            .with_height(1.25)
            .with_font_feature(FontFeature::enable("liga"))
            .with_font_variation(FontVariation::new("wght", 650.0))
            .with_shadow(TextShadow::new(Color::BLACK, 1.0, 1.0, 2.0))
    }

    #[test]
    fn builders_set_their_field() {
        let s = full();
        assert_eq!(s.color, Some(Color::RED));
        assert_eq!(s.font_size, Some(14.0));
        assert_eq!(s.font_weight, Some(FontWeight::BOLD));
        assert_eq!(s.font_style, Some(FontStyle::Italic));
        assert_eq!(s.font_family.as_deref(), Some("Inter"));
        assert_eq!(
            (s.letter_spacing, s.word_spacing, s.height),
            (Some(0.5), Some(1.0), Some(1.25))
        );
        assert_eq!(s.font_features, vec![FontFeature::enable("liga")]);
        assert_eq!(s.font_variations, vec![FontVariation::new("wght", 650.0)]);
        assert_eq!(
            s.shadows,
            vec![TextShadow::new(Color::BLACK, 1.0, 1.0, 2.0)]
        );
        assert_eq!(TextStyle::new(), TextStyle::default());
    }

    /// Every layout field breaks layout equality on its own; paint-only
    /// fields never do.
    #[test]
    fn layout_affecting_eq_per_field() {
        let base = full();
        assert!(base.layout_affecting_eq(&base.clone()));
        let layout_changes = [
            TextStyle {
                font_size: Some(15.0),
                ..full()
            },
            TextStyle {
                font_weight: None,
                ..full()
            },
            TextStyle {
                font_style: None,
                ..full()
            },
            TextStyle {
                letter_spacing: None,
                ..full()
            },
            TextStyle {
                word_spacing: None,
                ..full()
            },
            TextStyle {
                height: None,
                ..full()
            },
            TextStyle {
                font_family: None,
                ..full()
            },
            TextStyle {
                font_family_fallback: vec!["x".into()],
                ..full()
            },
            TextStyle {
                font_features: vec![],
                ..full()
            },
            TextStyle {
                font_variations: vec![],
                ..full()
            },
        ];
        for changed in &layout_changes {
            assert!(!base.layout_affecting_eq(changed), "{changed:?}");
        }
        let paint_changes = [
            TextStyle {
                color: None,
                ..full()
            },
            TextStyle {
                background_color: Some(Color::BLUE),
                ..full()
            },
            TextStyle {
                foreground: Some(Color::BLUE),
                ..full()
            },
            TextStyle {
                background: Some(Color::BLUE),
                ..full()
            },
            TextStyle {
                shadows: vec![],
                ..full()
            },
        ];
        for changed in &paint_changes {
            assert!(base.layout_affecting_eq(changed), "{changed:?}");
        }
    }

    /// `other` wins wherever it sets something; lists replace rather than
    /// append, and only when non-empty.
    #[test]
    fn merge() {
        let base = TextStyle {
            font_family_fallback: vec!["Noto".into()],
            ..full()
        };
        assert_eq!(base.merge(&TextStyle::new()), base);
        assert_eq!(TextStyle::new().merge(&base), base);

        let over = TextStyle {
            color: Some(Color::BLUE),
            background_color: Some(Color::GREEN),
            font_size: Some(20.0),
            font_weight: Some(FontWeight::W300),
            font_style: Some(FontStyle::Normal),
            letter_spacing: Some(2.0),
            word_spacing: Some(3.0),
            height: Some(2.0),
            font_family: Some("Mono".into()),
            font_family_fallback: vec!["Fallback".into()],
            font_features: vec![FontFeature::disable("kern")],
            font_variations: vec![FontVariation::new("opsz", 12.0)],
            foreground: Some(Color::WHITE),
            background: Some(Color::BLACK),
            shadows: vec![TextShadow::new(Color::RED, 0.0, 0.0, 1.0)],
        };
        assert_eq!(base.merge(&over), over);
        let back = TextStyle {
            foreground: Some(Color::GREEN),
            background: Some(Color::GRAY),
            ..full()
        };
        let merged = back.merge(&TextStyle::new());
        assert_eq!(
            (merged.foreground, merged.background),
            (Some(Color::GREEN), Some(Color::GRAY))
        );
    }
}
