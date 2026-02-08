//! Packed 32-bit color representation with premultiplied alpha.
//!
//! This module provides [`Color32`], a space-efficient color type that stores
//! colors as a packed 32-bit value with premultiplied alpha in sRGB gamma space.
//!
//! ## Premultiplied Alpha
//!
//! Unlike [`Color`] which uses separate alpha, `Color32` uses
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
//! For physically correct color operations, convert to [`Color`] first.

use super::Color;

#[derive(Copy, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Color32([u8; 4]);

impl std::fmt::Debug for Color32 {
    #[inline]
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

    #[inline]
    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self([r, g, b, 255])
    }

    #[inline]
    pub const fn from_rgb_additive(r: u8, g: u8, b: u8) -> Self {
        Self([r, g, b, 0])
    }

    #[inline]
    pub const fn from_rgba_premultiplied(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self([r, g, b, a])
    }

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

    #[inline]
    pub const fn from_gray(l: u8) -> Self {
        Self([l, l, l, 255])
    }

    #[inline]
    pub const fn from_black_alpha(a: u8) -> Self {
        Self([0, 0, 0, a])
    }

    #[inline]
    pub fn from_white_alpha(a: u8) -> Self {
        // Premultiply: white * alpha = (a, a, a, a)
        Self([a, a, a, a])
    }

    #[inline]
    pub const fn from_additive_luminance(l: u8) -> Self {
        Self([l, l, l, 0])
    }

    // ===== Component accessors =====

    #[inline]
    pub const fn r(&self) -> u8 {
        self.0[0]
    }

    #[inline]
    pub const fn g(&self) -> u8 {
        self.0[1]
    }

    #[inline]
    pub const fn b(&self) -> u8 {
        self.0[2]
    }

    #[inline]
    pub const fn a(&self) -> u8 {
        self.0[3]
    }

    // ===== Checks =====

    #[inline]
    pub const fn is_opaque(&self) -> bool {
        self.a() == 255
    }

    #[inline]
    pub const fn is_transparent(&self) -> bool {
        self.a() == 0
    }

    #[inline]
    pub fn is_additive(&self) -> bool {
        self.a() == 0 && (self.r() != 0 || self.g() != 0 || self.b() != 0)
    }

    // ===== Conversions =====

    #[inline]
    pub const fn to_array(&self) -> [u8; 4] {
        [self.r(), self.g(), self.b(), self.a()]
    }

    #[inline]
    pub const fn to_tuple(&self) -> (u8, u8, u8, u8) {
        (self.r(), self.g(), self.b(), self.a())
    }

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

    #[inline]
    pub const fn to_opaque(self) -> Self {
        let [r, g, b, _] = self.to_array();
        Self([r, g, b, 255])
    }

    #[inline]
    pub const fn to_additive(self) -> Self {
        let [r, g, b, _] = self.to_array();
        Self([r, g, b, 0])
    }

    // ===== Operations =====

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
    #[inline]
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
    #[inline]
    pub fn blend_over(self, background: Self) -> Self {
        // self is "on top", background is "behind"
        background.gamma_multiply_u8(255 - self.a()) + self
    }

    #[inline]
    pub fn intensity(&self) -> f32 {
        (self.r() as f32 * 0.299 + self.g() as f32 * 0.587 + self.b() as f32 * 0.114) / 255.0
    }

    #[inline]
    pub fn is_dark(&self) -> bool {
        self.intensity() < 0.5
    }

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
    #[inline]
    fn from(color: Color) -> Self {
        Self::from_rgba_unmultiplied(color.r, color.g, color.b, color.a)
    }
}

impl From<Color32> for Color {
    /// Converts from [`Color32`] to [`Color`] with unmultiplied alpha.
    #[inline]
    fn from(color: Color32) -> Self {
        let [r, g, b, a] = color.to_rgba_unmultiplied();
        Color::rgba(r, g, b, a)
    }
}

impl From<[u8; 4]> for Color32 {
    /// Creates from premultiplied RGBA array.
    #[inline]
    fn from([r, g, b, a]: [u8; 4]) -> Self {
        Self::from_rgba_premultiplied(r, g, b, a)
    }
}

impl From<Color32> for [u8; 4] {
    /// Converts to premultiplied RGBA array.
    #[inline]
    fn from(color: Color32) -> Self {
        color.to_array()
    }
}

impl From<(u8, u8, u8)> for Color32 {
    /// Creates opaque color from RGB tuple.
    #[inline]
    fn from((r, g, b): (u8, u8, u8)) -> Self {
        Self::from_rgb(r, g, b)
    }
}

impl From<(u8, u8, u8, u8)> for Color32 {
    /// Creates from unmultiplied RGBA tuple.
    #[inline]
    fn from((r, g, b, a): (u8, u8, u8, u8)) -> Self {
        Self::from_rgba_unmultiplied(r, g, b, a)
    }
}
