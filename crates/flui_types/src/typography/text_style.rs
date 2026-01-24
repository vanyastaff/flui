//! Text styling types.

use crate::Color;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

    #[must_use]
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

    #[must_use]
    pub const fn is_bold(&self) -> bool {
        self.value() >= 600
    }

    #[must_use]
    pub const fn from_css(value: i32) -> Self {
        match value {
            0..=150 => Self::W100,
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
    fn default() -> Self {
        Self::NORMAL
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FontStyle {
    #[default]
    Normal,
    /// Italic font style.
    Italic,
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FontFeature {
    /// OpenType feature tag (4 characters).
    pub feature: String,
    /// Feature value (typically 0 or 1).
    pub value: i32,
}

impl FontFeature {
    /// Creates a new font feature.
    pub fn new(feature: impl Into<String>, value: i32) -> Self {
        Self {
            feature: feature.into(),
            value,
        }
    }

    /// Creates an enabled font feature.
    pub fn enable(feature: impl Into<String>) -> Self {
        Self::new(feature, 1)
    }

    /// Creates a disabled font feature.
    pub fn disable(feature: impl Into<String>) -> Self {
        Self::new(feature, 0)
    }
}

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FontVariation {
    /// Variation axis tag (4 characters).
    pub axis: String,
    /// Variation axis value.
    pub value: f64,
}

impl FontVariation {
    /// Creates a new font variation.
    pub fn new(axis: impl Into<String>, value: f64) -> Self {
        Self {
            axis: axis.into(),
            value,
        }
    }
}

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
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the font family.
    pub fn with_font_family(mut self, font_family: impl Into<String>) -> Self {
        self.font_family = Some(font_family.into());
        self
    }

    /// Sets the font size.
    pub fn with_font_size(mut self, font_size: f64) -> Self {
        self.font_size = Some(font_size);
        self
    }

    /// Sets the line height.
    pub fn with_height(mut self, height: f64) -> Self {
        self.height = Some(height);
        self
    }

    /// Sets whether to force strut height.
    pub fn with_force_strut_height(mut self, force: bool) -> Self {
        self.force_strut_height = force;
        self
    }
}

#[derive(Default, Clone, Debug, PartialEq)]
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
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the text color.
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Sets the font size.
    pub fn with_font_size(mut self, font_size: f64) -> Self {
        self.font_size = Some(font_size);
        self
    }

    /// Sets the font weight.
    pub fn with_font_weight(mut self, font_weight: FontWeight) -> Self {
        self.font_weight = Some(font_weight);
        self
    }

    /// Sets the font style.
    pub fn with_font_style(mut self, font_style: FontStyle) -> Self {
        self.font_style = Some(font_style);
        self
    }

    /// Sets the font family.
    pub fn with_font_family(mut self, font_family: impl Into<String>) -> Self {
        self.font_family = Some(font_family.into());
        self
    }

    /// Sets the letter spacing.
    pub fn with_letter_spacing(mut self, letter_spacing: f64) -> Self {
        self.letter_spacing = Some(letter_spacing);
        self
    }

    /// Sets the word spacing.
    pub fn with_word_spacing(mut self, word_spacing: f64) -> Self {
        self.word_spacing = Some(word_spacing);
        self
    }

    /// Sets the line height.
    pub fn with_height(mut self, height: f64) -> Self {
        self.height = Some(height);
        self
    }

    /// Adds a font feature.
    pub fn with_font_feature(mut self, feature: FontFeature) -> Self {
        self.font_features.push(feature);
        self
    }

    /// Adds a font variation.
    pub fn with_font_variation(mut self, variation: FontVariation) -> Self {
        self.font_variations.push(variation);
        self
    }

    /// Adds a shadow.
    pub fn with_shadow(mut self, shadow: TextShadow) -> Self {
        self.shadows.push(shadow);
        self
    }

    /// Merges this style with another, with the other taking precedence.
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
    pub fn new(color: Color, offset_x: f64, offset_y: f64, blur_radius: f64) -> Self {
        Self {
            color,
            offset_x,
            offset_y,
            blur_radius,
        }
    }
}
