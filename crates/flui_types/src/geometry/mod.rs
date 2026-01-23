//! Type-safe geometry types for 2D graphics.
//!
//! This module provides fundamental geometry types with compile-time unit safety,
//! preventing accidental mixing of coordinate systems (e.g., Pixels vs DevicePixels).
//!
//! # Core Types
//!
//! | Type | Description | Example |
//! |------|-------------|---------|
//! | [`Point<T>`] | Absolute position in 2D space | `Point::<Pixels>::new(px(10.0), px(20.0))` |
//! | [`Vec2<T>`] | Direction and magnitude (displacement) | `Vec2::new(px(1.0), px(0.0)).normalize()` |
//! | [`Size<T>`] | Width and height dimensions | `Size::<Pixels>::new(px(100.0), px(200.0))` |
//! | [`Offset<T>`] | UI displacement (Flutter compat) | `Offset::new(px(5.0), px(10.0))` |
//! | [`Rect`] | Axis-aligned bounding rectangle | `Rect::new(origin, size)` |
//! | [`RRect`] | Rounded rectangle | `RRect::new(rect, radius)` |
//! | [`Circle`] | Circle with center and radius | `Circle::new(center, 50.0)` |
//! | [`Line`] | Line segment between two points | `Line::new(start, end)` |
//! | [`Matrix4`] | 4x4 transformation matrix | `Matrix4::identity()` |
//!
//! # Unit Types
//!
//! The geometry system uses type-safe units to prevent coordinate system mixing:
//!
//! | Unit | Description | Use Case |
//! |------|-------------|----------|
//! | [`Pixels`] | Logical pixels | UI layout, design coordinates |
//! | [`DevicePixels`] | Physical pixels (i32) | Framebuffer, pixel-perfect rendering |
//! | [`ScaledPixels`] | DPI-scaled pixels | High-DPI displays |
//! | [`Rems`] | Root em units | Font-relative sizing |
//! | `f32` | Raw float | GPU operations, math |
//!
//! # Type Safety Example
//!
//! ```rust,ignore
//! use flui_types::geometry::prelude::*;
//!
//! // Different coordinate systems are distinct types
//! let ui_pos = Point::<Pixels>::new(px(100.0), px(200.0));
//! let device_pos = Point::<DevicePixels>::new(device_px(800), device_px(600));
//!
//! // This would be a compile error - units don't match!
//! // let mixed = ui_pos + device_pos.to_vec2();  // ❌
//!
//! // Explicit conversion required
//! let gpu_pos: Point<Pixels> = ui_pos.into();  // ✅
//! ```
//!
//! # GPU Integration
//!
//! Multiple conversion strategies for wgpu integration:
//!
//! ```rust,ignore
//! use flui_types::geometry::prelude::*;
//!
//! let pos = Point::<Pixels>::new(px(100.0), px(200.0));
//!
//! // Into trait (ergonomic)
//! let gpu_pos: Point<Pixels> = pos.into();
//!
//! // Explicit cast (clarity)
//! let gpu_pos = pos.cast::<f32>();
//!
//! // Direct to f32
//! let gpu_pos = pos.to_f32();
//!
//! // Array for vertex buffers
//! let vertex_data: [f32; 2] = pos.to_array();
//! ```
//!
//! # Safety Levels
//!
//! Three constructor levels for different trust contexts:
//!
//! ```rust,ignore
//! use flui_types::geometry::prelude::*;
//!
//! // Fast (no validation) - for hot loops
//! let p = Point::new(x, y);
//!
//! // Safe (returns Result) - for user input
//! let p = Point::<f32>::try_new(x, y)?;
//!
//! // Validated (clamps) - for edge cases
//! let p = Point::new_clamped(x, y);
//! ```
//!
//! # Precision
//!
//! All geometry types use `f32` for GPU compatibility.
//! This matches Skia, Impeller, and other graphics APIs.
//!
//! # API Design
//!
//! API conventions follow kurbo/glam/GPUI best practices:
//! - Constructors: `new()`, `splat()`, `from_array()`, `from_tuple()`
//! - Accessors: `to_array()`, `to_tuple()`, `with_x()`, `with_y()`
//! - Operations: `lerp()`, `min()`, `max()`, `clamp()`
//! - Rounding: `round()`, `ceil()`, `floor()`, `trunc()`
//! - Validation: `is_finite()`, `is_valid()`, `try_new()`
//!
//! # Feature Modules
//!
//! - [`traits`] - Core traits (`Unit`, `NumericUnit`, `Along`, `Half`, etc.)
//! - [`units`] - Unit types (`Pixels`, `DevicePixels`, `ScaledPixels`)
//! - [`error`] - Error types for validation failures
//! - [`transform`] - 2D transformations
//! - [`bezier`] - Bézier curve types

// =============================================================================
// MODULES
// =============================================================================

pub mod bezier;
pub mod bounds;
pub mod circle;
pub mod corner;
pub mod corners;
pub mod edges;
pub mod error;
pub mod length;
pub mod line;
pub mod matrix4;
pub mod offset;
pub mod point;
pub mod rect;
pub mod relative_rect;
pub mod rotation;
pub mod rrect;
pub mod rsuperellipse;
pub mod size;
pub mod text_path;
pub mod traits;
pub mod transform;
pub mod units;
pub mod vector;

// =============================================================================
// PRELUDE - Convenient glob import for common usage
// =============================================================================

