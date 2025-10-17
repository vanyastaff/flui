//! Layout constraints
//!
//! This module provides BoxConstraints for layout.
//! Size is re-exported from flui_types.

use std::fmt;

// Re-export Size from flui_types for convenience
pub use flui_types::Size;

/// Box constraints for layout
///
/// Similar to Flutter's BoxConstraints. Defines minimum and maximum width and height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxConstraints {
    /// Minimum width
    pub min_width: f32,
    /// Maximum width
    pub max_width: f32,
    /// Minimum height
    pub min_height: f32,
    /// Maximum height
    pub max_height: f32,
}

impl BoxConstraints {
    /// Create new box constraints
    pub fn new(min_width: f32, max_width: f32, min_height: f32, max_height: f32) -> Self {
        Self {
            min_width,
            max_width,
            min_height,
            max_height,
        }
    }

    /// Create tight constraints (min == max)
    pub fn tight(size: Size) -> Self {
        Self {
            min_width: size.width,
            max_width: size.width,
            min_height: size.height,
            max_height: size.height,
        }
    }

    /// Create loose constraints (min = 0, max = size)
    pub fn loose(size: Size) -> Self {
        Self {
            min_width: 0.0,
            max_width: size.width,
            min_height: 0.0,
            max_height: size.height,
        }
    }

    /// Create constraints with tight width
    pub fn tight_for_width(width: f32) -> Self {
        Self {
            min_width: width,
            max_width: width,
            min_height: 0.0,
            max_height: f32::INFINITY,
        }
    }

    /// Create constraints with tight height
    pub fn tight_for_height(height: f32) -> Self {
        Self {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: height,
            max_height: height,
        }
    }

    /// Create constraints that expand to fill available space
    pub fn expand() -> Self {
        Self {
            min_width: f32::INFINITY,
            max_width: f32::INFINITY,
            min_height: f32::INFINITY,
            max_height: f32::INFINITY,
        }
    }

    /// Check if width is tight (min == max)
    pub fn has_tight_width(&self) -> bool {
        self.min_width == self.max_width
    }

    /// Check if height is tight (min == max)
    pub fn has_tight_height(&self) -> bool {
        self.min_height == self.max_height
    }

    /// Check if both dimensions are tight
    pub fn is_tight(&self) -> bool {
        self.has_tight_width() && self.has_tight_height()
    }

    /// Check if constraints are normalized (min <= max)
    pub fn is_normalized(&self) -> bool {
        self.min_width <= self.max_width && self.min_height <= self.max_height
    }

    /// Get the biggest size that satisfies the constraints
    pub fn biggest(&self) -> Size {
        Size::new(
            self.constrain_width(f32::INFINITY),
            self.constrain_height(f32::INFINITY),
        )
    }

    /// Get the smallest size that satisfies the constraints
    pub fn smallest(&self) -> Size {
        Size::new(self.min_width, self.min_height)
    }

    /// Constrain a width to be within min/max bounds
    pub fn constrain_width(&self, width: f32) -> f32 {
        width.clamp(self.min_width, self.max_width)
    }

    /// Constrain a height to be within min/max bounds
    pub fn constrain_height(&self, height: f32) -> f32 {
        height.clamp(self.min_height, self.max_height)
    }

    /// Constrain a size to be within the constraints
    pub fn constrain(&self, size: Size) -> Size {
        Size::new(
            self.constrain_width(size.width),
            self.constrain_height(size.height),
        )
    }

    /// Check if a size satisfies the constraints
    pub fn is_satisfied_by(&self, size: Size) -> bool {
        size.width >= self.min_width
            && size.width <= self.max_width
            && size.height >= self.min_height
            && size.height <= self.max_height
    }

    /// Tighten constraints by enforcing minimum constraints
    pub fn tighten(&self, width: Option<f32>, height: Option<f32>) -> Self {
        Self {
            min_width: width.unwrap_or(self.min_width),
            max_width: width.unwrap_or(self.max_width),
            min_height: height.unwrap_or(self.min_height),
            max_height: height.unwrap_or(self.max_height),
        }
    }

