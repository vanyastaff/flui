//! Canvas painting primitives.

/// How a shader (gradient or image) samples beyond its defined bounds.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[expect(
    clippy::exhaustive_enums,
    reason = "every backend encodes each tile mode in its shader"
)]
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

/// Quality levels for sampling images and scaled textures.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(flui_foundation::inspect::Inspect)]
#[non_exhaustive]
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

/// Fill rules that determine which regions of a path are inside it.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[expect(
    clippy::exhaustive_enums,
    reason = "a path fills by the non-zero or the even-odd rule"
)]
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
#[expect(
    clippy::exhaustive_enums,
    reason = "a stroke ends butt, round or square"
)]
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
#[expect(
    clippy::exhaustive_enums,
    reason = "a stroke joins mitred, round or bevelled"
)]
pub enum StrokeJoin {
    /// Join line segments with a sharp corner, extended to the miter limit.
    #[default]
    Miter,

    /// Join line segments with a rounded corner.
    Round,

    /// Join line segments with a beveled corner.
    Bevel,
}

/// An opaque identifier for a texture registered with the rendering engine.
///
/// A texture registry issues it: flui-layer's `TextureRegistry`, from 1 up.
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
}

/// How a list of points is interpreted when drawn to a canvas.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[expect(
    clippy::exhaustive_enums,
    reason = "points draw as points, lines or a polygon"
)]
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
