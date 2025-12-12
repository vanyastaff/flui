//! Box constraints for 2D cartesian layout

use std::fmt;

use flui_types::{Size, Offset};

/// Immutable layout constraints for box protocol
///
/// BoxConstraints describe the range of acceptable sizes for a box render object.
/// A box must choose a size that satisfies these constraints during layout.
///
/// # Constraints Model
/// - **min_width**: Minimum width (>= 0.0)
/// - **max_width**: Maximum width (>= min_width, can be INFINITY)
/// - **min_height**: Minimum height (>= 0.0)
/// - **max_height**: Maximum height (>= min_height, can be INFINITY)
///
/// # Examples
///
/// ```ignore
/// // Tight constraints (exact size)
/// let tight = BoxConstraints::tight(Size::new(100.0, 100.0));
/// assert_eq!(tight.min_width, 100.0);
/// assert_eq!(tight.max_width, 100.0);
///
/// // Loose constraints (can be any size)
/// let loose = BoxConstraints::loose(Size::new(200.0, 300.0));
/// assert_eq!(loose.min_width, 0.0);
/// assert_eq!(loose.max_width, 200.0);
///
/// // Expand constraints (fill available space)
/// let expand = BoxConstraints::expand(200.0, 300.0);
/// assert_eq!(expand.min_width, 200.0);
/// assert_eq!(expand.max_width, 200.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxConstraints {
    /// Minimum width the box must have
    pub min_width: f32,
    /// Maximum width the box can have
    pub max_width: f32,
    /// Minimum height the box must have
    pub min_height: f32,
    /// Maximum height the box can have
    pub max_height: f32,
}

impl BoxConstraints {
    /// Creates constraints with exact minimum and maximum values
    pub const fn new(min_width: f32, max_width: f32, min_height: f32, max_height: f32) -> Self {
        Self {
            min_width,
            max_width,
            min_height,
            max_height,
        }
    }

    /// Creates tight constraints (min == max)
    ///
    /// The box must be exactly this size.
    pub fn tight(size: Size) -> Self {
        Self {
            min_width: size.width,
            max_width: size.width,
            min_height: size.height,
            max_height: size.height,
        }
    }

    /// Creates tight constraints for a specific width
    pub fn tight_for_width(width: f32) -> Self {
        Self {
            min_width: width,
            max_width: width,
            min_height: 0.0,
            max_height: f32::INFINITY,
        }
    }

    /// Creates tight constraints for a specific height
    pub fn tight_for_height(height: f32) -> Self {
        Self {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: height,
            max_height: height,
        }
    }

    /// Creates loose constraints (min == 0)
    ///
    /// The box can be any size up to the maximum.
    pub fn loose(size: Size) -> Self {
        Self {
            min_width: 0.0,
            max_width: size.width,
            min_height: 0.0,
            max_height: size.height,
        }
    }

    /// Creates expand constraints (min == max == size)
    ///
    /// The box must fill the available space.
    pub fn expand(width: f32, height: f32) -> Self {
        Self {
            min_width: width,
            max_width: width,
            min_height: height,
            max_height: height,
        }
    }

    /// Creates constraints with infinite max bounds
    pub const fn unbounded() -> Self {
        Self {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: 0.0,
            max_height: f32::INFINITY,
        }
    }

    /// Returns whether the constraints require an exact width
    pub fn has_tight_width(&self) -> bool {
        self.min_width >= self.max_width
    }

    /// Returns whether the constraints require an exact height
    pub fn has_tight_height(&self) -> bool {
        self.min_height >= self.max_height
    }

    /// Returns whether the constraints are tight (exact size)
    pub fn is_tight(&self) -> bool {
        self.has_tight_width() && self.has_tight_height()
    }

    /// Returns whether width is bounded
    pub fn has_bounded_width(&self) -> bool {
        self.max_width < f32::INFINITY
    }

    /// Returns whether height is bounded
    pub fn has_bounded_height(&self) -> bool {
        self.max_height < f32::INFINITY
    }

    /// Returns whether both dimensions are bounded
    pub fn is_bounded(&self) -> bool {
        self.has_bounded_width() && self.has_bounded_height()
    }

    /// Returns the smallest size that satisfies the constraints
    pub fn smallest(&self) -> Size {
        Size::new(self.min_width, self.min_height)
    }

    /// Returns the largest size that satisfies the constraints
    pub fn biggest(&self) -> Size {
        Size::new(self.max_width, self.max_height)
    }

    /// Constrains a size to be within the valid range
    pub fn constrain(&self, size: Size) -> Size {
        Size::new(
            size.width.clamp(self.min_width, self.max_width),
            size.height.clamp(self.min_height, self.max_height),
        )
    }

    /// Constrains width only
    pub fn constrain_width(&self, width: f32) -> f32 {
        width.clamp(self.min_width, self.max_width)
    }

    /// Constrains height only
    pub fn constrain_height(&self, height: f32) -> f32 {
        height.clamp(self.min_height, self.max_height)
    }

