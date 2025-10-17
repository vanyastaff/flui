//! Box constraints for layout system.
//!
//! Similar to Flutter's BoxConstraints, this defines size constraints for widgets.

use crate::types::core::Size;

/// Immutable layout constraints for rectangular boxes.
///
/// A size respects a BoxConstraints if:
/// - `min_width <= size.width <= max_width`
/// - `min_height <= size.height <= max_height`
///
/// # Examples
///
/// ```rust
/// use nebula_ui::types::layout::BoxConstraints;
/// use nebula_ui::types::core::Size;
///
/// // Tight constraints (fixed size)
/// let tight = BoxConstraints::tight(Size::new(100.0, 50.0));
/// assert_eq!(tight.min_width(), 100.0);
/// assert_eq!(tight.max_width(), 100.0);
///
/// // Loose constraints (maximum size)
/// let loose = BoxConstraints::loose(Size::new(200.0, 100.0));
/// assert_eq!(loose.min_width(), 0.0);
/// assert_eq!(loose.max_width(), 200.0);
///
/// // Expand to fill available space
/// let expand = BoxConstraints::new(0.0, f32::INFINITY, 0.0, f32::INFINITY);
/// assert!(expand.has_infinite_width());
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxConstraints {
    min_width: f32,
    max_width: f32,
    min_height: f32,
    max_height: f32,
}

impl BoxConstraints {
    /// Creates box constraints with the given constraints.
    ///
    /// # Panics
    ///
    /// Panics in debug mode if the constraints are invalid:
    /// - min_width > max_width
    /// - min_height > max_height
    /// - Any value is NaN
    pub fn new(min_width: f32, max_width: f32, min_height: f32, max_height: f32) -> Self {
        debug_assert!(
            min_width.is_finite() && min_width >= 0.0,
            "min_width must be non-negative and finite"
        );
        debug_assert!(
            max_width >= 0.0 && !max_width.is_nan(),
            "max_width must be non-negative"
        );
        debug_assert!(
            min_height.is_finite() && min_height >= 0.0,
            "min_height must be non-negative and finite"
        );
        debug_assert!(
            max_height >= 0.0 && !max_height.is_nan(),
            "max_height must be non-negative"
        );
        debug_assert!(min_width <= max_width, "min_width must be <= max_width");
        debug_assert!(min_height <= max_height, "min_height must be <= max_height");

        Self {
            min_width,
            max_width,
            min_height,
            max_height,
        }
    }

    /// Creates tight constraints with the given size.
    ///
    /// The widget must be exactly this size.
    ///
    /// # Example
    ///
    /// ```rust
    /// use nebula_ui::types::layout::BoxConstraints;
    /// use nebula_ui::types::core::Size;
    ///
    /// let constraints = BoxConstraints::tight(Size::new(100.0, 50.0));
    /// assert!(constraints.is_tight());
    /// assert_eq!(constraints.constrain(Size::new(200.0, 200.0)), Size::new(100.0, 50.0));
    /// ```
    pub fn tight(size: Size) -> Self {
        Self::new(size.width, size.width, size.height, size.height)
    }

    /// Creates tight constraints for the given width and height.
    ///
    /// # Example
    ///
    /// ```rust
    /// use nebula_ui::types::layout::BoxConstraints;
    ///
    /// let constraints = BoxConstraints::tight_for(Some(100.0), Some(50.0));
    /// assert_eq!(constraints.min_width(), 100.0);
    /// assert_eq!(constraints.max_width(), 100.0);
    /// ```
    pub fn tight_for(width: Option<f32>, height: Option<f32>) -> Self {
        Self::new(
            width.unwrap_or(0.0),
            width.unwrap_or(f32::INFINITY),
            height.unwrap_or(0.0),
            height.unwrap_or(f32::INFINITY),
        )
    }

    /// Creates loose constraints with the given size.
    ///
    /// The widget can be any size from zero up to the given size.
    ///
    /// # Example
    ///
    /// ```rust
    /// use nebula_ui::types::layout::BoxConstraints;
    /// use nebula_ui::types::core::Size;
    ///
    /// let constraints = BoxConstraints::loose(Size::new(200.0, 100.0));
    /// assert_eq!(constraints.min_width(), 0.0);
    /// assert_eq!(constraints.max_width(), 200.0);
    /// ```
    pub fn loose(size: Size) -> Self {
        Self::new(0.0, size.width, 0.0, size.height)
    }

