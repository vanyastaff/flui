//! Packed 32-bit color representation with premultiplied alpha.
//!
//! This module provides [`Color32`], a space-efficient color type that stores
//! colors as a packed 32-bit value with premultiplied alpha in sRGB gamma space.
//!
//! ## Premultiplied Alpha
//!
//! Unlike [`Color`](super::Color) which uses separate alpha, `Color32` uses
//! premultiplied alpha where RGB values are already multiplied by alpha.
//!
//! Benefits of premultiplied alpha:
//! - Allows encoding additive colors (alpha=0 with non-zero RGB)
//! - Better for texture filtering and GPU operations
//! - Faster blending operations
//! - Standard format for most GPU APIs
//!
//! ## Gamma Space Operations
//!
//! All operations on `Color32` are performed in gamma space (sRGB), not linear space.
//! This is:
//! - Faster than linear space operations
//! - Perceptually more even for UI colors
//! - Standard for web and most UI frameworks
//!
//! For physically correct color operations, convert to [`Color`](super::Color) first.

use super::Color;

/// A 32-bit packed color with premultiplied alpha in sRGB gamma space.
///
/// This format is optimized for:
/// - Memory efficiency (4 bytes vs separate floats)
/// - GPU compatibility (standard RGBA8 format)
/// - Fast gamma-space blending
/// - Cache-friendly operations
///
/// The internal format is `[r, g, b, a]` where all components are in 0-255 range,
/// and RGB values are premultiplied by alpha.
///
/// # Examples
///
/// ```
/// use flui_types::Color32;
///
/// // Create from RGB (opaque)
/// let red = Color32::from_rgb(255, 0, 0);
///
/// // Create with alpha (automatically premultiplies)
/// let semi_red = Color32::from_rgba_unmultiplied(255, 0, 0, 128);
///
/// // Create additive color (for lighting effects)
/// let glow = Color32::from_rgb_additive(255, 255, 255);
///
/// // Blend colors
/// let result = red.blend_over(semi_red);
/// ```
#[repr(C)]
#[repr(align(4))]
#[derive(Clone, Copy, Default, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Color32([u8; 4]);

impl std::fmt::Debug for Color32 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [r, g, b, a] = self.0;
        write!(f, "#{r:02X}{g:02X}{b:02X}{a:02X}")
    }
}

impl std::ops::Index<usize> for Color32 {
    type Output = u8;

    #[inline]
    fn index(&self, index: usize) -> &u8 {
        &self.0[index]
    }
}

impl std::ops::IndexMut<usize> for Color32 {
    #[inline]
    fn index_mut(&mut self, index: usize) -> &mut u8 {
        &mut self.0[index]
    }
}

impl Color32 {
    // ===== Constructors =====

