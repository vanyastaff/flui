//! Border radius types for styling

use crate::styling::Radius;

#[derive(Copy, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
    /// Creates a border radius with all corners having the same circular radius.
    pub const fn circular(radius: f32) -> Self {
        Self::all(Radius::circular(radius))
    }

    /// Creates a border radius with all corners having the same elliptical radius.
    pub const fn elliptical(x: f32, y: f32) -> Self {
        Self::all(Radius::elliptical(x, y))
    }

    /// Creates a border radius with all corners having the same radius.
    pub const fn all(radius: Radius) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_left: radius,
            bottom_right: radius,
        }
    }

    /// Creates a border radius with only the specified corners having radii.
    pub const fn only(
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

    /// Creates a border radius with only the top-left corner having a radius.
    pub const fn top_left_only(radius: Radius) -> Self {
        Self {
            top_left: radius,
            top_right: Radius::ZERO,
            bottom_left: Radius::ZERO,
            bottom_right: Radius::ZERO,
        }
    }

    /// Creates a border radius with only the top-right corner having a radius.
    pub const fn top_right_only(radius: Radius) -> Self {
        Self {
            top_left: Radius::ZERO,
            top_right: radius,
            bottom_left: Radius::ZERO,
            bottom_right: Radius::ZERO,
        }
    }

    /// Creates a border radius with only the bottom-left corner having a radius.
    pub const fn bottom_left_only(radius: Radius) -> Self {
        Self {
            top_left: Radius::ZERO,
            top_right: Radius::ZERO,
            bottom_left: radius,
            bottom_right: Radius::ZERO,
        }
    }

    /// Creates a border radius with only the bottom-right corner having a radius.
    pub const fn bottom_right_only(radius: Radius) -> Self {
        Self {
            top_left: Radius::ZERO,
            top_right: Radius::ZERO,
            bottom_left: Radius::ZERO,
            bottom_right: radius,
        }
    }

    /// Creates a border radius with vertical (top and bottom) corners having the same radius.
    pub const fn vertical(top: Radius, bottom: Radius) -> Self {
        Self {
            top_left: top,
            top_right: top,
            bottom_left: bottom,
            bottom_right: bottom,
        }
    }

    /// Creates a border radius with horizontal (left and right) corners having the same radius.
    pub const fn horizontal(left: Radius, right: Radius) -> Self {
        Self {
            top_left: left,
            top_right: right,
            bottom_left: left,
            bottom_right: right,
        }
    }

    /// Creates a border radius for the top corners only.
    ///
    /// Bottom corners will have zero radius. Common pattern for cards, modals, and sheets.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::styling::{BorderRadius, Radius};
    ///
    /// let radius = BorderRadius::top(Radius::circular(16.0));
    /// ```
    pub const fn top(radius: Radius) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_left: Radius::ZERO,
            bottom_right: Radius::ZERO,
        }
    }

    /// Creates a border radius for the bottom corners only.
    ///
    /// Top corners will have zero radius. Common for bottom sheets and dropdowns.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::styling::{BorderRadius, Radius};
    ///
    /// let radius = BorderRadius::bottom(Radius::circular(16.0));
    /// ```
    pub const fn bottom(radius: Radius) -> Self {
        Self {
            top_left: Radius::ZERO,
            top_right: Radius::ZERO,
            bottom_left: radius,
            bottom_right: radius,
        }
    }

    /// Creates a "pill" border radius (fully rounded sides).
    ///
    /// Uses a very large radius (9999.0) to create pill-shaped elements.
    /// The actual rounding is clamped by the element's dimensions.
    ///
    /// # Examples
    ///
    /// ```
    /// use flui_types::styling::BorderRadius;
    ///
    /// // Perfect for buttons, tags, and badges
    /// let radius = BorderRadius::pill();
    /// ```
    pub const fn pill() -> Self {
        Self::circular(9999.0)
    }

    /// A border radius with zero radius on all corners.
    pub const ZERO: Self = Self {
        top_left: Radius::ZERO,
        top_right: Radius::ZERO,
        bottom_left: Radius::ZERO,
        bottom_right: Radius::ZERO,
    };

    /// Linearly interpolate between two border radii.
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            top_left: Radius::lerp(a.top_left, b.top_left, t),
            top_right: Radius::lerp(a.top_right, b.top_right, t),
            bottom_left: Radius::lerp(a.bottom_left, b.bottom_left, t),
            bottom_right: Radius::lerp(a.bottom_right, b.bottom_right, t),
        }
    }

    /// Returns a copy of this border radius with the top-left corner replaced.
    pub const fn with_top_left(self, top_left: Radius) -> Self {
        Self { top_left, ..self }
    }

    /// Returns a copy of this border radius with the top-right corner replaced.
    pub const fn with_top_right(self, top_right: Radius) -> Self {
        Self { top_right, ..self }
    }

    /// Returns a copy of this border radius with the bottom-left corner replaced.
    pub const fn with_bottom_left(self, bottom_left: Radius) -> Self {
        Self {
            bottom_left,
            ..self
        }
    }

    /// Returns a copy of this border radius with the bottom-right corner replaced.
    pub const fn with_bottom_right(self, bottom_right: Radius) -> Self {
        Self {
            bottom_right,
            ..self
        }
    }
}

impl Default for BorderRadius {
    fn default() -> Self {
        Self::ZERO
    }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BorderRadiusDirectional {
    /// The top-start corner radius.
    pub top_start: Radius,

    /// The top-end corner radius.
    pub top_end: Radius,

    /// The bottom-start corner radius.
    pub bottom_start: Radius,

    /// The bottom-end corner radius.
    pub bottom_end: Radius,
}

impl BorderRadiusDirectional {
    /// Creates a directional border radius with all corners having the same circular radius.
    pub const fn circular(radius: f32) -> Self {
        Self::all(Radius::circular(radius))
    }

    /// Creates a directional border radius with all corners having the same elliptical radius.
    pub const fn elliptical(x: f32, y: f32) -> Self {
        Self::all(Radius::elliptical(x, y))
    }

    /// Creates a directional border radius with all corners having the same radius.
    pub const fn all(radius: Radius) -> Self {
        Self {
            top_start: radius,
            top_end: radius,
            bottom_start: radius,
            bottom_end: radius,
        }
    }

    /// Creates a directional border radius with only the specified corners having radii.
    pub const fn only(
        top_start: Radius,
        top_end: Radius,
        bottom_start: Radius,
        bottom_end: Radius,
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
    /// * `ltr` - If true, uses left-to-right layout. If false, uses right-to-left.
    pub const fn resolve(self, ltr: bool) -> BorderRadius {
        if ltr {
            BorderRadius {
                top_left: self.top_start,
                top_right: self.top_end,
                bottom_left: self.bottom_start,
                bottom_right: self.bottom_end,
            }
        } else {
            BorderRadius {
                top_left: self.top_end,
                top_right: self.top_start,
                bottom_left: self.bottom_end,
                bottom_right: self.bottom_start,
            }
        }
    }

    /// Linearly interpolate between two directional border radii.
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
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
    fn default() -> Self {
        Self::ZERO
    }
}
