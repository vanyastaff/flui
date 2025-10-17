//! Scale type for 2D scaling transformations
//!
//! This module provides a type-safe wrapper for scale factors.

use egui::Vec2;

/// Represents a 2D scale factor.
///
/// Type-safe wrapper for scaling transformations with separate X and Y factors.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Scale {
    /// Horizontal scale factor
    pub x: f32,
    /// Vertical scale factor
    pub y: f32,
}

impl Scale {
    /// No scaling (identity scale).
    pub const IDENTITY: Scale = Scale { x: 1.0, y: 1.0 };

    /// Zero scale (collapse to point).
    pub const ZERO: Scale = Scale { x: 0.0, y: 0.0 };

    /// Double scale (2x in both directions).
    pub const DOUBLE: Scale = Scale { x: 2.0, y: 2.0 };

    /// Half scale (0.5x in both directions).
    pub const HALF: Scale = Scale { x: 0.5, y: 0.5 };

    /// Flip horizontally (-1 on X axis).
    pub const FLIP_HORIZONTAL: Scale = Scale { x: -1.0, y: 1.0 };

    /// Flip vertically (-1 on Y axis).
    pub const FLIP_VERTICAL: Scale = Scale { x: 1.0, y: -1.0 };

    /// Flip both axes.
    pub const FLIP_BOTH: Scale = Scale { x: -1.0, y: -1.0 };

    /// Create a new scale with separate X and Y factors.
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Create a uniform scale (same factor for both axes).
    pub const fn uniform(factor: f32) -> Self {
        Self {
            x: factor,
            y: factor,
        }
    }

    /// Create scale from percentage (100 = 100% = 1.0).
    pub fn from_percent(percent: f32) -> Self {
        Self::uniform(percent / 100.0)
    }

    /// Create scale from separate X and Y percentages.
    pub fn from_percent_xy(x_percent: f32, y_percent: f32) -> Self {
        Self::new(x_percent / 100.0, y_percent / 100.0)
    }

    /// Check if this is uniform scaling (x == y).
    pub fn is_uniform(&self) -> bool {
        (self.x - self.y).abs() < f32::EPSILON
    }

    /// Check if this is identity scale (no scaling).
    pub fn is_identity(&self) -> bool {
        (self.x - 1.0).abs() < f32::EPSILON && (self.y - 1.0).abs() < f32::EPSILON
    }

    /// Check if this flips the X axis.
    pub fn flips_x(&self) -> bool {
        self.x < 0.0
    }

    /// Check if this flips the Y axis.
    pub fn flips_y(&self) -> bool {
        self.y < 0.0
    }

    /// Get the uniform scale factor (only if uniform).
    pub fn get_uniform(&self) -> Option<f32> {
        if self.is_uniform() {
            Some(self.x)
        } else {
            None
        }
    }

    /// Get X scale as percentage.
    pub fn x_percent(&self) -> f32 {
        self.x * 100.0
    }

    /// Get Y scale as percentage.
    pub fn y_percent(&self) -> f32 {
        self.y * 100.0
    }

    /// Get the inverse scale (for reversing a transformation).
    pub fn inverse(&self) -> Self {
        Self {
            x: 1.0 / self.x,
            y: 1.0 / self.y,
        }
    }

    /// Apply this scale to a value.
    pub fn apply(&self, value: Vec2) -> Vec2 {
        Vec2::new(value.x * self.x, value.y * self.y)
    }

    /// Combine with another scale (multiply factors).
    pub fn then(&self, other: Scale) -> Self {
        Self {
            x: self.x * other.x,
            y: self.y * other.y,
        }
    }

    /// Scale the X factor.
    pub fn scale_x(mut self, factor: f32) -> Self {
        self.x *= factor;
        self
    }

    /// Scale the Y factor.
    pub fn scale_y(mut self, factor: f32) -> Self {
        self.y *= factor;
        self
    }

    /// Scale both factors uniformly.
    pub fn scale_uniform(mut self, factor: f32) -> Self {
        self.x *= factor;
        self.y *= factor;
        self
    }

