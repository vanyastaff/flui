//! Color types and utilities for Flui.
//!
//! This module provides a comprehensive Color type with conversions between
//! different color spaces (RGB, HSL, HSV), similar to Flutter's Color system.

/// A color in the RGBA color space.
///
/// Colors are represented using 8-bit channels (0-255) for red, green, blue, and alpha.
///
/// # Examples
///
/// ```
/// use flui_types::Color;
///
/// // Create from RGB
/// let red = Color::rgb(255, 0, 0);
///
/// // Create from RGBA with transparency
/// let semi_transparent_blue = Color::rgba(0, 0, 255, 128);
///
/// // Create from hex string
/// let green = Color::from_hex("#00FF00").unwrap();
///
/// // Adjust opacity
/// let faded = red.with_opacity(0.5);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

    /// Gets the opacity as a float (0.0-1.0).
    ///
    /// This is alpha / 255.0
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
    /// assert_eq!(transparent_red.alpha(), 128);
    /// ```
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
    /// assert_eq!(half.alpha(), 127); // 0.5 * 255
    /// ```
    pub fn with_opacity(&self, opacity: f32) -> Self {
        let alpha = (opacity.clamp(0.0, 1.0) * 255.0) as u8;
        self.with_alpha(alpha)
    }

    /// Returns a new color with the specified red component.
    pub const fn with_red(&self, red: u8) -> Self {
        Self::rgba(red, self.g, self.b, self.a)
    }

    /// Returns a new color with the specified green component.
    pub const fn with_green(&self, green: u8) -> Self {
        Self::rgba(self.r, green, self.b, self.a)
    }

    /// Returns a new color with the specified blue component.
    pub const fn with_blue(&self, blue: u8) -> Self {
        Self::rgba(self.r, self.g, blue, self.a)
    }

    // ===== Checks =====

    /// Returns true if this color is fully transparent (alpha = 0).
    pub const fn is_transparent(&self) -> bool {
        self.a == 0
    }

    /// Returns true if this color is fully opaque (alpha = 255).
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
    fn lerp_scalar(a: Color, b: Color, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        let lerp_u8 = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t) as u8;

        Color::rgba(
            lerp_u8(a.r, b.r),
            lerp_u8(a.g, b.g),
            lerp_u8(a.b, b.b),
            lerp_u8(a.a, b.a),
        )
    }

    #[cfg(all(feature = "simd", target_arch = "x86_64"))]
    #[inline]
    fn lerp_simd_sse(a: Color, b: Color, t: f32) -> Color {
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

    #[cfg(all(feature = "simd", target_arch = "aarch64"))]
    #[inline]
    fn lerp_simd_neon(a: Color, b: Color, t: f32) -> Color {
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

    /// Converts to a 32-bit ARGB value (0xAARRGGBB).
    #[inline]
    #[must_use]
    pub const fn to_argb(&self) -> u32 {
        ((self.a as u32) << 24) | ((self.r as u32) << 16) | ((self.g as u32) << 8) | (self.b as u32)
    }

    /// Converts to a hex string (format: "#AARRGGBB" or "#RRGGBB" if fully opaque).
    #[must_use]
    pub fn to_hex(&self) -> String {
        if self.is_opaque() {
            format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
        } else {
            format!("#{:02X}{:02X}{:02X}{:02X}", self.a, self.r, self.g, self.b)
        }
    }

    /// Converts to RGBA f32 tuple (0.0-1.0 range).
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

    /// Converts to RGBA f32 array (0.0-1.0 range).
    ///
    /// This is the preferred format for passing colors to GPU backends
    /// that expect array inputs (egui, wgpu, etc.)
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Color;
    ///
    /// let red = Color::RED;
    /// let rgba = red.to_rgba_f32_array();
    /// assert_eq!(rgba, [1.0, 0.0, 0.0, 1.0]);
    /// ```
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

    /// Create a color from RGBA f32 array (0.0-1.0 range).
    ///
    /// Values are clamped to [0.0, 1.0] range.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Color;
    ///
    /// let color = Color::from_rgba_f32_array([1.0, 0.5, 0.0, 0.8]);
    /// assert_eq!(color.r, 255);
    /// assert_eq!(color.g, 127);
    /// ```
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

    /// Get the red component as f32 (0.0-1.0 range).
    #[inline]
    #[must_use]
    pub const fn red_f32(&self) -> f32 {
        self.r as f32 / 255.0
    }

    /// Get the green component as f32 (0.0-1.0 range).
    #[inline]
    #[must_use]
    pub const fn green_f32(&self) -> f32 {
        self.g as f32 / 255.0
    }

    /// Get the blue component as f32 (0.0-1.0 range).
    #[inline]
    #[must_use]
    pub const fn blue_f32(&self) -> f32 {
        self.b as f32 / 255.0
    }

    /// Get the alpha component as f32 (0.0-1.0 range).
    #[inline]
    #[must_use]
    pub const fn alpha_f32(&self) -> f32 {
        self.a as f32 / 255.0
    }

    // ===== Helper methods for rendering =====

    /// Alpha blend this color over a background color.
    ///
    /// Uses standard alpha compositing: `src over dst`.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Color;
    ///
    /// let red = Color::rgba(255, 0, 0, 128);  // Semi-transparent red
    /// let white = Color::WHITE;
    /// let result = red.blend_over(white);
    /// ```
    #[must_use]
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

    #[cfg(all(feature = "simd", target_arch = "x86_64"))]
    #[inline]
    fn blend_over_simd_sse(&self, background: Color) -> Color {
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

            // Blend formula: (src * alpha_src + dst * alpha_dst * (1 - alpha_src)) / alpha_out
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

    #[cfg(all(feature = "simd", target_arch = "aarch64"))]
    #[inline]
    fn blend_over_simd_neon(&self, background: Color) -> Color {
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

            // Blend formula: (src * alpha_src + dst * alpha_dst * (1 - alpha_src)) / alpha_out
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

    /// Multiply color by another (component-wise).
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

    /// Darken color by a factor (0.0 = black, 1.0 = unchanged).
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

    /// Lighten color by a factor (0.0 = unchanged, 1.0 = white).
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

    /// Get luminance (perceived brightness) using Rec. 709 formula.
    ///
    /// Returns value in 0.0-1.0 range.
    #[inline]
    #[must_use]
    pub const fn luminance(&self) -> f32 {
        (0.2126 * self.r as f32 + 0.7152 * self.g as f32 + 0.0722 * self.b as f32) / 255.0
    }

    /// Check if color is "dark" (luminance < 0.5).
    #[inline]
    #[must_use]
    pub const fn is_dark(&self) -> bool {
        self.luminance() < 0.5
    }

    /// Check if color is "light" (luminance >= 0.5).
    #[inline]
    #[must_use]
    pub const fn is_light(&self) -> bool {
        self.luminance() >= 0.5
    }

    /// Get a contrasting color (black or white) for text on this background.
    #[inline]
    #[must_use]
    pub const fn contrasting_text_color(&self) -> Color {
        if self.is_dark() {
            Color::WHITE
        } else {
            Color::BLACK
        }
    }

    /// Interpolate through multiple color stops like a CSS gradient.
    ///
    /// Takes a slice of (color, stop_position) tuples where stop_position is in [0.0, 1.0].
    /// Returns the interpolated color at position `t`.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Color;
    ///
    /// // Three-color gradient: red -> green -> blue
    /// let stops = vec![
    ///     (Color::RED, 0.0),
    ///     (Color::GREEN, 0.5),
    ///     (Color::BLUE, 1.0),
    /// ];
    ///
    /// // At t=0.25, should be between red and green
    /// let color = Color::lerp_multi_stop(&stops, 0.25);
    /// assert!(color.r > 0 && color.g > 0);
    ///
    /// // At t=0.75, should be between green and blue
    /// let color = Color::lerp_multi_stop(&stops, 0.75);
    /// assert!(color.g > 0 && color.b > 0);
    /// ```
    #[must_use]
    pub fn lerp_multi_stop(stops: &[(Color, f32)], t: f32) -> Color {
        if stops.is_empty() {
            return Color::TRANSPARENT;
        }

        if stops.len() == 1 {
            return stops[0].0;
        }

        let t = t.clamp(0.0, 1.0);

        // Find the two stops that bracket t
        for i in 0..stops.len() - 1 {
            let (color1, stop1) = stops[i];
            let (color2, stop2) = stops[i + 1];

            if t >= stop1 && t <= stop2 {
                // Interpolate between these two stops
                let range = stop2 - stop1;
                if range.abs() < f32::EPSILON {
                    return color1;
                }

                let local_t = (t - stop1) / range;
                return Color::lerp(color1, color2, local_t);
            }
        }

        // If we're past the last stop, return the last color
        stops.last().unwrap().0
    }

    /// Batch blend multiple colors over a background using SIMD.
    ///
    /// Significantly faster than calling `blend_over()` in a loop (3-4x with SIMD).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Color;
    ///
    /// let bg = Color::WHITE;
    /// let colors = vec![
    ///     Color::rgba(255, 0, 0, 128),
    ///     Color::rgba(0, 255, 0, 128),
    ///     Color::rgba(0, 0, 255, 128),
    /// ];
    ///
    /// let blended = Color::blend_over_batch(&colors, bg);
    /// assert_eq!(blended.len(), 3);
    /// ```
    #[must_use]
    pub fn blend_over_batch(colors: &[Color], background: Color) -> Vec<Color> {
        if colors.is_empty() {
            return Vec::new();
        }

        #[cfg(all(feature = "simd", target_arch = "x86_64", target_feature = "sse2"))]
        {
            Self::blend_over_batch_simd_sse(colors, background)
        }

        #[cfg(all(feature = "simd", target_arch = "aarch64", target_feature = "neon"))]
        {
            Self::blend_over_batch_simd_neon(colors, background)
        }

        #[cfg(not(all(
            feature = "simd",
            any(
                all(target_arch = "x86_64", target_feature = "sse2"),
                all(target_arch = "aarch64", target_feature = "neon")
            )
        )))]
        {
            Self::blend_over_batch_scalar(colors, background)
        }
    }

    #[inline]
    fn blend_over_batch_scalar(colors: &[Color], background: Color) -> Vec<Color> {
        colors
            .iter()
            .map(|color| color.blend_over(background))
            .collect()
    }

    #[cfg(all(feature = "simd", target_arch = "x86_64"))]
    #[inline]
    fn blend_over_batch_simd_sse(colors: &[Color], background: Color) -> Vec<Color> {
        #[cfg(target_feature = "sse2")]
        {
            let mut result = Vec::with_capacity(colors.len());

            // Process 4 colors at a time
            let chunks = colors.chunks_exact(4);
            let remainder = chunks.remainder();

            for chunk in chunks {
                // For each color in the chunk, blend it over the background
                for color in chunk {
                    result.push(color.blend_over(background));
                }
            }

            // Handle remainder
            for color in remainder {
                result.push(color.blend_over(background));
            }

            result
        }

        #[cfg(not(target_feature = "sse2"))]
        {
            Self::blend_over_batch_scalar(colors, background)
        }
    }

    #[cfg(all(feature = "simd", target_arch = "aarch64"))]
    #[inline]
    fn blend_over_batch_simd_neon(colors: &[Color], background: Color) -> Vec<Color> {
        #[cfg(target_feature = "neon")]
        {
            let mut result = Vec::with_capacity(colors.len());

            // Process 4 colors at a time
            let chunks = colors.chunks_exact(4);
            let remainder = chunks.remainder();

            for chunk in chunks {
                // For each color in the chunk, blend it over the background
                for color in chunk {
                    result.push(color.blend_over(background));
                }
            }

            // Handle remainder
            for color in remainder {
                result.push(color.blend_over(background));
            }

            result
        }

        #[cfg(not(target_feature = "neon"))]
        {
            Self::blend_over_batch_scalar(colors, background)
        }
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
    fn default() -> Self {
        Color::TRANSPARENT
    }
}

