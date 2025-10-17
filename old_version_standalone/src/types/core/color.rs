//! Color types and utilities
//!
//! This module provides a unified Color type with conversions from various formats,
//! similar to Flutter's Color system but integrated with egui.

use egui::Color32;

/// A color represented in the RGB color space with an alpha channel.
///
/// This is a wrapper around egui::Color32 with additional Flutter-like functionality.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color(pub Color32);

impl Color {
    /// Create a color from RGBA values (0-255).
    pub const fn from_rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Color(Color32::from_rgba_premultiplied(r, g, b, a))
    }

    /// Create a color from RGB values with full opacity.
    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Color(Color32::from_rgb(r, g, b))
    }

    /// Create a color from a 32-bit ARGB value.
    pub const fn from_argb(argb: u32) -> Self {
        let a = ((argb >> 24) & 0xFF) as u8;
        let r = ((argb >> 16) & 0xFF) as u8;
        let g = ((argb >> 8) & 0xFF) as u8;
        let b = (argb & 0xFF) as u8;
        Color::from_rgba(r, g, b, a)
    }

    /// Create a color from a 32-bit RGB value with full opacity.
    pub const fn from_rgb_hex(rgb: u32) -> Self {
        let r = ((rgb >> 16) & 0xFF) as u8;
        let g = ((rgb >> 8) & 0xFF) as u8;
        let b = (rgb & 0xFF) as u8;
        Color::from_rgb(r, g, b)
    }

    /// Create a color from a hex string (e.g., "#FF0000" or "FF0000").
    pub fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.trim_start_matches('#');

        if hex.len() == 6 {
            let rgb = u32::from_str_radix(hex, 16).ok()?;
            Some(Color::from_rgb_hex(rgb))
        } else if hex.len() == 8 {
            let argb = u32::from_str_radix(hex, 16).ok()?;
            Some(Color::from_argb(argb))
        } else {
            None
        }
    }

    /// Create a color with a specified opacity (0.0 to 1.0).
    pub fn with_opacity(self, opacity: f32) -> Self {
        let opacity = (opacity.clamp(0.0, 1.0) * 255.0) as u8;
        Color(Color32::from_rgba_premultiplied(
            self.0.r(),
            self.0.g(),
            self.0.b(),
            opacity,
        ))
    }

    /// Create a color with a specified alpha value (0-255).
    pub fn with_alpha(self, alpha: u8) -> Self {
        Color(Color32::from_rgba_premultiplied(
            self.0.r(),
            self.0.g(),
            self.0.b(),
            alpha,
        ))
    }

    /// Get the red component (0-255).
    pub fn red(&self) -> u8 {
        self.0.r()
    }

    /// Get the green component (0-255).
    pub fn green(&self) -> u8 {
        self.0.g()
    }

    /// Get the blue component (0-255).
    pub fn blue(&self) -> u8 {
        self.0.b()
    }

    /// Get the alpha component (0-255).
    pub fn alpha(&self) -> u8 {
        self.0.a()
    }

    /// Get the opacity as a float (0.0 to 1.0).
    pub fn opacity(&self) -> f32 {
        self.0.a() as f32 / 255.0
    }

    /// Check if this color is fully transparent (alpha = 0).
    pub fn is_transparent(&self) -> bool {
        self.alpha() == 0
    }

    /// Check if this color is fully opaque (alpha = 255).
    pub fn is_opaque(&self) -> bool {
        self.alpha() == 255
    }

    /// Convert to egui::Color32.
    pub fn to_egui(self) -> Color32 {
        self.0
    }

    /// Linear interpolation between two colors.
    pub fn lerp(self, other: Color, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        let r = (self.red() as f32 + (other.red() as f32 - self.red() as f32) * t) as u8;
        let g = (self.green() as f32 + (other.green() as f32 - self.green() as f32) * t) as u8;
        let b = (self.blue() as f32 + (other.blue() as f32 - self.blue() as f32) * t) as u8;
        let a = (self.alpha() as f32 + (other.alpha() as f32 - self.alpha() as f32) * t) as u8;
        Color::from_rgba(r, g, b, a)
    }

    // Common color constants
    pub const TRANSPARENT: Color = Color(Color32::TRANSPARENT);
    pub const BLACK: Color = Color(Color32::BLACK);
    pub const WHITE: Color = Color(Color32::WHITE);
    pub const RED: Color = Color(Color32::RED);
    pub const GREEN: Color = Color(Color32::GREEN);
    pub const BLUE: Color = Color(Color32::BLUE);
    pub const YELLOW: Color = Color(Color32::YELLOW);
    pub const LIGHT_BLUE: Color = Color(Color32::LIGHT_BLUE);
    pub const LIGHT_RED: Color = Color(Color32::LIGHT_RED);
    pub const LIGHT_YELLOW: Color = Color(Color32::LIGHT_YELLOW);
    pub const LIGHT_GREEN: Color = Color(Color32::LIGHT_GREEN);
    pub const DARK_BLUE: Color = Color(Color32::DARK_BLUE);
    pub const DARK_RED: Color = Color(Color32::DARK_RED);
    pub const DARK_GREEN: Color = Color(Color32::DARK_GREEN);
    pub const BROWN: Color = Color(Color32::BROWN);
    pub const GOLD: Color = Color(Color32::GOLD);
    pub const GRAY: Color = Color(Color32::GRAY);
    pub const LIGHT_GRAY: Color = Color(Color32::LIGHT_GRAY);
    pub const DARK_GRAY: Color = Color(Color32::DARK_GRAY);
}

