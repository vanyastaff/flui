//! FractionalOffset - alignment using 0.0-1.0 coordinates
//!
//! Similar to Flutter's `FractionalOffset`. Unlike `Alignment` which uses
//! -1.0 to 1.0 coordinates, `FractionalOffset` uses 0.0 to 1.0 where
//! (0.0, 0.0) is the top-left corner.

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

    #[must_use]
    pub const fn new(dx: f32, dy: f32) -> Self {
        Self { dx, dy }
    }

    #[must_use]
    pub fn from_alignment(alignment: crate::layout::Alignment) -> Self {
        Self {
            dx: (alignment.x + 1.0) / 2.0,
            dy: (alignment.y + 1.0) / 2.0,
        }
    }

    #[must_use]
    pub fn to_alignment(&self) -> crate::layout::Alignment {
        crate::layout::Alignment::new(self.dx * 2.0 - 1.0, self.dy * 2.0 - 1.0)
    }

    #[must_use]
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        Self {
            dx: a.dx + (b.dx - a.dx) * t,
            dy: a.dy + (b.dy - a.dy) * t,
        }
    }

    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.dx.is_finite() && self.dy.is_finite()
    }

    #[must_use]
    pub fn negate(&self) -> Self {
        Self {
            dx: -self.dx,
            dy: -self.dy,
        }
    }
}

impl std::ops::Add for FractionalOffset {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            dx: self.dx + other.dx,
            dy: self.dy + other.dy,
        }
    }
}

impl std::ops::Sub for FractionalOffset {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self {
            dx: self.dx - other.dx,
            dy: self.dy - other.dy,
        }
    }
}

impl std::ops::Mul<f32> for FractionalOffset {
    type Output = Self;

    fn mul(self, factor: f32) -> Self {
        Self {
            dx: self.dx * factor,
            dy: self.dy * factor,
        }
    }
}

impl std::ops::Div<f32> for FractionalOffset {
    type Output = Self;

    fn div(self, divisor: f32) -> Self {
        Self {
            dx: self.dx / divisor,
            dy: self.dy / divisor,
        }
    }
}

impl std::ops::Neg for FractionalOffset {
    type Output = Self;

    fn neg(self) -> Self {
        self.negate()
    }
}