    /// Creates an opaque color from RGB values (0-255).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Color32;
    ///
    /// let red = Color32::from_rgb(255, 0, 0);
    /// assert!(red.is_opaque());
    /// ```
    #[inline]
    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self([r, g, b, 255])
    }

    /// Creates an additive color from RGB values.
    ///
    /// Additive colors have alpha=0 but non-zero RGB, making them add
    /// to any color they're blended with. Useful for lighting effects.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Color32;
    ///
    /// let glow = Color32::from_rgb_additive(100, 100, 255);
    /// assert!(glow.is_additive());
    /// ```
    #[inline]
    pub const fn from_rgb_additive(r: u8, g: u8, b: u8) -> Self {
        Self([r, g, b, 0])
    }

    /// Creates a color from premultiplied RGBA values.
    ///
    /// You likely want [`Self::from_rgba_unmultiplied`] instead, unless
    /// you're working with data that's already premultiplied.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Color32;
    ///
    /// // Half-transparent red: RGB already multiplied by 0.5
    /// let premul = Color32::from_rgba_premultiplied(127, 0, 0, 128);
    /// ```
    #[inline]
    pub const fn from_rgba_premultiplied(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self([r, g, b, a])
    }

    /// Creates a color from "normal" RGBA values with separate alpha.
    ///
    /// This is the standard RGBA format you'd find in color pickers.
    /// RGB values will be automatically premultiplied by alpha.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Color32;
    ///
    /// // Half-transparent red
    /// let color = Color32::from_rgba_unmultiplied(255, 0, 0, 128);
    /// ```
    #[inline]
    pub fn from_rgba_unmultiplied(r: u8, g: u8, b: u8, a: u8) -> Self {
        match a {
            0 => Self::TRANSPARENT,
            255 => Self::from_rgb(r, g, b),
            a => {
                let factor = a as f32 / 255.0;
                Self::from_rgba_premultiplied(
                    (r as f32 * factor) as u8,
                    (g as f32 * factor) as u8,
                    (b as f32 * factor) as u8,
                    a,
                )
            }
        }
    }

    /// Creates an opaque gray color.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Color32;
    ///
    /// let gray = Color32::from_gray(128);
    /// assert_eq!(gray.r(), 128);
    /// assert_eq!(gray.g(), 128);
    /// assert_eq!(gray.b(), 128);
    /// ```
    #[inline]
    pub const fn from_gray(l: u8) -> Self {
        Self([l, l, l, 255])
    }

    /// Creates black with the given opacity.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Color32;
    ///
    /// let shadow = Color32::from_black_alpha(128);
    /// ```
    #[inline]
    pub const fn from_black_alpha(a: u8) -> Self {
        Self([0, 0, 0, a])
    }

    /// Creates white with the given opacity.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Color32;
    ///
    /// let highlight = Color32::from_white_alpha(64);
    /// ```
    #[inline]
    pub fn from_white_alpha(a: u8) -> Self {
        // Premultiply: white * alpha = (a, a, a, a)
        Self([a, a, a, a])
    }

    /// Creates an additive white (luminance).
    ///
    /// Useful for creating glow or light effects.
    #[inline]
    pub const fn from_additive_luminance(l: u8) -> Self {
        Self([l, l, l, 0])
    }

    // ===== Component accessors =====

    /// Returns the premultiplied red component (0-255).
    #[inline]
    pub const fn r(&self) -> u8 {
        self.0[0]
    }

    /// Returns the premultiplied green component (0-255).
    #[inline]
    pub const fn g(&self) -> u8 {
        self.0[1]
    }

    /// Returns the premultiplied blue component (0-255).
    #[inline]
    pub const fn b(&self) -> u8 {
        self.0[2]
    }

    /// Returns the alpha component (0-255).
    #[inline]
    pub const fn a(&self) -> u8 {
        self.0[3]
    }

    // ===== Checks =====

    /// Returns true if alpha is 255 (fully opaque).
    #[inline]
    pub const fn is_opaque(&self) -> bool {
        self.a() == 255
    }

    /// Returns true if alpha is 0 (transparent or additive).
    #[inline]
    pub const fn is_transparent(&self) -> bool {
        self.a() == 0
    }

    /// Returns true if this is an additive color (alpha=0 with non-zero RGB).
    #[inline]
    pub fn is_additive(&self) -> bool {
        self.a() == 0 && (self.r() != 0 || self.g() != 0 || self.b() != 0)
    }

    // ===== Conversions =====

    /// Returns the premultiplied RGBA components as an array.
    #[inline]
    pub const fn to_array(&self) -> [u8; 4] {
        [self.r(), self.g(), self.b(), self.a()]
    }

    /// Returns the premultiplied RGBA components as a tuple.
    #[inline]
    pub const fn to_tuple(&self) -> (u8, u8, u8, u8) {
        (self.r(), self.g(), self.b(), self.a())
    }

    /// Converts to "normal" unmultiplied RGBA values.
    ///
    /// This reverses the premultiplication, giving you the original
    /// separate alpha values.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Color32;
    ///
    /// let color = Color32::from_rgba_unmultiplied(255, 0, 0, 128);
    /// let [r, g, b, a] = color.to_rgba_unmultiplied();
    /// assert_eq!(r, 255);
    /// assert_eq!(a, 128);
    /// ```
    #[inline]
    pub fn to_rgba_unmultiplied(&self) -> [u8; 4] {
        let [r, g, b, a] = self.to_array();
        match a {
            0 | 255 => self.to_array(),
            a => {
                let factor = 255.0 / a as f32;
                [
                    (r as f32 * factor) as u8,
                    (g as f32 * factor) as u8,
                    (b as f32 * factor) as u8,
                    a,
                ]
            }
        }
    }

    /// Converts to floating point RGBA in 0.0-1.0 range (gamma space).
    ///
    /// **Warning**: This does NOT convert to linear space!
    /// These are gamma-space values. For linear space, convert to [`Color`] first.
    #[inline]
    pub fn to_rgba_f32(&self) -> [f32; 4] {
        [
            self.r() as f32 / 255.0,
            self.g() as f32 / 255.0,
            self.b() as f32 / 255.0,
            self.a() as f32 / 255.0,
        ]
    }

    // ===== Modifiers =====

    /// Returns an opaque version of this color.
    #[inline]
    pub const fn to_opaque(self) -> Self {
        let [r, g, b, _] = self.to_array();
        Self([r, g, b, 255])
    }

    /// Returns an additive version of this color (sets alpha to 0).
    #[inline]
    pub const fn to_additive(self) -> Self {
        let [r, g, b, _] = self.to_array();
        Self([r, g, b, 0])
    }

    // ===== Operations =====

    /// Multiplies color by a factor in gamma space (fast, perceptually even).
    ///
    /// This is faster than [`Self::linear_multiply`] and looks better for UI.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Color32;
    ///
    /// let red = Color32::from_rgb(255, 0, 0);
    /// let half = red.gamma_multiply(0.5);
    /// ```
    #[inline]
    pub fn gamma_multiply(self, factor: f32) -> Self {
        debug_assert!(
            0.0 <= factor && factor.is_finite(),
            "factor must be finite and non-negative"
        );
        let Self([r, g, b, a]) = self;
        Self([
            (r as f32 * factor + 0.5) as u8,
            (g as f32 * factor + 0.5) as u8,
            (b as f32 * factor + 0.5) as u8,
            (a as f32 * factor + 0.5) as u8,
        ])
    }

    /// Multiplies color by a u8 factor (0-255) in gamma space.
    ///
    /// This is even faster than [`Self::gamma_multiply`] for integer factors.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Color32;
    ///
    /// let red = Color32::RED;
    /// let half = red.gamma_multiply_u8(127); // ~50%
    /// ```
    #[inline]
    pub fn gamma_multiply_u8(self, factor: u8) -> Self {
        let Self([r, g, b, a]) = self;
        let factor = factor as u32;
        Self([
            ((r as u32 * factor + 127) / 255) as u8,
            ((g as u32 * factor + 127) / 255) as u8,
            ((b as u32 * factor + 127) / 255) as u8,
            ((a as u32 * factor + 127) / 255) as u8,
        ])
    }

    /// Linear interpolation in gamma space.
    ///
    /// When t=0, returns `self`. When t=1, returns `other`.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Color32;
    ///
    /// let red = Color32::RED;
    /// let blue = Color32::BLUE;
    /// let purple = red.lerp_to(blue, 0.5);
    /// ```
    pub fn lerp_to(&self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let lerp_u8 = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t + 0.5) as u8;

        Self::from_rgba_premultiplied(
            lerp_u8(self[0], other[0]),
            lerp_u8(self[1], other[1]),
            lerp_u8(self[2], other[2]),
            lerp_u8(self[3], other[3]),
        )
    }

    /// Blends this color on top of another (gamma-space alpha blending).
    ///
    /// This uses the standard Porter-Duff "source over" operation in gamma space.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::Color32;
    ///
    /// let bg = Color32::WHITE;
    /// let fg = Color32::from_rgba_unmultiplied(255, 0, 0, 128);
    /// let result = fg.blend_over(bg);
    /// ```
    pub fn blend_over(self, background: Self) -> Self {
        // self is "on top", background is "behind"
        background.gamma_multiply_u8(255 - self.a()) + self
    }

    /// Calculates perceptual intensity/luminance (0.0-1.0).
    ///
    /// Uses standard Rec. 709 luma coefficients.
    #[inline]
    pub fn intensity(&self) -> f32 {
        (self.r() as f32 * 0.299 + self.g() as f32 * 0.587 + self.b() as f32 * 0.114) / 255.0
    }

    /// Returns true if color is perceptually dark (luminance < 0.5).
    #[inline]
    pub fn is_dark(&self) -> bool {
        self.intensity() < 0.5
    }

    /// Returns true if color is perceptually light (luminance >= 0.5).
    #[inline]
    pub fn is_light(&self) -> bool {
        self.intensity() >= 0.5
    }

    // ===== Color constants =====

    /// Fully transparent (alpha=0, RGB=0).
    pub const TRANSPARENT: Self = Self::from_rgba_premultiplied(0, 0, 0, 0);

    /// Black (0, 0, 0).
    pub const BLACK: Self = Self::from_rgb(0, 0, 0);

    /// Dark gray (96, 96, 96).
    pub const DARK_GRAY: Self = Self::from_rgb(96, 96, 96);

    /// Gray (160, 160, 160).
    pub const GRAY: Self = Self::from_rgb(160, 160, 160);

    /// Light gray (220, 220, 220).
    pub const LIGHT_GRAY: Self = Self::from_rgb(220, 220, 220);

    /// White (255, 255, 255).
    pub const WHITE: Self = Self::from_rgb(255, 255, 255);

    /// Brown (165, 42, 42).
    pub const BROWN: Self = Self::from_rgb(165, 42, 42);

    /// Dark red (139, 0, 0).
    pub const DARK_RED: Self = Self::from_rgb(0x8B, 0, 0);

    /// Red (255, 0, 0).
    pub const RED: Self = Self::from_rgb(255, 0, 0);

    /// Light red (255, 128, 128).
    pub const LIGHT_RED: Self = Self::from_rgb(255, 128, 128);

    /// Cyan (0, 255, 255).
    pub const CYAN: Self = Self::from_rgb(0, 255, 255);

    /// Magenta (255, 0, 255).
    pub const MAGENTA: Self = Self::from_rgb(255, 0, 255);

    /// Yellow (255, 255, 0).
    pub const YELLOW: Self = Self::from_rgb(255, 255, 0);

    /// Orange (255, 165, 0).
    pub const ORANGE: Self = Self::from_rgb(255, 165, 0);

    /// Light yellow (255, 255, 224).
    pub const LIGHT_YELLOW: Self = Self::from_rgb(255, 255, 0xE0);

    /// Khaki (240, 230, 140).
    pub const KHAKI: Self = Self::from_rgb(240, 230, 140);

    /// Dark green (0, 100, 0).
    pub const DARK_GREEN: Self = Self::from_rgb(0, 0x64, 0);

    /// Green (0, 255, 0).
    pub const GREEN: Self = Self::from_rgb(0, 255, 0);

    /// Light green (144, 238, 144).
    pub const LIGHT_GREEN: Self = Self::from_rgb(0x90, 0xEE, 0x90);

    /// Dark blue (0, 0, 139).
    pub const DARK_BLUE: Self = Self::from_rgb(0, 0, 0x8B);

    /// Blue (0, 0, 255).
    pub const BLUE: Self = Self::from_rgb(0, 0, 255);

    /// Light blue (173, 216, 230).
    pub const LIGHT_BLUE: Self = Self::from_rgb(0xAD, 0xD8, 0xE6);

    /// Purple (128, 0, 128).
    pub const PURPLE: Self = Self::from_rgb(0x80, 0, 0x80);

    /// Gold (255, 215, 0).
    pub const GOLD: Self = Self::from_rgb(255, 215, 0);

    /// Debug color - semi-transparent green.
    pub const DEBUG_COLOR: Self = Self::from_rgba_premultiplied(0, 200, 0, 128);
}

