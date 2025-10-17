//! Font types for typography
//!
//! This module contains types for representing fonts and typography,
//! similar to Flutter's font system.

use std::fmt;

/// Font size in logical pixels.
///
/// Type-safe wrapper around f32 for font sizes.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct FontSize(f32);

impl FontSize {
    /// Extra small font size (10px)
    pub const EXTRA_SMALL: FontSize = FontSize(10.0);

    /// Small font size (12px)
    pub const SMALL: FontSize = FontSize(12.0);

    /// Medium font size (14px)
    pub const MEDIUM: FontSize = FontSize(14.0);

    /// Large font size (16px)
    pub const LARGE: FontSize = FontSize(16.0);

    /// Extra large font size (20px)
    pub const EXTRA_LARGE: FontSize = FontSize(20.0);

    /// Heading 1 size (32px)
    pub const H1: FontSize = FontSize(32.0);

    /// Heading 2 size (24px)
    pub const H2: FontSize = FontSize(24.0);

    /// Heading 3 size (20px)
    pub const H3: FontSize = FontSize(20.0);

    /// Heading 4 size (18px)
    pub const H4: FontSize = FontSize(18.0);

    /// Heading 5 size (16px)
    pub const H5: FontSize = FontSize(16.0);

    /// Heading 6 size (14px)
    pub const H6: FontSize = FontSize(14.0);

    /// Create a new font size.
    pub fn new(size: f32) -> Self {
        Self(size.max(0.0))
    }

    /// Get the size in pixels.
    pub fn pixels(&self) -> f32 {
        self.0
    }

    /// Scale the font size by a factor.
    pub fn scale(&self, factor: f32) -> Self {
        Self::new(self.0 * factor)
    }

    /// Create a font size from points (1pt = 1.333px).
    pub fn from_points(points: f32) -> Self {
        Self::new(points * 1.333)
    }

    /// Convert to points.
    pub fn to_points(&self) -> f32 {
        self.0 / 1.333
    }
}

impl Default for FontSize {
    fn default() -> Self {
        Self::MEDIUM
    }
}

impl From<f32> for FontSize {
    fn from(size: f32) -> Self {
        Self::new(size)
    }
}

impl From<FontSize> for f32 {
    fn from(size: FontSize) -> Self {
        size.0
    }
}

/// Font weight (thickness).
///
/// Similar to CSS font-weight and Flutter's FontWeight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontWeight {
    /// Thin (100)
    Thin,
    /// Extra light (200)
    ExtraLight,
    /// Light (300)
    Light,
    /// Normal/Regular (400)
    Normal,
    /// Medium (500)
    Medium,
    /// Semi-bold (600)
    SemiBold,
    /// Bold (700)
    Bold,
    /// Extra bold (800)
    ExtraBold,
    /// Black (900)
    Black,
}

impl FontWeight {
    /// Convert to numeric weight (100-900).
    pub fn value(&self) -> u16 {
        match self {
            FontWeight::Thin => 100,
            FontWeight::ExtraLight => 200,
            FontWeight::Light => 300,
            FontWeight::Normal => 400,
            FontWeight::Medium => 500,
            FontWeight::SemiBold => 600,
            FontWeight::Bold => 700,
            FontWeight::ExtraBold => 800,
            FontWeight::Black => 900,
        }
    }

    /// Create from numeric weight, rounding to nearest standard weight.
    pub fn from_value(value: u16) -> Self {
        match value {
            0..=150 => FontWeight::Thin,
            151..=250 => FontWeight::ExtraLight,
            251..=350 => FontWeight::Light,
            351..=450 => FontWeight::Normal,
            451..=550 => FontWeight::Medium,
            551..=650 => FontWeight::SemiBold,
            651..=750 => FontWeight::Bold,
            751..=850 => FontWeight::ExtraBold,
            _ => FontWeight::Black,
        }
    }

    /// Get the next bolder weight.
    pub fn bolder(&self) -> Self {
        match self {
            FontWeight::Thin => FontWeight::ExtraLight,
            FontWeight::ExtraLight => FontWeight::Light,
            FontWeight::Light => FontWeight::Normal,
            FontWeight::Normal => FontWeight::Medium,
            FontWeight::Medium => FontWeight::SemiBold,
            FontWeight::SemiBold => FontWeight::Bold,
            FontWeight::Bold => FontWeight::ExtraBold,
            FontWeight::ExtraBold => FontWeight::Black,
            FontWeight::Black => FontWeight::Black,
        }
    }

