//! Text style types
//!
//! This module contains types for representing text styling,
//! combining font, color, and decoration properties.

use crate::types::core::color::Color;
use crate::types::typography::font::{FontFamily, FontSize, FontWeight, LineHeight};
use crate::types::styling::shadow::Shadow;

/// The style in which to draw a text decoration line.
///
/// Similar to CSS `text-decoration-style`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextDecorationStyle {
    /// Draw a solid line.
    Solid,
    /// Draw two lines.
    Double,
    /// Draw a dotted line.
    Dotted,
    /// Draw a dashed line.
    Dashed,
    /// Draw a wavy line.
    Wavy,
}

impl Default for TextDecorationStyle {
    fn default() -> Self {
        TextDecorationStyle::Solid
    }
}

impl std::fmt::Display for TextDecorationStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TextDecorationStyle::Solid => write!(f, "solid"),
            TextDecorationStyle::Double => write!(f, "double"),
            TextDecorationStyle::Dotted => write!(f, "dotted"),
            TextDecorationStyle::Dashed => write!(f, "dashed"),
            TextDecorationStyle::Wavy => write!(f, "wavy"),
        }
    }
}

/// Complete text style configuration.
///
/// Similar to Flutter's TextStyle, combining all text appearance properties.
#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    /// Font family
    pub family: FontFamily,
    /// Font size
    pub size: FontSize,
    /// Font weight
    pub weight: FontWeight,
    /// Text color
    pub color: Color,
    /// Line height
    pub line_height: LineHeight,
    /// Text decoration (underline, strikethrough, etc)
    pub decoration: TextDecoration,
    /// Decoration style (solid, double, dotted, dashed, wavy)
    pub decoration_style: TextDecorationStyle,
    /// Decoration color (if different from text color)
    pub decoration_color: Option<Color>,
    /// Letter spacing
    pub letter_spacing: f32,
    /// Word spacing
    pub word_spacing: f32,
    /// Text shadow
    pub shadow: Option<Shadow>,
    /// Whether to use italic style
    pub italic: bool,
}

impl TextStyle {
    /// Create a new text style with default values.
    pub fn new() -> Self {
        Self {
            family: FontFamily::default(),
            size: FontSize::default(),
            weight: FontWeight::default(),
            color: Color::BLACK,
            line_height: LineHeight::default(),
            decoration: TextDecoration::None,
            decoration_style: TextDecorationStyle::default(),
            decoration_color: None,
            letter_spacing: 0.0,
            word_spacing: 0.0,
            shadow: None,
            italic: false,
        }
    }

    /// Builder: set font family.
    pub fn with_family(mut self, family: FontFamily) -> Self {
        self.family = family;
        self
    }

    /// Builder: set font size.
    pub fn with_size(mut self, size: FontSize) -> Self {
        self.size = size;
        self
    }

    /// Builder: set font weight.
    pub fn with_weight(mut self, weight: FontWeight) -> Self {
        self.weight = weight;
        self
    }

    /// Builder: set text color.
    pub fn with_color(mut self, color: impl Into<Color>) -> Self {
        self.color = color.into();
        self
    }

    /// Builder: set line height.
    pub fn with_line_height(mut self, line_height: LineHeight) -> Self {
        self.line_height = line_height;
        self
    }

    /// Builder: set text decoration.
    pub fn with_decoration(mut self, decoration: TextDecoration) -> Self {
        self.decoration = decoration;
        self
    }

    /// Builder: set decoration style.
    pub fn with_decoration_style(mut self, style: TextDecorationStyle) -> Self {
        self.decoration_style = style;
        self
    }

    /// Builder: set decoration color.
    pub fn with_decoration_color(mut self, color: impl Into<Color>) -> Self {
        self.decoration_color = Some(color.into());
        self
    }

    /// Builder: set letter spacing.
    pub fn with_letter_spacing(mut self, spacing: f32) -> Self {
        self.letter_spacing = spacing;
        self
    }

    /// Builder: set word spacing.
    pub fn with_word_spacing(mut self, spacing: f32) -> Self {
        self.word_spacing = spacing;
        self
    }

