//! Box constraints for 2D layout
//!
//! This module provides BoxConstraints for constraining widget sizes.

use crate::geometry::Size;
use crate::layout::EdgeInsets;
use std::fmt;

/// Box constraints for layout
///
/// Similar to Flutter's `BoxConstraints`. Defines minimum and maximum width and height
/// that a widget must satisfy during layout.
///
/// # Examples
///
/// ```
/// use flui_types::constraints::BoxConstraints;
/// use flui_types::Size;
///
/// // Tight constraints force exact size
/// let tight = BoxConstraints::tight(Size::new(100.0, 50.0));
/// assert!(tight.is_tight());
///
/// // Loose constraints allow any size up to max
/// let loose = BoxConstraints::loose(Size::new(200.0, 100.0));
/// assert_eq!(loose.min_width, 0.0);
/// assert_eq!(loose.max_width, 200.0);
///
/// // Constrain a size to fit within bounds
/// let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);
/// let size = constraints.constrain(Size::new(200.0, 150.0));
/// assert_eq!(size, Size::new(150.0, 100.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::constraints::BoxConstraints;
    ///
    /// let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);
    /// assert_eq!(constraints.min_width, 50.0);
    /// assert_eq!(constraints.max_width, 150.0);
    /// ```
    pub const fn new(min_width: f32, max_width: f32, min_height: f32, max_height: f32) -> Self {
        Self {
            min_width,
            max_width,
            min_height,
            max_height,
        }
    }

    /// Create tight constraints (min == max)
    ///
    /// Tight constraints force a widget to be exactly the specified size.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::constraints::BoxConstraints;
    /// use flui_types::Size;
    ///
    /// let size = Size::new(100.0, 50.0);
    /// let constraints = BoxConstraints::tight(size);
    /// assert!(constraints.is_tight());
    /// assert_eq!(constraints.biggest(), size);
    /// assert_eq!(constraints.smallest(), size);
    /// ```
    pub fn tight(size: Size) -> Self {
        Self {
            min_width: size.width,
            max_width: size.width,
            min_height: size.height,
            max_height: size.height,
        }
    }

    /// Create loose constraints (min = 0, max = size)
    ///
    /// Loose constraints allow a widget to be any size from zero up to the specified size.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::constraints::BoxConstraints;
    /// use flui_types::Size;
    ///
    /// let size = Size::new(100.0, 50.0);
    /// let constraints = BoxConstraints::loose(size);
    /// assert!(!constraints.is_tight());
    /// assert_eq!(constraints.min_width, 0.0);
    /// assert_eq!(constraints.max_width, 100.0);
    /// ```
    pub fn loose(size: Size) -> Self {
        Self {
            min_width: 0.0,
            max_width: size.width,
            min_height: 0.0,
            max_height: size.height,
        }
    }

    /// Create constraints with tight width
    ///
    /// Forces a specific width but allows any height.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::constraints::BoxConstraints;
    ///
    /// let constraints = BoxConstraints::tight_for_width(100.0);
    /// assert!(constraints.has_tight_width());
    /// assert!(!constraints.has_tight_height());
    /// ```
    pub const fn tight_for_width(width: f32) -> Self {
        Self {
            min_width: width,
            max_width: width,
            min_height: 0.0,
            max_height: f32::INFINITY,
        }
    }

    /// Create constraints with tight height
    ///
    /// Forces a specific height but allows any width.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::constraints::BoxConstraints;
    ///
    /// let constraints = BoxConstraints::tight_for_height(50.0);
    /// assert!(!constraints.has_tight_width());
    /// assert!(constraints.has_tight_height());
    /// ```
    pub const fn tight_for_height(height: f32) -> Self {
        Self {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: height,
            max_height: height,
        }
    }

    /// Create constraints that expand to fill available space
    ///
    /// Forces both width and height to be infinite (fill parent).
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::constraints::BoxConstraints;
    ///
    /// let constraints = BoxConstraints::expand();
    /// assert!(constraints.min_width.is_infinite());
    /// assert!(constraints.min_height.is_infinite());
    /// ```
    pub const fn expand() -> Self {
        Self {
            min_width: f32::INFINITY,
            max_width: f32::INFINITY,
            min_height: f32::INFINITY,
            max_height: f32::INFINITY,
        }
    }

    /// Check if width is tight (min == max)
    pub const fn has_tight_width(&self) -> bool {
        self.min_width == self.max_width
    }

    /// Check if height is tight (min == max)
    pub const fn has_tight_height(&self) -> bool {
        self.min_height == self.max_height
    }

    /// Check if both dimensions are tight
    pub const fn is_tight(&self) -> bool {
        self.has_tight_width() && self.has_tight_height()
    }

    /// Check if constraints are normalized (min <= max)
    pub fn is_normalized(&self) -> bool {
        self.min_width <= self.max_width && self.min_height <= self.max_height
    }

    /// Get the biggest size that satisfies the constraints
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::constraints::BoxConstraints;
    /// use flui_types::Size;
    ///
    /// let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);
    /// assert_eq!(constraints.biggest(), Size::new(150.0, 100.0));
    /// ```
    pub fn biggest(&self) -> Size {
        Size::new(
            self.constrain_width(f32::INFINITY),
            self.constrain_height(f32::INFINITY),
        )
    }

    /// Get the smallest size that satisfies the constraints
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::constraints::BoxConstraints;
    /// use flui_types::Size;
    ///
    /// let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);
    /// assert_eq!(constraints.smallest(), Size::new(50.0, 30.0));
    /// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::constraints::BoxConstraints;
    /// use flui_types::Size;
    ///
    /// let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);
    ///
    /// // Too small - clamped to minimum
    /// let size1 = constraints.constrain(Size::new(40.0, 20.0));
    /// assert_eq!(size1, Size::new(50.0, 30.0));
    ///
    /// // Too large - clamped to maximum
    /// let size2 = constraints.constrain(Size::new(200.0, 150.0));
    /// assert_eq!(size2, Size::new(150.0, 100.0));
    ///
    /// // Within bounds - unchanged
    /// let size3 = constraints.constrain(Size::new(100.0, 50.0));
    /// assert_eq!(size3, Size::new(100.0, 50.0));
    /// ```
    pub fn constrain(&self, size: Size) -> Size {
        Size::new(
            self.constrain_width(size.width),
            self.constrain_height(size.height),
        )
    }

    /// Check if a size satisfies the constraints
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::constraints::BoxConstraints;
    /// use flui_types::Size;
    ///
    /// let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);
    /// assert!(constraints.is_satisfied_by(Size::new(100.0, 50.0)));
    /// assert!(!constraints.is_satisfied_by(Size::new(40.0, 50.0))); // Width too small
    /// assert!(!constraints.is_satisfied_by(Size::new(200.0, 50.0))); // Width too large
    /// ```
    pub fn is_satisfied_by(&self, size: Size) -> bool {
        size.width >= self.min_width
            && size.width <= self.max_width
            && size.height >= self.min_height
            && size.height <= self.max_height
    }

    /// Tighten constraints by enforcing minimum constraints
    ///
    /// If width or height is specified, sets both min and max to that value.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::constraints::BoxConstraints;
    ///
    /// let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);
    /// let tightened = constraints.tighten(Some(100.0), None);
    /// assert_eq!(tightened.min_width, 100.0);
    /// assert_eq!(tightened.max_width, 100.0);
    /// assert_eq!(tightened.min_height, 30.0);
    /// ```
    pub fn tighten(&self, width: Option<f32>, height: Option<f32>) -> Self {
        Self {
            min_width: width.unwrap_or(self.min_width),
            max_width: width.unwrap_or(self.max_width),
            min_height: height.unwrap_or(self.min_height),
            max_height: height.unwrap_or(self.max_height),
        }
    }

    /// Loosen constraints by reducing minimums to zero
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::constraints::BoxConstraints;
    ///
    /// let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);
    /// let loosened = constraints.loosen();
    /// assert_eq!(loosened.min_width, 0.0);
    /// assert_eq!(loosened.min_height, 0.0);
    /// assert_eq!(loosened.max_width, 150.0);
    /// assert_eq!(loosened.max_height, 100.0);
    /// ```
    pub fn loosen(&self) -> Self {
        Self {
            min_width: 0.0,
            max_width: self.max_width,
            min_height: 0.0,
            max_height: self.max_height,
        }
    }

    /// Enforce constraints with specific width
    ///
    /// Clamps the width to be within current bounds, then makes it tight.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::constraints::BoxConstraints;
    ///
    /// let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);
    /// let enforced = constraints.enforce_width(100.0);
    /// assert!(enforced.has_tight_width());
    /// assert_eq!(enforced.min_width, 100.0);
    /// ```
    pub fn enforce_width(&self, width: f32) -> Self {
        let clamped = width.clamp(self.min_width, self.max_width);
        Self {
            min_width: clamped,
            max_width: clamped,
            min_height: self.min_height,
            max_height: self.max_height,
        }
    }

    /// Enforce constraints with specific height
    ///
    /// Clamps the height to be within current bounds, then makes it tight.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::constraints::BoxConstraints;
    ///
    /// let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);
    /// let enforced = constraints.enforce_height(50.0);
    /// assert!(enforced.has_tight_height());
    /// assert_eq!(enforced.min_height, 50.0);
    /// ```
    pub fn enforce_height(&self, height: f32) -> Self {
        let clamped = height.clamp(self.min_height, self.max_height);
        Self {
            min_width: self.min_width,
            max_width: self.max_width,
            min_height: clamped,
            max_height: clamped,
        }
    }

    /// Enforce additional constraints on top of current constraints
    ///
    /// Combines two sets of constraints, taking the most restrictive values.
    /// This is used by ConstrainedBox and SizedBox to apply additional constraints.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::constraints::BoxConstraints;
    ///
    /// // Incoming constraints: 0-200, 0-200
    /// let incoming = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);
    ///
    /// // Additional constraints: 50-150, 50-150
    /// let additional = BoxConstraints::new(50.0, 150.0, 50.0, 150.0);
    ///
    /// // Result: max(0,50)-min(200,150), max(0,50)-min(200,150) = 50-150, 50-150
    /// let enforced = incoming.enforce(additional);
    /// assert_eq!(enforced.min_width, 50.0);
    /// assert_eq!(enforced.max_width, 150.0);
    /// assert_eq!(enforced.min_height, 50.0);
    /// assert_eq!(enforced.max_height, 150.0);
    /// ```
    pub fn enforce(&self, other: BoxConstraints) -> Self {
        Self {
            min_width: self.min_width.max(other.min_width),
            max_width: self.max_width.min(other.max_width),
            min_height: self.min_height.max(other.min_height),
            max_height: self.max_height.min(other.max_height),
        }
    }

    /// Deflate constraints by subtracting from all sides
    ///
    /// Useful for implementing padding.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::constraints::BoxConstraints;
    ///
    /// let constraints = BoxConstraints::new(100.0, 200.0, 100.0, 200.0);
    /// let deflated = constraints.deflate_width(20.0).deflate_height(10.0);
    /// assert_eq!(deflated.max_width, 180.0);
    /// assert_eq!(deflated.max_height, 190.0);
    /// ```
    pub fn deflate_width(&self, amount: f32) -> Self {
        Self {
            min_width: (self.min_width - amount).max(0.0),
            max_width: (self.max_width - amount).max(0.0),
            min_height: self.min_height,
            max_height: self.max_height,
        }
    }

    /// Deflate constraints by subtracting from height
    pub fn deflate_height(&self, amount: f32) -> Self {
        Self {
            min_width: self.min_width,
            max_width: self.max_width,
            min_height: (self.min_height - amount).max(0.0),
            max_height: (self.max_height - amount).max(0.0),
        }
    }

    /// Deflate constraints by edge insets
    ///
    /// Useful for implementing padding - shrinks available space by the insets.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{constraints::BoxConstraints, layout::EdgeInsets};
    ///
    /// let constraints = BoxConstraints::new(100.0, 200.0, 100.0, 200.0);
    /// let padding = EdgeInsets::all(10.0);
    /// let deflated = constraints.deflate(&padding);
    /// assert_eq!(deflated.max_width, 180.0); // 200 - 20 (left+right)
    /// assert_eq!(deflated.max_height, 180.0); // 200 - 20 (top+bottom)
    /// ```
    pub fn deflate(&self, insets: &EdgeInsets) -> Self {
        Self {
            min_width: (self.min_width - insets.horizontal_total()).max(0.0),
            max_width: (self.max_width - insets.horizontal_total()).max(0.0),
            min_height: (self.min_height - insets.vertical_total()).max(0.0),
            max_height: (self.max_height - insets.vertical_total()).max(0.0),
        }
    }

    /// Inflate constraints by edge insets
    ///
    /// Opposite of deflate - increases available space by the insets.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::{constraints::BoxConstraints, layout::EdgeInsets};
    ///
    /// let constraints = BoxConstraints::new(100.0, 200.0, 100.0, 200.0);
    /// let padding = EdgeInsets::all(10.0);
    /// let inflated = constraints.inflate(&padding);
    /// assert_eq!(inflated.max_width, 220.0); // 200 + 20
    /// assert_eq!(inflated.max_height, 220.0); // 200 + 20
    /// ```
    pub fn inflate(&self, insets: &EdgeInsets) -> Self {
        Self {
            min_width: self.min_width + insets.horizontal_total(),
            max_width: self.max_width + insets.horizontal_total(),
            min_height: self.min_height + insets.vertical_total(),
            max_height: self.max_height + insets.vertical_total(),
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

    #[test]
    fn test_constraints_tighten() {
        let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);
        let tightened = constraints.tighten(Some(100.0), None);

        assert_eq!(tightened.min_width, 100.0);
        assert_eq!(tightened.max_width, 100.0);
        assert_eq!(tightened.min_height, 30.0);
        assert_eq!(tightened.max_height, 100.0);
    }

    #[test]
    fn test_constraints_enforce_width_height() {
        let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);

        let enforced_width = constraints.enforce_width(100.0);
        assert!(enforced_width.has_tight_width());
        assert_eq!(enforced_width.min_width, 100.0);
        assert_eq!(enforced_width.max_width, 100.0);

        let enforced_height = constraints.enforce_height(50.0);
        assert!(enforced_height.has_tight_height());
        assert_eq!(enforced_height.min_height, 50.0);
        assert_eq!(enforced_height.max_height, 50.0);
    }

    #[test]
    fn test_constraints_enforce() {
        let incoming = BoxConstraints::new(0.0, 200.0, 0.0, 200.0);
        let additional = BoxConstraints::new(50.0, 150.0, 50.0, 150.0);
        let enforced = incoming.enforce(additional);

        // Should take most restrictive: max(min) and min(max)
        assert_eq!(enforced.min_width, 50.0);
        assert_eq!(enforced.max_width, 150.0);
        assert_eq!(enforced.min_height, 50.0);
        assert_eq!(enforced.max_height, 150.0);
    }

    #[test]
    fn test_constraints_deflate() {
        let constraints = BoxConstraints::new(100.0, 200.0, 100.0, 200.0);
        let deflated = constraints.deflate_width(20.0).deflate_height(10.0);

        assert_eq!(deflated.min_width, 80.0);
        assert_eq!(deflated.max_width, 180.0);
        assert_eq!(deflated.min_height, 90.0);
        assert_eq!(deflated.max_height, 190.0);
    }

    #[test]
    fn test_constraints_display() {
        let tight = BoxConstraints::tight(Size::new(100.0, 50.0));
        assert_eq!(tight.to_string(), "BoxConstraints(tight: 100x50)");

        let loose = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);
        assert_eq!(
            loose.to_string(),
            "BoxConstraints(50 <= w <= 150, 30 <= h <= 100)"
        );
    }
}