    /// Get the next lighter weight.
    pub fn lighter(&self) -> Self {
        match self {
            FontWeight::Thin => FontWeight::Thin,
            FontWeight::ExtraLight => FontWeight::Thin,
            FontWeight::Light => FontWeight::ExtraLight,
            FontWeight::Normal => FontWeight::Light,
            FontWeight::Medium => FontWeight::Normal,
            FontWeight::SemiBold => FontWeight::Medium,
            FontWeight::Bold => FontWeight::SemiBold,
            FontWeight::ExtraBold => FontWeight::Bold,
            FontWeight::Black => FontWeight::ExtraBold,
        }
    }
}

impl Default for FontWeight {
    fn default() -> Self {
        FontWeight::Normal
    }
}

impl fmt::Display for FontWeight {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.value())
    }
}

/// Font family name.
///
/// Type-safe wrapper around String for font family names.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FontFamily(String);

impl FontFamily {
    /// System default font
    pub const SYSTEM: FontFamily = FontFamily(String::new());

    /// Create a new font family.
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Monospace font family
    pub fn monospace() -> Self {
        Self::new("monospace")
    }

    /// Serif font family
    pub fn serif() -> Self {
        Self::new("serif")
    }

    /// Sans-serif font family
    pub fn sans_serif() -> Self {
        Self::new("sans-serif")
    }

    /// Get the font family name.
    pub fn name(&self) -> &str {
        &self.0
    }

    /// Check if this is the system default font.
    pub fn is_system(&self) -> bool {
        self.0.is_empty()
    }
}

impl Default for FontFamily {
    fn default() -> Self {
        Self::SYSTEM
    }
}

impl From<String> for FontFamily {
    fn from(name: String) -> Self {
        Self::new(name)
    }
}

impl From<&str> for FontFamily {
    fn from(name: &str) -> Self {
        Self::new(name)
    }
}

impl fmt::Display for FontFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_system() {
            write!(f, "system")
        } else {
            write!(f, "{}", self.0)
        }
    }
}

/// Convert nebula-ui FontFamily to egui's FontFamily.
///
/// # Examples
///
/// ```ignore
/// use nebula_ui::types::typography::FontFamily;
///
/// let family = FontFamily::monospace();
/// let egui_family: egui::FontFamily = (&family).into();
/// ```
impl From<&FontFamily> for egui::FontFamily {
    fn from(family: &FontFamily) -> Self {
        if family.is_system() {
            // System default - use Proportional
            egui::FontFamily::Proportional
        } else {
            match family.name() {
                "monospace" => egui::FontFamily::Monospace,
                "sans-serif" | "proportional" => egui::FontFamily::Proportional,
                name => egui::FontFamily::Name(name.into()),
            }
        }
    }
}

/// Line height multiplier.
///
/// Represents the spacing between lines of text.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct LineHeight(f32);

impl LineHeight {
    /// Tight line height (1.0)
    pub const TIGHT: LineHeight = LineHeight(1.0);

    /// Normal line height (1.5)
    pub const NORMAL: LineHeight = LineHeight(1.5);

    /// Relaxed line height (1.75)
    pub const RELAXED: LineHeight = LineHeight(1.75);

    /// Loose line height (2.0)
    pub const LOOSE: LineHeight = LineHeight(2.0);

    /// Create a new line height.
    pub fn new(multiplier: f32) -> Self {
        Self(multiplier.max(0.0))
    }

    /// Get the multiplier value.
    pub fn value(&self) -> f32 {
        self.0
    }

    /// Calculate the actual line height in pixels for a given font size.
    pub fn calculate(&self, font_size: FontSize) -> f32 {
        font_size.pixels() * self.0
    }

    /// Create from absolute pixel height and font size.
    pub fn from_pixels(pixels: f32, font_size: FontSize) -> Self {
        if font_size.pixels() > 0.0 {
            Self::new(pixels / font_size.pixels())
        } else {
            Self::NORMAL
        }
    }
}

impl Default for LineHeight {
    fn default() -> Self {
        Self::NORMAL
    }
}

impl From<f32> for LineHeight {
    fn from(multiplier: f32) -> Self {
        Self::new(multiplier)
    }
}