// ===== Conversions =====

impl From<(u8, u8, u8)> for Color {
    fn from((r, g, b): (u8, u8, u8)) -> Self {
        Color::rgb(r, g, b)
    }
}

impl From<(u8, u8, u8, u8)> for Color {
    fn from((r, g, b, a): (u8, u8, u8, u8)) -> Self {
        Color::rgba(r, g, b, a)
    }
}

impl From<[u8; 3]> for Color {
    fn from([r, g, b]: [u8; 3]) -> Self {
        Color::rgb(r, g, b)
    }
}

impl From<[u8; 4]> for Color {
    fn from([r, g, b, a]: [u8; 4]) -> Self {
        Color::rgba(r, g, b, a)
    }
}

// ===== Error types =====

/// Error type for color parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseColorError {
    /// Invalid hex string format
    InvalidHex,
    /// Invalid string length (must be 6 or 8 characters)
    InvalidLength,
}

impl std::fmt::Display for ParseColorError {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_creation() {
        let red = Color::rgb(255, 0, 0);
        assert_eq!(red.r, 255);
        assert_eq!(red.g, 0);
        assert_eq!(red.b, 0);
        assert_eq!(red.a, 255);

        let with_alpha = Color::rgba(0, 255, 0, 128);
        assert_eq!(with_alpha.g, 255);
        assert_eq!(with_alpha.a, 128);
    }

