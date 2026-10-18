//! Color types and utilities for Flui.
//!
//! This module provides a comprehensive Color type with conversions between
//! different color spaces (RGB, HSL, HSV).

use std::hash::{Hash, Hasher};

/// Weights within this range keep the direct f32 Oklab mix and its cubes finite.
/// Larger weights use scaled arithmetic, rather than truncating extrapolation.
const MAX_DIRECT_LERP_WEIGHT: f64 = 1e6;

/// A finite, normalized sRGB color with straight (unmultiplied) alpha.
///
/// Channels are in sRGB gamma space. The
/// renderer premultiplies when it converts a color for the GPU. Components retain
/// floating-point precision through interpolation and paint; explicit byte exports
/// round to RGBA8. Private storage excludes NaN, infinity and negative zero, making
/// exact equality and hashing lawful. Transparent colors retain authored hidden RGB.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Color {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

impl Eq for Color {}

/// Explicit normalized wire units; deserialization cannot bypass admission.
#[cfg(feature = "serde")]
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ColorWire {
    srgb: [f64; 3],
    alpha: f64,
}

#[cfg(feature = "serde")]
impl serde::Serialize for Color {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ColorWire {
            srgb: [self.r, self.g, self.b].map(f64::from),
            alpha: f64::from(self.a),
        }
        .serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Color {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = ColorWire::deserialize(deserializer)?;
        let [r, g, b] = wire.srgb;
        let rgba = [r, g, b, wire.alpha];
        if rgba
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err(serde::de::Error::custom(
                "Color requires finite normalized sRGB and alpha",
            ));
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "wire range validated before narrowing to Color precision"
        )]
        let rgba = rgba.map(|v| v as f32);
        Ok(Self::from_rgba_f32_array(rgba))
    }
}

impl Hash for Color {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.to_rgba_f32_array().map(f32::to_bits).hash(state);
    }
}

/// Admit normalized channels, including saturating conversion results.
#[inline]
fn normalized(channel: f32) -> f32 {
    if channel.is_nan() || channel <= 0.0 {
        0.0
    } else {
        channel.min(1.0)
    }
}

/// Round a private finite f32 channel in 0..=1 at the explicit byte boundary.
/// Widening first preserves the side of each half-byte rounding threshold.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "private normalized channels round into 0..=255"
)]
const fn byte_channel(channel: f32) -> u8 {
    (channel as f64 * 255.0 + 0.5) as u8
}

/// A color in the Oklab perceptually uniform color space.
///
/// Produced by [`Color::to_oklab`]; consumed by [`Color::from_oklab`].
/// `L` is perceived lightness in roughly `[0, 1]`;
/// `a`/`b` are the green–red and blue–yellow opponent axes (small values,
/// typically within `[-0.4, 0.4]` for sRGB colors).
///
/// Reference: Björn Ottosson, "A perceptual color space for image
/// processing" (2020), <https://bottosson.github.io/posts/oklab/>.
#[derive(Copy, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Oklab {
    /// Perceived lightness.
    pub l: f32,
    /// Green–red opponent axis.
    pub a: f32,
    /// Blue–yellow opponent axis.
    pub b: f32,
}

/// An Oklab colour with alpha, its `L`, `a`, `b` multiplied by `alpha`.
///
/// The space colours interpolate in ([`Color::lerp`], ADR-0149): a weighted sum of these
/// vectors is a premultiplied mix, so a component-wise animation of a colour (a spring,
/// say) runs on [`Color::to_premultiplied_oklab`] and converts back with
/// [`Color::from_premultiplied_oklab`]. `alpha` is in `[0, 1]` for a converted colour.
#[derive(Copy, Clone, Debug, PartialEq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PremultipliedOklab {
    /// Perceived lightness times alpha.
    pub l: f32,
    /// Green–red opponent axis times alpha.
    pub a: f32,
    /// Blue–yellow opponent axis times alpha.
    pub b: f32,
    /// Opacity in `[0, 1]`.
    pub alpha: f32,
}

impl Oklab {
    /// Multiply signed Oklab components by normalized alpha.
    fn premultiplied(self, alpha: f32) -> PremultipliedOklab {
        PremultipliedOklab {
            l: self.l * alpha,
            a: self.a * alpha,
            b: self.b * alpha,
            alpha,
        }
    }
}

impl Color {
    // ===== Constructors =====

