//! Canvas painting primitives.

/// Defines how a shader or gradient tiles the plane.
///
/// Similar to Flutter's `TileMode` and CSS repeat modes.
///
/// # Examples
///
/// ```
/// use flui_types::painting::TileMode;
///
/// let mode = TileMode::Repeat;
/// assert_eq!(mode, TileMode::Repeat);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TileMode {
    /// Edge colors are clamped to the final color.
    ///
    /// The edge of the last tile is extended infinitely.
    #[default]
    Clamp,

    /// Edge colors repeat from the first color again.
    ///
    /// This is the most common tiling mode for patterns.
    Repeat,

    /// Edge colors repeat, but mirrored.
    ///
    /// On each repetition, the pattern is mirrored.
    Mirror,

    /// Edge colors are rendered as transparent.
    ///
    /// Outside the bounds of the gradient or shader, transparent pixels are rendered.
    Decal,
}

/// Styles to use for blurs in MaskFilter objects.
///
/// Similar to Flutter's `BlurStyle`.
///
/// # Examples
///
/// ```
/// use flui_types::painting::BlurStyle;
///
/// let style = BlurStyle::Normal;
/// assert_eq!(style, BlurStyle::Normal);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BlurStyle {
    /// Blur inside and outside the shape.
    ///
    /// This is the default blur style.
    #[default]
    Normal,

    /// Draw solid inside the shape, blur outside.
    ///
    /// The interior of the shape is drawn opaque, while the exterior is blurred.
    Solid,

    /// Draw nothing inside the shape, blur outside.
    ///
    /// Only the blur outside the shape is visible.
    Outer,

    /// Blur inside the shape, draw nothing outside.
    ///
    /// Only the blur inside the shape is visible.
    Inner,
}

/// Quality levels for image sampling.
///
/// Similar to Flutter's `FilterQuality`.
///
/// # Examples
///
/// ```
/// use flui_types::painting::FilterQuality;
///
/// let quality = FilterQuality::Medium;
/// assert_eq!(quality, FilterQuality::Medium);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FilterQuality {
    /// Nearest neighbor sampling.
    ///
    /// Fastest, but lowest quality. Good for pixel art.
    None,

    /// Bilinear interpolation.
    ///
    /// Better quality than nearest neighbor, still quite fast.
    #[default]
    Low,

    /// Bicubic interpolation.
    ///
    /// Higher quality, slower than bilinear.
    Medium,

    /// Best available quality.
    ///
    /// Slowest, but highest quality. Use for high-quality image rendering.
    High,
}

/// Strategies for painting shapes and paths.
///
/// Similar to Flutter's `PaintingStyle`.
///
/// # Examples
///
/// ```
/// use flui_types::painting::PaintingStyle;
///
/// let style = PaintingStyle::Fill;
/// assert_eq!(style, PaintingStyle::Fill);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PaintingStyle {
    /// Fill the shape with the paint's color.
    #[default]
    Fill,

    /// Stroke the outline of the shape with the paint's color.
    Stroke,
}

/// Strategies for combining paths.
///
/// Similar to Flutter's `PathOperation`.
///
/// # Examples
///
/// ```
/// use flui_types::painting::PathOperation;
///
/// let op = PathOperation::Union;
/// assert_eq!(op, PathOperation::Union);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PathOperation {
    /// Subtract the second path from the first path.
    ///
    /// Results in the area of the first path that is not covered by the second.
    Difference,

    /// Create a path that includes the areas covered by either path.
    ///
    /// This is equivalent to a boolean OR operation.
    Union,

    /// Create a path that includes only the areas covered by both paths.
    ///
    /// This is equivalent to a boolean AND operation.
    Intersect,

    /// Create a path that includes the areas covered by either path, but not both.
    ///
    /// This is equivalent to a boolean XOR operation.
    Xor,

    /// Subtract the first path from the second path.
    ///
    /// Results in the area of the second path that is not covered by the first.
    ReverseDifference,
}