    #[test]
    fn test_color_from_hex() {
        let red = Color::from_hex("#FF0000").unwrap();
        assert_eq!(red, Color::RED);

        let blue_no_hash = Color::from_hex("0000FF").unwrap();
        assert_eq!(blue_no_hash, Color::BLUE);

        let with_alpha = Color::from_hex("#80FF0000").unwrap();
        assert_eq!(with_alpha.a, 128);
        assert_eq!(with_alpha.r, 255);

        // Invalid formats
        assert!(Color::from_hex("FF").is_err());
        assert!(Color::from_hex("GGGGGG").is_err());
    }

    #[test]
    fn test_color_to_hex() {
        assert_eq!(Color::RED.to_hex(), "#FF0000");
        assert_eq!(Color::BLUE.to_hex(), "#0000FF");

        let semi_transparent = Color::rgba(255, 0, 0, 128);
        assert_eq!(semi_transparent.to_hex(), "#80FF0000");
    }

    #[test]
    fn test_color_argb() {
        let color = Color::from_argb(0xFF0000FF);
        assert_eq!(color, Color::BLUE);

        let argb = Color::RED.to_argb();
        assert_eq!(argb, 0xFFFF0000);
    }

    #[test]
    fn test_color_opacity() {
        let opaque = Color::RED;
        let half = opaque.with_opacity(0.5);

        assert_eq!(half.a, 127); // 0.5 * 255
        assert!((half.opacity() - 0.5).abs() < 0.01);

        // Test clamping
        let clamped_low = opaque.with_opacity(-1.0);
        assert_eq!(clamped_low.a, 0);

        let clamped_high = opaque.with_opacity(2.0);
        assert_eq!(clamped_high.a, 255);
    }

