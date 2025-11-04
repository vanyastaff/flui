//! Geometry types - points, rectangles, sizes, offsets

pub mod matrix4;
pub mod offset;
pub mod point;
pub mod rect;
pub mod rotation;
pub mod rrect;
pub mod size;
pub mod text_path;


pub use matrix4::Matrix4;
pub use offset::Offset;
pub use point::Point;
pub use rect::Rect;
pub use rotation::QuarterTurns;
pub use rrect::RRect;
pub use size::Size;

// Re-export text path helpers for convenient access
pub use text_path::{
    CharTransform, arc_position, bezier_point, bezier_tangent_rotation, grid_position,
    parametric_position, spiral_position, vertical_scale, wave_offset, wave_rotation,
};