// ===== Operator overloads =====

impl std::ops::Mul for Color32 {
    type Output = Self;

    /// Component-wise multiplication in gamma space.
    #[inline]
    fn mul(self, other: Self) -> Self {
        Self([
            ((self[0] as u32 * other[0] as u32 + 127) / 255) as u8,
            ((self[1] as u32 * other[1] as u32 + 127) / 255) as u8,
            ((self[2] as u32 * other[2] as u32 + 127) / 255) as u8,
            ((self[3] as u32 * other[3] as u32 + 127) / 255) as u8,
        ])
    }
}

impl std::ops::Add for Color32 {
    type Output = Self;

    /// Component-wise addition with saturation.
    #[inline]
    fn add(self, other: Self) -> Self {
        Self([
            self[0].saturating_add(other[0]),
            self[1].saturating_add(other[1]),
            self[2].saturating_add(other[2]),
            self[3].saturating_add(other[3]),
        ])
    }
}

// ===== Conversions =====

impl From<Color> for Color32 {
    /// Converts from [`Color`] to [`Color32`] with premultiplied alpha.
    fn from(color: Color) -> Self {
        Self::from_rgba_unmultiplied(color.r, color.g, color.b, color.a)
    }
}

impl From<Color32> for Color {
    /// Converts from [`Color32`] to [`Color`] with unmultiplied alpha.
    fn from(color: Color32) -> Self {
        let [r, g, b, a] = color.to_rgba_unmultiplied();
        Color::rgba(r, g, b, a)
    }
}

