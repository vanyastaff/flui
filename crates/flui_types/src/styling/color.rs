//! Color types and utilities for Flui.
//!
//! This module provides a comprehensive Color type with conversions between
//! different color spaces (RGB, HSL, HSV), similar to Flutter's Color system.

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
    /// # Errors
    ///
    /// Returns [`ParseColorError::InvalidLength`] if the string is not 6 or 8 characters
    /// (excluding the optional `#` prefix).
    ///
    /// Returns [`ParseColorError::InvalidHex`] if the string contains non-hexadecimal characters.
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

    #[must_use]
    pub const fn to_argb(&self) -> u32 {
        ((self.a as u32) << 24) | ((self.r as u32) << 16) | ((self.g as u32) << 8) | (self.b as u32)
    }

    #[must_use]
    pub fn to_hex(&self) -> String {
        if self.is_opaque() {
            format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
        } else {
            format!("#{:02X}{:02X}{:02X}{:02X}", self.a, self.r, self.g, self.b)
        }
    }

    #[must_use]
    pub const fn to_rgba_f32(&self) -> (f32, f32, f32, f32) {
        (
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
            self.a as f32 / 255.0,
        )
    }

    #[must_use]
    pub const fn to_rgba_f32_array(&self) -> [f32; 4] {
        [
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
            self.a as f32 / 255.0,
        ]
    }

    #[must_use]
    pub fn from_rgba_f32_array(rgba: [f32; 4]) -> Self {
        Self::rgba(
            (rgba[0].clamp(0.0, 1.0) * 255.0) as u8,
            (rgba[1].clamp(0.0, 1.0) * 255.0) as u8,
            (rgba[2].clamp(0.0, 1.0) * 255.0) as u8,
            (rgba[3].clamp(0.0, 1.0) * 255.0) as u8,
        )
    }

    #[must_use]
    pub fn to_f32_array(&self) -> [f32; 4] {
        [
            self.r as f32 / 255.0,
            self.g as f32 / 255.0,
            self.b as f32 / 255.0,
            self.a as f32 / 255.0,
        ]
    }

    #[must_use]
    pub const fn red_f32(&self) -> f32 {
        self.r as f32 / 255.0
    }

    #[must_use]
    pub const fn green_f32(&self) -> f32 {
        self.g as f32 / 255.0
    }

    #[must_use]
    pub const fn blue_f32(&self) -> f32 {
        self.b as f32 / 255.0
    }

    #[must_use]
    pub const fn alpha_f32(&self) -> f32 {
        self.a as f32 / 255.0
    }

    // ===== Helper methods for rendering =====

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

    #[must_use]
    pub const fn multiply(&self, other: Color) -> Color {
        Color::rgba(
            ((self.r as u16 * other.r as u16) / 255) as u8,
            ((self.g as u16 * other.g as u16) / 255) as u8,
            ((self.b as u16 * other.b as u16) / 255) as u8,
            ((self.a as u16 * other.a as u16) / 255) as u8,
        )
    }

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

    #[must_use]
    pub const fn luminance(&self) -> f32 {
        (0.2126 * self.r as f32 + 0.7152 * self.g as f32 + 0.0722 * self.b as f32) / 255.0
    }

    #[must_use]
    pub const fn is_dark(&self) -> bool {
        self.luminance() < 0.5
    }

    #[must_use]
    pub const fn is_light(&self) -> bool {
        self.luminance() >= 0.5
    }

    #[must_use]
    pub const fn contrasting_text_color(&self) -> Color {
        if self.is_dark() {
            Color::WHITE
        } else {
            Color::BLACK
        }
    }

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
    /// use flui_types::Color;
    /// use flui_types::geometry::ApproxEq;
    ///
    /// let c1 = Color::rgb(100, 150, 200);
    /// let c2 = Color::rgb(100, 150, 200);
    /// let c3 = Color::rgb(100, 151, 200);  // 1 unit difference
    ///
    /// assert!(c1.approx_eq(&c2));
    /// assert!(c1.approx_eq(&c3));  // Within default epsilon
    /// ```
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
    fn test_approx_eq_hsl_conversion_roundtrip() {
        let original = Color::rgb(120, 180, 200);
        let hsl = original.to_hsl();
        let roundtrip = Color::from_hsl(hsl.0, hsl.1, hsl.2);

        // HSL conversion may introduce small rounding errors
        assert!(original.approx_eq(&roundtrip));
    }

    #[test]
    fn test_approx_eq_hsv_conversion_roundtrip() {
        let original = Color::rgb(80, 120, 160);
        let hsv = original.to_hsv();
        let roundtrip = Color::from_hsv(hsv.0, hsv.1, hsv.2);

        // HSV conversion may introduce small rounding errors
        assert!(original.approx_eq(&roundtrip));
    }

    #[test]
    fn test_approx_eq_lerp_precision() {
        let c1 = Color::rgb(0, 0, 0);
        let c2 = Color::rgb(100, 100, 100);

        // Lerp at 0.5 should give (50, 50, 50)
        let mid = c1.lerp(c2, 0.5);
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
