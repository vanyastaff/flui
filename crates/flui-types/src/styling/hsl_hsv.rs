//! HSL and HSV color space representations.
//!
//! These color spaces provide alternative ways to work with colors,
//! making it easier to adjust properties like brightness, saturation, and hue.

use super::color::Color;

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
    #[inline]
    pub fn new(hue: f32, saturation: f32, lightness: f32, alpha: f32) -> Self {
        Self {
            hue: hue.rem_euclid(360.0), // Correctly wraps negative hues
            saturation: saturation.clamp(0.0, 1.0),
            lightness: lightness.clamp(0.0, 1.0),
            alpha: alpha.clamp(0.0, 1.0),
        }
    }

    /// Returns a copy with adjusted lightness.
    #[inline]
    pub fn with_lightness(&self, lightness: f32) -> Self {
        Self::new(self.hue, self.saturation, lightness, self.alpha)
    }

    /// Returns a copy with adjusted saturation.
    #[inline]
    pub fn with_saturation(&self, saturation: f32) -> Self {
        Self::new(self.hue, saturation, self.lightness, self.alpha)
    }

    /// Returns a copy with adjusted hue.
    #[inline]
    pub fn with_hue(&self, hue: f32) -> Self {
        Self::new(hue, self.saturation, self.lightness, self.alpha)
    }

    /// Returns a copy with adjusted alpha.
    #[inline]
    pub fn with_alpha(&self, alpha: f32) -> Self {
        Self::new(self.hue, self.saturation, self.lightness, alpha)
    }
}

impl From<Color> for HSLColor {
    #[inline]
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
    #[inline]
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
    #[inline]
    pub fn new(hue: f32, saturation: f32, value: f32, alpha: f32) -> Self {
        Self {
            hue: hue.rem_euclid(360.0), // Correctly wraps negative hues
            saturation: saturation.clamp(0.0, 1.0),
            value: value.clamp(0.0, 1.0),
            alpha: alpha.clamp(0.0, 1.0),
        }
    }

    /// Returns a copy with adjusted value/brightness.
    #[inline]
    pub fn with_value(&self, value: f32) -> Self {
        Self::new(self.hue, self.saturation, value, self.alpha)
    }

    /// Returns a copy with adjusted saturation.
    #[inline]
    pub fn with_saturation(&self, saturation: f32) -> Self {
        Self::new(self.hue, saturation, self.value, self.alpha)
    }

    /// Returns a copy with adjusted hue.
    #[inline]
    pub fn with_hue(&self, hue: f32) -> Self {
        Self::new(hue, self.saturation, self.value, self.alpha)
    }

    /// Returns a copy with adjusted alpha.
    #[inline]
    pub fn with_alpha(&self, alpha: f32) -> Self {
        Self::new(self.hue, self.saturation, self.value, alpha)
    }
}

impl From<Color> for HSVColor {
    #[inline]
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
    #[inline]
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
