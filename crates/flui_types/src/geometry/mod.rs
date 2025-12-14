//! Geometry types for 2D graphics.
//!
//! This module provides fundamental geometry types:
//!
//! - [`Point`] - Absolute position in 2D space
//! - [`Vec2`] - Direction and magnitude (displacement vector)
//! - [`Size`] - Width and height dimensions
//! - [`Rect`] - Axis-aligned bounding rectangle
//! - [`RRect`] - Rounded rectangle
//! - [`Line`] - Line segment
//! - [`Circle`] - Circle with center and radius
//! - [`Offset`] - UI displacement (Flutter compatibility)
//! - [`Matrix4`] - 4x4 transformation matrix
//!
//! # Precision
//!
//! All geometry types use `f32` for GPU compatibility.
//! This matches Skia, Impeller, and other graphics APIs.
//!
//! # API Design
//!
//! API conventions follow kurbo/glam best practices:
//! - Constructors: `new()`, `splat()`, `from_array()`, `from_tuple()`
//! - Accessors: `to_array()`, `to_tuple()`, `with_x()`, `with_y()`
//! - Operations: `lerp()`, `min()`, `max()`, `clamp()`
//! - Rounding: `round()`, `ceil()`, `floor()`, `trunc()`, `expand()`
//! - Validation: `is_finite()`, `is_nan()`

pub mod bezier;
pub mod circle;
pub mod line;
pub mod matrix4;
pub mod offset;
pub mod point;
pub mod rect;
pub mod relative_rect;
pub mod rotation;
pub mod rrect;
pub mod size;
pub mod text_path;
pub mod transform;
pub mod vector;

// Core types
pub use bezier::{cubic_bez, quad_bez, CubicBez, QuadBez};
pub use circle::{circle, Circle};
pub use line::{line, Line};
pub use matrix4::Matrix4;
pub use offset::Offset;
pub use point::{point, Point};
pub use rect::{rect, Rect};
pub use relative_rect::RelativeRect;
pub use rotation::QuarterTurns;
pub use rrect::{RRect, Radius};
pub use size::{size, Size};
pub use transform::Transform;
pub use vector::{vec2, Vec2};

// Re-export text path helpers
pub use text_path::{
    arc_position, bezier_point, bezier_tangent_rotation, grid_position, parametric_position,
    spiral_position, vertical_scale, wave_offset, wave_rotation, CharTransform,
};
