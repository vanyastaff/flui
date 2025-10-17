//! Border radius types for rounded corners
//!
//! This module contains types for representing rounded corners,
//! similar to Flutter's Radius and BorderRadius system.

use crate::types::core::Size;

/// A radius for either circular or elliptical shapes.
///
/// Similar to Flutter's `Radius`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Radius {
    /// The x-axis radius.
    pub x: f32,

    /// The y-axis radius.
    pub y: f32,
}

impl Radius {
    /// Create a circular radius.
    pub const fn circular(radius: f32) -> Self {
        Self { x: radius, y: radius }
    }

    /// Create an elliptical radius.
    pub const fn elliptical(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Create a zero radius (no rounding).
    pub const ZERO: Self = Self::circular(0.0);

    /// Check if this radius is zero.
    pub fn is_zero(&self) -> bool {
        self.x == 0.0 && self.y == 0.0
    }

    /// Check if this radius is circular (x == y).
    pub fn is_circular(&self) -> bool {
        (self.x - self.y).abs() < f32::EPSILON
    }

    /// Check if this radius is elliptical (x != y).
    pub fn is_elliptical(&self) -> bool {
        !self.is_circular()
    }

    /// Clamp the radius to be non-negative.
    pub fn clamp_non_negative(&self) -> Self {
        Self {
            x: self.x.max(0.0),
            y: self.y.max(0.0),
        }
    }

    /// Get the minimum dimension.
    pub fn min_dimension(&self) -> f32 {
        self.x.min(self.y)
    }

    /// Get the maximum dimension.
    pub fn max_dimension(&self) -> f32 {
        self.x.max(self.y)
    }

    /// Multiply the radius by a scalar.
    pub fn scale(&self, factor: f32) -> Self {
        Self {
            x: self.x * factor,
            y: self.y * factor,
        }
    }

    /// Convert to a Size.
    pub fn to_size(&self) -> Size {
        Size::new(self.x, self.y)
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

impl std::ops::Mul<f32> for Radius {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        self.scale(rhs)
    }
}

impl std::ops::Neg for Radius {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self {
            x: -self.x,
            y: -self.y,
        }
    }
}

/// An immutable set of radii for each corner of a rectangle.
///
/// Similar to Flutter's `BorderRadius`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BorderRadius {
    /// The top-left corner radius.
    pub top_left: Radius,

    /// The top-right corner radius.
    pub top_right: Radius,

    /// The bottom-left corner radius.
    pub bottom_left: Radius,

    /// The bottom-right corner radius.
    pub bottom_right: Radius,
}