    /// Creates a color from RGBA values (0-255).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_painting::styling::Color;
    ///
    /// let red = Color::rgba(255, 0, 0, 255);
    /// let semi_transparent = Color::rgba(100, 200, 150, 128);
    /// ```
    #[inline]
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: a as f32 / 255.0,
        }
    }

    /// Creates a fully opaque color from RGB values (0-255).
    ///
    /// Equivalent to `Color::rgba(r, g, b, 255)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_painting::styling::Color;
    ///
    /// let blue = Color::rgb(0, 0, 255);
    /// assert!(blue.is_opaque());
    /// ```
    #[inline]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 255)
    }

    /// Creates a color from a 32-bit ARGB value.
    ///
    /// Format: 0xAARRGGBB (alpha, red, green, blue)
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_painting::styling::Color;
    ///
    /// let red = Color::from_argb(0xFFFF0000);
    /// assert_eq!(red, Color::rgb(255, 0, 0));
    /// ```
    #[inline]
    pub const fn from_argb(argb: u32) -> Self {
        let a = ((argb >> 24) & 0xFF) as u8;
        let r = ((argb >> 16) & 0xFF) as u8;
        let g = ((argb >> 8) & 0xFF) as u8;
        let b = (argb & 0xFF) as u8;
        Self::rgba(r, g, b, a)
    }

    /// Creates a color from a hex string.
    ///
    /// Supports formats: "#RRGGBB", "RRGGBB", "#AARRGGBB", "AARRGGBB"
    ///
    /// # Errors
    ///
    /// Returns [`ParseColorError::InvalidLength`] if the string is not 6 or 8
    /// characters (excluding the optional `#` prefix).
    ///
    /// Returns [`ParseColorError::InvalidHex`] if the string contains
    /// non-hexadecimal characters.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_painting::styling::Color;
    ///
    /// let red = Color::from_hex("#FF0000").unwrap();
    /// let blue = Color::from_hex("0000FF").unwrap();
    /// let semi_transparent = Color::from_hex("#80FF0000").unwrap();
    ///
    /// assert!(Color::from_hex("invalid").is_err());
    /// ```
    #[inline]
    pub fn from_hex(hex: &str) -> Result<Self, ParseColorError> {
        let hex = hex.trim_start_matches('#');

        match hex.len() {
            6 => {
                let rgb = u32::from_str_radix(hex, 16).map_err(|_| ParseColorError::InvalidHex)?;
                Ok(Self::from_argb((0xFF << 24) | rgb))
            }
            8 => {
                let argb = u32::from_str_radix(hex, 16).map_err(|_| ParseColorError::InvalidHex)?;
                Ok(Self::from_argb(argb))
            }
            _ => Err(ParseColorError::InvalidLength),
        }
    }

    // ===== Component accessors =====

    /// Returns the alpha channel as an opacity value in `0.0..=1.0`.
    #[inline]
    #[must_use]
    pub const fn opacity(&self) -> f32 {
        self.a
    }

    // ===== Modifiers =====

    /// Returns a new color with the specified alpha value (0-255).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_painting::styling::Color;
    ///
    /// let opaque_red = Color::rgb(255, 0, 0);
    /// let transparent_red = opaque_red.with_alpha(128);
    ///
    /// assert_eq!(transparent_red.to_rgba8()[3], 128);
    /// ```
    #[inline]
    pub const fn with_alpha(&self, alpha: u8) -> Self {
        Self {
            a: alpha as f32 / 255.0,
            ..*self
        }
    }

    /// Returns a new color with the specified opacity (0.0-1.0).
    ///
    /// Values saturate to the valid range; NaN becomes zero. No byte quantization
    /// occurs until an explicit export such as [`Self::to_rgba8`].
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_painting::styling::Color;
    ///
    /// let opaque = Color::rgb(255, 0, 0);
    /// let half = opaque.with_opacity(0.5);
    ///
    /// assert_eq!(half.opacity(), 0.5);
    /// ```
    #[inline]
    pub fn with_opacity(&self, opacity: f64) -> Self {
        #[expect(clippy::cast_possible_truncation, reason = "normalized alpha uses f32")]
        let alpha = normalized(opacity.clamp(0.0, 1.0) as f32);
        Self { a: alpha, ..*self }
    }

    /// Returns a new color with the specified red component.
    #[inline]
    pub const fn with_red(&self, red: u8) -> Self {
        Self {
            r: red as f32 / 255.0,
            ..*self
        }
    }

    /// Returns a new color with the specified green component.
    #[inline]
    pub const fn with_green(&self, green: u8) -> Self {
        Self {
            g: green as f32 / 255.0,
            ..*self
        }
    }

    /// Returns a new color with the specified blue component.
    #[inline]
    pub const fn with_blue(&self, blue: u8) -> Self {
        Self {
            b: blue as f32 / 255.0,
            ..*self
        }
    }

    // ===== Checks =====

    /// Returns true if this color is fully transparent (alpha = 0).
    #[inline]
    pub const fn is_transparent(&self) -> bool {
        self.a == 0.0
    }

    /// Returns true if this color is fully opaque (normalized alpha = 1).
    #[inline]
    pub const fn is_opaque(&self) -> bool {
        self.a == 1.0
    }

    // ===== Operations =====

    /// Interpolates from `a` to `b` in Oklab with premultiplied alpha, `t` clamped to
    /// `[0, 1]` (ADR-0149).
    ///
    /// Both colours are converted to Oklab, their `L`, `a`, `b` weighted by alpha
    /// ([`Color::to_premultiplied_oklab`]), mixed, and divided by the mixed alpha
    /// ([`Color::from_premultiplied_oklab`]). Oklab keeps perceived lightness even (black
    /// to white is `rgb(99, 99, 99)` half way, where a gamma-sRGB mix gives 128), and
    /// premultiplying keeps a fade to transparent on the opaque end's hue instead of
    /// darkening toward the transparent end's black. Where the mixed alpha is zero the
    /// colour components mix straight. Out-of-gamut channels clamp.
    ///
    /// `t <= 0` returns `a` and `t >= 1` returns `b` exactly; a NaN `t` returns `a`. The
    /// [`Lerp`](flui_foundation::geometry::Lerp) impl is the same interpolation without the
    /// clamp, for overshooting curves.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_painting::styling::Color;
    ///
    /// assert_eq!(Color::lerp(Color::BLACK, Color::WHITE, 0.5).to_rgba8(), [99, 99, 99, 255]);
    /// // A fade to transparent keeps the red.
    /// let half = Color::lerp(Color::rgb(255, 0, 0), Color::TRANSPARENT, 0.5);
    /// assert_eq!(half.opacity(), 0.5);
    /// assert_eq!(half.to_rgba8(), [255, 0, 0, 128]);
    /// ```
    #[must_use]
    pub fn lerp(a: Color, b: Color, t: f64) -> Color {
        if t.is_nan() {
            return a;
        }
        Self::lerp_unclamped(a, b, t.clamp(0.0, 1.0))
    }

    /// [`Self::lerp`] without the clamp: `t` outside `[0, 1]` extrapolates and the result
    /// saturates. `t == 0` and `t == 1` return the endpoints exactly; a NaN `t` returns `a`.
    pub(crate) fn lerp_unclamped(a: Color, b: Color, t: f64) -> Color {
        if t == 0.0 || t.is_nan() || a == b {
            return a;
        }
        if t == 1.0 {
            return b;
        }
        let (la, lb) = (a.to_oklab(), b.to_oklab());
        if t.abs() > MAX_DIRECT_LERP_WEIGHT {
            return Self::lerp_scaled(a, b, la, lb, t);
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "direct mix weight is finite and bounded; f32 is color math precision"
        )]
        let (wide_t, t) = (t, t as f32);
        let mix = |from: f32, to: f32| from + (to - from) * t;
        if a.a == b.a {
            // Equal alphas cancel out of the premultiplied mix exactly: mix straight
            // and keep the alpha (the common opaque-to-opaque case, no divisions).
            let straight = Oklab {
                l: mix(la.l, lb.l),
                a: mix(la.a, lb.a),
                b: mix(la.b, lb.b),
            };
            return Color::from_oklab_normalized(straight, a.a);
        }
        // Mixed in `f64`: toward a transparent end the weights cancel (`a * (1 - t)` with
        // `1 - t` near the `f32` epsilon), and the numerator and the alpha it is divided
        // by would each round differently, shifting the colour at vanishing alpha.
        let (alpha_a, alpha_b) = (f64::from(a.a), f64::from(b.a));
        let wide_mix = |from: f64, to: f64| from + (to - from) * wide_t;
        let alpha = wide_mix(alpha_a, alpha_b);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "mixed alpha is in [0, 1]; narrowing is the output's f32 precision"
        )]
        let alpha32 = alpha as f32;
        if alpha32 > 0.0 {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "the divided components are Oklab values; f32 is color math precision"
            )]
            let component = |from: f32, to: f32| {
                (wide_mix(f64::from(from) * alpha_a, f64::from(to) * alpha_b) / alpha) as f32
            };
            let lab = Oklab {
                l: component(la.l, lb.l),
                a: component(la.a, lb.a),
                b: component(la.b, lb.b),
            };
            return Color::from_oklab_normalized(lab, alpha32);
        }
        // No alpha to weight by at this `t`: mix the colour straight, fully transparent.
        let straight = Oklab {
            l: mix(la.l, lb.l),
            a: mix(la.a, lb.a),
            b: mix(la.b, lb.b),
        };
        Color::from_oklab_normalized(straight, 0.0)
    }

    /// Evaluate an affine mix divided by |t|, so even infinite directional
    /// limits retain endpoint differences without overflowing intermediate sums.
    fn lerp_scaled(a: Color, b: Color, la: Oklab, lb: Oklab, t: f64) -> Self {
        let inverse_scale = t.abs().recip();
        let weight = t.signum();
        let from = [la.l, la.a, la.b].map(f64::from);
        let to = [lb.l, lb.a, lb.b].map(f64::from);
        let mix = |from: f64, to: f64| from * inverse_scale + (to - from) * weight;
        let straight = std::array::from_fn(|i| mix(from[i], to[i]));
        if a.a == b.a {
            return Self::from_scaled_oklab(straight, inverse_scale, a.a);
        }
        let (from_alpha, to_alpha) = (f64::from(a.a), f64::from(b.a));
        let alpha_scaled = mix(from_alpha, to_alpha);
        if alpha_scaled <= 0.0 {
            return Self::from_scaled_oklab(straight, inverse_scale, 0.0);
        }
        let premultiplied = std::array::from_fn(|i| mix(from[i] * from_alpha, to[i] * to_alpha));
        #[expect(
            clippy::cast_possible_truncation,
            reason = "positive extrapolated alpha saturates before narrowing"
        )]
        let alpha = (alpha_scaled / inverse_scale).min(1.0) as f32;
        Self::from_scaled_oklab(premultiplied, alpha_scaled, alpha)
    }

    /// Oklab to linear sRGB is homogeneous of degree three. Convert a bounded
    /// direction through the color crate, then restore magnitude with saturated
    /// division. This keeps the output clamp without overflowing Oklab cubes.
    fn from_scaled_oklab(components: [f64; 3], divisor: f64, alpha: f32) -> Self {
        let magnitude = components.into_iter().map(f64::abs).fold(0.0, f64::max);
        if magnitude == 0.0 {
            return Self::from_rgba_f32_array([0.0, 0.0, 0.0, alpha]);
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "normalized Oklab direction components are within [-1, 1]"
        )]
        let [l, a, b] = components.map(|component| (component / magnitude) as f32);
        let linear =
            color::AlphaColor::<color::Oklab>::new([l, a, b, alpha]).convert::<color::LinearSrgb>();
        let inverse_magnitude = divisor / magnitude;
        let threshold = inverse_magnitude * inverse_magnitude * inverse_magnitude;
        let restore = |channel: f32| {
            let channel = f64::from(channel);
            if channel <= 0.0 {
                0.0
            } else if channel >= threshold {
                1.0
            } else {
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "channel below positive threshold restores within [0, 1]"
                )]
                {
                    (channel / threshold) as f32
                }
            }
        };
        let [r, g, b, _] = linear.components;
        let restored = [restore(r), restore(g), restore(b), alpha];
        let mut converted =
            color::AlphaColor::<color::LinearSrgb>::new(restored).convert::<color::Srgb>();
        // The transfer function fixes zero and one exactly. Preserve output
        // saturation even if the upstream floating evaluation rounds inward.
        for (channel, linear) in converted.components[..3].iter_mut().zip(restored) {
            if linear == 0.0 || linear == 1.0 {
                *channel = linear;
            }
        }
        Self::from_rgba_f32_array(converted.components)
    }

    /// Converts to a 32-bit ARGB value in `0xAARRGGBB` format.
    ///
    /// Rounds normalized channels to bytes. Byte-authored colors round-trip
    /// through [`Color::from_argb`]; fractional channels lose precision here.
    #[inline]
    #[must_use]
    pub const fn to_argb(&self) -> u32 {
        let [r, g, b, a] = self.to_rgba8();
        u32::from_be_bytes([a, r, g, b])
    }

    /// Exports gamma-encoded RGBA8, rounding each normalized channel to nearest.
    /// Fractional colors deliberately lose precision at this byte boundary.
    #[must_use]
    pub const fn to_rgba8(&self) -> [u8; 4] {
        [
            byte_channel(self.r),
            byte_channel(self.g),
            byte_channel(self.b),
            byte_channel(self.a),
        ]
    }

    /// Formats as an uppercase hex string: `#RRGGBB` when fully opaque,
    /// `#AARRGGBB` otherwise.
    ///
    /// Byte-authored colors round-trip through [`Color::from_hex`]. Fractional
    /// colors quantize at this export; use normalized components or serde to retain them.
    #[must_use]
    #[inline]
    pub fn to_hex(&self) -> String {
        let [r, g, b, a] = self.to_rgba8();
        // Lookup table avoids format! machinery (no padding, no Display trait
        // dispatch).
        const HEX: &[u8; 16] = b"0123456789ABCDEF";

        if self.is_opaque() {
            let mut s = String::with_capacity(7);
            s.push('#');
            for &b in &[r, g, b] {
                s.push(HEX[(b >> 4) as usize] as char);
                s.push(HEX[(b & 0x0F) as usize] as char);
            }
            s
        } else {
            let mut s = String::with_capacity(9);
            s.push('#');
            for &b in &[a, r, g, b] {
                s.push(HEX[(b >> 4) as usize] as char);
                s.push(HEX[(b & 0x0F) as usize] as char);
            }
            s
        }
    }

    /// Returns the RGBA channels as an `[r, g, b, a]` array of `f32` values
    /// in `0.0..=1.0`.
    #[inline]
    #[must_use]
    pub const fn to_rgba_f32_array(&self) -> [f32; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// Creates a color from an `[r, g, b, a]` array of `f32` values in
    /// `0.0..=1.0`.
    ///
    /// Values saturate to `0.0..=1.0`; NaN and negative infinity become zero,
    /// positive infinity becomes one, and negative zero becomes positive zero.
    /// Authored hidden RGB is retained when alpha is zero.
    #[inline]
    #[must_use]
    pub fn from_rgba_f32_array(rgba: [f32; 4]) -> Self {
        let [r, g, b, a] = rgba.map(normalized);
        Self { r, g, b, a }
    }

    /// Returns the red channel as an `f32` in `0.0..=1.0`.
    #[inline]
    #[must_use]
    pub const fn red_f32(&self) -> f32 {
        self.r
    }

    /// Returns the green channel as an `f32` in `0.0..=1.0`.
    #[inline]
    #[must_use]
    pub const fn green_f32(&self) -> f32 {
        self.g
    }

    /// Returns the blue channel as an `f32` in `0.0..=1.0`.
    #[inline]
    #[must_use]
    pub const fn blue_f32(&self) -> f32 {
        self.b
    }

    /// Returns the alpha channel as an `f32` in `0.0..=1.0`.
    #[inline]
    #[must_use]
    pub const fn alpha_f32(&self) -> f32 {
        self.a
    }

    // ===== Helper methods for rendering =====

    /// Alpha-blends this color over `background` (Porter-Duff "source over",
    /// straight alpha, gamma space).
    ///
    /// Uses a float formulation: the background keeps
    /// `back = a_bg · (1 − a_fg)` of its alpha, the result's alpha is
    /// `a_fg + back` (exactly 1 over an opaque background), each channel is
    /// `(fg · a_fg + bg · back) / alpha`. Channels retain normalized float
    /// precision. An opaque source or transparent backdrop returns `self`;
    /// a transparent source returns `background`.
    #[must_use]
    #[inline]
    pub fn blend_over(&self, background: Color) -> Color {
        // Fast paths
        if self.a == 1.0 {
            return *self;
        }
        if self.a == 0.0 {
            return background;
        }
        if background.a == 0.0 {
            return *self;
        }

        self.blend_over_scalar(background)
    }

    /// `(a_fg, back, alpha)` for [`Color::blend_over`], or `None` when the
    /// result is fully transparent.
    #[inline]
    fn blend_over_factors(&self, background: Color) -> Option<(f32, f32, f32)> {
        let alpha = self.a;
        let inv_alpha = 1.0 - alpha;
        if background.a == 1.0 {
            return Some((alpha, inv_alpha, 1.0));
        }
        let back = background.a * inv_alpha;
        let out = alpha + back;
        (out > 0.0).then_some((alpha, back, out))
    }

    #[inline]
    fn blend_over_scalar(&self, background: Color) -> Color {
        let Some((alpha, back, out)) = self.blend_over_factors(background) else {
            return Color::TRANSPARENT;
        };
        let channel = |fg: f32, bg: f32| (fg * alpha + bg * back) / out;
        Color::from_rgba_f32_array([
            channel(self.r, background.r),
            channel(self.g, background.g),
            channel(self.b, background.b),
            out,
        ])
    }

    /// Multiplies each channel (including alpha) componentwise with `other`.
    #[inline]
    #[must_use]
    pub const fn multiply(&self, other: Color) -> Color {
        // Products of canonical finite normalized channels remain canonical.
        Color {
            r: self.r * other.r,
            g: self.g * other.g,
            b: self.b * other.b,
            a: self.a * other.a,
        }
    }

    /// Darkens the color by scaling the RGB channels by `factor` (clamped to
    /// `0.0..=1.0`).
    ///
    /// `0.0` yields black; `1.0` leaves the color unchanged. Alpha is
    /// preserved.
    #[inline]
    #[must_use]
    pub fn darken(&self, factor: f32) -> Color {
        let factor = factor.clamp(0.0, 1.0);
        Color::from_rgba_f32_array([self.r * factor, self.g * factor, self.b * factor, self.a])
    }

    /// Lightens the color by moving the RGB channels toward white by
    /// `factor` (clamped to `0.0..=1.0`).
    ///
    /// `0.0` leaves the color unchanged; `1.0` yields white. Alpha is
    /// preserved.
    #[inline]
    #[must_use]
    pub fn lighten(&self, factor: f32) -> Color {
        let factor = factor.clamp(0.0, 1.0);
        Color::from_rgba_f32_array([
            self.r + (1.0 - self.r) * factor,
            self.g + (1.0 - self.g) * factor,
            self.b + (1.0 - self.b) * factor,
            self.a,
        ])
    }

    /// Returns relative luminance in `0.0..=1.0`, using linear sRGB channels.
    ///
    /// Alpha is excluded. For a translucent painted surface, first composite
    /// over its actual opaque backdrop with [`Self::blend_over`].
    #[inline]
    #[must_use]
    pub fn luminance(&self) -> f32 {
        0.2126 * srgb_to_linear(self.r)
            + 0.7152 * srgb_to_linear(self.g)
            + 0.0722 * srgb_to_linear(self.b)
    }

    /// Returns `true` when white contrasts more strongly with this RGB than black.
    /// Uses [`Self::luminance`]; an exact contrast tie is classified as light.
    #[inline]
    #[must_use]
    pub fn is_dark(&self) -> bool {
        let luminance = f64::from(self.luminance()) + 0.05;
        1.05 / luminance > luminance / 0.05
    }

    /// Returns `true` when black contrasts at least as strongly as white.
    /// Uses the same RGB and alpha policy as [`Self::is_dark`].
    #[inline]
    #[must_use]
    pub fn is_light(&self) -> bool {
        !self.is_dark()
    }

    /// Chooses black or white with the greatest contrast over an opaque background.
    ///
    /// Uses the [WCAG relative-luminance contrast ratio](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html).
    /// Alpha is excluded: composite a translucent surface over its actual opaque
    /// backdrop first. This color choice alone does not establish application accessibility.
    #[inline]
    #[must_use]
    pub fn contrasting_text_color(&self) -> Color {
        if self.is_dark() {
            Color::WHITE
        } else {
            Color::BLACK
        }
    }

    // ===== Perceptual (Oklab) interpolation =====

    /// Convert to Oklab (perceptually uniform, Björn Ottosson 2020).
    ///
    /// Uses the `color` crate's sRGB-to-Oklab conversion. See the
    /// [Oklab definition](https://bottosson.github.io/posts/oklab/).
    /// Alpha is not part of Oklab and is carried separately by the caller.
    #[must_use]
    pub fn to_oklab(self) -> Oklab {
        let converted = color::AlphaColor::<color::Srgb>::new(self.to_rgba_f32_array())
            .convert::<color::Oklab>();
        let [l, a, b, _] = converted.components;
        Oklab { l, a, b }
    }

    /// Convert from Oklab back to sRGB, with byte alpha in `0..=255`.
    ///
    /// Uses the `color` crate's Oklab-to-sRGB conversion. Out-of-gamut results
    /// are clamped per channel.
    #[must_use]
    pub fn from_oklab(lab: Oklab, alpha: u8) -> Color {
        Self::from_oklab_normalized(lab, f32::from(alpha) / 255.0)
    }

    /// Convert before clamping: signed Oklab axes are not normalized channels.
    fn from_oklab_normalized(lab: Oklab, alpha: f32) -> Self {
        let converted = color::AlphaColor::<color::Oklab>::new([lab.l, lab.a, lab.b, alpha])
            .convert::<color::Srgb>();
        Self::from_rgba_f32_array(converted.components)
    }

    /// Converts to Oklab with alpha in `[0, 1]`, the colour components multiplied by it —
    /// the space [`Color::lerp`] mixes in. Any linear combination of these vectors is a
    /// premultiplied mix; [`Color::from_premultiplied_oklab`] converts back.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_painting::styling::Color;
    ///
    /// let clear = Color::rgba(255, 0, 0, 0).to_premultiplied_oklab();
    /// assert_eq!((clear.l, clear.a, clear.b, clear.alpha), (0.0, 0.0, 0.0, 0.0));
    /// ```
    #[must_use]
    pub fn to_premultiplied_oklab(self) -> PremultipliedOklab {
        self.to_oklab().premultiplied(self.a)
    }

    /// Converts a premultiplied Oklab vector back to a colour: the components are divided
    /// by `alpha`, which saturates into `[0, 1]`, and out-of-gamut channels clamp.
    ///
    /// A vector with no positive alpha (zero, negative or NaN) carries no colour and
    /// returns [`Color::TRANSPARENT`]. Inverts [`Color::to_premultiplied_oklab`] up to
    /// floating-point conversion error; subnormal alpha reduces RGB precision.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_painting::styling::Color;
    /// use flui_foundation::geometry::ApproxEq;
    ///
    /// let red = Color::rgb(255, 0, 0);
    /// assert!(Color::from_premultiplied_oklab(red.to_premultiplied_oklab()).approx_eq(&red));
    /// ```
    #[must_use]
    pub fn from_premultiplied_oklab(premultiplied: PremultipliedOklab) -> Color {
        let alpha = premultiplied.alpha;
        if alpha.is_nan() || alpha <= 0.0 {
            return Color::TRANSPARENT;
        }
        let lab = Oklab {
            l: premultiplied.l / alpha,
            a: premultiplied.a / alpha,
            b: premultiplied.b / alpha,
        };
        // Direct division above avoids overflowing 1/alpha for subnormal alpha.
        Color::from_oklab_normalized(lab, alpha)
    }

    /// Samples a multi-stop color ramp at position `t` (clamped to
    /// `0.0..=1.0`), like evaluating a gradient.
    ///
    /// `stops` are `(color, position)` pairs sorted by ascending position.
    /// Returns `Color::TRANSPARENT` for an empty slice, and the nearest
    /// endpoint color when `t` falls outside the stop range.
    #[must_use]
    #[inline]
    pub fn lerp_multi_stop(stops: &[(Color, f32)], t: f32) -> Color {
        if stops.is_empty() {
            return Color::TRANSPARENT;
        }

        if stops.len() == 1 {
            return stops[0].0;
        }

        let t = t.clamp(0.0, 1.0);

        // Binary search for the interval containing t — O(log n) vs O(n) linear scan.
        // partition_point returns the first index where stop > t,
        // so the bracket is [idx-1, idx].
        let idx = stops.partition_point(|&(_, stop)| stop <= t);

        if idx == 0 {
            return stops[0].0;
        }
        if idx >= stops.len() {
            return stops[stops.len() - 1].0;
        }

        let (color1, stop1) = stops[idx - 1];
        let (color2, stop2) = stops[idx];

        let range = stop2 - stop1;
        if range.abs() < f32::EPSILON {
            return color1;
        }

        let local_t = (t - stop1) / range;
        Color::lerp(color1, color2, f64::from(local_t))
    }

    /// Blends each color over the given background.
    #[must_use]
    #[inline]
    pub fn blend_over_batch(colors: &[Color], background: Color) -> Vec<Color> {
        colors
            .iter()
            .map(|color| color.blend_over(background))
            .collect()
    }

    /// Composites `self` (the **source**) over `dst` (the **destination**)
    /// using `mode`, returning the straight-alpha result.
    ///
    /// This generalizes [`Color::blend_over`] (the [`SrcOver`] case) to the full
    /// Porter-Duff operator set plus the W3C separable and non-separable blend
    /// modes. It is the per-pixel function behind `ui.ColorFilter.mode(color,
    /// mode)`, where the filter computes `mode(src = color, dst = each layer
    /// pixel)` before the layer is merged with its background.
    ///
    /// Channels are treated as straight (un-premultiplied), sRGB-*encoded*
    /// values normalized to `[0, 1]` — the same non-color-managed space
    /// [`Color::blend_over`] and the GPU blend pipeline use, so the Porter-Duff
    /// results agree with hardware blending.
    ///
    /// [`SrcOver`]: crate::paint::BlendMode::SrcOver
    #[must_use]
    pub fn blend(&self, dst: Color, mode: crate::paint::BlendMode) -> Color {
        use crate::paint::BlendMode;

        // Preserve exact identities before premultiplication can underflow.
        match mode {
            BlendMode::Src => return *self,
            BlendMode::Dst => return dst,
            BlendMode::SrcOver => return self.blend_over(dst),
            BlendMode::DstOver => return dst.blend_over(*self),
            _ => {}
        }

        let [src_r, src_g, src_b, src_a] = self.to_rgba_f32_array();
        let [dst_r, dst_g, dst_b, dst_a] = dst.to_rgba_f32_array();

        // The output color is accumulated premultiplied, then un-premultiplied
        // once at the end. (`src_pm`/`dst_pm` are used by the Modulate and
        // Porter-Duff branches; the advanced branch reads straight channels.)
        let src_pm = [src_r * src_a, src_g * src_a, src_b * src_a];
        let dst_pm = [dst_r * dst_a, dst_g * dst_a, dst_b * dst_a];

        let (out_r_pm, out_g_pm, out_b_pm, out_a) = if matches!(mode, BlendMode::Modulate) {
            // Modulate is the component-wise product of the premultiplied colors
            // (Skia `kModulate`): r = s * d on every channel.
            (
                src_pm[0] * dst_pm[0],
                src_pm[1] * dst_pm[1],
                src_pm[2] * dst_pm[2],
                src_a * dst_a,
            )
        } else if let Some((fa, fb)) = porter_duff_factors(mode, src_a, dst_a) {
            // Porter-Duff coverage blend: r = Fa * src_premul + Fb * dst_premul
            // (and the same for alpha).
            (
                (src_pm[0] * fa + dst_pm[0] * fb).clamp(0.0, 1.0),
                (src_pm[1] * fa + dst_pm[1] * fb).clamp(0.0, 1.0),
                (src_pm[2] * fa + dst_pm[2] * fb).clamp(0.0, 1.0),
                (src_a * fa + dst_a * fb).clamp(0.0, 1.0),
            )
        } else {
            // Separable / non-separable blend modes composite source-over with a
            // per-mode blend function B(Cb, Cs) (W3C Compositing and Blending
            // Level 1, §10–11):
            //   co = αs·(1-αb)·Cs + αs·αb·B(Cb,Cs) + (1-αs)·αb·Cb   (premultiplied)
            //   αo = αs + αb·(1-αs)
            let backdrop = [dst_r, dst_g, dst_b];
            let source = [src_r, src_g, src_b];
            let blended = if matches!(
                mode,
                BlendMode::Hue | BlendMode::Saturation | BlendMode::Color | BlendMode::Luminosity
            ) {
                nonseparable_blend(mode, backdrop, source)
            } else {
                [
                    separable_blend(mode, backdrop[0], source[0]),
                    separable_blend(mode, backdrop[1], source[1]),
                    separable_blend(mode, backdrop[2], source[2]),
                ]
            };

            let composite = |cs: f32, cb: f32, b: f32| {
                src_a * (1.0 - dst_a) * cs + src_a * dst_a * b + (1.0 - src_a) * dst_a * cb
            };
            (
                composite(source[0], backdrop[0], blended[0]),
                composite(source[1], backdrop[1], blended[1]),
                composite(source[2], backdrop[2], blended[2]),
                src_a + dst_a * (1.0 - src_a),
            )
        };

        if out_a <= 0.0 {
            return Color::TRANSPARENT;
        }
        // Retain normalized precision; only explicit byte exports quantize.
        Color::from_rgba_f32_array([out_r_pm / out_a, out_g_pm / out_a, out_b_pm / out_a, out_a])
    }

    // ===== Common color constants =====

    /// Fully transparent (alpha = 0).
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);

    /// Black (0, 0, 0).
    pub const BLACK: Color = Color::rgb(0, 0, 0);

    /// White (255, 255, 255).
    pub const WHITE: Color = Color::rgb(255, 255, 255);

    /// Red (255, 0, 0).
    pub const RED: Color = Color::rgb(255, 0, 0);

    /// Green (0, 255, 0).
    pub const GREEN: Color = Color::rgb(0, 255, 0);

    /// Blue (0, 0, 255).
    pub const BLUE: Color = Color::rgb(0, 0, 255);

    /// Yellow (255, 255, 0).
    pub const YELLOW: Color = Color::rgb(255, 255, 0);

    /// Cyan (0, 255, 255).
    pub const CYAN: Color = Color::rgb(0, 255, 255);

    /// Magenta (255, 0, 255).
    pub const MAGENTA: Color = Color::rgb(255, 0, 255);

    /// Gray (128, 128, 128).
    pub const GRAY: Color = Color::rgb(128, 128, 128);

    /// Light gray (192, 192, 192).
    pub const LIGHT_GRAY: Color = Color::rgb(192, 192, 192);

    /// Dark gray (64, 64, 64).
    pub const DARK_GRAY: Color = Color::rgb(64, 64, 64);

    // Material Design colors will be added in future commits
}