    /// Clamp scale factors to a range.
    pub fn clamp(&self, min: f32, max: f32) -> Self {
        Self {
            x: self.x.clamp(min, max),
            y: self.y.clamp(min, max),
        }
    }

    /// Linear interpolation between two scales.
    pub fn lerp(a: Scale, b: Scale, t: f32) -> Scale {
        Scale {
            x: a.x + (b.x - a.x) * t,
            y: a.y + (b.y - a.y) * t,
        }
    }
}

impl Default for Scale {
    fn default() -> Self {
        Self::IDENTITY
    }
}

// Conversions from primitives
impl From<f32> for Scale {
    /// Create uniform scale from a single factor.
    fn from(factor: f32) -> Self {
        Self::uniform(factor)
    }
}

impl From<(f32, f32)> for Scale {
    /// Create scale from (x, y) tuple.
    fn from((x, y): (f32, f32)) -> Self {
        Self::new(x, y)
    }
}

impl From<Vec2> for Scale {
    fn from(vec: Vec2) -> Self {
        Self::new(vec.x, vec.y)
    }
}

impl From<Scale> for Vec2 {
    fn from(scale: Scale) -> Self {
        Vec2::new(scale.x, scale.y)
    }
}

// Arithmetic operations
impl std::ops::Mul for Scale {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        self.then(rhs)
    }
}

impl std::ops::Mul<f32> for Scale {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        self.scale_uniform(rhs)
    }
}

impl std::ops::Div<f32> for Scale {
    type Output = Self;

    fn div(self, rhs: f32) -> Self::Output {
        Self {
            x: self.x / rhs,
            y: self.y / rhs,
        }
    }
}

impl std::ops::Neg for Scale {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self {
            x: -self.x,
            y: -self.y,
        }
    }
}