impl BorderRadius {
    /// Create a border radius with all corners set to circular radii.
    pub const fn all(radius: Radius) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_left: radius,
            bottom_right: radius,
        }
    }

    /// Create a border radius with all corners set to the same circular radius value.
    pub const fn circular(radius: f32) -> Self {
        Self::all(Radius::circular(radius))
    }

    /// Create a border radius with zero radii (no rounding).
    pub const ZERO: Self = Self::circular(0.0);

    /// Create a border radius with only the top corners rounded.
    pub const fn vertical_top(radius: Radius) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_left: Radius::ZERO,
            bottom_right: Radius::ZERO,
        }
    }

    /// Create a border radius with only the bottom corners rounded.
    pub const fn vertical_bottom(radius: Radius) -> Self {
        Self {
            top_left: Radius::ZERO,
            top_right: Radius::ZERO,
            bottom_left: radius,
            bottom_right: radius,
        }
    }

    /// Create a border radius with only the left corners rounded.
    pub const fn horizontal_left(radius: Radius) -> Self {
        Self {
            top_left: radius,
            top_right: Radius::ZERO,
            bottom_left: radius,
            bottom_right: Radius::ZERO,
        }
    }

    /// Create a border radius with only the right corners rounded.
    pub const fn horizontal_right(radius: Radius) -> Self {
        Self {
            top_left: Radius::ZERO,
            top_right: radius,
            bottom_left: Radius::ZERO,
            bottom_right: radius,
        }
    }

    /// Create a border radius with only the top-left corner rounded.
    pub const fn only_top_left(radius: Radius) -> Self {
        Self {
            top_left: radius,
            top_right: Radius::ZERO,
            bottom_left: Radius::ZERO,
            bottom_right: Radius::ZERO,
        }
    }

    /// Create a border radius with only the top-right corner rounded.
    pub const fn only_top_right(radius: Radius) -> Self {
        Self {
            top_left: Radius::ZERO,
            top_right: radius,
            bottom_left: Radius::ZERO,
            bottom_right: Radius::ZERO,
        }
    }

    /// Create a border radius with only the bottom-left corner rounded.
    pub const fn only_bottom_left(radius: Radius) -> Self {
        Self {
            top_left: Radius::ZERO,
            top_right: Radius::ZERO,
            bottom_left: radius,
            bottom_right: Radius::ZERO,
        }
    }

    /// Create a border radius with only the bottom-right corner rounded.
    pub const fn only_bottom_right(radius: Radius) -> Self {
        Self {
            top_left: Radius::ZERO,
            top_right: Radius::ZERO,
            bottom_left: Radius::ZERO,
            bottom_right: radius,
        }
    }

    /// Create a custom border radius with individual corner values.
    pub const fn new(
        top_left: Radius,
        top_right: Radius,
        bottom_left: Radius,
        bottom_right: Radius,
    ) -> Self {
        Self {
            top_left,
            top_right,
            bottom_left,
            bottom_right,
        }
    }

    /// Check if all corners are zero.
    pub fn is_zero(&self) -> bool {
        self.top_left.is_zero()
            && self.top_right.is_zero()
            && self.bottom_left.is_zero()
            && self.bottom_right.is_zero()
    }

    /// Check if all corners have the same radius.
    pub fn is_uniform(&self) -> bool {
        self.top_left == self.top_right
            && self.top_left == self.bottom_left
            && self.top_left == self.bottom_right
    }

    /// Get the uniform radius if all corners are the same.
    pub fn uniform_radius(&self) -> Option<Radius> {
        if self.is_uniform() {
            Some(self.top_left)
        } else {
            None
        }
    }

    /// Multiply all radii by a scalar.
    pub fn scale(&self, factor: f32) -> Self {
        Self {
            top_left: self.top_left * factor,
            top_right: self.top_right * factor,
            bottom_left: self.bottom_left * factor,
            bottom_right: self.bottom_right * factor,
        }
    }

    /// Clamp all radii to be non-negative.
    pub fn clamp_non_negative(&self) -> Self {
        Self {
            top_left: self.top_left.clamp_non_negative(),
            top_right: self.top_right.clamp_non_negative(),
            bottom_left: self.bottom_left.clamp_non_negative(),
            bottom_right: self.bottom_right.clamp_non_negative(),
        }
    }

}

impl From<f32> for BorderRadius {
    fn from(value: f32) -> Self {
        Self::circular(value)
    }
}

impl From<Radius> for BorderRadius {
    fn from(radius: Radius) -> Self {
        Self::all(radius)
    }
}


impl std::ops::Mul<f32> for BorderRadius {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        self.scale(rhs)
    }
}

impl std::ops::Neg for BorderRadius {
    type Output = Self;

