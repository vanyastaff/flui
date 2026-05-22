//! Semantic color tokens for the theme system.
//!
//! `Color` itself is the canonical `flui_types::Color` (packed `u8` RGBA);
//! this module only owns the `ColorScheme` semantic-token bundle that the
//! theme system layers on top of it.

use flui_types::Color;

/// Color scheme - semantic color tokens.
///
/// This provides semantic meaning to colors rather than raw values.
/// Inspired by Material Design but simplified.
#[derive(Debug, Clone)]
pub struct ColorScheme {
    /// Primary brand color.
    pub primary: Color,
    /// Color for elements on primary.
    pub on_primary: Color,

    /// Secondary brand color.
    pub secondary: Color,
    /// Color for elements on secondary.
    pub on_secondary: Color,

    /// Background color.
    pub background: Color,
    /// Color for elements on background.
    pub on_background: Color,

    /// Surface color (cards, sheets).
    pub surface: Color,
    /// Color for elements on surface.
    pub on_surface: Color,

    /// Error color.
    pub error: Color,
    /// Color for elements on error.
    pub on_error: Color,

    /// Outline/border color.
    pub outline: Color,
}

impl ColorScheme {
    /// Create a light color scheme.
    pub const fn light() -> Self {
        Self {
            primary: Color::from_argb(0xFF6200EE),
            on_primary: Color::WHITE,
            secondary: Color::from_argb(0xFF03DAC6),
            on_secondary: Color::BLACK,
            background: Color::from_argb(0xFFFAFAFA),
            on_background: Color::BLACK,
            surface: Color::WHITE,
            on_surface: Color::BLACK,
            error: Color::from_argb(0xFFB00020),
            on_error: Color::WHITE,
            outline: Color::from_argb(0xFF79747E),
        }
    }

    /// Create a dark color scheme.
    pub const fn dark() -> Self {
        Self {
            primary: Color::from_argb(0xFFBB86FC),
            on_primary: Color::BLACK,
            secondary: Color::from_argb(0xFF03DAC6),
            on_secondary: Color::BLACK,
            background: Color::from_argb(0xFF121212),
            on_background: Color::WHITE,
            surface: Color::from_argb(0xFF1E1E1E),
            on_surface: Color::WHITE,
            error: Color::from_argb(0xFFCF6679),
            on_error: Color::BLACK,
            outline: Color::from_argb(0xFF938F99),
        }
    }
}

impl Default for ColorScheme {
    fn default() -> Self {
        Self::light()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_scheme_light_dark() {
        let light = ColorScheme::light();
        let dark = ColorScheme::dark();

        // Light background should be lighter than dark
        assert!(light.background.r > dark.background.r);
    }
}
