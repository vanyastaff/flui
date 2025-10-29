//! Shape border types for styling

use crate::styling::{BorderRadius, BorderSide};

/// Base trait for shape borders.
///
/// Similar to Flutter's `ShapeBorder`.
pub trait ShapeBorder: std::fmt::Debug {
    /// Returns the outer edge of the border.
    fn scale(&self, t: f32) -> Box<dyn ShapeBorder>;
}

/// A rectangular border with rounded corners.
///
/// Similar to Flutter's `RoundedRectangleBorder`.
///
/// # Examples
///
/// ```
/// use flui_types::styling::{RoundedRectangleBorder, BorderRadius, BorderSide, Color};
///
/// let border = RoundedRectangleBorder::new(
///     BorderSide::new(Color::BLACK, 2.0, Default::default()),
///     BorderRadius::circular(10.0),
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RoundedRectangleBorder {
    /// The style of the border's edge.
    pub side: BorderSide,

    /// The radii for each corner.
    pub border_radius: BorderRadius,
}

impl RoundedRectangleBorder {
    /// Creates a rounded rectangle border.
    pub const fn new(side: BorderSide, border_radius: BorderRadius) -> Self {
        Self {
            side,
            border_radius,
        }
    }

    /// Creates a rounded rectangle border with circular corners.
    pub const fn circular(side: BorderSide, radius: f32) -> Self {
        Self {
            side,
            border_radius: BorderRadius::circular(radius),
        }
    }

    /// Linearly interpolate between two rounded rectangle borders.
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            side: BorderSide::lerp(a.side, b.side, t),
            border_radius: BorderRadius::lerp(a.border_radius, b.border_radius, t),
        }
    }
}

impl Default for RoundedRectangleBorder {
    fn default() -> Self {
        Self {
            side: BorderSide::NONE,
            border_radius: BorderRadius::ZERO,
        }
    }
}

/// A rectangular border with beveled corners.
///
/// Similar to Flutter's `BeveledRectangleBorder`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BeveledRectangleBorder {
    /// The style of the border's edge.
    pub side: BorderSide,

    /// The radii for each corner (used as bevel distances).
    pub border_radius: BorderRadius,
}

impl BeveledRectangleBorder {
    /// Creates a beveled rectangle border.
    pub const fn new(side: BorderSide, border_radius: BorderRadius) -> Self {
        Self {
            side,
            border_radius,
        }
    }

    /// Creates a beveled rectangle border with uniform bevel.
    pub const fn uniform(side: BorderSide, radius: f32) -> Self {
        Self {
            side,
            border_radius: BorderRadius::circular(radius),
        }
    }

    /// Linearly interpolate between two beveled rectangle borders.
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            side: BorderSide::lerp(a.side, b.side, t),
            border_radius: BorderRadius::lerp(a.border_radius, b.border_radius, t),
        }
    }
}

impl Default for BeveledRectangleBorder {
    fn default() -> Self {
        Self {
            side: BorderSide::NONE,
            border_radius: BorderRadius::ZERO,
        }
    }
}

/// A circular border.
///
/// Similar to Flutter's `CircleBorder`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CircleBorder {
    /// The style of the border's edge.
    pub side: BorderSide,

    /// The eccentricity of the circle (0.0 = circle, approaching 1.0 = line).
    pub eccentricity: f32,
}

impl CircleBorder {
    /// Creates a circle border.
    pub const fn new(side: BorderSide) -> Self {
        Self {
            side,
            eccentricity: 0.0,
        }
    }

    /// Creates a circle border with eccentricity.
    pub const fn with_eccentricity(side: BorderSide, eccentricity: f32) -> Self {
        Self { side, eccentricity }
    }

    /// Linearly interpolate between two circle borders.
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            side: BorderSide::lerp(a.side, b.side, t),
            eccentricity: a.eccentricity + (b.eccentricity - a.eccentricity) * t,
        }
    }
}

impl Default for CircleBorder {
    fn default() -> Self {
        Self {
            side: BorderSide::NONE,
            eccentricity: 0.0,
        }
    }
}

