//! Text path transformation helpers
//!
//! This module provides mathematical utilities for calculating character
//! positions along various paths (arc, wave, spiral, etc.). These are low-level
//! primitives for developers to build custom text effects.
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
//! use flui_geometry::text_path::*;
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

use std::f64::consts::{PI, TAU};

use super::{Pixels, px};
use crate::{
    Point,
    traits::{FloatUnit, NumericUnit, Unit},
};

/// Position and rotation for a single character placed along a path.
#[derive(Debug, Clone, Copy)]
pub struct CharTransform<T: Unit> {
    /// Position of the character (center point or baseline)
    pub position: Point<T>,
    /// Rotation angle in radians (0 = horizontal)
    pub rotation: f64,
}

/// Calculate the position and rotation of a character along a circular arc.
///
/// The character at `char_index` (of `total_chars`) is placed on a circle of
/// `radius` centered at the origin, sweeping `arc_length` radians from
/// `start_angle`. The rotation is the arc angle plus 90° so glyphs face
/// outward along the tangent.
#[inline]
pub fn arc_position(
    char_index: usize,
    total_chars: usize,
    radius: f64,
    start_angle: f64,
    arc_length: f64,
) -> CharTransform<Pixels> {
    let t = char_index as f64 / (total_chars as f64).max(1.0);
    let angle = start_angle + arc_length * t;

    CharTransform {
        position: Point::new(
            px((radius * angle.cos()) as f32),
            px((radius * angle.sin()) as f32),
        ),
        rotation: angle + PI / 2.0, // Rotate 90° to face outward
    }
}

/// Calculate the sinusoidal vertical offset for a character in a wave effect.
///
/// Returns `sin(char_index * frequency) * amplitude`.
#[inline]
pub fn wave_offset(char_index: usize, frequency: f64, amplitude: f64) -> f64 {
    (char_index as f64 * frequency).sin() * amplitude
}

/// Calculate the position and rotation of a character along an Archimedean spiral.
///
/// The spiral starts at `start_radius` and grows by `radius_per_revolution`
/// for each of the `revolutions` turns; characters are spread evenly along it.
/// The rotation is the spiral angle plus 90° so glyphs face outward along the
/// tangent.
#[inline]
pub fn spiral_position(
    char_index: usize,
    total_chars: usize,
    start_radius: f64,
    radius_per_revolution: f64,
    revolutions: f64,
) -> CharTransform<Pixels> {
    let t = char_index as f64 / (total_chars as f64).max(1.0);
    let angle = revolutions * TAU * t;
    let radius = start_radius + (radius_per_revolution * revolutions * t);

    CharTransform {
        position: Point::new(
            px((radius * angle.cos()) as f32),
            px((radius * angle.sin()) as f32),
        ),
        rotation: angle + PI / 2.0,
    }
}

/// Calculate the sinusoidal rotation for a character in a wave effect, in radians.
///
/// Returns `sin(char_index * frequency) * max_angle`, oscillating between
/// `-max_angle` and `max_angle`.
#[inline]
pub fn wave_rotation(char_index: usize, frequency: f64, max_angle: f64) -> f64 {
    (char_index as f64 * frequency).sin() * max_angle
}

/// Linearly interpolate a scale factor between `top_scale` and `bottom_scale`.
///
/// `normalized_y` is clamped to `[0.0, 1.0]`, where 0.0 yields `top_scale`
/// and 1.0 yields `bottom_scale`. Useful for perspective-style text effects.
#[inline]
pub fn vertical_scale(normalized_y: f64, top_scale: f64, bottom_scale: f64) -> f64 {
    top_scale + (bottom_scale - top_scale) * normalized_y.clamp(0.0, 1.0)
}

/// Calculate the position of a character in a fixed-size grid layout.
///
/// Characters fill rows left-to-right, wrapping every `chars_per_row`
/// characters (treated as at least 1); each cell is `char_width` by
/// `char_height`.
#[inline]
pub fn grid_position<T>(
    char_index: usize,
    chars_per_row: usize,
    char_width: f64,
    char_height: f64,
) -> Point<T>
where
    T: NumericUnit + FloatUnit,
{
    let row = char_index / chars_per_row.max(1);
    let col = char_index % chars_per_row.max(1);

    Point::new(
        T::from_f32((col as f64 * char_width) as f32),
        T::from_f32((row as f64 * char_height) as f32),
    )
}

/// Evaluate a quadratic Bezier curve at parameter `t`.
///
/// `t` is clamped to `[0.0, 1.0]`; `p0` and `p2` are the endpoints and `p1`
/// is the control point.
#[inline]
pub fn bezier_point<T>(t: f64, p0: Point<T>, p1: Point<T>, p2: Point<T>) -> Point<T>
where
    T: NumericUnit + Into<f32> + FloatUnit,
{
    let t = t.clamp(0.0, 1.0);
    let mt = 1.0 - t;
    let mt2 = mt * mt;
    let t2 = t * t;

    // Convert to f32 for calculation, then back to T
    let x =
        mt2 as f32 * p0.x.into() + 2.0 * (mt * t) as f32 * p1.x.into() + t2 as f32 * p2.x.into();
    let y =
        mt2 as f32 * p0.y.into() + 2.0 * (mt * t) as f32 * p1.y.into() + t2 as f32 * p2.y.into();

    Point::new(T::from_f32(x), T::from_f32(y))
}

/// Calculate the tangent angle of a quadratic Bezier curve at parameter `t`, in radians.
///
/// `t` is clamped to `[0.0, 1.0]`. The angle is derived from the curve's
/// derivative via `atan2`, suitable for rotating characters to follow the
/// curve.
#[inline]
pub fn bezier_tangent_rotation<T>(t: f64, p0: Point<T>, p1: Point<T>, p2: Point<T>) -> f64
where
    T: NumericUnit + Into<f32>,
{
    let t = t.clamp(0.0, 1.0);
    let mt = 1.0 - t;

    // Convert to f64 for calculation
    let p0x = Into::<f32>::into(p0.x) as f64;
    let p0y = Into::<f32>::into(p0.y) as f64;
    let p1x = Into::<f32>::into(p1.x) as f64;
    let p1y = Into::<f32>::into(p1.y) as f64;
    let p2x = Into::<f32>::into(p2.x) as f64;
    let p2y = Into::<f32>::into(p2.y) as f64;

    // Derivative of quadratic Bezier
    let dx = 2.0 * mt * (p1x - p0x) + 2.0 * t * (p2x - p1x);
    let dy = 2.0 * mt * (p1y - p0y) + 2.0 * t * (p2y - p1y);

    dy.atan2(dx)
}

/// Calculate a character transform from a user-supplied parametric path function.
///
/// `path_fn` receives the normalized position `t` in `[0.0, 1.0)` for
/// `char_index` of `total_chars` and returns `(x, y, rotation)`, where the
/// coordinates are in pixels and the rotation is in radians.
#[inline]
pub fn parametric_position<F>(
    char_index: usize,
    total_chars: usize,
    path_fn: F,
) -> CharTransform<Pixels>
where
    F: Fn(f64) -> (f64, f64, f64),
{
    let t = char_index as f64 / (total_chars as f64).max(1.0);
    let (x, y, rotation) = path_fn(t);

    CharTransform {
        position: Point::new(px(x as f32), px(y as f32)),
        rotation,
    }
}
