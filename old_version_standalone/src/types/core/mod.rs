//! Core primitive types.
//!
//! This module contains fundamental primitives used throughout the UI system:
//!
//! ## Geometric Types - 2D Basics
//! - [`Point`]: Absolute position in 2D space (x, y)
//! - [`Offset`]: Displacement/translation vector (dx, dy)
//! - [`Size`]: 2D dimensions (width, height)
//! - [`Scale`]: 2D scale factors (x, y)
//! - [`Rotation`]: Type-safe angle wrapper (degrees/radians)
//!
//! ## Geometric Types - Vectors
//! - [`Vector2`]: 2D vector for direction and magnitude
//! - [`Vector3`]: 3D vector for direction and magnitude
//!
//! ## Geometric Types - Shapes
//! - [`Rect`]: Axis-aligned rectangle (min, max points)
//! - [`RectCorners`]: Four corners of a rectangle
//! - [`Circle`]: Circle (center, radius)
//! - [`Arc`]: Arc (partial circle with start/sweep angles)
//! - [`Bounds`]: Center-based bounding box (center, extents)
//!
//! ## Geometric Types - Paths
//! - [`Path`]: Vector path composed of segments
//! - [`PathSegment`]: Line, curve, or move commands
//! - [`CubicBezier`]: Cubic Bezier curve (4 points)
//! - [`QuadraticBezier`]: Quadratic Bezier curve (3 points)
//!
//! ## Geometric Types - Ranges
//! - [`Range1D`]: 1D interval (start, end)
//! - [`Range2D`]: 2D interval (x range, y range)
//!
//! ## Geometric Types - Layout
//! - [`Position`]: CSS-like absolute positioning with optional edges
//! - [`Transform`]: Combined 2D transformation (translation, rotation, scale)
//!
//! ## Visual Primitives
//! - [`Color`]: Type-safe RGBA color wrapper
//! - [`Opacity`]: Clamped opacity value (0.0 to 1.0)
//!
//! ## Time
//! - [`Duration`]: Type-safe time duration wrapper

pub mod bounds;
pub mod circle;
pub mod color;
pub mod duration;
pub mod matrix4;
pub mod offset;
pub mod opacity;
pub mod path;
pub mod point;
pub mod position;
pub mod range;
pub mod rect;
pub mod rotation;
pub mod scale;
pub mod size;
pub mod transform;
pub mod vector;



// Re-export types for convenience
pub use bounds::Bounds;
pub use circle::{Arc, Circle};
pub use color::Color;
pub use duration::Duration;
pub use matrix4::Matrix4;
pub use offset::Offset;
pub use opacity::Opacity;
pub use path::{CubicBezier, Path, PathSegment, QuadraticBezier};
pub use point::Point;
pub use position::{Position, PositionedRect};
pub use range::{Range1D, Range2D};
pub use rect::{Rect, RectCorners};
pub use rotation::Rotation;
pub use scale::Scale;
pub use size::Size;
pub use transform::Transform;
pub use vector::{Vector2, Vector3};

/// Prelude module for convenient imports of commonly used core types
pub mod prelude {
    pub use super::{
        Color, Offset, Point, Rect, Size, Scale, Transform, Matrix4,
        Duration, Opacity, Rotation, Vector2, Vector3,
        Circle, Arc, Bounds, Path, Range1D, Range2D,
    };
}