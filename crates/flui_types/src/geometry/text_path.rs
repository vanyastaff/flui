//! Text path transformation helpers
//!
//! This module provides mathematical utilities for calculating character positions
//! along various paths (arc, wave, spiral, etc.). These are low-level primitives
//! for developers to build custom text effects.
//!
//! # Design Philosophy
//!
//! These functions:
//! - Provide **mathematical calculations only** (no rendering, no widgets)
//! - Return **position and rotation** for each character
//! - Allow developers to **build custom effects** by combining primitives
//! - Work with any text rendering system (not specific to Text widget)
//!
//! # Examples
//!
//! ## Arc Text
//! ```rust,ignore
//! use flui_types::geometry::text_path::*;
//!
//! let text = "CIRCULAR TEXT";
//! for (i, ch) in text.chars().enumerate() {
//!     let CharTransform { position, rotation } =
//!         arc_position(i, text.len(), 100.0, 0.0, std::f32::consts::TAU);
//!
//!     // Use position and rotation to render character...
//! }
//! ```
//!
//! ## Wave Text
//! ```rust,ignore
//! for (i, ch) in text.chars().enumerate() {
//!     let offset_y = wave_offset(i, 0.5, 10.0);
//!     // Position character with vertical offset...
//! }
//! ```

use std::f32::consts::{PI, TAU};

use crate::Point;

/// Result of character position calculation along a path
#[derive(Debug, Clone, Copy)]
pub struct CharTransform {
    /// Position of the character (center point or baseline)
    pub position: Point,
    /// Rotation angle in radians (0 = horizontal)
    pub rotation: f32,
}

/// Calculates character position along an arc (circular path)
///
/// # Parameters
/// - `char_index`: Index of the character (0-based)
/// - `total_chars`: Total number of characters
/// - `radius`: Radius of the circle
/// - `start_angle`: Starting angle in radians (0 = right, PI/2 = top)
/// - `arc_length`: Total arc length in radians (TAU = full circle)
///
/// # Returns
/// CharTransform with position on the arc and rotation tangent to the circle
///
/// # Example
/// ```
/// use flui_types::geometry::text_path::arc_position;
/// use std::f32::consts::TAU;
///
/// // Position character 3 out of 10 on a circle
/// let transform = arc_position(3, 10, 100.0, 0.0, TAU);
/// println!("Position: ({}, {})", transform.position.x, transform.position.y);
/// ```
#[inline]
pub fn arc_position(
    char_index: usize,
    total_chars: usize,
    radius: f32,
    start_angle: f32,
    arc_length: f32,
) -> CharTransform {
    let t = char_index as f32 / (total_chars as f32).max(1.0);
    let angle = start_angle + arc_length * t;

    CharTransform {
        position: Point::new(radius * angle.cos(), radius * angle.sin()),
        rotation: angle + PI / 2.0, // Rotate 90° to face outward
    }
}

/// Calculates vertical offset for wave effect
///
/// # Parameters
/// - `char_index`: Index of the character
/// - `frequency`: Wave frequency (higher = more waves)
/// - `amplitude`: Wave amplitude (height of waves)
///
/// # Returns
/// Vertical offset to apply to character position
///
/// # Example
/// ```
/// use flui_types::geometry::text_path::wave_offset;
///
/// let offset_y = wave_offset(5, 0.5, 15.0);
/// // Apply offset_y to character Y position
/// ```
#[inline]
pub fn wave_offset(char_index: usize, frequency: f32, amplitude: f32) -> f32 {
    (char_index as f32 * frequency).sin() * amplitude
}

/// Calculates position along a spiral path
///
/// # Parameters
/// - `char_index`: Index of the character
/// - `total_chars`: Total number of characters
/// - `start_radius`: Starting radius
/// - `radius_per_revolution`: How much radius increases per full rotation
/// - `revolutions`: Total number of revolutions
///
/// # Returns
/// CharTransform with spiral position and tangent rotation
///
/// # Example
/// ```
/// use flui_types::geometry::text_path::spiral_position;
///
/// // 3 full spirals, expanding from radius 50 to 150
/// let transform = spiral_position(10, 50, 50.0, 100.0, 3.0);
/// ```
#[inline]
pub fn spiral_position(
    char_index: usize,
    total_chars: usize,
    start_radius: f32,
    radius_per_revolution: f32,
    revolutions: f32,
) -> CharTransform {
    let t = char_index as f32 / (total_chars as f32).max(1.0);
    let angle = revolutions * TAU * t;
    let radius = start_radius + (radius_per_revolution * revolutions * t);

    CharTransform {
        position: Point::new(radius * angle.cos(), radius * angle.sin()),
        rotation: angle + PI / 2.0,
    }
}

/// Calculates rotation for each character to create a wave rotation effect
///
/// # Parameters
/// - `char_index`: Index of the character
/// - `frequency`: Rotation frequency
/// - `max_angle`: Maximum rotation angle in radians
///
/// # Returns
/// Rotation angle in radians
///
/// # Example
/// ```
/// use flui_types::geometry::text_path::wave_rotation;
///
/// let rotation = wave_rotation(7, 0.3, 0.5); // Max ±28.6 degrees
/// ```
#[inline]
pub fn wave_rotation(char_index: usize, frequency: f32, max_angle: f32) -> f32 {
    (char_index as f32 * frequency).sin() * max_angle
}

