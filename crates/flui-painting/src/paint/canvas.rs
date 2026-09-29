//! Canvas painting primitives.

/// How a shader (gradient or image) samples beyond its defined bounds.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TileMode {
    /// Edge colors are extended (clamped) beyond the bounds.
    ///
    /// Samples outside the gradient or image use the nearest edge color.
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
    /// Outside the bounds of the gradient or shader, transparent pixels are
    /// rendered.
    Decal,
}

/// Styles for blur effects in a mask filter.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BlurStyle {
    /// Blur inside and outside the shape.
    ///
    /// This is the typical Gaussian blur applied across the shape's edge.
    #[default]
    Normal,

    /// Draw solid inside the shape, blur outside.
    ///
    /// The interior of the shape is drawn opaque, while the exterior is
    /// blurred.
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

/// Quality levels for sampling images and scaled textures.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FilterQuality {
    /// Nearest neighbor sampling.
    ///
    /// Fastest, but lowest quality. Good for pixel art.
    None,

    /// Bilinear interpolation.
    ///
    /// A reasonable quality/performance trade-off; the default.
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

/// Whether to paint a shape's interior, or its outline.
#[derive(Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PaintingStyle {
    /// Fill the interior of the shape with the paint's color.
    #[default]
    Fill,

    /// Stroke the outline of the shape with the paint's color.
    Stroke,
}

/// Boolean operations for combining two paths.
#[derive(Debug)]
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

    /// Create a path that includes the areas covered by either path, but not
    /// both.
    ///
    /// This is equivalent to a boolean XOR operation.
    Xor,

    /// Subtract the first path from the second path.
    ///
    /// Results in the area of the second path that is not covered by the first.
    ReverseDifference,
}

/// Fill rules that determine which regions of a path are inside it.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PathFillType {
    /// The non-zero winding rule.
    ///
    /// A point is inside the path if the signed sum of edge crossings
    /// (winding number) is non-zero.
    #[default]
    NonZero,

    /// The even-odd fill rule.
    ///
    /// A point is inside the path if the number of edge crossings is odd.
    EvenOdd,
}

/// Styles for the endings of unclosed stroked contours.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
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

/// Styles for the joints between stroked line segments.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum StrokeJoin {
    /// Join line segments with a sharp corner, extended to the miter limit.
    #[default]
    Miter,

    /// Join line segments with a rounded corner.
    Round,

    /// Join line segments with a beveled corner.
    Bevel,
}

/// How a list of vertices is interpreted when drawing a triangle mesh.
#[derive(Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum VertexMode {
    /// Draw each sequence of three vertices as a separate triangle.
    ///
    /// Vertices 0, 1, 2 form a triangle, then 3, 4, 5, etc.
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

/// An opaque identifier for a texture registered with the rendering engine.
///
/// The raw value `0` is reserved as the null identifier (see `is_null`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextureId(u64);

impl TextureId {
    /// Creates a texture identifier wrapping the given raw id.
    #[must_use]
    #[inline]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Returns the raw `u64` value of this identifier.
    #[must_use]
    #[inline]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Returns `true` if this is the null identifier (raw value `0`).
    #[must_use]
    #[inline]
    pub const fn is_null(self) -> bool {
        self.0 == 0
    }
}

impl From<u64> for TextureId {
    #[inline]
    fn from(id: u64) -> Self {
        Self::new(id)
    }
}

impl From<TextureId> for u64 {
    #[inline]
    fn from(id: TextureId) -> Self {
        id.get()
    }
}

/// How a list of points is interpreted when drawn to a canvas.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PointMode {
    /// Draw each point as a separate dot.
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
    /// Returns `true` if this is `PointMode::Points`.
    #[must_use]
    #[inline]
    pub const fn is_points(&self) -> bool {
        matches!(self, PointMode::Points)
    }

    /// Returns `true` if this is `PointMode::Lines`.
    #[must_use]
    #[inline]
    pub const fn is_lines(&self) -> bool {
        matches!(self, PointMode::Lines)
    }

    /// Returns `true` if this is `PointMode::Polygon`.
    #[must_use]
    #[inline]
    pub const fn is_polygon(&self) -> bool {
        matches!(self, PointMode::Polygon)
    }

    /// Returns the minimum number of points needed to draw anything in this
    /// mode (1 for points, 2 for lines, 3 for a polygon).
    #[must_use]
    #[inline]
    pub const fn min_points(&self) -> usize {
        match self {
            PointMode::Points => 1,
            PointMode::Lines => 2,
            PointMode::Polygon => 3,
        }
    }
}
