//! Border radius types for styling
//!
//! This module provides [`BorderRadius`] as a type alias to
//! [`Corners<Radius>`](flui_foundation::geometry::Corners), offering ergonomic
//! constructors and methods for defining corner radii in UI elements.

use flui_foundation::geometry::{Corners, Radius};

/// Border radius for all four corners of a rectangle.
///
/// This is a type alias to [`Corners<Radius>`](Corners), providing
/// convenient constructors for common border radius patterns.
///
/// # Examples
///
/// ```
/// use flui_foundation::geometry::Radius;
/// use flui_painting::styling::{BorderRadius, BorderRadiusExt};
///
/// // All corners with the same circular radius
/// let radius = BorderRadius::circular(16.0);
///
/// // Top corners only (for cards, modals)
/// let radius = BorderRadius::top(Radius::circular(16.0));
///
/// // Custom per-corner
/// let radius = BorderRadius::only(
///     Radius::circular(8.0),  // top-left
///     Radius::circular(16.0), // top-right
///     Radius::circular(8.0),  // bottom-right
///     Radius::circular(16.0), // bottom-left
/// );
/// ```
pub type BorderRadius = Corners<Radius<f64>>;

/// Extension trait providing BorderRadius-specific constructors and methods.
///
/// This trait is automatically implemented for [`BorderRadius`] (which is
/// [`Corners<Radius>`](Corners)) to provide ergonomic APIs that match
/// Flutter's BorderRadius.
pub trait BorderRadiusExt {
    /// Creates a border radius with all corners having the same circular
    /// radius.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_painting::styling::{BorderRadius, BorderRadiusExt};
    ///
    /// let radius = BorderRadius::circular(16.0);
    /// ```
    fn circular(radius: f64) -> Self;

    /// Creates a border radius with all corners having the same elliptical
    /// radius.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_painting::styling::{BorderRadius, BorderRadiusExt};
    ///
    /// let radius = BorderRadius::elliptical(20.0, 10.0);
    /// ```
    fn elliptical(x: f64, y: f64) -> Self;

    /// Creates a border radius with all corners having the same radius.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Radius;
    /// use flui_painting::styling::BorderRadius;
    ///
    /// let r = Radius::elliptical(20.0, 10.0);
    /// let radius = BorderRadius::all(r);
    /// ```
    fn all(radius: Radius<f64>) -> Self;

    /// Creates a border radius with only the specified corners having radii.
    fn only(
        top_left: Radius<f64>,
        top_right: Radius<f64>,
        bottom_right: Radius<f64>,
        bottom_left: Radius<f64>,
    ) -> Self;

    /// Creates a border radius with only the top-left corner having a radius.
    fn top_left_only(radius: Radius<f64>) -> Self;

    /// Creates a border radius with only the top-right corner having a radius.
    fn top_right_only(radius: Radius<f64>) -> Self;

    /// Creates a border radius with only the bottom-left corner having a
    /// radius.
    fn bottom_left_only(radius: Radius<f64>) -> Self;

    /// Creates a border radius with only the bottom-right corner having a
    /// radius.
    fn bottom_right_only(radius: Radius<f64>) -> Self;

    /// Creates a border radius with vertical (top and bottom) corners having
    /// the same radius.
    fn vertical(top: Radius<f64>, bottom: Radius<f64>) -> Self;

    /// Creates a border radius with horizontal (left and right) corners having
    /// the same radius.
    fn horizontal(left: Radius<f64>, right: Radius<f64>) -> Self;

    /// Creates a border radius for the top corners only.
    ///
    /// Bottom corners will have zero radius. Common pattern for cards, modals,
    /// and sheets.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Radius;
    /// use flui_painting::styling::BorderRadius;
    ///
    /// let radius = BorderRadius::top(Radius::circular(16.0));
    /// ```
    fn top(radius: Radius<f64>) -> Self;

    /// Creates a border radius for the bottom corners only.
    ///
    /// Top corners will have zero radius. Common for bottom sheets and
    /// dropdowns.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_foundation::geometry::Radius;
    /// use flui_painting::styling::BorderRadius;
    ///
    /// let radius = BorderRadius::bottom(Radius::circular(16.0));
    /// ```
    fn bottom(radius: Radius<f64>) -> Self;

    /// Creates a "pill" border radius (fully rounded sides).
    ///
    /// Uses a very large radius (9999.0) to create pill-shaped elements.
    /// The actual rounding is clamped by the element's dimensions.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_painting::styling::{BorderRadius, BorderRadiusExt};
    ///
    /// // Perfect for buttons, tags, and badges
    /// let radius = BorderRadius::pill();
    /// ```
    fn pill() -> Self;

    /// A border radius with zero radius on all corners.
    const ZERO: Self;

