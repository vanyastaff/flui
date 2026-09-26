//! HSL and HSV color space representations.
//!
//! These color spaces provide alternative ways to work with colors,
//! making it easier to adjust properties like brightness, saturation, and hue.

use super::color::Color;

/// A color in the HSL (hue, saturation, lightness) color space.
#[derive(Debug)]
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
    #[expect(clippy::manual_midpoint)]
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

        // `% 6.0` leaves the red sector negative below 0°; `new` wraps it.
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

        // Round like Flutter's `_colorFromHue`; truncating loses a unit
        // whenever the float lands just under an integer.
        Color::rgba(
            ((r + m) * 255.0).round() as u8,
            ((g + m) * 255.0).round() as u8,
            ((b + m) * 255.0).round() as u8,
            (hsl.alpha * 255.0).round() as u8,
        )
    }
}

/// A color in the HSV (hue, saturation, value) color space.
#[derive(Debug)]
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

        // `% 6.0` leaves the red sector negative below 0°; `new` wraps it.
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

        // Round like Flutter's `_colorFromHue`; truncating loses a unit
        // whenever the float lands just under an integer.
        Color::rgba(
            ((r + m) * 255.0).round() as u8,
            ((g + m) * 255.0).round() as u8,
            ((b + m) * 255.0).round() as u8,
            (hsv.alpha * 255.0).round() as u8,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn arb_color() -> impl Strategy<Value = Color> {
        (any::<u8>(), any::<u8>(), any::<u8>(), any::<u8>())
            .prop_map(|(r, g, b, a)| Color::rgba(r, g, b, a))
    }

    proptest! {
        /// Converting to HSL or HSV and back is lossless for every 8-bit
        /// color, alpha included.
        #[test]
        fn roundtrips_are_exact(c in arb_color()) {
            prop_assert_eq!(Color::from(HSLColor::from(c)), c);
            prop_assert_eq!(Color::from(HSVColor::from(c)), c);
        }
    }

    /// One primary or secondary color per 60° sector, so every branch of
    /// both directions is taken.
    #[test]
    fn hue_sectors() {
        for (color, hue) in [
            (Color::rgb(255, 0, 0), 0.0),
            (Color::rgb(255, 255, 0), 60.0),
            (Color::rgb(0, 255, 0), 120.0),
            (Color::rgb(0, 255, 255), 180.0),
            (Color::rgb(0, 0, 255), 240.0),
            (Color::rgb(255, 0, 255), 300.0),
            (Color::rgb(255, 0, 128), 330.0),
        ] {
            let (hsl, hsv) = (HSLColor::from(color), HSVColor::from(color));
            assert!(
                (hsl.hue - hue).abs() < 0.5 && (hsv.hue - hue).abs() < 0.5,
                "{color:?}: {hsl:?}"
            );
            assert_eq!(
                (hsl.saturation, hsl.lightness),
                (1.0, if hue == 330.0 { hsl.lightness } else { 0.5 })
            );
            assert_eq!((hsv.saturation, hsv.value), (1.0, 1.0));
        }
        // Grays have no hue or saturation, black and white included, where
        // the HSL saturation formula would divide zero by zero.
        for gray in [Color::BLACK, Color::rgb(128, 128, 128), Color::WHITE] {
            let hsl = HSLColor::from(gray);
            assert_eq!((hsl.hue, hsl.saturation), (0.0, 0.0), "{gray:?}");
            assert_eq!(Color::from(hsl), gray);
            let hsv = HSVColor::from(gray);
            assert_eq!((hsv.hue, hsv.saturation), (0.0, 0.0), "{gray:?}");
            assert_eq!(Color::from(hsv), gray);
        }
    }

    /// Hue wraps into `0..360`, the rest clamps into `0..=1`.
    #[test]
    fn constructors_wrap_and_clamp() {
        let hsl = HSLColor::new(-30.0, 1.5, -0.5, 2.0);
        assert_eq!(
            (hsl.hue, hsl.saturation, hsl.lightness, hsl.alpha),
            (330.0, 1.0, 0.0, 1.0)
        );
        let hsv = HSVColor::new(390.0, -1.0, 1.5, -1.0);
        assert_eq!(
            (hsv.hue, hsv.saturation, hsv.value, hsv.alpha),
            (30.0, 0.0, 1.0, 0.0)
        );
    }

    #[test]
    fn setters_replace_one_component() {
        let hsl = HSLColor::new(10.0, 0.2, 0.3, 0.4);
        let get = |c: HSLColor| (c.hue, c.saturation, c.lightness, c.alpha);
        assert_eq!(get(hsl.with_hue(370.0)), (10.0, 0.2, 0.3, 0.4));
        assert_eq!(get(hsl.with_saturation(0.9)), (10.0, 0.9, 0.3, 0.4));
        assert_eq!(get(hsl.with_lightness(0.9)), (10.0, 0.2, 0.9, 0.4));
        assert_eq!(get(hsl.with_alpha(0.9)), (10.0, 0.2, 0.3, 0.9));

        let hsv = HSVColor::new(10.0, 0.2, 0.3, 0.4);
        let get = |c: HSVColor| (c.hue, c.saturation, c.value, c.alpha);
        assert_eq!(get(hsv.with_hue(20.0)), (20.0, 0.2, 0.3, 0.4));
        assert_eq!(get(hsv.with_saturation(0.9)), (10.0, 0.9, 0.3, 0.4));
        assert_eq!(get(hsv.with_value(0.9)), (10.0, 0.2, 0.9, 0.4));
        assert_eq!(get(hsv.with_alpha(0.9)), (10.0, 0.2, 0.3, 0.9));
    }
}