impl std::fmt::Display for Scale {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_uniform() {
            write!(f, "{}x", self.x)
        } else {
            write!(f, "{}x × {}x", self.x, self.y)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scale_creation() {
        let scale = Scale::new(2.0, 3.0);
        assert_eq!(scale.x, 2.0);
        assert_eq!(scale.y, 3.0);

        let uniform = Scale::uniform(2.5);
        assert_eq!(uniform.x, 2.5);
        assert_eq!(uniform.y, 2.5);
        assert!(uniform.is_uniform());

        let from_percent = Scale::from_percent(200.0);
        assert_eq!(from_percent.x, 2.0);
        assert_eq!(from_percent.y, 2.0);
    }

    #[test]
    fn test_scale_constants() {
        assert!(Scale::IDENTITY.is_identity());
        assert_eq!(Scale::IDENTITY.x, 1.0);
        assert_eq!(Scale::IDENTITY.y, 1.0);

        assert_eq!(Scale::DOUBLE.x, 2.0);
        assert_eq!(Scale::HALF.x, 0.5);

        assert!(Scale::FLIP_HORIZONTAL.flips_x());
        assert!(!Scale::FLIP_HORIZONTAL.flips_y());

        assert!(Scale::FLIP_VERTICAL.flips_y());
        assert!(!Scale::FLIP_VERTICAL.flips_x());
    }

    #[test]
    fn test_scale_checks() {
        assert!(Scale::uniform(2.0).is_uniform());
        assert!(!Scale::new(2.0, 3.0).is_uniform());

        assert!(Scale::IDENTITY.is_identity());
        assert!(!Scale::DOUBLE.is_identity());

        assert_eq!(Scale::uniform(2.5).get_uniform(), Some(2.5));
        assert_eq!(Scale::new(2.0, 3.0).get_uniform(), None);
    }

    #[test]
    fn test_scale_percentages() {
        let scale = Scale::from_percent(150.0);
        assert_eq!(scale.x, 1.5);
        assert_eq!(scale.x_percent(), 150.0);

        let scale = Scale::from_percent_xy(200.0, 50.0);
        assert_eq!(scale.x, 2.0);
        assert_eq!(scale.y, 0.5);
    }

    #[test]
    fn test_scale_inverse() {
        let scale = Scale::new(2.0, 4.0);
        let inverse = scale.inverse();
        assert_eq!(inverse.x, 0.5);
        assert_eq!(inverse.y, 0.25);

        let combined = scale.then(inverse);
        assert!(combined.is_identity());
    }

    #[test]
    fn test_scale_apply() {
        let scale = Scale::new(2.0, 3.0);
        let vec = Vec2::new(10.0, 20.0);
        let scaled = scale.apply(vec);
        assert_eq!(scaled, Vec2::new(20.0, 60.0));
    }

    #[test]
    fn test_scale_then() {
        let scale1 = Scale::new(2.0, 3.0);
        let scale2 = Scale::new(1.5, 2.0);
        let combined = scale1.then(scale2);
        assert_eq!(combined.x, 3.0);
        assert_eq!(combined.y, 6.0);
    }

    #[test]
    fn test_scale_modifications() {
        let scale = Scale::uniform(2.0)
            .scale_x(1.5)
            .scale_y(2.0);
        assert_eq!(scale.x, 3.0);
        assert_eq!(scale.y, 4.0);

        let scaled = Scale::new(1.0, 1.0).scale_uniform(3.0);
        assert_eq!(scaled.x, 3.0);
        assert_eq!(scaled.y, 3.0);
    }

    #[test]
    fn test_scale_clamp() {
        let scale = Scale::new(0.3, 5.0);
        let clamped = scale.clamp(0.5, 2.0);
        assert_eq!(clamped.x, 0.5);
        assert_eq!(clamped.y, 2.0);
    }

    #[test]
    fn test_scale_lerp() {
        let a = Scale::uniform(1.0);
        let b = Scale::uniform(3.0);
        let mid = Scale::lerp(a, b, 0.5);
        assert_eq!(mid.x, 2.0);
        assert_eq!(mid.y, 2.0);

        let quarter = Scale::lerp(a, b, 0.25);
        assert_eq!(quarter.x, 1.5);
    }

    #[test]
    fn test_scale_arithmetic() {
        let scale1 = Scale::new(2.0, 3.0);
        let scale2 = Scale::new(1.5, 2.0);

        // Multiplication (combine scales)
        let combined = scale1 * scale2;
        assert_eq!(combined.x, 3.0);
        assert_eq!(combined.y, 6.0);

        // Multiply by scalar
        let scaled = Scale::uniform(2.0) * 3.0;
        assert_eq!(scaled.x, 6.0);
        assert_eq!(scaled.y, 6.0);

        // Division by scalar
        let divided = Scale::uniform(6.0) / 2.0;
        assert_eq!(divided.x, 3.0);

        // Negation (flip both axes)
        let neg = -Scale::uniform(2.0);
        assert_eq!(neg.x, -2.0);
        assert_eq!(neg.y, -2.0);
    }

    #[test]
    fn test_scale_from_conversions() {
        // From f32 (uniform)
        let scale: Scale = 2.5.into();
        assert_eq!(scale.x, 2.5);
        assert_eq!(scale.y, 2.5);

        // From tuple
        let scale: Scale = (2.0, 3.0).into();
        assert_eq!(scale.x, 2.0);
        assert_eq!(scale.y, 3.0);

        // From Vec2
        let vec = Vec2::new(1.5, 2.5);
        let scale: Scale = vec.into();
        assert_eq!(scale.x, 1.5);
        assert_eq!(scale.y, 2.5);

        // To Vec2
        let scale = Scale::new(3.0, 4.0);
        let vec: Vec2 = scale.into();
        assert_eq!(vec.x, 3.0);
        assert_eq!(vec.y, 4.0);
    }

    #[test]
    fn test_scale_display() {
        let uniform = Scale::uniform(2.5);
        assert_eq!(format!("{}", uniform), "2.5x");

        let non_uniform = Scale::new(2.0, 3.0);
        assert_eq!(format!("{}", non_uniform), "2x × 3x");
    }

    #[test]
    fn test_scale_flips() {
        assert!(Scale::new(-1.0, 1.0).flips_x());
        assert!(Scale::new(1.0, -1.0).flips_y());
        assert!(Scale::FLIP_BOTH.flips_x());
        assert!(Scale::FLIP_BOTH.flips_y());
    }
}