    /// Linearly interpolate between two border radii.
    fn lerp(a: Self, b: Self, t: f64) -> Self;

    /// Returns a copy of this border radius with the top-left corner replaced.
    fn with_top_left(self, top_left: Radius<f64>) -> Self;

    /// Returns a copy of this border radius with the top-right corner replaced.
    fn with_top_right(self, top_right: Radius<f64>) -> Self;

    /// Returns a copy of this border radius with the bottom-left corner
    /// replaced.
    fn with_bottom_left(self, bottom_left: Radius<f64>) -> Self;

    /// Returns a copy of this border radius with the bottom-right corner
    /// replaced.
    fn with_bottom_right(self, bottom_right: Radius<f64>) -> Self;
}

impl BorderRadiusExt for BorderRadius {
    #[inline]
    fn circular(radius: f64) -> Self {
        Self::all(Radius::circular(radius))
    }

    #[inline]
    fn elliptical(x: f64, y: f64) -> Self {
        Self::all(Radius::elliptical(x, y))
    }

    #[inline]
    fn all(radius: Radius<f64>) -> Self {
        Corners::all(radius)
    }

    #[inline]
    fn only(
        top_left: Radius<f64>,
        top_right: Radius<f64>,
        bottom_right: Radius<f64>,
        bottom_left: Radius<f64>,
    ) -> Self {
        Corners::new(top_left, top_right, bottom_right, bottom_left)
    }

    #[inline]
    fn top_left_only(radius: Radius<f64>) -> Self {
        Corners::new(radius, Radius::ZERO, Radius::ZERO, Radius::ZERO)
    }

    #[inline]
    fn top_right_only(radius: Radius<f64>) -> Self {
        Corners::new(Radius::ZERO, radius, Radius::ZERO, Radius::ZERO)
    }

    #[inline]
    fn bottom_left_only(radius: Radius<f64>) -> Self {
        Corners::new(Radius::ZERO, Radius::ZERO, Radius::ZERO, radius)
    }

    #[inline]
    fn bottom_right_only(radius: Radius<f64>) -> Self {
        Corners::new(Radius::ZERO, Radius::ZERO, radius, Radius::ZERO)
    }

    #[inline]
    fn vertical(top: Radius<f64>, bottom: Radius<f64>) -> Self {
        Corners::new(top, top, bottom, bottom)
    }

    #[inline]
    fn horizontal(left: Radius<f64>, right: Radius<f64>) -> Self {
        // `Corners::new` takes the corners clockwise from the top left.
        Corners::new(left, right, right, left)
    }

    #[inline]
    fn top(radius: Radius<f64>) -> Self {
        Corners::new(radius, radius, Radius::ZERO, Radius::ZERO)
    }

    #[inline]
    fn bottom(radius: Radius<f64>) -> Self {
        Corners::new(Radius::ZERO, Radius::ZERO, radius, radius)
    }

    #[inline]
    fn pill() -> Self {
        Self::circular(9999.0)
    }

    const ZERO: Self = Corners {
        top_left: Radius::ZERO,
        top_right: Radius::ZERO,
        bottom_right: Radius::ZERO,
        bottom_left: Radius::ZERO,
    };

    #[inline]
    fn lerp(a: Self, b: Self, t: f64) -> Self {
        let t = t.clamp(0.0, 1.0);
        Corners::new(
            Radius::lerp(a.top_left, b.top_left, t),
            Radius::lerp(a.top_right, b.top_right, t),
            Radius::lerp(a.bottom_right, b.bottom_right, t),
            Radius::lerp(a.bottom_left, b.bottom_left, t),
        )
    }

    #[inline]
    fn with_top_left(self, top_left: Radius<f64>) -> Self {
        Corners { top_left, ..self }
    }

    #[inline]
    fn with_top_right(self, top_right: Radius<f64>) -> Self {
        Corners { top_right, ..self }
    }

    #[inline]
    fn with_bottom_left(self, bottom_left: Radius<f64>) -> Self {
        Corners {
            bottom_left,
            ..self
        }
    }

    #[inline]
    fn with_bottom_right(self, bottom_right: Radius<f64>) -> Self {
        Corners {
            bottom_right,
            ..self
        }
    }
}

/// Directional border radius that supports both LTR and RTL text directions.
///
/// Unlike [`BorderRadius`] which uses physical corners (top-left, top-right,
/// etc.), `BorderRadiusDirectional` uses logical corners (top-start, top-end)
/// that adapt to text direction.
///
/// # Examples
///
/// ```
/// use flui_foundation::geometry::Radius;
/// use flui_painting::styling::BorderRadiusDirectional;
///
/// let directional = BorderRadiusDirectional::circular(16.0);
///
/// // Resolve to physical corners based on text direction
/// let ltr_radius = directional.resolve(true); // LTR
/// let rtl_radius = directional.resolve(false); // RTL
/// ```
#[derive(Copy, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BorderRadiusDirectional {
    /// The top-start corner radius.
    pub top_start: Radius<f64>,

    /// The top-end corner radius.
    pub top_end: Radius<f64>,

    /// The bottom-start corner radius.
    pub bottom_start: Radius<f64>,

    /// The bottom-end corner radius.
    pub bottom_end: Radius<f64>,
}