    /// Builder: set text shadow.
    pub fn with_shadow(mut self, shadow: Shadow) -> Self {
        self.shadow = Some(shadow);
        self
    }

    /// Builder: set italic.
    pub fn with_italic(mut self, italic: bool) -> Self {
        self.italic = italic;
        self
    }

    /// Create a bold variant of this style.
    pub fn bold(&self) -> Self {
        let mut style = self.clone();
        style.weight = FontWeight::Bold;
        style
    }

    /// Create an italic variant of this style.
    pub fn italic(&self) -> Self {
        let mut style = self.clone();
        style.italic = true;
        style
    }

    /// Scale the font size.
    pub fn scale(&self, factor: f32) -> Self {
        let mut style = self.clone();
        style.size = self.size.scale(factor);
        style
    }

    /// Copy with a different color.
    pub fn with_different_color(&self, color: impl Into<Color>) -> Self {
        let mut style = self.clone();
        style.color = color.into();
        style
    }

    /// Merge this style with another, with other's properties taking precedence.
    ///
    /// This is used for TextSpan inheritance where child styles override parent styles.
    /// For now, this is a simple implementation - child completely overrides parent.
    pub fn merge(&self, other: &TextStyle) -> TextStyle {
        // Simple merge: child overrides all properties
        // In future, we might want smarter merging (e.g., preserve some parent properties)
        other.clone()
    }
}

impl Default for TextStyle {
    fn default() -> Self {
        Self::new()
    }
}

/// Text decoration type.
///
/// Similar to CSS text-decoration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextDecoration {
    /// No decoration
    None,
    /// Underline
    Underline,
    /// Line through (strikethrough)
    LineThrough,
    /// Overline
    Overline,
    /// Combined underline and overline
    UnderlineOverline,
}

impl TextDecoration {
    /// Check if this decoration includes underline.
    pub fn has_underline(&self) -> bool {
        matches!(
            self,
            TextDecoration::Underline | TextDecoration::UnderlineOverline
        )
    }

    /// Check if this decoration includes overline.
    pub fn has_overline(&self) -> bool {
        matches!(
            self,
            TextDecoration::Overline | TextDecoration::UnderlineOverline
        )
    }

    /// Check if this decoration includes line-through.
    pub fn has_line_through(&self) -> bool {
        matches!(self, TextDecoration::LineThrough)
    }
}

impl Default for TextDecoration {
    fn default() -> Self {
        TextDecoration::None
    }
}

/// Predefined text styles.
impl TextStyle {
    /// Display 1 style (large display text)
    pub fn display1() -> Self {
        Self::new()
            .with_size(FontSize::new(57.0))
            .with_weight(FontWeight::Normal)
            .with_line_height(LineHeight::new(1.2))
    }

    /// Display 2 style (medium display text)
    pub fn display2() -> Self {
        Self::new()
            .with_size(FontSize::new(45.0))
            .with_weight(FontWeight::Normal)
            .with_line_height(LineHeight::new(1.2))
    }

    /// Display 3 style (small display text)
    pub fn display3() -> Self {
        Self::new()
            .with_size(FontSize::new(36.0))
            .with_weight(FontWeight::Normal)
            .with_line_height(LineHeight::new(1.2))
    }

    /// Headline 1 style
    pub fn headline1() -> Self {
        Self::new()
            .with_size(FontSize::H1)
            .with_weight(FontWeight::Bold)
            .with_line_height(LineHeight::TIGHT)
    }

    /// Headline 2 style
    pub fn headline2() -> Self {
        Self::new()
            .with_size(FontSize::H2)
            .with_weight(FontWeight::Bold)
            .with_line_height(LineHeight::TIGHT)
    }

    /// Headline 3 style
    pub fn headline3() -> Self {
        Self::new()
            .with_size(FontSize::H3)
            .with_weight(FontWeight::SemiBold)
            .with_line_height(LineHeight::TIGHT)
    }