/// Prelude module for convenient imports.
///
/// # Usage
///
/// ```rust,ignore
/// use flui_types::geometry::prelude::*;
///
/// let pos = Point::<Pixels>::new(px(100.0), px(200.0));
/// let size = Size::<Pixels>::new(px(50.0), px(30.0));
/// let offset = pos.to_vec2();
/// ```
pub mod prelude {
    // Core generic types
    pub use super::point::Point;
    pub use super::size::Size;
    pub use super::vector::Vec2;
    pub use super::offset::Offset;

    // Shape types
    pub use super::bounds::Bounds;
    pub use super::circle::Circle;
    pub use super::line::Line;
    pub use super::rect::Rect;
    pub use super::rrect::{RRect, Radius};

    // Unit types
    pub use super::units::{
        delta_px, device_px, px, scaled_px, DevicePixels, PixelDelta, Pixels, ScaledPixels,
    };
    pub use super::length::Rems;

    // Traits
    pub use super::traits::{Along, Axis, GeometryOps, Half, IsZero, NumericUnit, Sign, Unit};

    // Error types
    pub use super::error::GeometryError;

    // Constructor functions
    pub use super::point::point;
    pub use super::size::size;
    pub use super::vector::vec2;
    pub use super::rect::rect;
}

// =============================================================================
// CORE GENERIC TYPES - Type-safe geometry with unit parameters
// =============================================================================

/// Generic 2D point with unit-safe coordinates.
///
/// See [`point`](mod@point) module for full documentation.
pub use point::Point;

/// Generic 2D vector (displacement) with unit-safe coordinates.
///
/// See [`vector`] module for full documentation.
pub use vector::Vec2;

/// Generic 2D size (dimensions) with unit-safe coordinates.
///
/// See [`size`](mod@size) module for full documentation.
pub use size::Size;

/// Generic 2D offset (Flutter-compatible displacement).
///
/// See [`offset`] module for full documentation.
pub use offset::Offset;

// =============================================================================
// SHAPE TYPES
// =============================================================================

pub use bezier::{CubicBez, QuadBez};
pub use bounds::Bounds;
pub use circle::Circle;
pub use line::Line;
pub use rect::{rect, Rect};
pub use rrect::{RRect, Radius};
pub use rsuperellipse::RSuperellipse;

// =============================================================================
// STRUCTURAL TYPES
// =============================================================================

pub use corner::Corner;
pub use corners::{corners, Corners};
pub use edges::{edges, Edges};
pub use relative_rect::RelativeRect;

// =============================================================================
// TRANSFORMATION TYPES
// =============================================================================

pub use matrix4::Matrix4;
pub use rotation::QuarterTurns;
pub use transform::Transform;

// =============================================================================
// UNIT TYPES
// =============================================================================

pub use units::{
    delta_px, device_px, px, radians, scaled_px,
    DevicePixels, ParseLengthError, PixelDelta, Pixels, Radians, ScaledPixels,
};

// =============================================================================
// LENGTH TYPES
// =============================================================================

pub use length::{
    auto, relative, rems,
    AbsoluteLength, DefiniteLength, Length, Percentage, Rems,
};

// =============================================================================
// TRAITS
// =============================================================================

pub use traits::{Along, ApproxEq, Axis, Double, GeometryOps, Half, IsZero, NumericUnit, Sign, Unit};

// =============================================================================
// ERROR TYPES
// =============================================================================

pub use error::GeometryError;

// =============================================================================
// CONSTRUCTOR FUNCTIONS
// =============================================================================

pub use point::point;
pub use size::size;
pub use vector::vec2;

// =============================================================================
// TEXT PATH HELPERS
// =============================================================================

pub use text_path::{
    arc_position, bezier_point, bezier_tangent_rotation, grid_position, parametric_position,
    spiral_position, vertical_scale, wave_offset, wave_rotation, CharTransform,
};

// =============================================================================
// TYPE ALIASES - Common instantiations for convenience
// =============================================================================

/// Point in logical pixel coordinates.
pub type PixelPoint = Point<Pixels>;

/// Point in device (physical) pixel coordinates.
pub type DevicePoint = Point<DevicePixels>;

/// Point in scaled pixel coordinates.
pub type ScaledPoint = Point<ScaledPixels>;

/// Point in raw float coordinates (GPU-ready).
pub type FloatPoint = Point<Pixels>;

/// Vector in logical pixel coordinates.
pub type PixelVec2 = Vec2<Pixels>;

/// Vector in device (physical) pixel coordinates.
pub type DeviceVec2 = Vec2<DevicePixels>;

/// Vector in scaled pixel coordinates.
pub type ScaledVec2 = Vec2<ScaledPixels>;

/// Vector in raw float coordinates (GPU-ready).
pub type FloatVec2 = Vec2<Pixels>;

/// Size in logical pixel coordinates.
pub type PixelSize = Size<Pixels>;

/// Size in device (physical) pixel coordinates.
pub type DeviceSize = Size<DevicePixels>;

/// Size in scaled pixel coordinates.
pub type ScaledSize = Size<ScaledPixels>;

/// Size in raw float coordinates (GPU-ready).
pub type FloatSize = Size<Pixels>;

/// Offset in logical pixel coordinates.
pub type PixelOffset = Offset<Pixels>;

/// Offset in device (physical) pixel coordinates.
pub type DeviceOffset = Offset<DevicePixels>;

/// Offset in raw float coordinates (GPU-ready).
pub type FloatOffset = Offset<Pixels>;

// =============================================================================
// TESTS
// =============================================================================
