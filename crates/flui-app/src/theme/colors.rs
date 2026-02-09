//! Color types and color schemes.

/// RGBA color.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    /// Red component (0.0 - 1.0)
    pub r: f32,
    /// Green component (0.0 - 1.0)
    pub g: f32,
    /// Blue component (0.0 - 1.0)
    pub b: f32,
    /// Alpha component (0.0 - 1.0)
    pub a: f32,
}

impl Color {
    /// Create a new color from RGBA values (0.0 - 1.0).
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Create a new opaque color from RGB values (0.0 - 1.0).
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    /// Create from 8-bit RGBA values (0-255).
    pub fn from_rgba8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: a as f32 / 255.0,
        }
    }

    /// Create from hex string (e.g., "#FF5733" or "FF5733").
    pub fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.trim_start_matches('#');

        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(Self::from_rgba8(r, g, b, 255))
        } else if hex.len() == 8 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            Some(Self::from_rgba8(r, g, b, a))
        } else {
            None
        }
    }

    /// Create a color with modified alpha.
    pub const fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    /// Convert to 32-bit RGBA.
    pub fn to_rgba8(&self) -> [u8; 4] {
        [
            (self.r * 255.0) as u8,
            (self.g * 255.0) as u8,
            (self.b * 255.0) as u8,
            (self.a * 255.0) as u8,
        ]
    }

    // Common colors
    /// Pure white color.
    pub const WHITE: Self = Self::rgb(1.0, 1.0, 1.0);
    /// Pure black color.
    pub const BLACK: Self = Self::rgb(0.0, 0.0, 0.0);
    /// Fully transparent color.
    pub const TRANSPARENT: Self = Self::new(0.0, 0.0, 0.0, 0.0);
    /// Pure red color.
    pub const RED: Self = Self::rgb(1.0, 0.0, 0.0);
    /// Pure green color.
    pub const GREEN: Self = Self::rgb(0.0, 1.0, 0.0);
    /// Pure blue color.
    pub const BLUE: Self = Self::rgb(0.0, 0.0, 1.0);
}

impl Default for Color {
    fn default() -> Self {
        Self::BLACK
    }
}

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
    pub fn light() -> Self {
        Self {
            primary: Color::from_hex("#6200EE").unwrap(),
            on_primary: Color::WHITE,
            secondary: Color::from_hex("#03DAC6").unwrap(),
            on_secondary: Color::BLACK,
            background: Color::from_hex("#FAFAFA").unwrap(),
            on_background: Color::BLACK,
            surface: Color::WHITE,
            on_surface: Color::BLACK,
            error: Color::from_hex("#B00020").unwrap(),
            on_error: Color::WHITE,
            outline: Color::from_hex("#79747E").unwrap(),
        }
    }

    /// Create a dark color scheme.
    pub fn dark() -> Self {
        Self {
            primary: Color::from_hex("#BB86FC").unwrap(),
            on_primary: Color::BLACK,
            secondary: Color::from_hex("#03DAC6").unwrap(),
            on_secondary: Color::BLACK,
            background: Color::from_hex("#121212").unwrap(),
            on_background: Color::WHITE,
            surface: Color::from_hex("#1E1E1E").unwrap(),
            on_surface: Color::WHITE,
            error: Color::from_hex("#CF6679").unwrap(),
            on_error: Color::BLACK,
            outline: Color::from_hex("#938F99").unwrap(),
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
    fn test_color_from_hex() {
        let color = Color::from_hex("#FF5733").unwrap();
        assert_eq!(color.to_rgba8(), [255, 87, 51, 255]);

        let color = Color::from_hex("00FF00").unwrap();
        assert_eq!(color.to_rgba8(), [0, 255, 0, 255]);
    }

    #[test]
    fn test_color_with_alpha() {
        let color = Color::WHITE.with_alpha(0.5);
        assert!((color.a - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_color_scheme_light_dark() {
        let light = ColorScheme::light();
        let dark = ColorScheme::dark();

        // Light background should be lighter than dark
        assert!(light.background.r > dark.background.r);
    }
}