impl Default for Color {
    fn default() -> Self {
        Color::TRANSPARENT
    }
}

impl From<Color32> for Color {
    fn from(color: Color32) -> Self {
        Color(color)
    }
}

impl From<Color> for Color32 {
    fn from(color: Color) -> Self {
        color.0
    }
}

impl From<(u8, u8, u8)> for Color {
    fn from((r, g, b): (u8, u8, u8)) -> Self {
        Color::from_rgb(r, g, b)
    }
}

impl From<(u8, u8, u8, u8)> for Color {
    fn from((r, g, b, a): (u8, u8, u8, u8)) -> Self {
        Color::from_rgba(r, g, b, a)
    }
}

impl From<u32> for Color {
    fn from(value: u32) -> Self {
        Color::from_rgb_hex(value)
    }
}

/// HSL color representation (Hue, Saturation, Lightness).
///
/// Similar to Flutter's HSLColor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HSLColor {
    /// Hue in degrees (0.0 to 360.0).
    pub hue: f32,

    /// Saturation (0.0 to 1.0).
    pub saturation: f32,

    /// Lightness (0.0 to 1.0).
    pub lightness: f32,

    /// Alpha/opacity (0.0 to 1.0).
    pub alpha: f32,
}

impl HSLColor {
    /// Create a new HSL color.
    pub fn new(hue: f32, saturation: f32, lightness: f32, alpha: f32) -> Self {
        Self {
            hue: hue % 360.0,
            saturation: saturation.clamp(0.0, 1.0),
            lightness: lightness.clamp(0.0, 1.0),
            alpha: alpha.clamp(0.0, 1.0),
        }
    }

    /// Create a copy with adjusted lightness.
    pub fn with_lightness(&self, lightness: f32) -> Self {
        Self::new(self.hue, self.saturation, lightness, self.alpha)
    }

    /// Create a copy with adjusted saturation.
    pub fn with_saturation(&self, saturation: f32) -> Self {
        Self::new(self.hue, saturation, self.lightness, self.alpha)
    }

    /// Create a copy with adjusted hue.
    pub fn with_hue(&self, hue: f32) -> Self {
        Self::new(hue, self.saturation, self.lightness, self.alpha)
    }

    /// Create a copy with adjusted alpha.
    pub fn with_alpha(&self, alpha: f32) -> Self {
        Self::new(self.hue, self.saturation, self.lightness, alpha)
    }
}

impl From<Color> for HSLColor {
    fn from(color: Color) -> Self {
        let r = color.red() as f32 / 255.0;
        let g = color.green() as f32 / 255.0;
        let b = color.blue() as f32 / 255.0;
        let a = color.alpha() as f32 / 255.0;

        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;

        let lightness = (max + min) / 2.0;

        let saturation = if delta == 0.0 {
            0.0
        } else {
            delta / (1.0 - (2.0 * lightness - 1.0).abs())
        };

        let hue = if delta == 0.0 {
            0.0
        } else if max == r {
            60.0 * (((g - b) / delta) % 6.0)
        } else if max == g {
            60.0 * (((b - r) / delta) + 2.0)
        } else {
            60.0 * (((r - g) / delta) + 4.0)
        };

        let hue = if hue < 0.0 { hue + 360.0 } else { hue };

        Self::new(hue, saturation, lightness, a)
    }
}

