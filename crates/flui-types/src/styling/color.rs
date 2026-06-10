//! Color types and utilities for Flui.
//!
//! This module provides a comprehensive Color type with conversions between
//! different color spaces (RGB, HSL, HSV), similar to Flutter's Color system.

// `Color` is a plain RGBA quadruple of independent `u8` channels — every bit
// pattern is a valid `Color`. The derived `Deserialize` therefore cannot
// produce an instance that violates any invariant the `unsafe` SIMD helpers
// rely on (they only read the four channels), so the lint's concern does not
// apply here.
#[cfg_attr(
    feature = "serde",
    allow(
        clippy::unsafe_derive_deserialize,
        reason = "Color has no field invariant; all u8 quadruples are valid"
    )
)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Color {
    /// Red channel (0-255)
    pub r: u8,
    /// Green channel (0-255)
    pub g: u8,
    /// Blue channel (0-255)
    pub b: u8,
    /// Alpha channel (0-255, 0 = transparent, 255 = opaque)
    pub a: u8,
}

/// A color in the Oklab perceptually uniform color space.
///
/// Produced by [`Color::to_oklab`]; consumed by [`Color::from_oklab`] and
/// [`Color::lerp_oklab`]. `L` is perceived lightness in roughly `[0, 1]`;
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

impl Color {
    // ===== Constructors =====

    /// Creates a color from RGBA values (0-255).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Color;
    ///
    /// let red = Color::rgba(255, 0, 0, 255);
    /// let semi_transparent = Color::rgba(100, 200, 150, 128);
    /// ```
    #[inline]
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Creates a fully opaque color from RGB values (0-255).
    ///
    /// Equivalent to `Color::rgba(r, g, b, 255)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Color;
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
    /// use flui_types::Color;
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
    /// use flui_types::Color;
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

    #[inline]
    #[must_use]
    pub const fn opacity(&self) -> f32 {
        self.a as f32 / 255.0
    }

    // ===== Modifiers =====

    /// Returns a new color with the specified alpha value (0-255).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Color;
    ///
    /// let opaque_red = Color::rgb(255, 0, 0);
    /// let transparent_red = opaque_red.with_alpha(128);
    ///
    /// assert_eq!(transparent_red.a, 128);
    /// ```
    #[inline]
    pub const fn with_alpha(&self, alpha: u8) -> Self {
        Self::rgba(self.r, self.g, self.b, alpha)
    }

    /// Returns a new color with the specified opacity (0.0-1.0).
    ///
    /// Values are clamped to the valid range.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Color;
    ///
    /// let opaque = Color::rgb(255, 0, 0);
    /// let half = opaque.with_opacity(0.5);
    ///
    /// assert_eq!(half.a, 127); // 0.5 * 255
    /// ```
    #[inline]
    pub fn with_opacity(&self, opacity: f32) -> Self {
        let alpha = (opacity.clamp(0.0, 1.0) * 255.0) as u8;
        self.with_alpha(alpha)
    }

    /// Returns a new color with the specified red component.
    #[inline]
    pub const fn with_red(&self, red: u8) -> Self {
        Self::rgba(red, self.g, self.b, self.a)
    }

    /// Returns a new color with the specified green component.
    #[inline]
    pub const fn with_green(&self, green: u8) -> Self {
        Self::rgba(self.r, green, self.b, self.a)
    }

    /// Returns a new color with the specified blue component.
    #[inline]
    pub const fn with_blue(&self, blue: u8) -> Self {
        Self::rgba(self.r, self.g, blue, self.a)
    }

    // ===== Checks =====

    /// Returns true if this color is fully transparent (alpha = 0).
    #[inline]
    pub const fn is_transparent(&self) -> bool {
        self.a == 0
    }

    /// Returns true if this color is fully opaque (alpha = 255).
    #[inline]
    pub const fn is_opaque(&self) -> bool {
        self.a == 255
    }

    // ===== Operations =====

