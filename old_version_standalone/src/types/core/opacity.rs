//! Opacity types for transparency control
//!
//! This module provides type-safe opacity values.

/// Represents opacity/transparency as a value between 0.0 (transparent) and 1.0 (opaque).
///
/// Type-safe wrapper around f32 that ensures values stay in valid range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Opacity(f32);

impl Opacity {
    /// Fully transparent (0.0)
    pub const TRANSPARENT: Opacity = Opacity(0.0);

    /// Fully opaque (1.0)
    pub const OPAQUE: Opacity = Opacity(1.0);

    /// Half transparent (0.5)
    pub const HALF: Opacity = Opacity(0.5);

    /// Mostly transparent (0.25)
    pub const FAINT: Opacity = Opacity(0.25);

    /// Mostly opaque (0.75)
    pub const STRONG: Opacity = Opacity(0.75);

    /// Create a new opacity value, clamping to [0.0, 1.0].
    pub fn new(value: f32) -> Self {
        Self(value.clamp(0.0, 1.0))
    }

    /// Create an opacity from a percentage (0-100).
    pub fn from_percent(percent: f32) -> Self {
        Self::new(percent / 100.0)
    }

    /// Create an opacity from a byte value (0-255).
    pub fn from_u8(value: u8) -> Self {
        Self::new(value as f32 / 255.0)
    }

    /// Get the opacity value as f32 (0.0 - 1.0).
    pub fn value(&self) -> f32 {
        self.0
    }

    /// Get the opacity as a percentage (0.0 - 100.0).
    pub fn as_percent(&self) -> f32 {
        self.0 * 100.0
    }

    /// Get the opacity as a byte value (0 - 255).
    pub fn as_u8(&self) -> u8 {
        (self.0 * 255.0).round() as u8
    }

    /// Check if this opacity is fully transparent.
    pub fn is_transparent(&self) -> bool {
        self.0 <= 0.0
    }

    /// Check if this opacity is fully opaque.
    pub fn is_opaque(&self) -> bool {
        self.0 >= 1.0
    }

    /// Get the inverse opacity (1.0 - value).
    pub fn inverse(&self) -> Self {
        Self(1.0 - self.0)
    }

    /// Linear interpolation between two opacity values.
    pub fn lerp(a: Opacity, b: Opacity, t: f32) -> Opacity {
        Opacity::new(a.0 + (b.0 - a.0) * t)
    }

    /// Multiply two opacity values (composition).
    pub fn compose(&self, other: Opacity) -> Self {
        Self(self.0 * other.0)
    }
}

impl Default for Opacity {
    fn default() -> Self {
        Self::OPAQUE
    }
}

impl From<f32> for Opacity {
    fn from(value: f32) -> Self {
        Self::new(value)
    }
}

impl From<Opacity> for f32 {
    fn from(opacity: Opacity) -> Self {
        opacity.0
    }
}

impl From<u8> for Opacity {
    fn from(value: u8) -> Self {
        Self::from_u8(value)
    }
}

impl std::ops::Mul for Opacity {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        self.compose(rhs)
    }
}

impl std::ops::Mul<f32> for Opacity {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        Self::new(self.0 * rhs)
    }
}

impl std::fmt::Display for Opacity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.1}%", self.as_percent())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opacity_creation() {
        let opacity = Opacity::new(0.5);
        assert_eq!(opacity.value(), 0.5);

        let clamped_high = Opacity::new(1.5);
        assert_eq!(clamped_high.value(), 1.0);

        let clamped_low = Opacity::new(-0.5);
        assert_eq!(clamped_low.value(), 0.0);
    }

    #[test]
    fn test_opacity_constants() {
        assert_eq!(Opacity::TRANSPARENT.value(), 0.0);
        assert_eq!(Opacity::OPAQUE.value(), 1.0);
        assert_eq!(Opacity::HALF.value(), 0.5);
        assert_eq!(Opacity::FAINT.value(), 0.25);
        assert_eq!(Opacity::STRONG.value(), 0.75);
    }

    #[test]
    fn test_opacity_from_percent() {
        let opacity = Opacity::from_percent(50.0);
        assert_eq!(opacity.value(), 0.5);

        let full = Opacity::from_percent(100.0);
        assert_eq!(full.value(), 1.0);
    }

    #[test]
    fn test_opacity_from_u8() {
        let opacity = Opacity::from_u8(128);
        assert!((opacity.value() - 0.502).abs() < 0.01);

        let full = Opacity::from_u8(255);
        assert_eq!(full.value(), 1.0);

        let none = Opacity::from_u8(0);
        assert_eq!(none.value(), 0.0);
    }

    #[test]
    fn test_opacity_as_percent() {
        let opacity = Opacity::new(0.5);
        assert_eq!(opacity.as_percent(), 50.0);

        let full = Opacity::OPAQUE;
        assert_eq!(full.as_percent(), 100.0);
    }

    #[test]
    fn test_opacity_as_u8() {
        let half = Opacity::HALF;
        assert_eq!(half.as_u8(), 128);

        let full = Opacity::OPAQUE;
        assert_eq!(full.as_u8(), 255);

        let none = Opacity::TRANSPARENT;
        assert_eq!(none.as_u8(), 0);
    }

    #[test]
    fn test_opacity_checks() {
        assert!(Opacity::TRANSPARENT.is_transparent());
        assert!(!Opacity::TRANSPARENT.is_opaque());

        assert!(Opacity::OPAQUE.is_opaque());
        assert!(!Opacity::OPAQUE.is_transparent());

        assert!(!Opacity::HALF.is_transparent());
        assert!(!Opacity::HALF.is_opaque());
    }

    #[test]
    fn test_opacity_inverse() {
        let half = Opacity::HALF;
        assert_eq!(half.inverse(), Opacity::HALF);

        let quarter = Opacity::new(0.25);
        assert_eq!(quarter.inverse(), Opacity::new(0.75));
    }

    #[test]
    fn test_opacity_lerp() {
        let transparent = Opacity::TRANSPARENT;
        let opaque = Opacity::OPAQUE;

        let mid = Opacity::lerp(transparent, opaque, 0.5);
        assert_eq!(mid, Opacity::HALF);

        let quarter = Opacity::lerp(transparent, opaque, 0.25);
        assert_eq!(quarter, Opacity::FAINT);
    }

    #[test]
    fn test_opacity_compose() {
        let half = Opacity::HALF;
        let composed = half.compose(half);
        assert_eq!(composed.value(), 0.25);

        let full = Opacity::OPAQUE;
        let composed2 = half.compose(full);
        assert_eq!(composed2, half);
    }

    #[test]
    fn test_opacity_multiply() {
        let half = Opacity::HALF;
        let result = half * half;
        assert_eq!(result.value(), 0.25);

        let scaled = half * 0.5;
        assert_eq!(scaled.value(), 0.25);
    }

    #[test]
    fn test_opacity_conversions() {
        let from_f32: Opacity = 0.75.into();
        assert_eq!(from_f32, Opacity::STRONG);

        let to_f32: f32 = Opacity::HALF.into();
        assert_eq!(to_f32, 0.5);

        let from_u8: Opacity = 128u8.into();
        assert!((from_u8.value() - 0.502).abs() < 0.01);
    }

    #[test]
    fn test_opacity_display() {
        assert_eq!(format!("{}", Opacity::HALF), "50.0%");
        assert_eq!(format!("{}", Opacity::OPAQUE), "100.0%");
        assert_eq!(format!("{}", Opacity::TRANSPARENT), "0.0%");
    }
}