// ===== Transfer functions (IEC 61966-2-1 sRGB ↔ linear) =====

/// sRGB electro-optical transfer function: gamma-encoded → linear light.
///
/// Applies the IEC 61966-2-1 piecewise formula to a single channel `c` in
/// `[0, 1]` (straight sRGB). Returns the linearized value in `[0, 1]`.
///
/// Used by [`Color::luminance`] and the GPU gamma `ColorFilter`'s CPU oracle.
///
/// # Examples
/// ```
/// use flui_painting::styling::color::srgb_to_linear;
/// assert!((srgb_to_linear(0.0) - 0.0).abs() < 1e-6);
/// assert!((srgb_to_linear(1.0) - 1.0).abs() < 1e-6);
/// ```
#[inline]
#[must_use]
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Linear-light → sRGB opto-electronic transfer function (inverse of
/// [`srgb_to_linear`]).
///
/// Applies the IEC 61966-2-1 piecewise formula to a single linearized channel
/// `c` in `[0, 1]`. Returns the gamma-encoded sRGB value in `[0, 1]`.
///
/// Also available to CPU oracles for the GPU gamma `ColorFilter`.
///
/// # Examples
/// ```
/// use flui_painting::styling::color::linear_to_srgb;
/// assert!((linear_to_srgb(0.0) - 0.0).abs() < 1e-6);
/// assert!((linear_to_srgb(1.0) - 1.0).abs() < 1e-6);
/// ```
#[inline]
#[must_use]
pub fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        12.92 * c
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

// ===== Blend-mode evaluation helpers (used by `Color::blend`) =====

/// Porter-Duff source/destination coverage factors `(Fa, Fb)` for `mode`, given
/// source alpha `sa` and destination alpha `da`. The composited premultiplied
/// result is `Fa·src + Fb·dst` for every channel (and for alpha).
///
/// Returns `None` for modes that are not a coverage-factor blend — `Modulate`
/// (a component product) and the advanced separable/non-separable modes — which
/// [`Color::blend`] dispatches down its other branches.
fn porter_duff_factors(mode: crate::paint::BlendMode, sa: f32, da: f32) -> Option<(f32, f32)> {
    use crate::paint::BlendMode;
    Some(match mode {
        BlendMode::Clear => (0.0, 0.0),
        BlendMode::Src => (1.0, 0.0),
        BlendMode::Dst => (0.0, 1.0),
        BlendMode::SrcOver => (1.0, 1.0 - sa),
        BlendMode::DstOver => (1.0 - da, 1.0),
        BlendMode::SrcIn => (da, 0.0),
        BlendMode::DstIn => (0.0, sa),
        BlendMode::SrcOut => (1.0 - da, 0.0),
        BlendMode::DstOut => (0.0, 1.0 - sa),
        BlendMode::SrcATop => (da, 1.0 - sa),
        BlendMode::DstATop => (1.0 - da, sa),
        BlendMode::Xor => (1.0 - da, 1.0 - sa),
        BlendMode::Plus => (1.0, 1.0),
        _ => return None,
    })
}

/// W3C separable blend function `B(cb, cs)` for one channel, where `cb` is the
/// backdrop and `cs` the source (both straight, in `[0, 1]`). Only the separable
/// advanced modes are defined here; the four non-separable HSL modes are handled
/// by [`nonseparable_blend`], and Porter-Duff modes never reach this function.
fn separable_blend(mode: crate::paint::BlendMode, cb: f32, cs: f32) -> f32 {
    use crate::paint::BlendMode;
    match mode {
        BlendMode::Multiply => cb * cs,
        BlendMode::Screen => cb + cs - cb * cs,
        // overlay(cb, cs) == hardlight(cs, cb).
        BlendMode::Overlay => hard_light(cs, cb),
        BlendMode::Darken => cb.min(cs),
        BlendMode::Lighten => cb.max(cs),
        BlendMode::ColorDodge => {
            if cb <= 0.0 {
                0.0
            } else if cs >= 1.0 {
                1.0
            } else {
                (cb / (1.0 - cs)).min(1.0)
            }
        }
        BlendMode::ColorBurn => {
            if cb >= 1.0 {
                1.0
            } else if cs <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - cb) / cs).min(1.0)
            }
        }
        BlendMode::HardLight => hard_light(cb, cs),
        BlendMode::SoftLight => {
            if cs <= 0.5 {
                cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
            } else {
                let d = if cb <= 0.25 {
                    ((16.0 * cb - 12.0) * cb + 4.0) * cb
                } else {
                    cb.sqrt()
                };
                cb + (2.0 * cs - 1.0) * (d - cb)
            }
        }
        BlendMode::Difference => (cb - cs).abs(),
        BlendMode::Exclusion => cb + cs - 2.0 * cb * cs,
        _ => cs,
    }
}