    /// Linear interpolation between two colors.
    ///
    /// When `t` = 0.0, returns `self`. When `t` = 1.0, returns `other`.
    /// Values are clamped to [0.0, 1.0].
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Color;
    ///
    /// let red = Color::rgb(255, 0, 0);
    /// let blue = Color::rgb(0, 0, 255);
    ///
    /// let purple = Color::lerp(red, blue, 0.5);
    /// assert!(purple.r > 0 && purple.b > 0);
    /// ```
    #[inline]
    pub fn lerp(a: Color, b: Color, t: f32) -> Color {
        #[cfg(all(feature = "simd", target_arch = "x86_64", target_feature = "sse2"))]
        {
            Self::lerp_simd_sse(a, b, t)
        }

        #[cfg(all(feature = "simd", target_arch = "aarch64", target_feature = "neon"))]
        {
            Self::lerp_simd_neon(a, b, t)
        }

        #[cfg(not(all(
            feature = "simd",
            any(
                all(target_arch = "x86_64", target_feature = "sse2"),
                all(target_arch = "aarch64", target_feature = "neon")
            )
        )))]
        {
            Self::lerp_scalar(a, b, t)
        }
    }

    #[inline]
    #[allow(
        dead_code,
        reason = "scalar fallback for `lerp`; unused when a SIMD path is compiled in (e.g. --features simd on x86_64), used on every other target"
    )]
    fn lerp_scalar(a: Color, b: Color, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        // Round, not truncate: `x as u8` truncates toward zero, biasing every
        // interpolated channel down by up to ~1 and producing a visibly darker
        // mid-tween. `.round()` matches Flutter's `Color.lerp` (and the `as u8`
        // cast still saturates out-of-range values to [0, 255]).
        let lerp_u8 = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;

        Color::rgba(
            lerp_u8(a.r, b.r),
            lerp_u8(a.g, b.g),
            lerp_u8(a.b, b.b),
            lerp_u8(a.a, b.a),
        )
    }

    #[inline]
    #[cfg(all(target_arch = "x86_64", not(target_family = "wasm")))]
    #[allow(
        dead_code,
        unsafe_code,
        reason = "SIMD twin of `lerp_scalar`: compiled on every x86_64 build but only called when the `simd` feature selects it in `lerp`; SSE2 intrinsics require unsafe"
    )]
    fn lerp_simd_sse(a: Color, b: Color, t: f32) -> Color {
        // SAFETY: gated on `target_feature = "sse2"`, so the intrinsics are
        // available; `_mm_storeu_ps` is an unaligned store into a live 4-f32
        // stack array, in bounds by construction.
        #[cfg(target_feature = "sse2")]
        unsafe {
            use std::arch::x86_64::*;

            let t = t.clamp(0.0, 1.0);

            // Convert u8 channels to f32
            let a_vec = _mm_set_ps(a.a as f32, a.b as f32, a.g as f32, a.r as f32);
            let b_vec = _mm_set_ps(b.a as f32, b.b as f32, b.g as f32, b.r as f32);
            let t_vec = _mm_set1_ps(t);

            // lerp: a + (b - a) * t
            let diff = _mm_sub_ps(b_vec, a_vec);
            let scaled = _mm_mul_ps(diff, t_vec);
            let result = _mm_add_ps(a_vec, scaled);

            // Convert back to u8
            let mut out = [0.0f32; 4];
            _mm_storeu_ps(out.as_mut_ptr(), result);

            Color::rgba(out[0] as u8, out[1] as u8, out[2] as u8, out[3] as u8)
        }

        #[cfg(not(target_feature = "sse2"))]
        {
            Self::lerp_scalar(a, b, t)
        }
    }

    #[inline]
    #[cfg(all(target_arch = "aarch64", not(target_family = "wasm")))]
    #[allow(
        dead_code,
        unsafe_code,
        reason = "SIMD twin of `lerp_scalar`: compiled on every aarch64 build but only called when the `simd` feature selects it in `lerp`; NEON intrinsics require unsafe"
    )]
    fn lerp_simd_neon(a: Color, b: Color, t: f32) -> Color {
        // SAFETY: gated on `target_feature = "neon"`, so the intrinsics are
        // available; `vld1q_f32`/`vst1q_f32` load/store 4 f32s from/into live
        // f32-aligned stack arrays, in bounds by construction.
        #[cfg(target_feature = "neon")]
        unsafe {
            use std::arch::aarch64::*;

            let t = t.clamp(0.0, 1.0);

            // Convert u8 channels to f32
            let a_vec = vld1q_f32([a.r as f32, a.g as f32, a.b as f32, a.a as f32].as_ptr());
            let b_vec = vld1q_f32([b.r as f32, b.g as f32, b.b as f32, b.a as f32].as_ptr());
            let t_vec = vdupq_n_f32(t);

            // lerp: a + (b - a) * t
            let diff = vsubq_f32(b_vec, a_vec);
            let scaled = vmulq_f32(diff, t_vec);
            let result = vaddq_f32(a_vec, scaled);

            // Convert back to u8
            let mut out = [0.0f32; 4];
            vst1q_f32(out.as_mut_ptr(), result);

            Color::rgba(out[0] as u8, out[1] as u8, out[2] as u8, out[3] as u8)
        }

        #[cfg(not(target_feature = "neon"))]
        {
            Self::lerp_scalar(a, b, t)
        }
    }

    #[inline]
    #[must_use]
    pub const fn to_argb(&self) -> u32 {
        ((self.a as u32) << 24) | ((self.r as u32) << 16) | ((self.g as u32) << 8) | (self.b as u32)
    }

    #[must_use]
    #[inline]
    pub fn to_hex(&self) -> String {
        // Lookup table avoids format! machinery (no padding, no Display trait
        // dispatch).
        const HEX: &[u8; 16] = b"0123456789ABCDEF";

        if self.is_opaque() {
            let mut s = String::with_capacity(7);
            s.push('#');
            for &b in &[self.r, self.g, self.b] {
                s.push(HEX[(b >> 4) as usize] as char);
                s.push(HEX[(b & 0x0F) as usize] as char);
            }
            s
        } else {
            let mut s = String::with_capacity(9);
            s.push('#');
            for &b in &[self.a, self.r, self.g, self.b] {
                s.push(HEX[(b >> 4) as usize] as char);
                s.push(HEX[(b & 0x0F) as usize] as char);
            }
            s
        }
    }

    #[inline]
    #[must_use]
    pub const fn to_rgba_f32(&self) -> (f32, f32, f32, f32) {
        (
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
            self.a as f32 / 255.0,
        )
    }

    #[inline]
    #[must_use]
    pub const fn to_rgba_f32_array(&self) -> [f32; 4] {
        [
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
            self.a as f32 / 255.0,
        ]
    }

    #[inline]
    #[must_use]
    pub fn from_rgba_f32_array(rgba: [f32; 4]) -> Self {
        Self::rgba(
            (rgba[0].clamp(0.0, 1.0) * 255.0) as u8,
            (rgba[1].clamp(0.0, 1.0) * 255.0) as u8,
            (rgba[2].clamp(0.0, 1.0) * 255.0) as u8,
            (rgba[3].clamp(0.0, 1.0) * 255.0) as u8,
        )
    }

    #[inline]
    #[must_use]
    pub fn to_f32_array(&self) -> [f32; 4] {
        [
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
            self.a as f32 / 255.0,
        ]
    }

    #[inline]
    #[must_use]
    pub const fn red_f32(&self) -> f32 {
        self.r as f32 / 255.0
    }

    #[inline]
    #[must_use]
    pub const fn green_f32(&self) -> f32 {
        self.g as f32 / 255.0
    }

    #[inline]
    #[must_use]
    pub const fn blue_f32(&self) -> f32 {
        self.b as f32 / 255.0
    }

    #[inline]
    #[must_use]
    pub const fn alpha_f32(&self) -> f32 {
        self.a as f32 / 255.0
    }

    // ===== Helper methods for rendering =====

    #[must_use]
    #[inline]
    pub fn blend_over(&self, background: Color) -> Color {
        // Fast paths
        if self.a == 255 {
            return *self;
        }
        if self.a == 0 {
            return background;
        }

        #[cfg(all(feature = "simd", target_arch = "x86_64", target_feature = "sse2"))]
        {
            self.blend_over_simd_sse(background)
        }

        #[cfg(all(feature = "simd", target_arch = "aarch64", target_feature = "neon"))]
        {
            self.blend_over_simd_neon(background)
        }

        #[cfg(not(all(
            feature = "simd",
            any(
                all(target_arch = "x86_64", target_feature = "sse2"),
                all(target_arch = "aarch64", target_feature = "neon")
            )
        )))]
        {
            self.blend_over_scalar(background)
        }
    }

    #[inline]
    #[allow(
        dead_code,
        reason = "scalar fallback for `blend_over`; unused when a SIMD path is compiled in (e.g. --features simd on x86_64), used on every other target"
    )]
    fn blend_over_scalar(&self, background: Color) -> Color {
        let alpha_src = self.a as f32 / 255.0;
        let alpha_dst = background.a as f32 / 255.0;
        let alpha_out = alpha_src + alpha_dst * (1.0 - alpha_src);

        if alpha_out == 0.0 {
            return Color::TRANSPARENT;
        }

        let r = ((self.r as f32 * alpha_src + background.r as f32 * alpha_dst * (1.0 - alpha_src))
            / alpha_out) as u8;
        let g = ((self.g as f32 * alpha_src + background.g as f32 * alpha_dst * (1.0 - alpha_src))
            / alpha_out) as u8;
        let b = ((self.b as f32 * alpha_src + background.b as f32 * alpha_dst * (1.0 - alpha_src))
            / alpha_out) as u8;
        let a = (alpha_out * 255.0) as u8;

        Color::rgba(r, g, b, a)
    }

    #[inline]
    #[cfg(all(target_arch = "x86_64", not(target_family = "wasm")))]
    #[allow(
        dead_code,
        unsafe_code,
        reason = "SIMD twin of `blend_over_scalar`: compiled on every x86_64 build but only called when the `simd` feature selects it in `blend_over`; SSE2 intrinsics require unsafe"
    )]
    fn blend_over_simd_sse(&self, background: Color) -> Color {
        // SAFETY: gated on `target_feature = "sse2"`, so the intrinsics are
        // available; `_mm_storeu_ps` is an unaligned store into a live 4-f32
        // stack array, in bounds by construction.
        #[cfg(target_feature = "sse2")]
        unsafe {
            use std::arch::x86_64::*;

            let alpha_src = self.a as f32 / 255.0;
            let alpha_dst = background.a as f32 / 255.0;
            let alpha_out = alpha_src + alpha_dst * (1.0 - alpha_src);

            if alpha_out == 0.0 {
                return Color::TRANSPARENT;
            }

            // Load colors as f32 vectors
            let src_vec = _mm_set_ps(self.a as f32, self.b as f32, self.g as f32, self.r as f32);
            let dst_vec = _mm_set_ps(
                background.a as f32,
                background.b as f32,
                background.g as f32,
                background.r as f32,
            );

            // Blend formula: (src * alpha_src + dst * alpha_dst * (1 - alpha_src)) /
            // alpha_out
            let alpha_src_vec = _mm_set1_ps(alpha_src);
            let alpha_dst_factor = _mm_set1_ps(alpha_dst * (1.0 - alpha_src));
            let alpha_out_vec = _mm_set1_ps(alpha_out);

            let src_contrib = _mm_mul_ps(src_vec, alpha_src_vec);
            let dst_contrib = _mm_mul_ps(dst_vec, alpha_dst_factor);
            let sum = _mm_add_ps(src_contrib, dst_contrib);
            let result = _mm_div_ps(sum, alpha_out_vec);

            // Convert back to u8
            let mut out = [0.0f32; 4];
            _mm_storeu_ps(out.as_mut_ptr(), result);

            Color::rgba(
                out[0] as u8,
                out[1] as u8,
                out[2] as u8,
                (alpha_out * 255.0) as u8,
            )
        }

        #[cfg(not(target_feature = "sse2"))]
        {
            self.blend_over_scalar(background)
        }
    }

    #[inline]
    #[cfg(all(target_arch = "aarch64", not(target_family = "wasm")))]
    #[allow(
        dead_code,
        unsafe_code,
        reason = "SIMD twin of `blend_over_scalar`: compiled on every aarch64 build but only called when the `simd` feature selects it in `blend_over`; NEON intrinsics require unsafe"
    )]
    fn blend_over_simd_neon(&self, background: Color) -> Color {
        // SAFETY: gated on `target_feature = "neon"`, so the intrinsics are
        // available; `vld1q_f32`/`vst1q_f32` load/store 4 f32s from/into live
        // f32-aligned stack arrays, in bounds by construction.
        #[cfg(target_feature = "neon")]
        unsafe {
            use std::arch::aarch64::*;

            let alpha_src = self.a as f32 / 255.0;
            let alpha_dst = background.a as f32 / 255.0;
            let alpha_out = alpha_src + alpha_dst * (1.0 - alpha_src);

            if alpha_out == 0.0 {
                return Color::TRANSPARENT;
            }

            // Load colors as f32 vectors
            let src_vec =
                vld1q_f32([self.r as f32, self.g as f32, self.b as f32, self.a as f32].as_ptr());
            let dst_vec = vld1q_f32(
                [
                    background.r as f32,
                    background.g as f32,
                    background.b as f32,
                    background.a as f32,
                ]
                .as_ptr(),
            );

            // Blend formula: (src * alpha_src + dst * alpha_dst * (1 - alpha_src)) /
            // alpha_out
            let alpha_src_vec = vdupq_n_f32(alpha_src);
            let alpha_dst_factor = vdupq_n_f32(alpha_dst * (1.0 - alpha_src));
            let alpha_out_vec = vdupq_n_f32(alpha_out);

            let src_contrib = vmulq_f32(src_vec, alpha_src_vec);
            let dst_contrib = vmulq_f32(dst_vec, alpha_dst_factor);
            let sum = vaddq_f32(src_contrib, dst_contrib);
            let result = vdivq_f32(sum, alpha_out_vec);

            // Convert back to u8
            let mut out = [0.0f32; 4];
            vst1q_f32(out.as_mut_ptr(), result);

            Color::rgba(
                out[0] as u8,
                out[1] as u8,
                out[2] as u8,
                (alpha_out * 255.0) as u8,
            )
        }

        #[cfg(not(target_feature = "neon"))]
        {
            self.blend_over_scalar(background)
        }
    }

    #[inline]
    #[must_use]
    pub const fn multiply(&self, other: Color) -> Color {
        Color::rgba(
            ((self.r as u16 * other.r as u16) / 255) as u8,
            ((self.g as u16 * other.g as u16) / 255) as u8,
            ((self.b as u16 * other.b as u16) / 255) as u8,
            ((self.a as u16 * other.a as u16) / 255) as u8,
        )
    }

    #[inline]
    #[must_use]
    pub fn darken(&self, factor: f32) -> Color {
        let factor = factor.clamp(0.0, 1.0);
        Color::rgba(
            (self.r as f32 * factor) as u8,
            (self.g as f32 * factor) as u8,
            (self.b as f32 * factor) as u8,
            self.a,
        )
    }

    #[inline]
    #[must_use]
    pub fn lighten(&self, factor: f32) -> Color {
        let factor = factor.clamp(0.0, 1.0);
        Color::rgba(
            (self.r as f32 + (255.0 - self.r as f32) * factor) as u8,
            (self.g as f32 + (255.0 - self.g as f32) * factor) as u8,
            (self.b as f32 + (255.0 - self.b as f32) * factor) as u8,
            self.a,
        )
    }

    #[inline]
    #[must_use]
    pub const fn luminance(&self) -> f32 {
        (0.2126 * self.r as f32 + 0.7152 * self.g as f32 + 0.0722 * self.b as f32) / 255.0
    }

    #[inline]
    #[must_use]
    pub const fn is_dark(&self) -> bool {
        self.luminance() < 0.5
    }

    #[inline]
    #[must_use]
    pub const fn is_light(&self) -> bool {
        self.luminance() >= 0.5
    }

    #[inline]
    #[must_use]
    pub const fn contrasting_text_color(&self) -> Color {
        if self.is_dark() {
            Color::WHITE
        } else {
            Color::BLACK
        }
    }

    // ===== Perceptual (Oklab) interpolation =====

    /// Convert to Oklab (perceptually uniform, Björn Ottosson 2020).
    ///
    /// Pipeline: sRGB → linear → LMS (M1) → cube root → Lab (M2). Exact
    /// matrices from <https://bottosson.github.io/posts/oklab/>. Alpha is not
    /// part of Oklab and is carried separately by the caller.
    #[must_use]
    pub fn to_oklab(self) -> Oklab {
        #[inline]
        fn srgb_to_linear(c: f32) -> f32 {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        let r = srgb_to_linear(f32::from(self.r) / 255.0);
        let g = srgb_to_linear(f32::from(self.g) / 255.0);
        let b = srgb_to_linear(f32::from(self.b) / 255.0);

        let l = 0.412_221_47 * r + 0.536_332_54 * g + 0.051_445_995 * b;
        let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
        let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;

        let l_ = l.cbrt();
        let m_ = m.cbrt();
        let s_ = s.cbrt();

        Oklab {
            l: 0.210_454_26 * l_ + 0.793_617_8 * m_ - 0.004_072_047 * s_,
            a: 1.977_998_5 * l_ - 2.428_592_2 * m_ + 0.450_593_7 * s_,
            b: 0.025_904_037 * l_ + 0.782_771_77 * m_ - 0.808_675_77 * s_,
        }
    }

    /// Convert from Oklab back to sRGB, with the given alpha channel.
    ///
    /// Out-of-gamut results are clamped per channel (sufficient for
    /// interpolation between two in-gamut endpoints; the Oklab segment
    /// between two sRGB colors leaves the gamut only marginally).
    #[must_use]
    pub fn from_oklab(lab: Oklab, alpha: u8) -> Color {
        #[inline]
        fn linear_to_srgb(c: f32) -> f32 {
            if c <= 0.003_130_8 {
                12.92 * c
            } else {
                1.055 * c.powf(1.0 / 2.4) - 0.055
            }
        }
        // `.round() as u8` saturates: clamping out-of-gamut channels.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // saturating by design
        #[inline]
        fn to_channel(c: f32) -> u8 {
            (linear_to_srgb(c).clamp(0.0, 1.0) * 255.0).round() as u8
        }

        let l_ = lab.l + 0.396_337_78 * lab.a + 0.215_803_76 * lab.b;
        let m_ = lab.l - 0.105_561_346 * lab.a - 0.063_854_17 * lab.b;
        let s_ = lab.l - 0.089_484_18 * lab.a - 1.291_485_5 * lab.b;

        let l = l_ * l_ * l_;
        let m = m_ * m_ * m_;
        let s = s_ * s_ * s_;

        let r = 4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s;
        let g = -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s;
        let b = -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s;

        Color::rgba(to_channel(r), to_channel(g), to_channel(b), alpha)
    }

    /// Perceptually uniform interpolation through Oklab space.
    ///
    /// Componentwise sRGB lerp (what [`Color::lerp`] and Flutter's
    /// `Color.lerp` compute) averages gamma-encoded values, so midpoints go
    /// dark and gray — blue→yellow passes through mud. Interpolating L/a/b
    /// linearly keeps lightness and chroma perceptually steady. Costs two
    /// conversions per call (`powf`/`cbrt`); use [`Color::lerp`] when the
    /// endpoints are close or the budget is tight.
    ///
    /// Alpha interpolates linearly, matching [`Color::lerp`].
    #[must_use]
    pub fn lerp_oklab(a: Color, b: Color, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        let la = a.to_oklab();
        let lb = b.to_oklab();
        let mixed = Oklab {
            l: la.l + (lb.l - la.l) * t,
            a: la.a + (lb.a - la.a) * t,
            b: la.b + (lb.b - la.b) * t,
        };
        // Alpha is linear, same rounding contract as `lerp_scalar`.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // saturating by design
        let alpha = (f32::from(a.a) + (f32::from(b.a) - f32::from(a.a)) * t).round() as u8;
        Color::from_oklab(mixed, alpha)
    }

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
        Color::lerp(color1, color2, local_t)
    }

    /// Blends each color over the given background.
    ///
    /// Each element's `blend_over` already uses SIMD internally when available,
    /// so no additional batch SIMD wrapper is needed.
    #[must_use]
    #[inline]
    pub fn blend_over_batch(colors: &[Color], background: Color) -> Vec<Color> {
        colors
            .iter()
            .map(|color| color.blend_over(background))
            .collect()
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

impl crate::geometry::ApproxEq for Color {
    /// Default epsilon for color comparison (1/255 ≈ 0.004).
    ///
    /// This allows for 1 unit difference in u8 color channels.
    const DEFAULT_EPSILON: f32 = 1.0 / 255.0;

    /// Compares colors in normalized f32 space with epsilon tolerance.
    ///
    /// This is useful when comparing colors that have been converted through
    /// different color spaces or undergone floating-point calculations.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{Color, geometry::ApproxEq};
    ///
    /// let c1 = Color::rgb(100, 150, 200);
    /// let c2 = Color::rgb(100, 150, 200);
    /// let c3 = Color::rgb(100, 151, 200); // 1 unit difference
    ///
    /// assert!(c1.approx_eq(&c2));
    /// assert!(c1.approx_eq(&c3)); // Within default epsilon
    /// ```
    #[inline]
    fn approx_eq_eps(&self, other: &Self, epsilon: f32) -> bool {
        let (r1, g1, b1, a1) = self.to_rgba_f32();
        let (r2, g2, b2, a2) = other.to_rgba_f32();

        (r1 - r2).abs() <= epsilon
            && (g1 - g2).abs() <= epsilon
            && (b1 - b2).abs() <= epsilon
            && (a1 - a2).abs() <= epsilon
    }
}

// ===== Error types =====

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::ApproxEq;

    #[test]
    fn test_approx_eq_identical() {
        let c1 = Color::rgb(100, 150, 200);
        let c2 = Color::rgb(100, 150, 200);
        assert!(c1.approx_eq(&c2));
    }

    #[test]
    fn test_approx_eq_one_unit_difference() {
        let c1 = Color::rgb(100, 150, 200);
        let c2 = Color::rgb(100, 151, 200);
        let c3 = Color::rgb(101, 150, 200);
        let c4 = Color::rgb(100, 150, 201);

        // 1 unit difference should be within default epsilon
        assert!(c1.approx_eq(&c2));
        assert!(c1.approx_eq(&c3));
        assert!(c1.approx_eq(&c4));
    }

    #[test]
    fn oklab_roundtrip_preserves_color() {
        // sRGB -> Oklab -> sRGB must come back within 1 channel unit
        // (cbrt/powf rounding) for representative colors.
        for color in [
            Color::rgb(0, 0, 0),
            Color::rgb(255, 255, 255),
            Color::rgb(255, 0, 0),
            Color::rgb(0, 255, 0),
            Color::rgb(0, 0, 255),
            Color::rgb(128, 64, 200),
            Color::rgb(13, 250, 99),
        ] {
            let back = Color::from_oklab(color.to_oklab(), color.a);
            assert!(
                (i16::from(back.r) - i16::from(color.r)).abs() <= 1
                    && (i16::from(back.g) - i16::from(color.g)).abs() <= 1
                    && (i16::from(back.b) - i16::from(color.b)).abs() <= 1,
                "roundtrip {color:?} -> {back:?} drifted more than 1 unit"
            );
        }
    }

    #[test]
    fn oklab_white_has_unit_lightness() {
        // Ottosson reference values: white = (L=1, a≈0, b≈0), black = (0,0,0).
        let white = Color::rgb(255, 255, 255).to_oklab();
        assert!((white.l - 1.0).abs() < 1e-2, "white L = {}", white.l);
        assert!(white.a.abs() < 1e-2 && white.b.abs() < 1e-2);

        let black = Color::rgb(0, 0, 0).to_oklab();
        assert!(black.l.abs() < 1e-3);
    }

    #[test]
    fn oklab_lerp_endpoints_and_midpoint() {
        let blue = Color::rgb(0, 0, 255);
        let yellow = Color::rgb(255, 255, 0);

        // Endpoints round-trip through the conversion.
        let at0 = Color::lerp_oklab(blue, yellow, 0.0);
        let at1 = Color::lerp_oklab(blue, yellow, 1.0);
        assert!((i16::from(at0.b) - 255).abs() <= 1 && i16::from(at0.r) <= 1);
        assert!((i16::from(at1.r) - 255).abs() <= 1 && i16::from(at1.b) <= 1);

        // The perceptual midpoint must be brighter than the muddy sRGB
        // midpoint (128,128,128): Oklab preserves perceived lightness.
        let mid = Color::lerp_oklab(blue, yellow, 0.5);
        let srgb_mid = Color::lerp(blue, yellow, 0.5);
        let sum = u16::from(mid.r) + u16::from(mid.g) + u16::from(mid.b);
        let srgb_sum = u16::from(srgb_mid.r) + u16::from(srgb_mid.g) + u16::from(srgb_mid.b);
        assert!(
            sum > srgb_sum,
            "Oklab midpoint {mid:?} must be brighter than sRGB midpoint {srgb_mid:?}"
        );
    }

    #[test]
    fn oklab_lerp_interpolates_alpha_linearly() {
        let a = Color::rgba(255, 0, 0, 0);
        let b = Color::rgba(255, 0, 0, 200);
        assert_eq!(Color::lerp_oklab(a, b, 0.5).a, 100);
    }

    #[test]
    fn test_approx_eq_alpha_channel() {
        let c1 = Color::rgba(100, 150, 200, 255);
        let c2 = Color::rgba(100, 150, 200, 254);

        // 1 unit alpha difference should be within epsilon
        assert!(c1.approx_eq(&c2));
    }

    #[test]
    fn test_approx_eq_large_difference() {
        let c1 = Color::rgb(100, 150, 200);
        let c2 = Color::rgb(105, 150, 200);

        // 5 unit difference should exceed default epsilon
        assert!(!c1.approx_eq(&c2));
    }

    #[test]
    fn test_approx_eq_eps_custom_epsilon() {
        let c1 = Color::rgb(100, 150, 200);
        let c2 = Color::rgb(110, 150, 200);

        // 10 units = 10/255 ≈ 0.039
        assert!(!c1.approx_eq(&c2));

        // But should pass with larger epsilon
        assert!(c1.approx_eq_eps(&c2, 0.05));
    }

    #[test]
    #[ignore = "TODO: Implement to_hsl and from_hsl methods"]
    fn test_approx_eq_hsl_conversion_roundtrip() {
        let _ = Color::rgb(120, 180, 200);
        // let hsl = original.to_hsl();
        // let roundtrip = Color::from_hsl(hsl.0, hsl.1, hsl.2);

        // HSL conversion may introduce small rounding errors
        // assert!(original.approx_eq(&roundtrip));
    }

    #[test]
    #[ignore = "TODO: Implement to_hsv and from_hsv methods"]
    fn test_approx_eq_hsv_conversion_roundtrip() {
        let _ = Color::rgb(80, 120, 160);
        // let hsv = original.to_hsv();
        // let roundtrip = Color::from_hsv(hsv.0, hsv.1, hsv.2);

        // HSV conversion may introduce small rounding errors
        // assert!(original.approx_eq(&roundtrip));
    }

    #[test]
    fn test_approx_eq_lerp_precision() {
        let c1 = Color::rgb(0, 0, 0);
        let c2 = Color::rgb(100, 100, 100);

        // Lerp at 0.5 should give (50, 50, 50)
        let mid = Color::lerp(c1, c2, 0.5);
        let expected = Color::rgb(50, 50, 50);

        assert!(mid.approx_eq(&expected));
    }

    #[test]
    fn test_approx_eq_blend_precision() {
        let foreground = Color::rgba(255, 0, 0, 128); // 50% transparent red
        let background = Color::rgb(0, 0, 255); // opaque blue

        let blended = foreground.blend_over(background);

        // Expected: roughly purple (127, 0, 127)
        let expected = Color::rgb(127, 0, 127);

        // Blending calculations may have rounding errors
        assert!(blended.approx_eq_eps(&expected, 0.01));
    }

    #[test]
    fn test_approx_eq_epsilon_boundary() {
        let c1 = Color::rgb(100, 100, 100);

        // Test at exactly 1/255 difference
        let c2 = Color::from_rgba_f32_array([
            100.0 / 255.0 + 1.0 / 255.0,
            100.0 / 255.0,
            100.0 / 255.0,
            1.0,
        ]);

        // Should be within epsilon
        assert!(c1.approx_eq(&c2));
    }

    #[test]
    fn test_default_epsilon_value() {
        // Verify default epsilon is 1/255
        assert!((Color::DEFAULT_EPSILON - 1.0 / 255.0).abs() < 1e-10);
    }
}