    fn neg(self) -> Self::Output {
        Self {
            top_left: -self.top_left,
            top_right: -self.top_right,
            bottom_left: -self.bottom_left,
            bottom_right: -self.bottom_right,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_radius_creation() {
        let circular = Radius::circular(10.0);
        assert_eq!(circular.x, 10.0);
        assert_eq!(circular.y, 10.0);
        assert!(circular.is_circular());
        assert!(!circular.is_elliptical());

        let elliptical = Radius::elliptical(10.0, 5.0);
        assert_eq!(elliptical.x, 10.0);
        assert_eq!(elliptical.y, 5.0);
        assert!(!elliptical.is_circular());
        assert!(elliptical.is_elliptical());

        let zero = Radius::ZERO;
        assert!(zero.is_zero());
    }

    #[test]
    fn test_radius_dimensions() {
        let radius = Radius::elliptical(10.0, 5.0);
        assert_eq!(radius.min_dimension(), 5.0);
        assert_eq!(radius.max_dimension(), 10.0);
    }

    #[test]
    fn test_radius_operations() {
        let radius = Radius::circular(10.0);

        let scaled = radius.scale(2.0);
        assert_eq!(scaled, Radius::circular(20.0));

        let product = radius * 2.0;
        assert_eq!(product, Radius::circular(20.0));

        let negated = -radius;
        assert_eq!(negated, Radius::circular(-10.0));
    }

    #[test]
    fn test_radius_conversions() {
        let from_f32: Radius = 10.0.into();
        assert_eq!(from_f32, Radius::circular(10.0));

        let from_tuple: Radius = (10.0, 5.0).into();
        assert_eq!(from_tuple, Radius::elliptical(10.0, 5.0));
    }

    #[test]
    fn test_border_radius_creation() {
        let all = BorderRadius::all(Radius::circular(10.0));
        assert_eq!(all.top_left, Radius::circular(10.0));
        assert_eq!(all.top_right, Radius::circular(10.0));
        assert_eq!(all.bottom_left, Radius::circular(10.0));
        assert_eq!(all.bottom_right, Radius::circular(10.0));
        assert!(all.is_uniform());

        let circular = BorderRadius::circular(10.0);
        assert_eq!(circular, all);

        let zero = BorderRadius::ZERO;
        assert!(zero.is_zero());
    }

    #[test]
    fn test_border_radius_partial() {
        let top = BorderRadius::vertical_top(Radius::circular(10.0));
        assert_eq!(top.top_left, Radius::circular(10.0));
        assert_eq!(top.top_right, Radius::circular(10.0));
        assert!(top.bottom_left.is_zero());
        assert!(top.bottom_right.is_zero());

        let bottom = BorderRadius::vertical_bottom(Radius::circular(10.0));
        assert!(bottom.top_left.is_zero());
        assert!(bottom.top_right.is_zero());
        assert_eq!(bottom.bottom_left, Radius::circular(10.0));
        assert_eq!(bottom.bottom_right, Radius::circular(10.0));

        let left = BorderRadius::horizontal_left(Radius::circular(10.0));
        assert_eq!(left.top_left, Radius::circular(10.0));
        assert!(left.top_right.is_zero());
        assert_eq!(left.bottom_left, Radius::circular(10.0));
        assert!(left.bottom_right.is_zero());

        let right = BorderRadius::horizontal_right(Radius::circular(10.0));
        assert!(right.top_left.is_zero());
        assert_eq!(right.top_right, Radius::circular(10.0));
        assert!(right.bottom_left.is_zero());
        assert_eq!(right.bottom_right, Radius::circular(10.0));
    }

    #[test]
    fn test_border_radius_only() {
        let tl = BorderRadius::only_top_left(Radius::circular(10.0));
        assert_eq!(tl.top_left, Radius::circular(10.0));
        assert!(tl.top_right.is_zero());
        assert!(tl.bottom_left.is_zero());
        assert!(tl.bottom_right.is_zero());

        let tr = BorderRadius::only_top_right(Radius::circular(10.0));
        assert!(tr.top_left.is_zero());
        assert_eq!(tr.top_right, Radius::circular(10.0));

        let bl = BorderRadius::only_bottom_left(Radius::circular(10.0));
        assert_eq!(bl.bottom_left, Radius::circular(10.0));

        let br = BorderRadius::only_bottom_right(Radius::circular(10.0));
        assert_eq!(br.bottom_right, Radius::circular(10.0));
    }

    #[test]
    fn test_border_radius_uniform() {
        let uniform = BorderRadius::circular(10.0);
        assert!(uniform.is_uniform());
        assert_eq!(uniform.uniform_radius(), Some(Radius::circular(10.0)));

        let non_uniform = BorderRadius::new(
            Radius::circular(10.0),
            Radius::circular(5.0),
            Radius::circular(10.0),
            Radius::circular(10.0),
        );
        assert!(!non_uniform.is_uniform());
        assert_eq!(non_uniform.uniform_radius(), None);
    }

    #[test]
    fn test_border_radius_operations() {
        let radius = BorderRadius::circular(10.0);

        let scaled = radius.scale(2.0);
        assert_eq!(scaled, BorderRadius::circular(20.0));

        let product = radius * 2.0;
        assert_eq!(product, BorderRadius::circular(20.0));

        let negated = -radius;
        assert_eq!(negated, BorderRadius::circular(-10.0));
    }

    #[test]
    fn test_border_radius_clamp() {
        let negative = BorderRadius::new(
            Radius::circular(-5.0),
            Radius::circular(10.0),
            Radius::circular(-3.0),
            Radius::circular(2.0),
        );

        let clamped = negative.clamp_non_negative();
        assert_eq!(clamped.top_left, Radius::ZERO);
        assert_eq!(clamped.top_right, Radius::circular(10.0));
        assert_eq!(clamped.bottom_left, Radius::ZERO);
        assert_eq!(clamped.bottom_right, Radius::circular(2.0));
    }

    #[test]
    fn test_border_radius_conversions() {
        let from_f32: BorderRadius = 10.0.into();
        assert_eq!(from_f32, BorderRadius::circular(10.0));

        let from_radius: BorderRadius = Radius::circular(10.0).into();
        assert_eq!(from_radius, BorderRadius::circular(10.0));
    }

}