/// An oval border.
///
/// Similar to Flutter's `OvalBorder`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OvalBorder {
    /// The style of the border's edge.
    pub side: BorderSide,

    /// The eccentricity of the oval.
    pub eccentricity: f32,
}

impl OvalBorder {
    /// Creates an oval border.
    pub const fn new(side: BorderSide) -> Self {
        Self {
            side,
            eccentricity: 0.0,
        }
    }

    /// Creates an oval border with eccentricity.
    pub const fn with_eccentricity(side: BorderSide, eccentricity: f32) -> Self {
        Self { side, eccentricity }
    }

    /// Linearly interpolate between two oval borders.
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            side: BorderSide::lerp(a.side, b.side, t),
            eccentricity: a.eccentricity + (b.eccentricity - a.eccentricity) * t,
        }
    }
}

impl Default for OvalBorder {
    fn default() -> Self {
        Self {
            side: BorderSide::NONE,
            eccentricity: 0.0,
        }
    }
}

/// A stadium-shaped border (rectangle with semicircular ends).
///
/// Similar to Flutter's `StadiumBorder`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StadiumBorder {
    /// The style of the border's edge.
    pub side: BorderSide,
}

impl StadiumBorder {
    /// Creates a stadium border.
    pub const fn new(side: BorderSide) -> Self {
        Self { side }
    }

    /// Linearly interpolate between two stadium borders.
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            side: BorderSide::lerp(a.side, b.side, t),
        }
    }
}

impl Default for StadiumBorder {
    fn default() -> Self {
        Self {
            side: BorderSide::NONE,
        }
    }
}

/// A star-shaped border.
///
/// Similar to Flutter's `StarBorder`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StarBorder {
    /// The style of the border's edge.
    pub side: BorderSide,

    /// The number of points on the star.
    pub points: u32,

    /// The depth of the star's inner radius as a percentage of the outer radius.
    /// Valid range: 0.0 to 1.0.
    pub inner_radius_ratio: f32,

    /// The rotation of the star in radians.
    pub rotation: f32,

    /// The amount of rounding on the points.
    pub point_rounding: f32,

    /// The amount of rounding on the valleys between points.
    pub valley_rounding: f32,

    /// The squareness of the star (0.0 = smooth, 1.0 = sharp).
    pub squash: f32,
}

impl StarBorder {
    /// Creates a star border.
    pub const fn new(side: BorderSide, points: u32) -> Self {
        Self {
            side,
            points,
            inner_radius_ratio: 0.4,
            rotation: 0.0,
            point_rounding: 0.0,
            valley_rounding: 0.0,
            squash: 0.0,
        }
    }

    /// Creates a star border with all parameters.
    #[allow(clippy::too_many_arguments)]
    pub const fn with_params(
        side: BorderSide,
        points: u32,
        inner_radius_ratio: f32,
        rotation: f32,
        point_rounding: f32,
        valley_rounding: f32,
        squash: f32,
    ) -> Self {
        Self {
            side,
            points,
            inner_radius_ratio,
            rotation,
            point_rounding,
            valley_rounding,
            squash,
        }
    }

    /// Linearly interpolate between two star borders.
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            side: BorderSide::lerp(a.side, b.side, t),
            points: if t < 0.5 { a.points } else { b.points },
            inner_radius_ratio: a.inner_radius_ratio
                + (b.inner_radius_ratio - a.inner_radius_ratio) * t,
            rotation: a.rotation + (b.rotation - a.rotation) * t,
            point_rounding: a.point_rounding + (b.point_rounding - a.point_rounding) * t,
            valley_rounding: a.valley_rounding + (b.valley_rounding - a.valley_rounding) * t,
            squash: a.squash + (b.squash - a.squash) * t,
        }
    }
}

impl Default for StarBorder {
    fn default() -> Self {
        Self {
            side: BorderSide::NONE,
            points: 5,
            inner_radius_ratio: 0.4,
            rotation: 0.0,
            point_rounding: 0.0,
            valley_rounding: 0.0,
            squash: 0.0,
        }
    }
}

/// A continuous rectangular border with smooth corners.
///
/// Similar to Flutter's `ContinuousRectangleBorder`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ContinuousRectangleBorder {
    /// The style of the border's edge.
    pub side: BorderSide,

    /// The radii for each corner.
    pub border_radius: BorderRadius,
}