    /// Body text style (normal)
    pub fn body() -> Self {
        Self::new()
            .with_size(FontSize::MEDIUM)
            .with_weight(FontWeight::Normal)
            .with_line_height(LineHeight::NORMAL)
    }

    /// Body text style (large)
    pub fn body_large() -> Self {
        Self::new()
            .with_size(FontSize::LARGE)
            .with_weight(FontWeight::Normal)
            .with_line_height(LineHeight::NORMAL)
    }

    /// Body text style (small)
    pub fn body_small() -> Self {
        Self::new()
            .with_size(FontSize::SMALL)
            .with_weight(FontWeight::Normal)
            .with_line_height(LineHeight::NORMAL)
    }

    /// Caption style (small secondary text)
    pub fn caption() -> Self {
        Self::new()
            .with_size(FontSize::SMALL)
            .with_weight(FontWeight::Normal)
            .with_line_height(LineHeight::TIGHT)
    }

    /// Button text style
    pub fn button() -> Self {
        Self::new()
            .with_size(FontSize::MEDIUM)
            .with_weight(FontWeight::Medium)
            .with_letter_spacing(0.5)
    }

    /// Label style
    pub fn label() -> Self {
        Self::new()
            .with_size(FontSize::SMALL)
            .with_weight(FontWeight::Medium)
            .with_letter_spacing(0.5)
    }

    /// Code/monospace style
    pub fn code() -> Self {
        Self::new()
            .with_family(FontFamily::monospace())
            .with_size(FontSize::SMALL)
            .with_weight(FontWeight::Normal)
            .with_line_height(LineHeight::RELAXED)
    }
}

// ============================================================================
// egui conversion helpers
// ============================================================================

use super::TextScaler;

/// Helper function to convert TextStyle to egui FontId and Color with scaling.
///
/// # Examples
///
/// ```ignore
/// use nebula_ui::types::typography::{TextStyle, TextScaler, text_style_to_egui};
///
/// let style = TextStyle::headline1();
/// let scaler = TextScaler::none();
/// let (font_id, color) = text_style_to_egui(&style, &scaler);
/// ```
pub fn text_style_to_egui(
    style: &TextStyle,
    scaler: &TextScaler,
) -> (egui::FontId, egui::Color32) {
    // Convert font family using From trait
    let family: egui::FontFamily = (&style.family).into();

    // Apply text scaler to font size
    let scaled_size = scaler.scale(style.size.pixels());

    // Create FontId
    let font_id = egui::FontId::new(scaled_size, family);

    // Convert color
    let color = style.color.to_egui();

    (font_id, color)
}

/// Helper function to convert TextStyle to egui RichText with all styling applied.
///
/// This creates a fully styled egui::RichText with:
/// - Font family and size
/// - Color
/// - Bold (for weights >= 600)
/// - Italic
///
/// # Examples
///
/// ```ignore
/// use nebula_ui::types::typography::{TextStyle, TextScaler, text_style_to_rich_text};
///
/// let style = TextStyle::body().bold().italic();
/// let scaler = TextScaler::none();
/// let rich_text = text_style_to_rich_text("Hello World", &style, &scaler);
/// ```
pub fn text_style_to_rich_text(
    text: &str,
    style: &TextStyle,
    scaler: &TextScaler,
) -> egui::RichText {
    let (font_id, color) = text_style_to_egui(style, scaler);

    let mut rich_text = egui::RichText::new(text).font(font_id).color(color);

    // Apply bold for weights >= SemiBold (600)
    if style.weight.value() >= 600 {
        rich_text = rich_text.strong();
    }

    // Apply italic
    if style.italic {
        rich_text = rich_text.italics();
    }

    rich_text
}