    /// Creates constraints that expand to fill the available space.
    ///
    /// The widget will take up as much space as possible (infinite max),
    /// but can be smaller (zero min).
    ///
    /// # Example
    ///
    /// ```rust
    /// use nebula_ui::types::layout::BoxConstraints;
    ///
    /// let constraints = BoxConstraints::expand();
    /// assert!(constraints.has_infinite_width());
    /// assert!(constraints.has_infinite_height());
    /// assert_eq!(constraints.min_width(), 0.0);
    /// ```
    pub fn expand() -> Self {
        Self::new(0.0, f32::INFINITY, 0.0, f32::INFINITY)
    }

    /// Creates constraints with specific width and height, expanding in other dimensions.
    ///
    /// # Example
    ///
    /// ```rust
    /// use nebula_ui::types::layout::BoxConstraints;
    ///
    /// let constraints = BoxConstraints::expand_with(Some(100.0), None);
    /// assert_eq!(constraints.min_width(), 100.0);
    /// assert_eq!(constraints.max_width(), 100.0);
    /// assert!(constraints.has_infinite_height());
    /// ```
    pub fn expand_with(width: Option<f32>, height: Option<f32>) -> Self {
        Self::new(
            width.unwrap_or(0.0),
            width.unwrap_or(f32::INFINITY),
            height.unwrap_or(0.0),
            height.unwrap_or(f32::INFINITY),
        )
    }

    /// The minimum width that satisfies the constraints.
    pub fn min_width(&self) -> f32 {
        self.min_width
    }

    /// The maximum width that satisfies the constraints.
    pub fn max_width(&self) -> f32 {
        self.max_width
    }

    /// The minimum height that satisfies the constraints.
    pub fn min_height(&self) -> f32 {
        self.min_height
    }

    /// The maximum height that satisfies the constraints.
    pub fn max_height(&self) -> f32 {
        self.max_height
    }

    /// Whether there is exactly one width value that satisfies the constraints.
    pub fn has_tight_width(&self) -> bool {
        self.min_width >= self.max_width
    }

    /// Whether there is exactly one height value that satisfies the constraints.
    pub fn has_tight_height(&self) -> bool {
        self.min_height >= self.max_height
    }

    /// Whether there is exactly one size that satisfies the constraints.
    pub fn is_tight(&self) -> bool {
        self.has_tight_width() && self.has_tight_height()
    }

    /// Whether the constraints have infinite width.
    pub fn has_infinite_width(&self) -> bool {
        self.max_width.is_infinite()
    }

    /// Whether the constraints have infinite height.
    pub fn has_infinite_height(&self) -> bool {
        self.max_height.is_infinite()
    }

    /// Whether the constraints have bounded width.
    pub fn has_bounded_width(&self) -> bool {
        self.max_width.is_finite()
    }

    /// Whether the constraints have bounded height.
    pub fn has_bounded_height(&self) -> bool {
        self.max_height.is_finite()
    }

    /// Returns the size that both satisfies the constraints and is as close as possible to the given size.
    ///
    /// # Example
    ///
    /// ```rust
    /// use nebula_ui::types::layout::BoxConstraints;
    /// use nebula_ui::types::core::Size;
    ///
    /// let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);
    /// let size = constraints.constrain(Size::new(200.0, 200.0));
    /// assert_eq!(size, Size::new(150.0, 100.0)); // Clamped to max
    /// ```
    pub fn constrain(&self, size: Size) -> Size {
        Size::new(
            size.width.clamp(self.min_width, self.max_width),
            size.height.clamp(self.min_height, self.max_height),
        )
    }

    /// Returns the width that both satisfies the constraints and is as close as possible to the given width.
    pub fn constrain_width(&self, width: f32) -> f32 {
        width.clamp(self.min_width, self.max_width)
    }

    /// Returns the height that both satisfies the constraints and is as close as possible to the given height.
    pub fn constrain_height(&self, height: f32) -> f32 {
        height.clamp(self.min_height, self.max_height)
    }