impl From<[u8; 4]> for Color32 {
    /// Creates from premultiplied RGBA array.
    fn from([r, g, b, a]: [u8; 4]) -> Self {
        Self::from_rgba_premultiplied(r, g, b, a)
    }
}

impl From<Color32> for [u8; 4] {
    /// Converts to premultiplied RGBA array.
    fn from(color: Color32) -> Self {
        color.to_array()
    }
}

impl From<(u8, u8, u8)> for Color32 {
    /// Creates opaque color from RGB tuple.
    fn from((r, g, b): (u8, u8, u8)) -> Self {
        Self::from_rgb(r, g, b)
    }
}

impl From<(u8, u8, u8, u8)> for Color32 {
    /// Creates from unmultiplied RGBA tuple.
    fn from((r, g, b, a): (u8, u8, u8, u8)) -> Self {
        Self::from_rgba_unmultiplied(r, g, b, a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_rgb() {
        let red = Color32::from_rgb(255, 0, 0);
        assert_eq!(red.r(), 255);
        assert_eq!(red.g(), 0);
        assert_eq!(red.b(), 0);
        assert_eq!(red.a(), 255);
        assert!(red.is_opaque());
    }

    #[test]
    fn test_from_rgba_unmultiplied() {
        let semi_red = Color32::from_rgba_unmultiplied(255, 0, 0, 128);
        // RGB should be premultiplied: 255 * 0.5 = 127 or 128 (rounding)
        assert!(semi_red.r() >= 127 && semi_red.r() <= 128);
        assert_eq!(semi_red.a(), 128);
    }

    #[test]
    fn test_to_rgba_unmultiplied() {
        let color = Color32::from_rgba_unmultiplied(255, 0, 0, 128);
        let [r, g, b, a] = color.to_rgba_unmultiplied();
        assert_eq!(r, 255);
        assert_eq!(g, 0);
        assert_eq!(b, 0);
        assert_eq!(a, 128);
    }

    #[test]
    fn test_additive() {
        let glow = Color32::from_rgb_additive(100, 100, 255);
        assert!(glow.is_additive());
        assert_eq!(glow.a(), 0);
        assert_eq!(glow.r(), 100);
    }

    #[test]
    fn test_blend_over() {
        let bg = Color32::WHITE;
        let fg = Color32::from_rgba_unmultiplied(255, 0, 0, 128);
        let result = fg.blend_over(bg);

        // Should be pinkish (red blended with white)
        assert!(result.r() > 200);
        assert!(result.g() > 100);
    }

    #[test]
    fn test_gamma_multiply() {
        let red = Color32::RED;
        let half = red.gamma_multiply(0.5);
        assert!(half.r() <= 128); // Should be darker or equal (rounding)
    }

    #[test]
    fn test_lerp() {
        let red = Color32::RED;
        let blue = Color32::BLUE;

        let at_start = red.lerp_to(blue, 0.0);
        assert_eq!(at_start, red);

        let at_end = red.lerp_to(blue, 1.0);
        assert_eq!(at_end, blue);

        let middle = red.lerp_to(blue, 0.5);
        assert!(middle.r() > 0 && middle.r() < 255);
        assert!(middle.b() > 0 && middle.b() < 255);
    }

    #[test]
    fn test_color_conversion() {
        let color = Color::rgba(255, 128, 64, 200);
        let color32 = Color32::from(color);
        let back = Color::from(color32);

        // Should be close (within rounding)
        assert!((color.r as i16 - back.r as i16).abs() <= 2);
        assert!((color.g as i16 - back.g as i16).abs() <= 2);
        assert!((color.b as i16 - back.b as i16).abs() <= 2);
        assert!((color.a as i16 - back.a as i16).abs() <= 2);
    }

    #[test]
    fn test_intensity() {
        assert!(Color32::WHITE.intensity() > 0.9);
        assert!(Color32::BLACK.intensity() < 0.1);
        assert!(Color32::RED.intensity() < Color32::GREEN.intensity());
    }

    #[test]
    fn test_constants() {
        assert_eq!(Color32::BLACK, Color32::from_rgb(0, 0, 0));
        assert_eq!(Color32::WHITE, Color32::from_rgb(255, 255, 255));
        assert_eq!(Color32::RED, Color32::from_rgb(255, 0, 0));
        assert_eq!(Color32::GREEN, Color32::from_rgb(0, 255, 0));
        assert_eq!(Color32::BLUE, Color32::from_rgb(0, 0, 255));
    }

    #[test]
    fn test_ops() {
        let a = Color32::from_rgb(100, 100, 100);
        let b = Color32::from_rgb(50, 50, 50);

        // Addition
        let sum = a + b;
        assert_eq!(sum.r(), 150);

        // Multiplication
        let product = Color32::from_rgb(255, 128, 64) * Color32::from_rgb(255, 255, 128);
        assert_eq!(product.r(), 255);
        assert!(product.g() <= 128); // 128 * 255/255 could be 128 due to rounding
        assert!(product.b() <= 64); // 64 * 128/255 should be < 64
    }
}