/// W3C `HardLight(cb, cs)`: multiply for a dark source, screen for a light one.
/// Also the kernel of `Overlay` with the arguments swapped.
fn hard_light(cb: f32, cs: f32) -> f32 {
    if cs <= 0.5 {
        2.0 * cb * cs
    } else {
        1.0 - 2.0 * (1.0 - cb) * (1.0 - cs)
    }
}

/// W3C non-separable blend (`Hue`, `Saturation`, `Color`, `Luminosity`) over the
/// whole RGB triple; `cb` is the backdrop and `cs` the source.
fn nonseparable_blend(mode: crate::paint::BlendMode, cb: [f32; 3], cs: [f32; 3]) -> [f32; 3] {
    use crate::paint::BlendMode;
    match mode {
        BlendMode::Hue => set_lum(set_sat(cs, sat(cb)), lum(cb)),
        BlendMode::Saturation => set_lum(set_sat(cb, sat(cs)), lum(cb)),
        BlendMode::Color => set_lum(cs, lum(cb)),
        BlendMode::Luminosity => set_lum(cb, lum(cs)),
        _ => cs,
    }
}

/// Luminosity of an RGB triple (W3C `Lum`).
fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

/// Clip an RGB triple back into `[0, 1]` while preserving its luminosity
/// (W3C `ClipColor`). The epsilon guards avoid a `0/0` when all channels are
/// equal (a degenerate triple has nothing to scale).
fn clip_color(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut out = c;
    if n < 0.0 && (l - n).abs() > f32::EPSILON {
        for ch in &mut out {
            *ch = l + (*ch - l) * l / (l - n);
        }
    }
    if x > 1.0 && (x - l).abs() > f32::EPSILON {
        for ch in &mut out {
            *ch = l + (*ch - l) * (1.0 - l) / (x - l);
        }
    }
    out
}