    /// Returns the smallest size that satisfies the constraints.
    pub fn smallest(&self) -> Size {
        Size::new(self.min_width, self.min_height)
    }

    /// Returns the biggest size that satisfies the constraints.
    ///
    /// If the constraints are unbounded, returns the smallest size.
    pub fn biggest(&self) -> Size {
        Size::new(
            if self.max_width.is_finite() {
                self.max_width
            } else {
                self.min_width
            },
            if self.max_height.is_finite() {
                self.max_height
            } else {
                self.min_height
            },
        )
    }

    /// Returns new constraints with a tight width.
    pub fn tighten_width(&self, width: f32) -> Self {
        Self::new(
            width.max(self.min_width).min(self.max_width),
            width.max(self.min_width).min(self.max_width),
            self.min_height,
            self.max_height,
        )
    }

    /// Returns new constraints with a tight height.
    pub fn tighten_height(&self, height: f32) -> Self {
        Self::new(
            self.min_width,
            self.max_width,
            height.max(self.min_height).min(self.max_height),
            height.max(self.min_height).min(self.max_height),
        )
    }

    /// Returns new constraints with the width made loose (minimum width of zero).
    pub fn loosen_width(&self) -> Self {
        Self::new(0.0, self.max_width, self.min_height, self.max_height)
    }

    /// Returns new constraints with the height made loose (minimum height of zero).
    pub fn loosen_height(&self) -> Self {
        Self::new(self.min_width, self.max_width, 0.0, self.max_height)
    }

    /// Returns new constraints that are entirely loose (zero minimums).
    pub fn loosen(&self) -> Self {
        Self::new(0.0, self.max_width, 0.0, self.max_height)
    }

    /// Returns new constraints that remove the width constraints while preserving height.
    pub fn width_constraints(&self) -> Self {
        Self::new(self.min_width, self.max_width, 0.0, f32::INFINITY)
    }

    /// Returns new constraints that remove the height constraints while preserving width.
    pub fn height_constraints(&self) -> Self {
        Self::new(0.0, f32::INFINITY, self.min_height, self.max_height)
    }

    /// Returns new constraints with the maximum width and height constrained to the given size.
    pub fn constrain_dimensions(&self, width: f32, height: f32) -> Self {
        Self::new(
            self.min_width,
            self.max_width.min(width),
            self.min_height,
            self.max_height.min(height),
        )
    }

    /// Returns whether the given size satisfies the constraints.
    pub fn is_satisfied_by(&self, size: Size) -> bool {
        size.width >= self.min_width
            && size.width <= self.max_width
            && size.height >= self.min_height
            && size.height <= self.max_height
    }
}

impl Default for BoxConstraints {
    /// Creates unbounded constraints (zero minimums, infinite maximums).
    fn default() -> Self {
        Self::new(0.0, f32::INFINITY, 0.0, f32::INFINITY)
    }
}