    /// Returns whether a size satisfies the constraints
    pub fn is_satisfied_by(&self, size: Size) -> bool {
        size.width >= self.min_width
            && size.width <= self.max_width
            && size.height >= self.min_height
            && size.height <= self.max_height
    }

    /// Tightens the constraints by clamping to new bounds
    pub fn tighten(&self, width: Option<f32>, height: Option<f32>) -> Self {
        Self {
            min_width: width.map_or(self.min_width, |w| self.min_width.max(w)),
            max_width: width.map_or(self.max_width, |w| self.max_width.min(w)),
            min_height: height.map_or(self.min_height, |h| self.min_height.max(h)),
            max_height: height.map_or(self.max_height, |h| self.max_height.min(h)),
        }
    }

    /// Loosens the constraints by allowing smaller sizes
    pub fn loosen(&self) -> Self {
        Self {
            min_width: 0.0,
            max_width: self.max_width,
            min_height: 0.0,
            max_height: self.max_height,
        }
    }

    /// Enforces constraints with a maximum size
    pub fn enforce(&self, size: Size) -> Self {
        Self {
            min_width: self.min_width.clamp(0.0, size.width),
            max_width: self.max_width.clamp(0.0, size.width),
            min_height: self.min_height.clamp(0.0, size.height),
            max_height: self.max_height.clamp(0.0, size.height),
        }
    }

    /// Deflates constraints by the given amount (for padding)
    pub fn deflate(&self, offset: Offset) -> Self {
        let horizontal = offset.dx.abs() * 2.0;
        let vertical = offset.dy.abs() * 2.0;

        Self {
            min_width: (self.min_width - horizontal).max(0.0),
            max_width: (self.max_width - horizontal).max(0.0),
            min_height: (self.min_height - vertical).max(0.0),
            max_height: (self.max_height - vertical).max(0.0),
        }
    }

    /// Inflates constraints by the given amount
    pub fn inflate(&self, offset: Offset) -> Self {
        let horizontal = offset.dx.abs() * 2.0;
        let vertical = offset.dy.abs() * 2.0;

        Self {
            min_width: self.min_width + horizontal,
            max_width: if self.max_width == f32::INFINITY {
                f32::INFINITY
            } else {
                self.max_width + horizontal
            },
            min_height: self.min_height + vertical,
            max_height: if self.max_height == f32::INFINITY {
                f32::INFINITY
            } else {
                self.max_height + vertical
            },
        }
    }

    /// Normalizes constraints to ensure valid ranges
    pub fn normalize(&self) -> Self {
        Self {
            min_width: self.min_width.max(0.0),
            max_width: self.max_width.max(self.min_width),
            min_height: self.min_height.max(0.0),
            max_height: self.max_height.max(self.min_height),
        }
    }
}

impl Default for BoxConstraints {
    fn default() -> Self {
        Self::unbounded()
    }
}

impl fmt::Display for BoxConstraints {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_tight() {
            write!(f, "BoxConstraints({}x{})", self.min_width, self.min_height)
        } else {
            write!(
                f,
                "BoxConstraints({}<=w<={}, {}<=h<={})",
                self.min_width, self.max_width, self.min_height, self.max_height
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tight() {
        let constraints = BoxConstraints::tight(Size::new(100.0, 50.0));
        assert_eq!(constraints.min_width, 100.0);
        assert_eq!(constraints.max_width, 100.0);
        assert_eq!(constraints.min_height, 50.0);
        assert_eq!(constraints.max_height, 50.0);
        assert!(constraints.is_tight());
    }

    #[test]
    fn test_loose() {
        let constraints = BoxConstraints::loose(Size::new(200.0, 100.0));
        assert_eq!(constraints.min_width, 0.0);
        assert_eq!(constraints.max_width, 200.0);
        assert_eq!(constraints.min_height, 0.0);
        assert_eq!(constraints.max_height, 100.0);
        assert!(!constraints.is_tight());
    }

    #[test]
    fn test_constrain() {
        let constraints = BoxConstraints::new(50.0, 150.0, 50.0, 150.0);

        assert_eq!(constraints.constrain(Size::new(100.0, 100.0)), Size::new(100.0, 100.0));
        assert_eq!(constraints.constrain(Size::new(10.0, 10.0)), Size::new(50.0, 50.0));
        assert_eq!(constraints.constrain(Size::new(200.0, 200.0)), Size::new(150.0, 150.0));
    }

    #[test]
    fn test_is_satisfied_by() {
        let constraints = BoxConstraints::new(50.0, 150.0, 50.0, 150.0);

        assert!(constraints.is_satisfied_by(Size::new(100.0, 100.0)));
        assert!(!constraints.is_satisfied_by(Size::new(10.0, 100.0)));
        assert!(!constraints.is_satisfied_by(Size::new(100.0, 10.0)));
        assert!(!constraints.is_satisfied_by(Size::new(200.0, 100.0)));
    }
}
