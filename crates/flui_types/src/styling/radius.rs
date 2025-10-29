//! Border radius types

use std::ops::{Add, Div, Mul, Neg, Sub};

/// An immutable radius with separate x and y components.
///
/// Used to define circular or elliptical corner radii.
/// Similar to Flutter's `Radius`.
///
/// # Examples
///
/// ```
/// use flui_types::styling::Radius;
///
/// // Circular radius
/// let circular = Radius::circular(10.0);
/// assert_eq!(circular.x, 10.0);
/// assert_eq!(circular.y, 10.0);
///
/// // Elliptical radius
/// let elliptical = Radius::elliptical(20.0, 10.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Radius {
    /// The radius value on the horizontal axis.
    pub x: f32,
    /// The radius value on the vertical axis.
    pub y: f32,
}

impl Radius {
    /// A radius with no curvature (both x and y are 0).
    pub const ZERO: Self = Self::circular(0.0);

    /// Creates a circular radius (x and y are equal).
    ///
    /// # Arguments
    ///
    /// * `radius` - The radius value for both x and y axes
    pub const fn circular(radius: f32) -> Self {
        Self {
            x: radius,
            y: radius,
        }
    }

    /// Creates an elliptical radius with different x and y values.
    ///
    /// # Arguments
    ///
    /// * `x` - The radius value on the horizontal axis
    /// * `y` - The radius value on the vertical axis
    pub const fn elliptical(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Returns true if both x and y are zero.
    pub fn is_zero(&self) -> bool {
        self.x == 0.0 && self.y == 0.0
    }

    /// Returns true if x and y are equal (circular radius).
    pub fn is_circular(&self) -> bool {
        self.x == self.y
    }

    /// Returns true if both x and y are finite.
    pub fn is_finite(&self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }

    /// Linearly interpolate between two radii.
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            x: a.x + (b.x - a.x) * t,
            y: a.y + (b.y - a.y) * t,
        }
    }

    /// Clamp the radius to ensure it's non-negative.
    pub fn clamp_non_negative(&self) -> Self {
        Self {
            x: self.x.max(0.0),
            y: self.y.max(0.0),
        }
    }

    /// Scale the radius by a factor.
    ///
    /// # Arguments
    ///
    /// * `factor` - The scaling factor
    pub fn scale(&self, factor: f32) -> Self {
        *self * factor
    }
}

impl Default for Radius {
    fn default() -> Self {
        Self::ZERO
    }
}

impl From<f32> for Radius {
    fn from(value: f32) -> Self {
        Self::circular(value)
    }
}

impl From<(f32, f32)> for Radius {
    fn from((x, y): (f32, f32)) -> Self {
        Self::elliptical(x, y)
    }
}

impl Add for Radius {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl Sub for Radius {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

impl Mul<f32> for Radius {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        Self {
            x: self.x * rhs,
            y: self.y * rhs,
        }
    }
}

impl Div<f32> for Radius {
    type Output = Self;

    fn div(self, rhs: f32) -> Self::Output {
        Self {
            x: self.x / rhs,
            y: self.y / rhs,
        }
    }
}

impl Neg for Radius {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self {
            x: -self.x,
            y: -self.y,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_radius_circular() {
        let radius = Radius::circular(10.0);
        assert_eq!(radius.x, 10.0);
        assert_eq!(radius.y, 10.0);
        assert!(radius.is_circular());
    }

    #[test]
    fn test_radius_elliptical() {
        let radius = Radius::elliptical(20.0, 10.0);
        assert_eq!(radius.x, 20.0);
        assert_eq!(radius.y, 10.0);
        assert!(!radius.is_circular());
    }

    #[test]
    fn test_radius_zero() {
        let zero = Radius::ZERO;
        assert!(zero.is_zero());
        assert_eq!(zero.x, 0.0);
        assert_eq!(zero.y, 0.0);
    }

    #[test]
    fn test_radius_is_finite() {
        let finite = Radius::circular(10.0);
        assert!(finite.is_finite());

        let infinite = Radius::circular(f32::INFINITY);
        assert!(!infinite.is_finite());
    }

    #[test]
    fn test_radius_lerp() {
        let a = Radius::circular(0.0);
        let b = Radius::circular(10.0);
        let mid = Radius::lerp(a, b, 0.5);
        assert_eq!(mid.x, 5.0);
        assert_eq!(mid.y, 5.0);
    }

    #[test]
    fn test_radius_clamp_non_negative() {
        let negative = Radius::elliptical(-5.0, 10.0);
        let clamped = negative.clamp_non_negative();
        assert_eq!(clamped.x, 0.0);
        assert_eq!(clamped.y, 10.0);
    }

    #[test]
    fn test_radius_arithmetic() {
        let a = Radius::circular(10.0);
        let b = Radius::circular(5.0);

        let sum = a + b;
        assert_eq!(sum.x, 15.0);

        let diff = a - b;
        assert_eq!(diff.x, 5.0);

        let product = a * 2.0;
        assert_eq!(product.x, 20.0);

        let quotient = a / 2.0;
        assert_eq!(quotient.x, 5.0);

        let negated = -a;
        assert_eq!(negated.x, -10.0);
    }

    #[test]
    fn test_radius_from_f32() {
        let radius: Radius = 10.0.into();
        assert_eq!(radius, Radius::circular(10.0));
    }

    #[test]
    fn test_radius_from_tuple() {
        let radius: Radius = (20.0, 10.0).into();
        assert_eq!(radius, Radius::elliptical(20.0, 10.0));
    }

    #[test]
    fn test_radius_default() {
        let default = Radius::default();
        assert_eq!(default, Radius::ZERO);
    }

    #[test]
    fn test_radius_scale() {
        let radius = Radius::elliptical(10.0, 5.0);
        let scaled = radius.scale(2.0);
        assert_eq!(scaled.x, 20.0);
        assert_eq!(scaled.y, 10.0);
    }
}
