//! Canvas painting primitives.

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TileMode {
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

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum BlurStyle {
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

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum FilterQuality {
    /// Nearest neighbor sampling.
    ///
    /// Fastest, but lowest quality. Good for pixel art.
    None,

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

#[derive(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PaintingStyle {
    #[default]
    Fill,

    /// Stroke the outline of the shape with the paint's color.
    Stroke,
}

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

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PathFillType {
    #[default]
    NonZero,

    /// The even-odd fill rule.
    ///
    /// A point is inside the path if the number of edge crossings is odd.
    EvenOdd,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum StrokeCap {
    #[default]
    Butt,

    /// Begin and end contours with a semi-circle extension.
    Round,

    /// Begin and end contours with a half-square extension.
    Square,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum StrokeJoin {
    #[default]
    Miter,

    /// Join line segments with a rounded corner.
    Round,

    /// Join line segments with a beveled corner.
    Bevel,
}

#[derive(Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum VertexMode {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextureId(u64);

impl TextureId {
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    #[must_use]
    pub const fn is_null(self) -> bool {
        self.0 == 0
    }
}

impl From<u64> for TextureId {
    fn from(id: u64) -> Self {
        Self::new(id)
    }
}

impl From<TextureId> for u64 {
    fn from(id: TextureId) -> Self {
        id.get()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum PointMode {
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
    #[must_use]
    pub const fn is_points(&self) -> bool {
        matches!(self, PointMode::Points)
    }

    #[must_use]
    pub const fn is_lines(&self) -> bool {
        matches!(self, PointMode::Lines)
    }

    #[must_use]
    pub const fn is_polygon(&self) -> bool {
        matches!(self, PointMode::Polygon)
    }

    #[must_use]
    pub const fn min_points(&self) -> usize {
        match self {
            PointMode::Points => 1,
            PointMode::Lines => 2,
            PointMode::Polygon => 3,
        }
    }
}