impl BorderRadiusDirectional {
    /// Creates a directional border radius with all corners having the same
    /// circular radius.
    #[inline]
    pub fn circular(radius: f64) -> Self {
        Self::all(Radius::circular(radius))
    }

    /// Creates a directional border radius with all corners having the same
    /// elliptical radius.
    #[inline]
    pub fn elliptical(x: f64, y: f64) -> Self {
        Self::all(Radius::elliptical(x, y))
    }

    /// Creates a directional border radius with all corners having the same
    /// radius.
    #[inline]
    pub const fn all(radius: Radius<f64>) -> Self {
        Self {
            top_start: radius,
            top_end: radius,
            bottom_start: radius,
            bottom_end: radius,
        }
    }

    /// Creates a directional border radius with only the specified corners
    /// having radii.
    #[inline]
    pub const fn only(
        top_start: Radius<f64>,
        top_end: Radius<f64>,
        bottom_start: Radius<f64>,
        bottom_end: Radius<f64>,
    ) -> Self {
        Self {
            top_start,
            top_end,
            bottom_start,
            bottom_end,
        }
    }

    /// A border radius with zero radius on all corners.
    pub const ZERO: Self = Self {
        top_start: Radius::ZERO,
        top_end: Radius::ZERO,
        bottom_start: Radius::ZERO,
        bottom_end: Radius::ZERO,
    };

    /// Converts this directional border radius to a regular border radius.
    ///
    /// # Arguments
    ///
    /// * `ltr` - If true, uses left-to-right layout. If false, uses
    ///   right-to-left.
    #[inline]
    pub const fn resolve(self, ltr: bool) -> BorderRadius {
        if ltr {
            Corners {
                top_left: self.top_start,
                top_right: self.top_end,
                bottom_left: self.bottom_start,
                bottom_right: self.bottom_end,
            }
        } else {
            Corners {
                top_left: self.top_end,
                top_right: self.top_start,
                bottom_left: self.bottom_end,
                bottom_right: self.bottom_start,
            }
        }
    }

    /// Linearly interpolate between two directional border radii.
    #[inline]
    pub fn lerp(a: Self, b: Self, t: f64) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            top_start: Radius::lerp(a.top_start, b.top_start, t),
            top_end: Radius::lerp(a.top_end, b.top_end, t),
            bottom_start: Radius::lerp(a.bottom_start, b.bottom_start, t),
            bottom_end: Radius::lerp(a.bottom_end, b.bottom_end, t),
        }
    }
}

impl Default for BorderRadiusDirectional {
    #[inline]
    fn default() -> Self {
        Self::ZERO
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(v: f64) -> Radius<f64> {
        Radius::circular(v)
    }

    /// `(top_left, top_right, bottom_right, bottom_left)`, with a distinct
    /// radius per argument so any swapped corner shows.
    fn corners(b: BorderRadius) -> [Radius<f64>; 4] {
        [b.top_left, b.top_right, b.bottom_right, b.bottom_left]
    }

    #[test]
    fn directional_resolves_start_and_end_per_direction() {
        let (a, b, c, d) = (r(1.0), r(2.0), r(3.0), r(4.0));
        let dir = BorderRadiusDirectional::only(a, b, c, d);
        assert_eq!(corners(dir.resolve(true)), [a, b, d, c]);
        assert_eq!(corners(dir.resolve(false)), [b, a, c, d]);

        assert_eq!(
            BorderRadiusDirectional::circular(1.0),
            BorderRadiusDirectional::all(a)
        );
        assert_eq!(
            BorderRadiusDirectional::elliptical(1.0, 2.0),
            BorderRadiusDirectional::all(Radius::elliptical(1.0, 2.0))
        );
        assert_eq!(
            BorderRadiusDirectional::default(),
            BorderRadiusDirectional::all(Radius::ZERO)
        );

        let other = BorderRadiusDirectional::only(r(3.0), r(4.0), r(5.0), r(6.0));
        assert_eq!(
            BorderRadiusDirectional::lerp(dir, other, 0.5),
            BorderRadiusDirectional::only(r(2.0), r(3.0), r(4.0), r(5.0))
        );
        assert_eq!(BorderRadiusDirectional::lerp(dir, other, 2.0), other);
    }
}
