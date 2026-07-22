//! FractionalOffset - alignment using 0.0-1.0 coordinates
//!
//! Similar to Flutter's `FractionalOffset`. Unlike `Alignment` which uses
//! -1.0 to 1.0 coordinates, `FractionalOffset` uses 0.0 to 1.0 where
//! (0.0, 0.0) is the top-left corner.

/// An offset expressed as a fraction of a container's size.
///
/// Mirrors Flutter's `FractionalOffset`. Unlike `Alignment`, which is
/// centered at (0, 0), coordinates run from 0.0 to 1.0 with (0, 0) at
/// the top-left corner and (1, 1) at the bottom-right corner.
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FractionalOffset {
    /// The distance fraction in the horizontal direction.
    ///
    /// A value of 0.0 corresponds to the left edge, 1.0 to the right edge.
    pub dx: f32,

    /// The distance fraction in the vertical direction.
    ///
    /// A value of 0.0 corresponds to the top edge, 1.0 to the bottom edge.
    pub dy: f32,
}

impl FractionalOffset {
    /// The top-left corner (0.0, 0.0).
    pub const TOP_LEFT: Self = Self { dx: 0.0, dy: 0.0 };

    /// The top-center (0.5, 0.0).
    pub const TOP_CENTER: Self = Self { dx: 0.5, dy: 0.0 };

    /// The top-right corner (1.0, 0.0).
    pub const TOP_RIGHT: Self = Self { dx: 1.0, dy: 0.0 };

    /// The center-left (0.0, 0.5).
    pub const CENTER_LEFT: Self = Self { dx: 0.0, dy: 0.5 };

    /// The center (0.5, 0.5).
    pub const CENTER: Self = Self { dx: 0.5, dy: 0.5 };

    /// The center-right (1.0, 0.5).
    pub const CENTER_RIGHT: Self = Self { dx: 1.0, dy: 0.5 };

    /// The bottom-left corner (0.0, 1.0).
    pub const BOTTOM_LEFT: Self = Self { dx: 0.0, dy: 1.0 };

    /// The bottom-center (0.5, 1.0).
    pub const BOTTOM_CENTER: Self = Self { dx: 0.5, dy: 1.0 };

    /// The bottom-right corner (1.0, 1.0).
    pub const BOTTOM_RIGHT: Self = Self { dx: 1.0, dy: 1.0 };

    /// Creates a fractional offset with the given horizontal and
    /// vertical fractions.
    #[must_use]
    #[inline]
    pub const fn new(dx: f32, dy: f32) -> Self {
        Self { dx, dy }
    }

    /// Converts an `Alignment` (-1.0..=1.0 coordinates) into the
    /// equivalent fractional offset (0.0..=1.0 coordinates).
    #[must_use]
    #[inline]
    #[allow(clippy::manual_midpoint)]
    pub fn from_alignment(alignment: crate::layout::Alignment) -> Self {
        Self {
            dx: (alignment.x + 1.0) / 2.0,
            dy: (alignment.y + 1.0) / 2.0,
        }
    }

    /// Converts this fractional offset into the equivalent `Alignment`
    /// (-1.0..=1.0 coordinates); the inverse of
    /// [`from_alignment`](Self::from_alignment).
    #[must_use]
    #[inline]
    pub fn to_alignment(&self) -> crate::layout::Alignment {
        crate::layout::Alignment::new(self.dx * 2.0 - 1.0, self.dy * 2.0 - 1.0)
    }

    /// Linearly interpolates between two fractional offsets.
    ///
    /// `t == 0.0` returns `a`; `t == 1.0` returns `b`. Values of `t`
    /// outside `[0, 1]` extrapolate — they are **not** clamped,
    /// matching `Alignment::lerp`.
    #[must_use]
    #[inline]
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self {
            dx: a.dx + (b.dx - a.dx) * t,
            dy: a.dy + (b.dy - a.dy) * t,
        }
    }

    /// Returns `true` if both `dx` and `dy` are finite
    /// (neither infinite nor NaN).
    #[must_use]
    #[inline]
    pub fn is_finite(&self) -> bool {
        self.dx.is_finite() && self.dy.is_finite()
    }

    /// Returns the offset with both components negated
    /// (the `-` operator delegates to this).
    #[must_use]
    #[inline]
    pub fn negate(&self) -> Self {
        Self {
            dx: -self.dx,
            dy: -self.dy,
        }
    }
}

impl std::ops::Add for FractionalOffset {
    type Output = Self;

    #[inline]
    fn add(self, other: Self) -> Self {
        Self {
            dx: self.dx + other.dx,
            dy: self.dy + other.dy,
        }
    }
}

impl std::ops::Sub for FractionalOffset {
    type Output = Self;

    #[inline]
    fn sub(self, other: Self) -> Self {
        Self {
            dx: self.dx - other.dx,
            dy: self.dy - other.dy,
        }
    }
}

impl std::ops::Mul<f32> for FractionalOffset {
    type Output = Self;

    #[inline]
    fn mul(self, factor: f32) -> Self {
        Self {
            dx: self.dx * factor,
            dy: self.dy * factor,
        }
    }
}

impl std::ops::Div<f32> for FractionalOffset {
    type Output = Self;

    #[inline]
    fn div(self, divisor: f32) -> Self {
        Self {
            dx: self.dx / divisor,
            dy: self.dy / divisor,
        }
    }
}

impl std::ops::Neg for FractionalOffset {
    type Output = Self;

    #[inline]
    fn neg(self) -> Self {
        self.negate()
    }
}