impl From<LineHeight> for f32 {
    fn from(height: LineHeight) -> Self {
        height.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_font_size_creation() {
        let size = FontSize::new(16.0);
        assert_eq!(size.pixels(), 16.0);

        let negative = FontSize::new(-10.0);
        assert_eq!(negative.pixels(), 0.0);
    }

    #[test]
    fn test_font_size_constants() {
        assert_eq!(FontSize::SMALL.pixels(), 12.0);
        assert_eq!(FontSize::MEDIUM.pixels(), 14.0);
        assert_eq!(FontSize::LARGE.pixels(), 16.0);
        assert_eq!(FontSize::H1.pixels(), 32.0);
    }

    #[test]
    fn test_font_size_scale() {
        let size = FontSize::new(16.0);
        let scaled = size.scale(2.0);
        assert_eq!(scaled.pixels(), 32.0);
    }

    #[test]
    fn test_font_size_points() {
        let size = FontSize::from_points(12.0);
        assert!((size.pixels() - 15.996).abs() < 0.01);

        let points = size.to_points();
        assert!((points - 12.0).abs() < 0.01);
    }

    #[test]
    fn test_font_weight_value() {
        assert_eq!(FontWeight::Thin.value(), 100);
        assert_eq!(FontWeight::Normal.value(), 400);
        assert_eq!(FontWeight::Bold.value(), 700);
        assert_eq!(FontWeight::Black.value(), 900);
    }

    #[test]
    fn test_font_weight_from_value() {
        assert_eq!(FontWeight::from_value(100), FontWeight::Thin);
        assert_eq!(FontWeight::from_value(400), FontWeight::Normal);
        assert_eq!(FontWeight::from_value(700), FontWeight::Bold);
        assert_eq!(FontWeight::from_value(425), FontWeight::Normal);
    }

    #[test]
    fn test_font_weight_bolder_lighter() {
        assert_eq!(FontWeight::Normal.bolder(), FontWeight::Medium);
        assert_eq!(FontWeight::Bold.bolder(), FontWeight::ExtraBold);
        assert_eq!(FontWeight::Black.bolder(), FontWeight::Black);

        assert_eq!(FontWeight::Normal.lighter(), FontWeight::Light);
        assert_eq!(FontWeight::Bold.lighter(), FontWeight::SemiBold);
        assert_eq!(FontWeight::Thin.lighter(), FontWeight::Thin);
    }

    #[test]
    fn test_font_family() {
        let family = FontFamily::new("Arial");
        assert_eq!(family.name(), "Arial");
        assert!(!family.is_system());

        let system = FontFamily::SYSTEM;
        assert!(system.is_system());

        let mono = FontFamily::monospace();
        assert_eq!(mono.name(), "monospace");
    }

    #[test]
    fn test_font_family_conversions() {
        let from_str: FontFamily = "Roboto".into();
        assert_eq!(from_str.name(), "Roboto");

        let from_string: FontFamily = String::from("Helvetica").into();
        assert_eq!(from_string.name(), "Helvetica");
    }

    #[test]
    fn test_line_height_creation() {
        let height = LineHeight::new(1.5);
        assert_eq!(height.value(), 1.5);

        let negative = LineHeight::new(-1.0);
        assert_eq!(negative.value(), 0.0);
    }

    #[test]
    fn test_line_height_constants() {
        assert_eq!(LineHeight::TIGHT.value(), 1.0);
        assert_eq!(LineHeight::NORMAL.value(), 1.5);
        assert_eq!(LineHeight::RELAXED.value(), 1.75);
        assert_eq!(LineHeight::LOOSE.value(), 2.0);
    }

    #[test]
    fn test_line_height_calculate() {
        let height = LineHeight::new(1.5);
        let font_size = FontSize::new(16.0);
        assert_eq!(height.calculate(font_size), 24.0);
    }

    #[test]
    fn test_line_height_from_pixels() {
        let font_size = FontSize::new(16.0);
        let height = LineHeight::from_pixels(24.0, font_size);
        assert_eq!(height.value(), 1.5);
    }

    #[test]
    fn test_font_size_conversions() {
        let from_f32: FontSize = 16.0.into();
        assert_eq!(from_f32.pixels(), 16.0);

        let to_f32: f32 = FontSize::new(16.0).into();
        assert_eq!(to_f32, 16.0);
    }

    #[test]
    fn test_font_family_to_egui_system() {
        let family = FontFamily::default();
        let egui_family: egui::FontFamily = (&family).into();
        assert_eq!(egui_family, egui::FontFamily::Proportional);
    }

    #[test]
    fn test_font_family_to_egui_monospace() {
        let family = FontFamily::monospace();
        let egui_family: egui::FontFamily = (&family).into();
        assert_eq!(egui_family, egui::FontFamily::Monospace);
    }

    #[test]
    fn test_font_family_to_egui_custom() {
        let family = FontFamily::new("Arial");
        let egui_family: egui::FontFamily = (&family).into();
        match egui_family {
            egui::FontFamily::Name(name) => assert_eq!(name.as_ref(), "Arial"),
            _ => panic!("Expected Name variant"),
        }
    }
}
