//! Geometry types - points, rectangles, sizes, offsets

pub mod matrix4;
pub mod offset;
pub mod point;
pub mod rect;
pub mod rrect;
pub mod size;
pub mod text_path;




pub use matrix4::Matrix4;
pub use offset::Offset;
pub use point::Point;
pub use rect::Rect;
pub use rrect::RRect;
pub use size::Size;

// Re-export text path helpers for convenient access
pub use text_path::{
    CharTransform, arc_position, wave_offset, spiral_position,
    wave_rotation, vertical_scale, grid_position,
    bezier_point, bezier_tangent_rotation, parametric_position,
};