impl std::fmt::Display for BoxConstraints {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_tight() {
            write!(
                f,
                "BoxConstraints(tight: {}x{})",
                self.min_width, self.min_height
            )
        } else {
            write!(
                f,
                "BoxConstraints(w: {}-{}, h: {}-{})",
                self.min_width, self.max_width, self.min_height, self.max_height
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_box_constraints_new() {
        let constraints = BoxConstraints::new(10.0, 100.0, 20.0, 200.0);
        assert_eq!(constraints.min_width(), 10.0);
        assert_eq!(constraints.max_width(), 100.0);
        assert_eq!(constraints.min_height(), 20.0);
        assert_eq!(constraints.max_height(), 200.0);
    }

    #[test]
    fn test_box_constraints_tight() {
        let size = Size::new(100.0, 50.0);
        let constraints = BoxConstraints::tight(size);
        assert!(constraints.is_tight());
        assert_eq!(constraints.min_width(), 100.0);
        assert_eq!(constraints.max_width(), 100.0);
        assert_eq!(constraints.min_height(), 50.0);
        assert_eq!(constraints.max_height(), 50.0);
    }

    #[test]
    fn test_box_constraints_tight_for() {
        let constraints = BoxConstraints::tight_for(Some(100.0), None);
        assert!(constraints.has_tight_width());
        assert!(!constraints.has_tight_height());
        assert_eq!(constraints.min_width(), 100.0);
        assert_eq!(constraints.max_width(), 100.0);
    }

    #[test]
    fn test_box_constraints_loose() {
        let size = Size::new(200.0, 100.0);
        let constraints = BoxConstraints::loose(size);
        assert_eq!(constraints.min_width(), 0.0);
        assert_eq!(constraints.max_width(), 200.0);
        assert_eq!(constraints.min_height(), 0.0);
        assert_eq!(constraints.max_height(), 100.0);
    }

    #[test]
    fn test_box_constraints_expand() {
        let constraints = BoxConstraints::expand();
        assert!(constraints.has_infinite_width());
        assert!(constraints.has_infinite_height());
        assert_eq!(constraints.min_width(), 0.0);
        assert_eq!(constraints.min_height(), 0.0);
    }

    #[test]
    fn test_box_constraints_expand_with() {
        let constraints = BoxConstraints::expand_with(Some(100.0), None);
        assert_eq!(constraints.min_width(), 100.0);
        assert_eq!(constraints.max_width(), 100.0);
        assert_eq!(constraints.min_height(), 0.0);
        assert!(constraints.has_infinite_height());
    }

    #[test]
    fn test_box_constraints_constrain() {
        let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);

        // Size within constraints
        let size1 = constraints.constrain(Size::new(100.0, 50.0));
        assert_eq!(size1, Size::new(100.0, 50.0));

        // Size too large
        let size2 = constraints.constrain(Size::new(200.0, 200.0));
        assert_eq!(size2, Size::new(150.0, 100.0));

        // Size too small
        let size3 = constraints.constrain(Size::new(10.0, 10.0));
        assert_eq!(size3, Size::new(50.0, 30.0));
    }

    #[test]
    fn test_box_constraints_smallest_biggest() {
        let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);
        assert_eq!(constraints.smallest(), Size::new(50.0, 30.0));
        assert_eq!(constraints.biggest(), Size::new(150.0, 100.0));
    }

    #[test]
    fn test_box_constraints_tighten() {
        let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);

        let tight_width = constraints.tighten_width(100.0);
        assert!(tight_width.has_tight_width());
        assert_eq!(tight_width.min_width(), 100.0);
        assert_eq!(tight_width.max_width(), 100.0);

        let tight_height = constraints.tighten_height(50.0);
        assert!(tight_height.has_tight_height());
        assert_eq!(tight_height.min_height(), 50.0);
        assert_eq!(tight_height.max_height(), 50.0);
    }

    #[test]
    fn test_box_constraints_loosen() {
        let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);

        let loose_width = constraints.loosen_width();
        assert_eq!(loose_width.min_width(), 0.0);
        assert_eq!(loose_width.max_width(), 150.0);

        let loose = constraints.loosen();
        assert_eq!(loose.min_width(), 0.0);
        assert_eq!(loose.min_height(), 0.0);
    }

    #[test]
    fn test_box_constraints_is_satisfied_by() {
        let constraints = BoxConstraints::new(50.0, 150.0, 30.0, 100.0);

        assert!(constraints.is_satisfied_by(Size::new(100.0, 50.0)));
        assert!(!constraints.is_satisfied_by(Size::new(30.0, 50.0))); // Too narrow
        assert!(!constraints.is_satisfied_by(Size::new(100.0, 20.0))); // Too short
        assert!(!constraints.is_satisfied_by(Size::new(200.0, 50.0))); // Too wide
    }

    #[test]
    fn test_box_constraints_default() {
        let constraints = BoxConstraints::default();
        assert_eq!(constraints.min_width(), 0.0);
        assert_eq!(constraints.min_height(), 0.0);
        assert!(constraints.has_infinite_width());
        assert!(constraints.has_infinite_height());
    }

    #[test]
    fn test_box_constraints_display() {
        let tight = BoxConstraints::tight(Size::new(100.0, 50.0));
        assert_eq!(format!("{}", tight), "BoxConstraints(tight: 100x50)");

        let loose = BoxConstraints::new(10.0, 100.0, 20.0, 200.0);
        assert_eq!(format!("{}", loose), "BoxConstraints(w: 10-100, h: 20-200)");
    }
}