impl ContinuousRectangleBorder {
    /// Creates a continuous rectangle border.
    pub const fn new(side: BorderSide, border_radius: BorderRadius) -> Self {
        Self {
            side,
            border_radius,
        }
    }

    /// Creates a continuous rectangle border with circular corners.
    pub const fn circular(side: BorderSide, radius: f32) -> Self {
        Self {
            side,
            border_radius: BorderRadius::circular(radius),
        }
    }

    /// Linearly interpolate between two continuous rectangle borders.
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            side: BorderSide::lerp(a.side, b.side, t),
            border_radius: BorderRadius::lerp(a.border_radius, b.border_radius, t),
        }
    }
}

impl Default for ContinuousRectangleBorder {
    fn default() -> Self {
        Self {
            side: BorderSide::NONE,
            border_radius: BorderRadius::ZERO,
        }
    }
}

/// A rectangular border with linear sides.
///
/// Similar to Flutter's `LinearBorder`.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LinearBorder {
    /// The style of the border's edge.
    pub side: BorderSide,

    /// The edges to draw.
    pub edges: LinearBorderEdges,
}

impl LinearBorder {
    /// Creates a linear border.
    pub const fn new(side: BorderSide, edges: LinearBorderEdges) -> Self {
        Self { side, edges }
    }

    /// Creates a linear border with all edges.
    pub const fn all(side: BorderSide) -> Self {
        Self {
            side,
            edges: LinearBorderEdges::ALL,
        }
    }

    /// Linearly interpolate between two linear borders.
    pub fn lerp(a: Self, b: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self {
            side: BorderSide::lerp(a.side, b.side, t),
            edges: if t < 0.5 { a.edges } else { b.edges },
        }
    }
}

impl Default for LinearBorder {
    fn default() -> Self {
        Self {
            side: BorderSide::NONE,
            edges: LinearBorderEdges::ALL,
        }
    }
}

/// Defines which edges to draw for a LinearBorder.
///
/// Similar to Flutter's `LinearBorderEdge`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LinearBorderEdges {
    /// Whether to draw the top edge.
    pub top: bool,

    /// Whether to draw the right edge.
    pub right: bool,

    /// Whether to draw the bottom edge.
    pub bottom: bool,

    /// Whether to draw the left edge.
    pub left: bool,
}

impl LinearBorderEdges {
    /// All edges.
    pub const ALL: Self = Self {
        top: true,
        right: true,
        bottom: true,
        left: true,
    };

    /// No edges.
    pub const NONE: Self = Self {
        top: false,
        right: false,
        bottom: false,
        left: false,
    };

    /// Only top edge.
    pub const TOP: Self = Self {
        top: true,
        right: false,
        bottom: false,
        left: false,
    };

    /// Only right edge.
    pub const RIGHT: Self = Self {
        top: false,
        right: true,
        bottom: false,
        left: false,
    };

    /// Only bottom edge.
    pub const BOTTOM: Self = Self {
        top: false,
        right: false,
        bottom: true,
        left: false,
    };

    /// Only left edge.
    pub const LEFT: Self = Self {
        top: false,
        right: false,
        bottom: false,
        left: true,
    };

    /// Creates a new LinearBorderEdges.
    pub const fn new(top: bool, right: bool, bottom: bool, left: bool) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }
}