/// Determines the winding rule for filling a path.
///
/// Similar to Flutter's `PathFillType`.
///
/// # Examples
///
/// ```
/// use flui_types::painting::PathFillType;
///
/// let fill_type = PathFillType::NonZero;
/// assert_eq!(fill_type, PathFillType::NonZero);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PathFillType {
    /// The non-zero fill rule.
    ///
    /// A point is inside the path if the sum of all edge crossings is non-zero.
    #[default]
    NonZero,

    /// The even-odd fill rule.
    ///
    /// A point is inside the path if the number of edge crossings is odd.
    EvenOdd,
}

/// Styles for ending a stroked line.
///
/// Similar to Flutter's `StrokeCap`.
///
/// # Examples
///
/// ```
/// use flui_types::painting::StrokeCap;
///
/// let cap = StrokeCap::Round;
/// assert_eq!(cap, StrokeCap::Round);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum StrokeCap {
    /// Begin and end contours with a flat edge and no extension.
    #[default]
    Butt,

    /// Begin and end contours with a semi-circle extension.
    Round,

    /// Begin and end contours with a half-square extension.
    Square,
}

/// Styles for joining two line segments.
///
/// Similar to Flutter's `StrokeJoin`.
///
/// # Examples
///
/// ```
/// use flui_types::painting::StrokeJoin;
///
/// let join = StrokeJoin::Round;
/// assert_eq!(join, StrokeJoin::Round);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum StrokeJoin {
    /// Join line segments with a sharp point.
    ///
    /// This is the default join style.
    #[default]
    Miter,

    /// Join line segments with a rounded corner.
    Round,

    /// Join line segments with a beveled corner.
    Bevel,
}

/// Defines how a list of points is interpreted when drawing.
///
/// Similar to Flutter's `VertexMode`.
///
/// # Examples
///
/// ```
/// use flui_types::painting::VertexMode;
///
/// let mode = VertexMode::Triangles;
/// assert_eq!(mode, VertexMode::Triangles);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum VertexMode {
    /// Draw each sequence of three vertices as an independent triangle.
    #[default]
    Triangles,

    /// Draw each sliding window of three vertices as a triangle.
    ///
    /// Vertices 0, 1, 2 form a triangle, then 1, 2, 3, then 2, 3, 4, etc.
    TriangleStrip,

    /// Draw all vertices as a single triangle fan.
    ///
    /// The first vertex is shared by all triangles.
    TriangleFan,
}

/// Point drawing mode for DrawPoints command
///
/// Defines how a sequence of points should be interpreted when drawing.
/// Similar to Flutter's `PointMode`.
///
/// # Examples
///
/// ```
/// use flui_types::painting::PointMode;
///
/// let mode = PointMode::Points;
/// assert!(mode.is_points());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PointMode {
    /// Draw each point as a separate dot
    ///
    /// Each point in the list is rendered as an individual dot with the
    /// stroke width determining the dot size.
    #[default]
    Points,

    /// Draw lines between consecutive points
    ///
    /// Points are connected pairwise: [p0, p1], [p2, p3], etc.
    /// If there's an odd number of points, the last point is ignored.
    Lines,

    /// Draw a closed polygon connecting all points
    ///
    /// All points are connected sequentially, and the last point is
    /// connected back to the first point to close the polygon.
    Polygon,
}

impl PointMode {
    /// Returns true if this mode draws individual points
    #[inline]
    #[must_use]
    pub const fn is_points(&self) -> bool {
        matches!(self, PointMode::Points)
    }

    /// Returns true if this mode draws lines
    #[inline]
    #[must_use]
    pub const fn is_lines(&self) -> bool {
        matches!(self, PointMode::Lines)
    }

    /// Returns true if this mode draws a polygon
    #[inline]
    #[must_use]
    pub const fn is_polygon(&self) -> bool {
        matches!(self, PointMode::Polygon)
    }

