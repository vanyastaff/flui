//! HSL and HSV color space representations.
//!
//! These color spaces provide alternative ways to work with colors,
//! making it easier to adjust properties like brightness, saturation, and hue.

use super::color::Color;

/// A color represented in the HSL (Hue, Saturation, Lightness) color space.
///
/// HSL is useful for making colors lighter or darker while maintaining the same hue.
///
/// # Examples
///
/// ```
/// use flui_types::{Color, HSLColor};
///
/// let red = Color::RED;
/// let hsl: HSLColor = red.into();
///
/// // Make it lighter
/// let lighter_red: Color = hsl.with_lightness(0.7).into();
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HSLColor {
    /// Hue in degrees (0.0-360.0).
    pub hue: f32,
    /// Saturation (0.0-1.0).
    pub saturation: f32,
    /// Lightness (0.0-1.0).
    pub lightness: f32,
    /// Alpha/opacity (0.0-1.0).
    pub alpha: f32,
}

impl HSLColor {
    /// Creates a new HSL color.
    ///
    /// Values are clamped/wrapped to valid ranges:
    /// - hue: wrapped to 0-360 (handles negative values correctly)
    /// - saturation, lightness, alpha: clamped to 0-1
    pub fn new(hue: f32, saturation: f32, lightness: f32, alpha: f32) -> Self {
        Self {
            hue: hue.rem_euclid(360.0),  // Correctly wraps negative hues
            saturation: saturation.clamp(0.0, 1.0),
            lightness: lightness.clamp(0.0, 1.0),
            alpha: alpha.clamp(0.0, 1.0),
        }
    }

    /// Returns a copy with adjusted lightness.
    pub fn with_lightness(&self, lightness: f32) -> Self {
        Self::new(self.hue, self.saturation, lightness, self.alpha)
    }

    /// Returns a copy with adjusted saturation.
    pub fn with_saturation(&self, saturation: f32) -> Self {
        Self::new(self.hue, saturation, self.lightness, self.alpha)
    }

    /// Returns a copy with adjusted hue.
    pub fn with_hue(&self, hue: f32) -> Self {
        Self::new(hue, self.saturation, self.lightness, self.alpha)
    }

    /// Returns a copy with adjusted alpha.
    pub fn with_alpha(&self, alpha: f32) -> Self {
        Self::new(self.hue, self.saturation, self.lightness, alpha)
    }
}

impl From<Color> for HSLColor {
    fn from(color: Color) -> Self {
        let r = color.r as f32 / 255.0;
        let g = color.g as f32 / 255.0;
        let b = color.b as f32 / 255.0;
        let a = color.a as f32 / 255.0;

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

        Color::rgba(
            ((r + m) * 255.0) as u8,
            ((g + m) * 255.0) as u8,
            ((b + m) * 255.0) as u8,
            (hsl.alpha * 255.0) as u8,
        )
    }
}

/// A color represented in the HSV (Hue, Saturation, Value/Brightness) color space.
///
/// HSV is useful for adjusting the brightness of colors.
///
/// # Examples
///
/// ```
/// use flui_types::{Color, HSVColor};
///
/// let blue = Color::BLUE;
/// let hsv: HSVColor = blue.into();
///
/// // Make it darker
/// let darker_blue: Color = hsv.with_value(0.5).into();
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HSVColor {
    /// Hue in degrees (0.0-360.0).
    pub hue: f32,
    /// Saturation (0.0-1.0).
    pub saturation: f32,
    /// Value/Brightness (0.0-1.0).
    pub value: f32,
    /// Alpha/opacity (0.0-1.0).
    pub alpha: f32,
}

impl HSVColor {
    /// Creates a new HSV color.
    ///
    /// Values are clamped/wrapped to valid ranges:
    /// - hue: wrapped to 0-360 (handles negative values correctly)
    /// - saturation, value, alpha: clamped to 0-1
    pub fn new(hue: f32, saturation: f32, value: f32, alpha: f32) -> Self {
        Self {
            hue: hue.rem_euclid(360.0),  // Correctly wraps negative hues
            saturation: saturation.clamp(0.0, 1.0),
            value: value.clamp(0.0, 1.0),
            alpha: alpha.clamp(0.0, 1.0),
        }
    }

    /// Returns a copy with adjusted value/brightness.
    pub fn with_value(&self, value: f32) -> Self {
        Self::new(self.hue, self.saturation, value, self.alpha)
    }

    /// Returns a copy with adjusted saturation.
    pub fn with_saturation(&self, saturation: f32) -> Self {
        Self::new(self.hue, saturation, self.value, self.alpha)
    }

    /// Returns a copy with adjusted hue.
    pub fn with_hue(&self, hue: f32) -> Self {
        Self::new(hue, self.saturation, self.value, self.alpha)
    }

    /// Returns a copy with adjusted alpha.
    pub fn with_alpha(&self, alpha: f32) -> Self {
        Self::new(self.hue, self.saturation, self.value, alpha)
    }
}

