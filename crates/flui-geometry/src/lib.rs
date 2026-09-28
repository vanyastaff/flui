//! Geometry values for FLUI: points, offsets, sizes, rectangles, insets, radii and matrices.
//!
//! A logical length is a plain `f64`: `10.0` is ten logical pixels. The types are generic
//! only over their scalar, which defaults to `f64`; `i32` instantiations are the device-pixel
//! grid ([`DevicePoint`], [`DeviceSize`], [`DeviceRect`]). There are no unit wrappers
//! (ADR-0098): the scalar type itself keeps logical and device values apart.
//!
//! | Type | Meaning |
//! |------|---------|
//! | [`Point`] | a position |
//! | [`Offset`] | a displacement; `Point - Point = Offset` |
//! | [`Size`] | width and height |
//! | [`Rect`] | axis-aligned rectangle stored as `min`/`max` corners |
//! | [`Edges`] / [`EdgeInsets`] | per-side insets |
//! | [`RRect`] / [`Radius`] | rounded rectangle with elliptical corners |
//! | [`Matrix4`] | 4×4 transform (glam inside) |

// Math-crate idiom: single-letter coordinate names are the domain's vocabulary.
#![expect(clippy::many_single_char_names)]
#![deny(missing_docs)]

pub mod bounds;
pub mod circle;
pub mod corner;
pub mod corners;
pub mod edges;
/// Error types for geometry operations.
pub mod error;
pub mod keys;
pub mod lerp;
pub mod line;
pub mod matrix4;
pub mod offset;
pub mod point;
pub mod rect;
pub mod relative_rect;
/// Quarter-turn rotations.
pub mod rotation;
pub mod rrect;
pub mod rsuperellipse;
pub mod size;
pub mod traits;
pub mod transform;
pub mod vector;

/// Common imports.
pub mod prelude {
    pub use super::traits::{
        Along, Axis, FloatUnit, GeometryOps, Half, IsZero, NumericUnit, Sign, Unit,
    };
    pub use super::{
        bounds::Bounds,
        circle::Circle,
        error::GeometryError,
        line::Line,
        offset::Offset,
        point::{Point, point},
        rect::{Rect, rect},
        rrect::{RRect, Radius},
        size::{Size, size},
        vector::{Vec2, vec2},
    };
}

pub use bounds::{Bounds, bounds};
pub use circle::Circle;
pub use corner::Corner;
pub use corners::{Corners, corners};
pub use edges::{Edges, edges};
pub use error::GeometryError;
pub use keys::{canonical_bits, canonical_bits_f64};
pub use lerp::{Lerp, MaybeLerp};
pub use line::Line;
pub use matrix4::Matrix4;
pub use offset::Offset;
pub use point::{Point, point};
pub use rect::{Rect, rect};
pub use relative_rect::RelativeRect;
pub use rotation::QuarterTurns;
pub use rrect::{RRect, Radius};
pub use rsuperellipse::RSuperellipse;
pub use size::{Size, size};
pub use traits::{
    Along, ApproxEq, Axis, Double, FloatUnit, GeometryOps, Half, IsZero, NumericUnit, Sign, Unit,
};
pub use transform::Transform;
pub use vector::{Vec2, vec2};

/// A point on the device-pixel grid.
pub type DevicePoint = Point<i32>;

/// A size on the device-pixel grid.
pub type DeviceSize = Size<i32>;

/// A rectangle on the device-pixel grid.
pub type DeviceRect = Rect<i32>;

/// Padding and margin insets in logical pixels.
pub type EdgeInsets = Edges<f64>;