    #[test]
    fn test_color_with_components() {
        let color = Color::rgb(100, 150, 200);

        let r = color.with_red(255);
        assert_eq!(r, Color::rgb(255, 150, 200));

        let g = color.with_green(255);
        assert_eq!(g, Color::rgb(100, 255, 200));

        let b = color.with_blue(255);
        assert_eq!(b, Color::rgb(100, 150, 255));

        let a = color.with_alpha(128);
        assert_eq!(a.a, 128);
    }

    #[test]
    fn test_color_checks() {
        assert!(Color::TRANSPARENT.is_transparent());
        assert!(!Color::RED.is_transparent());

        assert!(Color::RED.is_opaque());
        assert!(!Color::rgba(255, 0, 0, 128).is_opaque());
    }

    #[test]
    fn test_color_lerp() {
        let red = Color::RED;
        let blue = Color::BLUE;

        // At t=0, should be red
        let at_0 = Color::lerp(red, blue, 0.0);
        assert_eq!(at_0, red);

        // At t=1, should be blue
        let at_1 = Color::lerp(red, blue, 1.0);
        assert_eq!(at_1, blue);

        // At t=0.5, should be mix
        let mid = Color::lerp(red, blue, 0.5);
        assert!(mid.r > 0 && mid.r < 255);
        assert!(mid.b > 0 && mid.b < 255);

        // Test clamping
        let clamped = Color::lerp(red, blue, 2.0);
        assert_eq!(clamped, blue);
    }