/// Calculates scaling factor along a gradient (for pyramid/trapezoid effects)
///
/// # Parameters
/// - `normalized_y`: Normalized Y position (0.0 = top, 1.0 = bottom)
/// - `top_scale`: Scale factor at the top
/// - `bottom_scale`: Scale factor at the bottom
///
/// # Returns
/// Interpolated scale factor
///
/// # Example
/// ```
/// use flui_types::geometry::text_path::vertical_scale;
///
/// // Pyramid: narrow top (0.5), wide bottom (1.0)
/// let scale_at_middle = vertical_scale(0.5, 0.5, 1.0); // Returns 0.75
/// ```
#[inline]
pub fn vertical_scale(normalized_y: f32, top_scale: f32, bottom_scale: f32) -> f32 {
    top_scale + (bottom_scale - top_scale) * normalized_y.clamp(0.0, 1.0)
}

/// Calculates position for a character in a grid layout with custom spacing
///
/// # Parameters
/// - `char_index`: Index of the character
/// - `chars_per_row`: Number of characters per row
/// - `char_width`: Width of each character cell
/// - `char_height`: Height of each character cell
///
/// # Returns
/// Position in a grid layout
///
/// # Example
/// ```
/// use flui_types::geometry::text_path::grid_position;
///
/// // 10 characters per row, 30px wide, 40px tall
/// let pos = grid_position(15, 10, 30.0, 40.0);
/// // Returns position for character at column 5, row 1
/// ```
#[inline]
pub fn grid_position(
    char_index: usize,
    chars_per_row: usize,
    char_width: f32,
    char_height: f32,
) -> Point {
    let row = char_index / chars_per_row.max(1);
    let col = char_index % chars_per_row.max(1);

    Point::new(col as f32 * char_width, row as f32 * char_height)
}

/// Calculates position along a Bezier curve (quadratic)
///
/// # Parameters
/// - `t`: Parameter along curve (0.0 to 1.0)
/// - `p0`: Start point
/// - `p1`: Control point
/// - `p2`: End point
///
/// # Returns
/// Point on the Bezier curve at parameter t
///
/// # Example
/// ```
/// use flui_types::{Point, geometry::text_path::bezier_point};
///
/// let start = Point::new(0.0, 0.0);
/// let control = Point::new(50.0, 100.0);
/// let end = Point::new(100.0, 0.0);
///
/// let mid_point = bezier_point(0.5, start, control, end);
/// ```
#[inline]
pub fn bezier_point(t: f32, p0: Point, p1: Point, p2: Point) -> Point {
    let t = t.clamp(0.0, 1.0);
    let mt = 1.0 - t;
    let mt2 = mt * mt;
    let t2 = t * t;

    Point::new(
        mt2 * p0.x + 2.0 * mt * t * p1.x + t2 * p2.x,
        mt2 * p0.y + 2.0 * mt * t * p1.y + t2 * p2.y,
    )
}

/// Calculates tangent rotation for a point on a Bezier curve
///
/// # Parameters
/// - `t`: Parameter along curve (0.0 to 1.0)
/// - `p0`: Start point
/// - `p1`: Control point
/// - `p2`: End point
///
/// # Returns
/// Rotation angle in radians tangent to the curve at t
#[inline]
pub fn bezier_tangent_rotation(t: f32, p0: Point, p1: Point, p2: Point) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let mt = 1.0 - t;

    // Derivative of quadratic Bezier
    let dx = 2.0 * mt * (p1.x - p0.x) + 2.0 * t * (p2.x - p1.x);
    let dy = 2.0 * mt * (p1.y - p0.y) + 2.0 * t * (p2.y - p1.y);

    dy.atan2(dx)
}

/// Calculates character position along a custom parametric path
///
/// # Parameters
/// - `char_index`: Index of the character
/// - `total_chars`: Total number of characters
/// - `path_fn`: Function that takes t (0.0 to 1.0) and returns (x, y, rotation)
///
/// # Returns
/// CharTransform from the parametric function
///
/// # Example
/// ```rust,ignore
/// use flui_types::geometry::text_path::parametric_position;
///
/// // Custom lemniscate (figure-8) path
/// let transform = parametric_position(5, 20, |t| {
///     let angle = t * TAU;
///     let scale = 100.0;
///     let x = scale * (2.0 * angle).cos();
///     let y = scale * angle.sin() * angle.cos();
///     let rotation = angle;
///     (x, y, rotation)
/// });
/// ```
#[inline]
pub fn parametric_position<F>(char_index: usize, total_chars: usize, path_fn: F) -> CharTransform
where
    F: Fn(f32) -> (f32, f32, f32),
{
    let t = char_index as f32 / (total_chars as f32).max(1.0);
    let (x, y, rotation) = path_fn(t);

    CharTransform {
        position: Point::new(x, y),
        rotation,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arc_position() {
        let transform = arc_position(0, 10, 100.0, 0.0, TAU);
        assert!((transform.position.x - 100.0).abs() < 0.01);
        assert!((transform.position.y - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_wave_offset() {
        let offset = wave_offset(0, 1.0, 10.0);
        assert!((offset - 0.0).abs() < 0.01); // sin(0) = 0
    }

    #[test]
    fn test_spiral_position() {
        let transform = spiral_position(0, 10, 50.0, 100.0, 2.0);
        assert!((transform.position.x - 50.0).abs() < 0.01);
    }

    #[test]
    fn test_vertical_scale() {
        let scale = vertical_scale(0.5, 0.5, 1.0);
        assert!((scale - 0.75).abs() < 0.01);
    }

    #[test]
    fn test_grid_position() {
        let pos = grid_position(15, 10, 30.0, 40.0);
        assert_eq!(pos.x, 5.0 * 30.0);
        assert_eq!(pos.y, 1.0 * 40.0);
    }

    #[test]
    fn test_bezier_point() {
        let start = Point::new(0.0, 0.0);
        let control = Point::new(50.0, 100.0);
        let end = Point::new(100.0, 0.0);

        let mid = bezier_point(0.5, start, control, end);
        assert_eq!(mid.x, 50.0); // Symmetric curve
    }
}