impl From<HSLColor> for Color {
    fn from(hsl: HSLColor) -> Self {
        let c = (1.0 - (2.0 * hsl.lightness - 1.0).abs()) * hsl.saturation;
        let x = c * (1.0 - ((hsl.hue / 60.0) % 2.0 - 1.0).abs());
        let m = hsl.lightness - c / 2.0;

        let (r, g, b) = if hsl.hue < 60.0 {
            (c, x, 0.0)
        } else if hsl.hue < 120.0 {
            (x, c, 0.0)
        } else if hsl.hue < 180.0 {
            (0.0, c, x)
        } else if hsl.hue < 240.0 {
            (0.0, x, c)
        } else if hsl.hue < 300.0 {
            (x, 0.0, c)
        } else {
            (c, 0.0, x)
        };

        Color::from_rgba(
            ((r + m) * 255.0) as u8,
            ((g + m) * 255.0) as u8,
            ((b + m) * 255.0) as u8,
            (hsl.alpha * 255.0) as u8,
        )
    }
}

/// HSV color representation (Hue, Saturation, Value/Brightness).
///
/// Similar to Flutter's HSVColor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HSVColor {
    /// Hue in degrees (0.0 to 360.0).
    pub hue: f32,

    /// Saturation (0.0 to 1.0).
    pub saturation: f32,

    /// Value/Brightness (0.0 to 1.0).
    pub value: f32,

    /// Alpha/opacity (0.0 to 1.0).
    pub alpha: f32,
}

impl HSVColor {
    /// Create a new HSV color.
    pub fn new(hue: f32, saturation: f32, value: f32, alpha: f32) -> Self {
        Self {
            hue: hue % 360.0,
            saturation: saturation.clamp(0.0, 1.0),
            value: value.clamp(0.0, 1.0),
            alpha: alpha.clamp(0.0, 1.0),
        }
    }

    /// Create a copy with adjusted value.
    pub fn with_value(&self, value: f32) -> Self {
        Self::new(self.hue, self.saturation, value, self.alpha)
    }

    /// Create a copy with adjusted saturation.
    pub fn with_saturation(&self, saturation: f32) -> Self {
        Self::new(self.hue, saturation, self.value, self.alpha)
    }

    /// Create a copy with adjusted hue.
    pub fn with_hue(&self, hue: f32) -> Self {
        Self::new(hue, self.saturation, self.value, self.alpha)
    }

    /// Create a copy with adjusted alpha.
    pub fn with_alpha(&self, alpha: f32) -> Self {
        Self::new(self.hue, self.saturation, self.value, alpha)
    }
}

impl From<Color> for HSVColor {
    fn from(color: Color) -> Self {
        let r = color.red() as f32 / 255.0;
        let g = color.green() as f32 / 255.0;
        let b = color.blue() as f32 / 255.0;
        let a = color.alpha() as f32 / 255.0;

        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;

        let value = max;

        let saturation = if max == 0.0 { 0.0 } else { delta / max };

        let hue = if delta == 0.0 {
            0.0
        } else if max == r {
            60.0 * (((g - b) / delta) % 6.0)
        } else if max == g {
            60.0 * (((b - r) / delta) + 2.0)
        } else {
            60.0 * (((r - g) / delta) + 4.0)
        };

        let hue = if hue < 0.0 { hue + 360.0 } else { hue };

        Self::new(hue, saturation, value, a)
    }
}