/// Helper function to get default egui FontId and Color with optional scaling.
///
/// Returns sensible defaults when no style is provided.
///
/// # Examples
///
/// ```ignore
/// use nebula_ui::types::typography::{TextScaler, default_egui_style};
///
/// let scaler = TextScaler::none();
/// let (font_id, color) = default_egui_style(&scaler);
/// ```
pub fn default_egui_style(scaler: &TextScaler) -> (egui::FontId, egui::Color32) {
    let default_size = scaler.scale(14.0); // egui default
    let mut font_id = egui::FontId::default();
    font_id.size = default_size;

    (font_id, egui::Color32::from_gray(200))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_style_creation() {
        let style = TextStyle::new();
        assert_eq!(style.weight, FontWeight::Normal);
        assert_eq!(style.decoration, TextDecoration::None);
        assert!(!style.italic);
    }

    #[test]
    fn test_text_style_builder() {
        let style = TextStyle::new()
            .with_size(FontSize::LARGE)
            .with_weight(FontWeight::Bold)
            .with_color(Color::RED)
            .with_italic(true);

        assert_eq!(style.size, FontSize::LARGE);
        assert_eq!(style.weight, FontWeight::Bold);
        assert_eq!(style.color, Color::RED);
        assert!(style.italic);
    }

    #[test]
    fn test_text_style_bold_italic() {
        let base = TextStyle::new();

        let bold = base.bold();
        assert_eq!(bold.weight, FontWeight::Bold);

        let italic = base.italic();
        assert!(italic.italic);
    }

    #[test]
    fn test_text_style_scale() {
        let style = TextStyle::new().with_size(FontSize::new(16.0));
        let scaled = style.scale(2.0);
        assert_eq!(scaled.size.pixels(), 32.0);
    }

    #[test]
    fn test_text_decoration() {
        assert!(TextDecoration::Underline.has_underline());
        assert!(!TextDecoration::Underline.has_overline());
        assert!(!TextDecoration::Underline.has_line_through());

        assert!(TextDecoration::LineThrough.has_line_through());

        assert!(TextDecoration::UnderlineOverline.has_underline());
        assert!(TextDecoration::UnderlineOverline.has_overline());
    }

    #[test]
    fn test_predefined_styles() {
        let h1 = TextStyle::headline1();
        assert_eq!(h1.size, FontSize::H1);
        assert_eq!(h1.weight, FontWeight::Bold);

        let body = TextStyle::body();
        assert_eq!(body.size, FontSize::MEDIUM);
        assert_eq!(body.weight, FontWeight::Normal);

        let code = TextStyle::code();
        assert_eq!(code.family, FontFamily::monospace());
    }

    #[test]
    fn test_text_style_with_different_color() {
        let style = TextStyle::new();
        let colored = style.with_different_color(Color::BLUE);
        assert_eq!(colored.color, Color::BLUE);
    }

    #[test]
    fn test_text_style_to_egui() {
        let style = TextStyle::headline1();
        let scaler = TextScaler::none();
        let (font_id, _color) = text_style_to_egui(&style, &scaler);

        assert_eq!(font_id.size, style.size.pixels());
    }

    #[test]
    fn test_text_style_to_egui_with_scaling() {
        let style = TextStyle::body();
        let scaler = TextScaler::new(2.0);
        let (font_id, _color) = text_style_to_egui(&style, &scaler);

        assert_eq!(font_id.size, style.size.pixels() * 2.0);
    }

    #[test]
    fn test_text_style_to_rich_text_bold() {
        let style = TextStyle::body().bold();
        let scaler = TextScaler::none();
        let rich_text = text_style_to_rich_text("Test", &style, &scaler);

        assert_eq!(rich_text.text(), "Test");
    }

    #[test]
    fn test_text_style_to_rich_text_italic() {
        let style = TextStyle::body().italic();
        let scaler = TextScaler::none();
        let rich_text = text_style_to_rich_text("Test", &style, &scaler);

        assert_eq!(rich_text.text(), "Test");
    }

    #[test]
    fn test_default_egui_style() {
        let scaler = TextScaler::none();
        let (font_id, color) = default_egui_style(&scaler);

        assert_eq!(font_id.size, 14.0);
        assert_eq!(color, egui::Color32::from_gray(200));
    }

    #[test]
    fn test_default_egui_style_with_scaling() {
        let scaler = TextScaler::new(1.5);
        let (font_id, _color) = default_egui_style(&scaler);

        assert_eq!(font_id.size, 21.0); // 14.0 * 1.5
    }
}