/// Shift an RGB triple to the target luminosity `l` (W3C `SetLum`).
fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color([c[0] + d, c[1] + d, c[2] + d])
}

/// Saturation of an RGB triple (W3C `Sat`): max channel minus min channel.
fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

/// Rescale an RGB triple to the target saturation `s` (W3C `SetSat`), keeping
/// the relative channel ordering. A flat triple (max == min) collapses to black.
fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let mut idx = [0usize, 1, 2];
    idx.sort_by(|&a, &b| c[a].total_cmp(&c[b]));
    let (i_min, i_mid, i_max) = (idx[0], idx[1], idx[2]);
    let mut out = [0.0f32; 3];
    if c[i_max] > c[i_min] {
        out[i_mid] = (c[i_mid] - c[i_min]) * s / (c[i_max] - c[i_min]);
        out[i_max] = s;
    }
    out
}

impl Default for Color {
    #[inline]
    fn default() -> Self {
        Color::TRANSPARENT
    }
}

// ===== Conversions =====

impl From<(u8, u8, u8)> for Color {
    #[inline]
    fn from((r, g, b): (u8, u8, u8)) -> Self {
        Color::rgb(r, g, b)
    }
}

impl From<(u8, u8, u8, u8)> for Color {
    #[inline]
    fn from((r, g, b, a): (u8, u8, u8, u8)) -> Self {
        Color::rgba(r, g, b, a)
    }
}