    /// Returns the minimum number of points required for this mode
    #[inline]
    #[must_use]
    pub const fn min_points(&self) -> usize {
        match self {
            PointMode::Points => 1,
            PointMode::Lines => 2,
            PointMode::Polygon => 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tile_mode_default() {
        assert_eq!(TileMode::default(), TileMode::Clamp);
    }

    #[test]
    fn test_tile_mode_variants() {
        assert_ne!(TileMode::Repeat, TileMode::Mirror);
        assert_ne!(TileMode::Clamp, TileMode::Decal);
    }

    #[test]
    fn test_blur_style_default() {
        assert_eq!(BlurStyle::default(), BlurStyle::Normal);
    }

    #[test]
    fn test_blur_style_variants() {
        assert_ne!(BlurStyle::Normal, BlurStyle::Solid);
        assert_ne!(BlurStyle::Outer, BlurStyle::Inner);
    }

    #[test]
    fn test_filter_quality_default() {
        assert_eq!(FilterQuality::default(), FilterQuality::Low);
    }

    #[test]
    fn test_filter_quality_variants() {
        assert_ne!(FilterQuality::None, FilterQuality::Low);
        assert_ne!(FilterQuality::Medium, FilterQuality::High);
    }

    #[test]
    fn test_painting_style_default() {
        assert_eq!(PaintingStyle::default(), PaintingStyle::Fill);
    }

    #[test]
    fn test_painting_style_variants() {
        assert_ne!(PaintingStyle::Fill, PaintingStyle::Stroke);
    }

    #[test]
    fn test_path_operation_variants() {
        assert_ne!(PathOperation::Union, PathOperation::Intersect);
        assert_ne!(PathOperation::Difference, PathOperation::ReverseDifference);
        assert_ne!(PathOperation::Xor, PathOperation::Union);
    }

    #[test]
    fn test_path_fill_type_default() {
        assert_eq!(PathFillType::default(), PathFillType::NonZero);
    }

    #[test]
    fn test_path_fill_type_variants() {
        assert_ne!(PathFillType::NonZero, PathFillType::EvenOdd);
    }

    #[test]
    fn test_stroke_cap_default() {
        assert_eq!(StrokeCap::default(), StrokeCap::Butt);
    }

    #[test]
    fn test_stroke_cap_variants() {
        assert_ne!(StrokeCap::Butt, StrokeCap::Round);
        assert_ne!(StrokeCap::Round, StrokeCap::Square);
    }

    #[test]
    fn test_stroke_join_default() {
        assert_eq!(StrokeJoin::default(), StrokeJoin::Miter);
    }

    #[test]
    fn test_stroke_join_variants() {
        assert_ne!(StrokeJoin::Miter, StrokeJoin::Round);
        assert_ne!(StrokeJoin::Round, StrokeJoin::Bevel);
    }

    #[test]
    fn test_vertex_mode_default() {
        assert_eq!(VertexMode::default(), VertexMode::Triangles);
    }

    #[test]
    fn test_vertex_mode_variants() {
        assert_ne!(VertexMode::Triangles, VertexMode::TriangleStrip);
        assert_ne!(VertexMode::TriangleStrip, VertexMode::TriangleFan);
    }

    #[test]
    fn test_point_mode_default() {
        assert_eq!(PointMode::default(), PointMode::Points);
    }

    #[test]
    fn test_point_mode_is_methods() {
        let points = PointMode::Points;
        assert!(points.is_points());
        assert!(!points.is_lines());
        assert!(!points.is_polygon());

        let lines = PointMode::Lines;
        assert!(!lines.is_points());
        assert!(lines.is_lines());
        assert!(!lines.is_polygon());

        let polygon = PointMode::Polygon;
        assert!(!polygon.is_points());
        assert!(!polygon.is_lines());
        assert!(polygon.is_polygon());
    }

    #[test]
    fn test_point_mode_min_points() {
        assert_eq!(PointMode::Points.min_points(), 1);
        assert_eq!(PointMode::Lines.min_points(), 2);
        assert_eq!(PointMode::Polygon.min_points(), 3);
    }
}