impl Default for LinearBorderEdges {
    fn default() -> Self {
        Self::ALL
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::styling::{BorderStyle, Color, Radius};

    fn test_side() -> BorderSide {
        BorderSide::new(Color::BLACK, 1.0, BorderStyle::Solid)
    }

    #[test]
    fn test_rounded_rectangle_border_new() {
        let border = RoundedRectangleBorder::new(test_side(), BorderRadius::circular(10.0));
        assert_eq!(border.side, test_side());
        assert_eq!(border.border_radius, BorderRadius::circular(10.0));
    }

    #[test]
    fn test_rounded_rectangle_border_circular() {
        let border = RoundedRectangleBorder::circular(test_side(), 10.0);
        assert_eq!(border.border_radius, BorderRadius::circular(10.0));
    }

    #[test]
    fn test_rounded_rectangle_border_lerp() {
        let a = RoundedRectangleBorder::circular(test_side(), 5.0);
        let b = RoundedRectangleBorder::circular(test_side(), 15.0);
        let mid = RoundedRectangleBorder::lerp(a, b, 0.5);
        assert_eq!(mid.border_radius.top_left, Radius::circular(10.0));
    }

    #[test]
    fn test_beveled_rectangle_border() {
        let border = BeveledRectangleBorder::uniform(test_side(), 10.0);
        assert_eq!(border.side, test_side());
    }

    #[test]
    fn test_circle_border_new() {
        let border = CircleBorder::new(test_side());
        assert_eq!(border.side, test_side());
        assert_eq!(border.eccentricity, 0.0);
    }

    #[test]
    fn test_circle_border_with_eccentricity() {
        let border = CircleBorder::with_eccentricity(test_side(), 0.5);
        assert_eq!(border.eccentricity, 0.5);
    }

    #[test]
    fn test_circle_border_lerp() {
        let a = CircleBorder::with_eccentricity(test_side(), 0.0);
        let b = CircleBorder::with_eccentricity(test_side(), 1.0);
        let mid = CircleBorder::lerp(a, b, 0.5);
        assert_eq!(mid.eccentricity, 0.5);
    }

    #[test]
    fn test_oval_border() {
        let border = OvalBorder::new(test_side());
        assert_eq!(border.side, test_side());
    }

    #[test]
    fn test_stadium_border() {
        let border = StadiumBorder::new(test_side());
        assert_eq!(border.side, test_side());
    }

    #[test]
    fn test_star_border_new() {
        let border = StarBorder::new(test_side(), 5);
        assert_eq!(border.points, 5);
        assert_eq!(border.inner_radius_ratio, 0.4);
    }

    #[test]
    fn test_star_border_with_params() {
        let border = StarBorder::with_params(test_side(), 6, 0.3, 1.0, 0.1, 0.2, 0.5);
        assert_eq!(border.points, 6);
        assert_eq!(border.inner_radius_ratio, 0.3);
        assert_eq!(border.rotation, 1.0);
    }

    #[test]
    fn test_star_border_lerp() {
        let a = StarBorder::new(test_side(), 5);
        let b = StarBorder::with_params(test_side(), 5, 0.6, 2.0, 0.2, 0.4, 1.0);
        let mid = StarBorder::lerp(a, b, 0.5);
        assert_eq!(mid.inner_radius_ratio, 0.5);
        assert_eq!(mid.rotation, 1.0);
    }

    #[test]
    fn test_continuous_rectangle_border() {
        let border = ContinuousRectangleBorder::circular(test_side(), 10.0);
        assert_eq!(border.side, test_side());
    }

    #[test]
    fn test_linear_border_new() {
        let border = LinearBorder::new(test_side(), LinearBorderEdges::ALL);
        assert_eq!(border.side, test_side());
        assert_eq!(border.edges, LinearBorderEdges::ALL);
    }

    #[test]
    fn test_linear_border_all() {
        let border = LinearBorder::all(test_side());
        assert_eq!(border.edges, LinearBorderEdges::ALL);
    }

    #[test]
    fn test_linear_border_edges_constants() {
        assert!(LinearBorderEdges::ALL.top);
        assert!(LinearBorderEdges::ALL.right);
        assert!(LinearBorderEdges::ALL.bottom);
        assert!(LinearBorderEdges::ALL.left);

        assert!(!LinearBorderEdges::NONE.top);
        assert!(!LinearBorderEdges::NONE.right);

        assert!(LinearBorderEdges::TOP.top);
        assert!(!LinearBorderEdges::TOP.bottom);
    }

    #[test]
    fn test_linear_border_lerp() {
        let a = LinearBorder::all(test_side());
        let b = LinearBorder::new(test_side(), LinearBorderEdges::TOP);
        let mid = LinearBorder::lerp(a, b, 0.5);
        // Should switch at t = 0.5
        assert_eq!(mid.edges, LinearBorderEdges::TOP);
    }
}