impl From<[u8; 3]> for Color {
    #[inline]
    fn from([r, g, b]: [u8; 3]) -> Self {
        Color::rgb(r, g, b)
    }
}

impl From<[u8; 4]> for Color {
    #[inline]
    fn from([r, g, b, a]: [u8; 4]) -> Self {
        Color::rgba(r, g, b, a)
    }
}

// ===== Approximate equality =====

impl flui_foundation::geometry::ApproxEq for Color {
    /// Default epsilon for color comparison (approximately 1/255).
    ///
    /// Includes one normalized f32 rounding unit so adjacent byte-authored
    /// channels remain within the promised one-byte tolerance. An explicitly
    /// supplied epsilon is used exactly, without this allowance.
    const DEFAULT_EPSILON: f64 = 1.0 / 255.0 + f32::EPSILON as f64;

    /// Compares colors in normalized f32 space with epsilon tolerance.
    ///
    /// This is useful when comparing colors that have been converted through
    /// different color spaces or undergone floating-point calculations.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_painting::styling::Color;
    /// use flui_foundation::geometry::ApproxEq;
    ///
    /// let c1 = Color::rgb(100, 150, 200);
    /// let c2 = Color::rgb(100, 150, 200);
    /// let c3 = Color::rgb(100, 151, 200); // 1 unit difference
    ///
    /// assert!(c1.approx_eq(&c2));
    /// assert!(c1.approx_eq(&c3)); // Within default epsilon
    /// ```
    #[inline]
    fn approx_eq_eps(&self, other: &Self, epsilon: f64) -> bool {
        // Compare the stored normalized values, independently of byte exports.
        let within = |x: f32, y: f32| (f64::from(x) - f64::from(y)).abs() <= epsilon;
        within(self.r, other.r)
            && within(self.g, other.g)
            && within(self.b, other.b)
            && within(self.a, other.a)
    }
}

// ===== Error types =====

/// Error returned by [`Color::from_hex`] when parsing a hex color string
/// fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseColorError {
    /// Invalid hex string format
    InvalidHex,
    /// Invalid string length (must be 6 or 8 characters)
    InvalidLength,
}

impl std::fmt::Display for ParseColorError {
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseColorError::InvalidHex => write!(f, "Invalid hex color format"),
            ParseColorError::InvalidLength => {
                write!(f, "Invalid hex color length (expected 6 or 8 characters)")
            }
        }
    }
}

impl std::error::Error for ParseColorError {}

// ===== Tests =====
