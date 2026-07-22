//! Semantic color tokens for the theme system.
//!
//! `Color` itself is the canonical `flui_types::Color` (packed `u8` RGBA);
//! this module only owns the `AppColorScheme` semantic-token bundle that the
//! theme system layers on top of it.

use flui_types::Color;

/// Color scheme - semantic color tokens.
///
/// This provides semantic meaning to colors rather than raw values.
/// Inspired by Material Design but simplified.
#[derive(Debug, Clone)]
pub struct AppColorScheme {
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

impl AppColorScheme {
    /// Create a light color scheme.
    pub const fn light() -> Self {
        Self {
            primary: Color::from_argb(0xFF_62_00_EE),
            on_primary: Color::WHITE,
            secondary: Color::from_argb(0xFF_03_DA_C6),
            on_secondary: Color::BLACK,
            background: Color::from_argb(0xFF_FA_FA_FA),
            on_background: Color::BLACK,
            surface: Color::WHITE,
            on_surface: Color::BLACK,
            error: Color::from_argb(0xFF_B0_00_20),
            on_error: Color::WHITE,
            outline: Color::from_argb(0xFF_79_74_7E),
        }
    }

    /// Create a dark color scheme.
    pub const fn dark() -> Self {
        Self {
            primary: Color::from_argb(0xFF_BB_86_FC),
            on_primary: Color::BLACK,
            secondary: Color::from_argb(0xFF_03_DA_C6),
            on_secondary: Color::BLACK,
            background: Color::from_argb(0xFF_12_12_12),
            on_background: Color::WHITE,
            surface: Color::from_argb(0xFF_1E_1E_1E),
            on_surface: Color::WHITE,
            error: Color::from_argb(0xFF_CF_66_79),
            on_error: Color::BLACK,
            outline: Color::from_argb(0xFF_93_8F_99),
        }
    }
}

impl Default for AppColorScheme {
    fn default() -> Self {
        Self::light()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_background_is_lighter_than_dark_background() {
        let light = AppColorScheme::light();
        let dark = AppColorScheme::dark();

        // Light background should be lighter than dark
        assert!(light.background.r > dark.background.r);
    }
}