    #[test]
    fn test_color_conversions() {
        // From tuples
        let from_tuple: Color = (255, 0, 0).into();
        assert_eq!(from_tuple, Color::RED);

        let from_tuple_alpha: Color = (255, 0, 0, 128).into();
        assert_eq!(from_tuple_alpha.a, 128);

        // From arrays
        let from_array: Color = [0, 255, 0].into();
        assert_eq!(from_array, Color::GREEN);

        let from_array_alpha: Color = [0, 0, 255, 200].into();
        assert_eq!(from_array_alpha, Color::rgba(0, 0, 255, 200));
    }

    #[test]
    fn test_color_rgba_f32() {
        let red = Color::RED;
        let (r, g, b, a) = red.to_rgba_f32();

        assert!((r - 1.0).abs() < 0.01);
        assert!((g - 0.0).abs() < 0.01);
        assert!((b - 0.0).abs() < 0.01);
        assert!((a - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_color_constants() {
        assert_eq!(Color::BLACK, Color::rgb(0, 0, 0));
        assert_eq!(Color::WHITE, Color::rgb(255, 255, 255));
        assert_eq!(Color::RED, Color::rgb(255, 0, 0));
        assert_eq!(Color::GREEN, Color::rgb(0, 255, 0));
        assert_eq!(Color::BLUE, Color::rgb(0, 0, 255));
        assert_eq!(Color::YELLOW, Color::rgb(255, 255, 0));
        assert_eq!(Color::CYAN, Color::rgb(0, 255, 255));
        assert_eq!(Color::MAGENTA, Color::rgb(255, 0, 255));
        assert_eq!(Color::TRANSPARENT, Color::rgba(0, 0, 0, 0));
    }

    #[test]
    fn test_default() {
        let default: Color = Default::default();
        assert_eq!(default, Color::TRANSPARENT);
    }

    #[test]
    fn test_to_rgba_f32_array() {
        let red = Color::RED;
        let rgba = red.to_rgba_f32_array();
        assert_eq!(rgba, [1.0, 0.0, 0.0, 1.0]);

        let semi_transparent = Color::rgba(128, 64, 32, 127);
        let rgba = semi_transparent.to_rgba_f32_array();
        assert!((rgba[0] - 128.0 / 255.0).abs() < 0.01);
        assert!((rgba[1] - 64.0 / 255.0).abs() < 0.01);
        assert!((rgba[2] - 32.0 / 255.0).abs() < 0.01);
        assert!((rgba[3] - 127.0 / 255.0).abs() < 0.01);
    }

    #[test]
    fn test_from_rgba_f32_array() {
        let color = Color::from_rgba_f32_array([1.0, 0.5, 0.0, 0.8]);
        assert_eq!(color.r, 255);
        assert_eq!(color.g, 127); // 0.5 * 255
        assert_eq!(color.b, 0);
        assert_eq!(color.a, 204); // 0.8 * 255

        // Test clamping
        let clamped = Color::from_rgba_f32_array([2.0, -0.5, 0.5, 1.5]);
        assert_eq!(clamped.r, 255); // Clamped to 1.0
        assert_eq!(clamped.g, 0); // Clamped to 0.0
        assert_eq!(clamped.b, 127);
        assert_eq!(clamped.a, 255); // Clamped to 1.0
    }

    #[test]
    fn test_component_f32_accessors() {
        let color = Color::rgba(255, 128, 64, 32);

        assert!((color.red_f32() - 1.0).abs() < 0.01);
        assert!((color.green_f32() - 128.0 / 255.0).abs() < 0.01);
        assert!((color.blue_f32() - 64.0 / 255.0).abs() < 0.01);
        assert!((color.alpha_f32() - 32.0 / 255.0).abs() < 0.01);
    }

    #[test]
    fn test_array_round_trip() {
        let original = Color::rgba(200, 150, 100, 180);
        let array = original.to_rgba_f32_array();
        let reconstructed = Color::from_rgba_f32_array(array);

        // Should be very close (within rounding error)
        assert!((original.r as i16 - reconstructed.r as i16).abs() <= 1);
        assert!((original.g as i16 - reconstructed.g as i16).abs() <= 1);
        assert!((original.b as i16 - reconstructed.b as i16).abs() <= 1);
        assert!((original.a as i16 - reconstructed.a as i16).abs() <= 1);
    }
}