    /// Loosen constraints by reducing minimums to zero
    pub fn loosen(&self) -> Self {
        Self {
            min_width: 0.0,
            max_width: self.max_width,
            min_height: 0.0,
            max_height: self.max_height,
        }
    }

    /// Enforce constraints with specific width
    pub fn enforce_width(&self, width: f32) -> Self {
        Self {
            min_width: width.clamp(self.min_width, self.max_width),
            max_width: width.clamp(self.min_width, self.max_width),
            min_height: self.min_height,
            max_height: self.max_height,
        }
    }

    /// Enforce constraints with specific height
    pub fn enforce_height(&self, height: f32) -> Self {
        Self {
            min_width: self.min_width,
            max_width: self.max_width,
            min_height: height.clamp(self.min_height, self.max_height),
            max_height: height.clamp(self.min_height, self.max_height),
        }
    }
}

impl Default for BoxConstraints {
    fn default() -> Self {
        Self {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: 0.0,
            max_height: f32::INFINITY,
        }
    }
}

impl fmt::Display for BoxConstraints {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_tight() {
            write!(
                f,
                "BoxConstraints(tight: {}x{})",
                self.min_width, self.min_height
            )
        } else {
            write!(
                f,
                "BoxConstraints({} <= w <= {}, {} <= h <= {})",
                self.min_width, self.max_width, self.min_height, self.max_height
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constraints_tight() {
        let size = Size::new(100.0, 50.0);
        let constraints = BoxConstraints::tight(size);

        assert!(constraints.is_tight());
        assert!(constraints.has_tight_width());
        assert!(constraints.has_tight_height());
        assert_eq!(constraints.biggest(), size);
        assert_eq!(constraints.smallest(), size);
    }

    #[test]
    fn test_constraints_loose() {
        let size = Size::new(100.0, 50.0);
        let constraints = BoxConstraints::loose(size);

        assert!(!constraints.is_tight());
        assert_eq!(constraints.min_width, 0.0);
        assert_eq!(constraints.max_width, 100.0);
        assert_eq!(constraints.smallest(), Size::zero());
    }

    #[test]
    fn test_constraints_constrain() {
        let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);

        let size1 = Size::new(40.0, 20.0); // Too small
        let constrained1 = constraints.constrain(size1);
        assert_eq!(constrained1, Size::new(50.0, 30.0));

        let size2 = Size::new(200.0, 150.0); // Too large
        let constrained2 = constraints.constrain(size2);
        assert_eq!(constrained2, Size::new(150.0, 100.0));

        let size3 = Size::new(100.0, 50.0); // Within bounds
        let constrained3 = constraints.constrain(size3);
        assert_eq!(constrained3, size3);
    }

    #[test]
    fn test_constraints_is_satisfied_by() {
        let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);

        assert!(!constraints.is_satisfied_by(Size::new(40.0, 50.0))); // Width too small
        assert!(!constraints.is_satisfied_by(Size::new(100.0, 20.0))); // Height too small
        assert!(!constraints.is_satisfied_by(Size::new(200.0, 50.0))); // Width too large
        assert!(constraints.is_satisfied_by(Size::new(100.0, 50.0))); // Valid
    }

    #[test]
    fn test_constraints_loosen() {
        let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);
        let loosened = constraints.loosen();

        assert_eq!(loosened.min_width, 0.0);
        assert_eq!(loosened.min_height, 0.0);
        assert_eq!(loosened.max_width, 150.0);
        assert_eq!(loosened.max_height, 100.0);
    }

    #[test]
    fn test_constraints_expand() {
        let constraints = BoxConstraints::expand();
        assert!(constraints.min_width.is_infinite());
        assert!(constraints.max_width.is_infinite());
        assert!(constraints.min_height.is_infinite());
        assert!(constraints.max_height.is_infinite());
    }
}