impl From<Color> for HSVColor {
    fn from(color: Color) -> Self {
        let r = color.r as f32 / 255.0;
        let g = color.g as f32 / 255.0;
        let b = color.b as f32 / 255.0;
        let a = color.a as f32 / 255.0;

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

        Color::rgba(
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
    fn test_hsl_conversion() {
        let red = Color::RED;
        let hsl: HSLColor = red.into();

        assert!((hsl.hue - 0.0).abs() < 1.0);
        assert!((hsl.saturation - 1.0).abs() < 0.01);
        assert!((hsl.lightness - 0.5).abs() < 0.01);
        assert_eq!(hsl.alpha, 1.0);

        let back: Color = hsl.into();
        assert!((back.r as i16 - red.r as i16).abs() <= 2);
    }

    #[test]
    fn test_hsv_conversion() {
        let red = Color::RED;
        let hsv: HSVColor = red.into();

        assert!((hsv.hue - 0.0).abs() < 1.0);
        assert!((hsv.saturation - 1.0).abs() < 0.01);
        assert!((hsv.value - 1.0).abs() < 0.01);
        assert_eq!(hsv.alpha, 1.0);

        let back: Color = hsv.into();
        assert!((back.r as i16 - red.r as i16).abs() <= 2);
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

        let transparent = hsl.with_alpha(0.5);
        assert_eq!(transparent.alpha, 0.5);
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

        let transparent = hsv.with_alpha(0.5);
        assert_eq!(transparent.alpha, 0.5);
    }

    #[test]
    fn test_hsl_wrapping() {
        let hsl = HSLColor::new(400.0, 1.5, -0.5, 2.0);
        assert_eq!(hsl.hue, 40.0); // 400 % 360
        assert_eq!(hsl.saturation, 1.0); // clamped
        assert_eq!(hsl.lightness, 0.0); // clamped
        assert_eq!(hsl.alpha, 1.0); // clamped
    }

    #[test]
    fn test_hsv_wrapping() {
        let hsv = HSVColor::new(400.0, 1.5, -0.5, 2.0);
        assert_eq!(hsv.hue, 40.0); // 400 % 360
        assert_eq!(hsv.saturation, 1.0); // clamped
        assert_eq!(hsv.value, 0.0); // clamped
        assert_eq!(hsv.alpha, 1.0); // clamped
    }

    #[test]
    fn test_color_round_trip_hsl() {
        let colors = [
            Color::RED,
            Color::GREEN,
            Color::BLUE,
            Color::WHITE,
            Color::BLACK,
            Color::GRAY,
        ];

        for original in colors {
            let hsl: HSLColor = original.into();
            let back: Color = hsl.into();

            // Allow small error due to rounding
            assert!((back.r as i16 - original.r as i16).abs() <= 2);
            assert!((back.g as i16 - original.g as i16).abs() <= 2);
            assert!((back.b as i16 - original.b as i16).abs() <= 2);
        }
    }

    #[test]
    fn test_color_round_trip_hsv() {
        let colors = [
            Color::RED,
            Color::GREEN,
            Color::BLUE,
            Color::WHITE,
            Color::BLACK,
            Color::GRAY,
        ];

        for original in colors {
            let hsv: HSVColor = original.into();
            let back: Color = hsv.into();

            // Allow small error due to rounding
            assert!((back.r as i16 - original.r as i16).abs() <= 2);
            assert!((back.g as i16 - original.g as i16).abs() <= 2);
            assert!((back.b as i16 - original.b as i16).abs() <= 2);
        }
    }

    #[test]
    fn test_negative_hue_wrapping_hsl() {
        // Test negative hue values wrap correctly using rem_euclid
        let hsl1 = HSLColor::new(-30.0, 0.5, 0.5, 1.0);
        assert_eq!(hsl1.hue, 330.0, "hue -30 should wrap to 330");

        let hsl2 = HSLColor::new(-90.0, 0.5, 0.5, 1.0);
        assert_eq!(hsl2.hue, 270.0, "hue -90 should wrap to 270");

        let hsl3 = HSLColor::new(-360.0, 0.5, 0.5, 1.0);
        assert_eq!(hsl3.hue, 0.0, "hue -360 should wrap to 0");

        let hsl4 = HSLColor::new(-720.0, 0.5, 0.5, 1.0);
        assert_eq!(hsl4.hue, 0.0, "hue -720 should wrap to 0");
    }

    #[test]
    fn test_negative_hue_wrapping_hsv() {
        // Test negative hue values wrap correctly using rem_euclid
        let hsv1 = HSVColor::new(-30.0, 0.5, 0.5, 1.0);
        assert_eq!(hsv1.hue, 330.0, "hue -30 should wrap to 330");

        let hsv2 = HSVColor::new(-90.0, 0.5, 0.5, 1.0);
        assert_eq!(hsv2.hue, 270.0, "hue -90 should wrap to 270");

        let hsv3 = HSVColor::new(-360.0, 0.5, 0.5, 1.0);
        assert_eq!(hsv3.hue, 0.0, "hue -360 should wrap to 0");

        let hsv4 = HSVColor::new(-720.0, 0.5, 0.5, 1.0);
        assert_eq!(hsv4.hue, 0.0, "hue -720 should wrap to 0");
    }
}
