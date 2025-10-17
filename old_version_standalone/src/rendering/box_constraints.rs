//! BoxConstraints - layout constraints for box protocol
//!
//! Similar to Flutter's BoxConstraints. Defines the min/max width/height
//! that a render object must satisfy during layout.

use crate::types::core::Size;
use std::fmt;

/// BoxConstraints - defines layout constraints
///
/// Similar to Flutter's BoxConstraints. Constraints are passed down the tree
/// during layout, and sizes are returned up.
///
/// # Layout Protocol
///
/// Parent sends constraints → Child returns size
///
/// # Examples
///
/// ```rust
/// use nebula_ui::rendering::BoxConstraints;
/// use nebula_ui::types::core::Size;
///
/// // Tight constraints (exact size)
/// let tight = BoxConstraints::tight(Size::new(100.0, 50.0));
/// assert!(tight.is_tight());
/// assert_eq!(tight.max_width, 100.0);
/// assert_eq!(tight.min_width, 100.0);
///
/// // Loose constraints (flexible size)
/// let loose = BoxConstraints::loose(Size::new(200.0, 100.0));
/// assert!(!loose.is_tight());
/// assert_eq!(loose.min_width, 0.0);
///
/// // Expand constraints (fill available space)
/// let expand = BoxConstraints::expand();
/// assert_eq!(expand.min_width, f32::INFINITY);
/// ```
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
    pub fn new(
        min_width: f32,
        max_width: f32,
        min_height: f32,
        max_height: f32,
    ) -> Self {
        debug_assert!(min_width >= 0.0 && min_width <= max_width);
        debug_assert!(min_height >= 0.0 && min_height <= max_height);

        Self {
            min_width,
            max_width,
            min_height,
            max_height,
        }
    }

    /// Tight constraints - exact size required
    ///
    /// min == max for both width and height
    pub fn tight(size: Size) -> Self {
        Self {
            min_width: size.width,
            max_width: size.width,
            min_height: size.height,
            max_height: size.height,
        }
    }

    /// Tight constraints for width only
    pub fn tight_for_width(width: f32) -> Self {
        Self {
            min_width: width,
            max_width: width,
            min_height: 0.0,
            max_height: f32::INFINITY,
        }
    }

    /// Tight constraints for height only
    pub fn tight_for_height(height: f32) -> Self {
        Self {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: height,
            max_height: height,
        }
    }

    /// Loose constraints - child can be any size up to max
    ///
    /// min = 0, max = size
    pub fn loose(size: Size) -> Self {
        Self {
            min_width: 0.0,
            max_width: size.width,
            min_height: 0.0,
            max_height: size.height,
        }
    }

    /// Expand constraints - child must fill available space
    ///
    /// min = max = INFINITY (will be clamped by parent)
    pub fn expand() -> Self {
        Self {
            min_width: f32::INFINITY,
            max_width: f32::INFINITY,
            min_height: f32::INFINITY,
            max_height: f32::INFINITY,
        }
    }

    /// Unbounded constraints - no limits
    pub fn unbounded() -> Self {
        Self {
            min_width: 0.0,
            max_width: f32::INFINITY,
            min_height: 0.0,
            max_height: f32::INFINITY,
        }
    }

    /// Check if constraints are tight (exact size)
    pub fn is_tight(&self) -> bool {
        self.min_width >= self.max_width && self.min_height >= self.max_height
    }

    /// Check if width is tight
    pub fn has_tight_width(&self) -> bool {
        self.min_width >= self.max_width
    }

    /// Check if height is tight
    pub fn has_tight_height(&self) -> bool {
        self.min_height >= self.max_height
    }

    /// Check if constraints allow unbounded width
    pub fn has_infinite_width(&self) -> bool {
        self.max_width >= f32::INFINITY
    }

    /// Check if constraints allow unbounded height
    pub fn has_infinite_height(&self) -> bool {
        self.max_height >= f32::INFINITY
    }

    /// Check if constraints have bounded width
    pub fn has_bounded_width(&self) -> bool {
        self.max_width < f32::INFINITY
    }

    /// Check if constraints have bounded height
    pub fn has_bounded_height(&self) -> bool {
        self.max_height < f32::INFINITY
    }

    /// Get the biggest size that satisfies these constraints
    pub fn biggest(&self) -> Size {
        Size::new(
            self.constrain_width(f32::INFINITY),
            self.constrain_height(f32::INFINITY),
        )
    }

    /// Get the smallest size that satisfies these constraints
    pub fn smallest(&self) -> Size {
        Size::new(self.min_width, self.min_height)
    }

    /// Constrain a width to these constraints
    pub fn constrain_width(&self, width: f32) -> f32 {
        width.clamp(self.min_width, self.max_width)
    }

    /// Constrain a height to these constraints
    pub fn constrain_height(&self, height: f32) -> f32 {
        height.clamp(self.min_height, self.max_height)
    }

    /// Constrain a size to these constraints
    pub fn constrain(&self, size: Size) -> Size {
        Size::new(
            self.constrain_width(size.width),
            self.constrain_height(size.height),
        )
    }

    /// Check if a size satisfies these constraints
    pub fn is_satisfied_by(&self, size: Size) -> bool {
        size.width >= self.min_width
            && size.width <= self.max_width
            && size.height >= self.min_height
            && size.height <= self.max_height
    }

    /// Enforce these constraints on another set of constraints
    pub fn enforce(&self, other: BoxConstraints) -> BoxConstraints {
        BoxConstraints {
            min_width: self.constrain_width(other.min_width),
            max_width: self.constrain_width(other.max_width),
            min_height: self.constrain_height(other.min_height),
            max_height: self.constrain_height(other.max_height),
        }
    }

    /// Tighten constraints by width
    pub fn tighten_width(mut self, width: f32) -> Self {
        self.min_width = self.min_width.max(width);
        self.max_width = self.max_width.min(width);
        self
    }

    /// Tighten constraints by height
    pub fn tighten_height(mut self, height: f32) -> Self {
        self.min_height = self.min_height.max(height);
        self.max_height = self.max_height.min(height);
        self
    }

    /// Loosen constraints (set mins to 0)
    pub fn loosen(mut self) -> Self {
        self.min_width = 0.0;
        self.min_height = 0.0;
        self
    }

    /// Deflate constraints by EdgeInsets (for padding)
    pub fn deflate_size(&self, width: f32, height: f32) -> BoxConstraints {
        let deflated_min_width = (self.min_width - width).max(0.0);
        let deflated_min_height = (self.min_height - height).max(0.0);

        BoxConstraints {
            min_width: deflated_min_width,
            max_width: (self.max_width - width).max(deflated_min_width),
            min_height: deflated_min_height,
            max_height: (self.max_height - height).max(deflated_min_height),
        }
    }

    /// Normalize constraints (ensure min <= max)
    pub fn normalize(mut self) -> Self {
        self.min_width = self.min_width.min(self.max_width);
        self.min_height = self.min_height.min(self.max_height);
        self
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
            write!(
                f,
                "BoxConstraints({}x{})",
                self.max_width, self.max_height
            )
        } else {
            write!(
                f,
                "BoxConstraints(w: {}..{}, h: {}..{})",
                self.min_width, self.max_width, self.min_height, self.max_height
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tight_constraints() {
        let constraints = BoxConstraints::tight(Size::new(100.0, 50.0));

        assert!(constraints.is_tight());
        assert!(constraints.has_tight_width());
        assert!(constraints.has_tight_height());
        assert_eq!(constraints.min_width, 100.0);
        assert_eq!(constraints.max_width, 100.0);
        assert_eq!(constraints.min_height, 50.0);
        assert_eq!(constraints.max_height, 50.0);
    }

    #[test]
    fn test_loose_constraints() {
        let constraints = BoxConstraints::loose(Size::new(200.0, 100.0));

        assert!(!constraints.is_tight());
        assert_eq!(constraints.min_width, 0.0);
        assert_eq!(constraints.max_width, 200.0);
        assert_eq!(constraints.min_height, 0.0);
        assert_eq!(constraints.max_height, 100.0);
    }

    #[test]
    fn test_expand_constraints() {
        let constraints = BoxConstraints::expand();

        assert_eq!(constraints.min_width, f32::INFINITY);
        assert_eq!(constraints.max_width, f32::INFINITY);
        assert!(constraints.has_infinite_width());
        assert!(constraints.has_infinite_height());
    }

    #[test]
    fn test_constrain_size() {
        let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);

        // Within bounds
        let size1 = Size::new(100.0, 60.0);
        assert_eq!(constraints.constrain(size1), size1);

        // Too small
        let size2 = Size::new(20.0, 10.0);
        assert_eq!(constraints.constrain(size2), Size::new(50.0, 30.0));

        // Too large
        let size3 = Size::new(200.0, 150.0);
        assert_eq!(constraints.constrain(size3), Size::new(150.0, 100.0));
    }

    #[test]
    fn test_is_satisfied_by() {
        let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);

        assert!(constraints.is_satisfied_by(Size::new(100.0, 60.0)));
        assert!(!constraints.is_satisfied_by(Size::new(20.0, 60.0))); // Too narrow
        assert!(!constraints.is_satisfied_by(Size::new(100.0, 150.0))); // Too tall
    }

    #[test]
    fn test_biggest_smallest() {
        let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);

        assert_eq!(constraints.biggest(), Size::new(150.0, 100.0));
        assert_eq!(constraints.smallest(), Size::new(50.0, 30.0));
    }

    #[test]
    fn test_tighten() {
        let constraints = BoxConstraints::new(0.0, 200.0, 0.0, 100.0);

        let tightened = constraints.tighten_width(100.0).tighten_height(50.0);

        assert_eq!(tightened.min_width, 100.0);
        assert_eq!(tightened.max_width, 100.0);
        assert_eq!(tightened.min_height, 50.0);
        assert_eq!(tightened.max_height, 50.0);
    }

    #[test]
    fn test_loosen() {
        let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);
        let loosened = constraints.loosen();

        assert_eq!(loosened.min_width, 0.0);
        assert_eq!(loosened.min_height, 0.0);
        assert_eq!(loosened.max_width, 150.0);
        assert_eq!(loosened.max_height, 100.0);
    }

    #[test]
    fn test_deflate() {
        let constraints = BoxConstraints::new(100.0, 200.0, 80.0, 150.0);
        let deflated = constraints.deflate_size(20.0, 30.0);

        assert_eq!(deflated.min_width, 80.0);  // 100 - 20
        assert_eq!(deflated.max_width, 180.0); // 200 - 20
        assert_eq!(deflated.min_height, 50.0); // 80 - 30
        assert_eq!(deflated.max_height, 120.0); // 150 - 30
    }

    #[test]
    fn test_tight_for_width() {
        let constraints = BoxConstraints::tight_for_width(100.0);

        assert!(constraints.has_tight_width());
        assert!(!constraints.has_tight_height());
        assert_eq!(constraints.min_width, 100.0);
        assert_eq!(constraints.max_width, 100.0);
    }

    #[test]
    fn test_tight_for_height() {
        let constraints = BoxConstraints::tight_for_height(50.0);

        assert!(!constraints.has_tight_width());
        assert!(constraints.has_tight_height());
        assert_eq!(constraints.min_height, 50.0);
        assert_eq!(constraints.max_height, 50.0);
    }

    #[test]
    fn test_display() {
        let tight = BoxConstraints::tight(Size::new(100.0, 50.0));
        assert_eq!(format!("{}", tight), "BoxConstraints(100x50)");

        let loose = BoxConstraints::new(0.0, 200.0, 0.0, 100.0);
        assert_eq!(format!("{}", loose), "BoxConstraints(w: 0..200, h: 0..100)");
    }
}