impl From<HSVColor> for Color {
    fn from(hsv: HSVColor) -> Self {
        let c = hsv.value * hsv.saturation;
        let x = c * (1.0 - ((hsv.hue / 60.0) % 2.0 - 1.0).abs());
        let m = hsv.value - c;

        let (r, g, b) = if hsv.hue < 60.0 {
            (c, x, 0.0)
        } else if hsv.hue < 120.0 {
            (x, c, 0.0)
        } else if hsv.hue < 180.0 {
            (0.0, c, x)
        } else if hsv.hue < 240.0 {
            (0.0, x, c)
        } else if hsv.hue < 300.0 {
            (x, 0.0, c)
        } else {
            (c, 0.0, x)
        };

        Color::from_rgba(
            ((r + m) * 255.0) as u8,
            ((g + m) * 255.0) as u8,
            ((b + m) * 255.0) as u8,
            (hsv.alpha * 255.0) as u8,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_creation() {
        let color = Color::from_rgb(255, 0, 0);
        assert_eq!(color.red(), 255);
        assert_eq!(color.green(), 0);
        assert_eq!(color.blue(), 0);
        assert_eq!(color.alpha(), 255);

        let color_with_alpha = Color::from_rgba(0, 255, 0, 128);
        assert_eq!(color_with_alpha.green(), 255);
        assert_eq!(color_with_alpha.alpha(), 128);
    }

    #[test]
    fn test_color_from_hex() {
        let red = Color::from_hex("#FF0000").unwrap();
        assert_eq!(red.red(), 255);
        assert_eq!(red.green(), 0);
        assert_eq!(red.blue(), 0);

        let red_no_hash = Color::from_hex("FF0000").unwrap();
        assert_eq!(red_no_hash, red);

        let with_alpha = Color::from_hex("#80FF0000").unwrap();
        assert_eq!(with_alpha.alpha(), 128);
    }

    #[test]
    fn test_color_opacity() {
        let color = Color::from_rgb(255, 0, 0);
        let half_opacity = color.with_opacity(0.5);
        assert_eq!(half_opacity.alpha(), 127); // 0.5 * 255

        assert!((half_opacity.opacity() - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_color_lerp() {
        let red = Color::from_rgb(255, 0, 0);
        let blue = Color::from_rgb(0, 0, 255);

        let mid = red.lerp(blue, 0.5);
        assert!(mid.red() > 0 && mid.red() < 255);
        assert!(mid.blue() > 0 && mid.blue() < 255);

        let still_red = red.lerp(blue, 0.0);
        assert_eq!(still_red, red);

        let now_blue = red.lerp(blue, 1.0);
        assert_eq!(now_blue, blue);
    }

    #[test]
    fn test_color_conversions() {
        let from_tuple: Color = (255, 0, 0).into();
        assert_eq!(from_tuple.red(), 255);

        let from_tuple_alpha: Color = (255, 0, 0, 128).into();
        assert_eq!(from_tuple_alpha.alpha(), 128);

        let from_hex: Color = 0xFF0000.into();
        assert_eq!(from_hex.red(), 255);
    }

    #[test]
    fn test_hsl_conversion() {
        let red = Color::from_rgb(255, 0, 0);
        let hsl: HSLColor = red.into();

        assert!((hsl.hue - 0.0).abs() < 1.0);
        assert!((hsl.saturation - 1.0).abs() < 0.01);
        assert!((hsl.lightness - 0.5).abs() < 0.01);

        let back: Color = hsl.into();
        assert!((back.red() as i16 - red.red() as i16).abs() <= 2);
    }

    #[test]
    fn test_hsv_conversion() {
        let red = Color::from_rgb(255, 0, 0);
        let hsv: HSVColor = red.into();

        assert!((hsv.hue - 0.0).abs() < 1.0);
        assert!((hsv.saturation - 1.0).abs() < 0.01);
        assert!((hsv.value - 1.0).abs() < 0.01);

        let back: Color = hsv.into();
        assert!((back.red() as i16 - red.red() as i16).abs() <= 2);
    }

    #[test]
    fn test_hsl_adjustments() {
        let hsl = HSLColor::new(180.0, 0.5, 0.5, 1.0);

        let lighter = hsl.with_lightness(0.8);
        assert_eq!(lighter.lightness, 0.8);
        assert_eq!(lighter.hue, hsl.hue);

        let more_saturated = hsl.with_saturation(1.0);
        assert_eq!(more_saturated.saturation, 1.0);

        let different_hue = hsl.with_hue(90.0);
        assert_eq!(different_hue.hue, 90.0);
    }

    #[test]
    fn test_hsv_adjustments() {
        let hsv = HSVColor::new(180.0, 0.5, 0.5, 1.0);

        let brighter = hsv.with_value(0.8);
        assert_eq!(brighter.value, 0.8);
        assert_eq!(brighter.hue, hsv.hue);

        let more_saturated = hsv.with_saturation(1.0);
        assert_eq!(more_saturated.saturation, 1.0);

        let different_hue = hsv.with_hue(90.0);
        assert_eq!(different_hue.hue, 90.0);
    }
}
