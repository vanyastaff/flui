//! Type-safe geometry types for 2D graphics.
//!
//! This module provides fundamental geometry types with compile-time unit
//! safety, preventing accidental mixing of coordinate systems (e.g., Pixels vs
//! DevicePixels).
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
//! The geometry system uses type-safe units to prevent coordinate system
//! mixing:
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
//! use flui_geometry::prelude::*;
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
//! use flui_geometry::prelude::*;
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
//! use flui_geometry::prelude::*;
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
/// Error types for geometry operations.
pub mod error;
pub mod length;
pub mod line;
pub mod matrix4;
pub mod offset;
pub mod point;
pub mod rect;
pub mod relative_rect;
/// 2D rotation type.
pub mod rotation;
pub mod rrect;
pub mod rsuperellipse;
pub mod size;
pub mod text_path;
pub mod traits;
pub mod transform;
pub mod transform2d; // PORT-CHECK-OK-SP4: transform2d API surface; consumed via flui_types::geometry re-export chain
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
/// use flui_geometry::prelude::*;
///
/// let pos = Point::<Pixels>::new(px(100.0), px(200.0));
/// let size = Size::<Pixels>::new(px(50.0), px(30.0));
/// let offset = pos.to_vec2();
/// ```
pub mod prelude {
    // Core generic types
    // Shape types
    // Error types
    // Unit types
    // Constructor functions
    // Traits
    pub use super::traits::{
        Along, Axis, FloatUnit, GeometryOps, Half, IsZero, NumericUnit, Sign, Unit,
    };
    pub use super::{
        bounds::Bounds,
        circle::Circle,
        error::GeometryError,
        length::Rems,
        line::Line,
        offset::Offset,
        point::{Point, point},
        rect::{Rect, rect},
        rrect::{RRect, Radius},
        size::{Size, size},
        units::{
            DevicePixels, PixelDelta, Pixels, ScaledPixels, delta_px, device_px, px, scaled_px,
        },
        vector::{Vec2, vec2},
    };
}

// =============================================================================
// CORE GENERIC TYPES - Type-safe geometry with unit parameters
// =============================================================================

// =============================================================================
// SHAPE TYPES
// =============================================================================
pub use bezier::{CubicBez, QuadBez};
pub use bounds::{Bounds, bounds};
pub use circle::Circle;
// =============================================================================
// STRUCTURAL TYPES
// =============================================================================
pub use corner::Corner;
pub use corners::{Corners, corners};
pub use edges::{Edges, edges};
// =============================================================================
// ERROR TYPES
// =============================================================================
pub use error::GeometryError;
// =============================================================================
// LENGTH TYPES
// =============================================================================
pub use length::{AbsoluteLength, DefiniteLength, Length, Percentage, Rems, auto, relative, rems};
pub use line::Line;
// =============================================================================
// TRANSFORMATION TYPES
// =============================================================================
pub use matrix4::Matrix4;
/// Generic 2D offset (Flutter-compatible displacement).
///
/// See [`offset`] module for full documentation.
pub use offset::Offset;
/// Generic 2D point with unit-safe coordinates.
///
/// See [`point`](mod@point) module for full documentation.
pub use point::Point;
// =============================================================================
// CONSTRUCTOR FUNCTIONS
// =============================================================================
pub use point::point;
pub use rect::{Rect, rect};
pub use relative_rect::RelativeRect;
pub use rotation::QuarterTurns;
pub use rrect::{RRect, Radius};
pub use rsuperellipse::RSuperellipse;
/// Generic 2D size (dimensions) with unit-safe coordinates.
///
/// See [`size`](mod@size) module for full documentation.
pub use size::Size;
pub use size::size;
// =============================================================================
// TEXT PATH HELPERS
// =============================================================================
pub use text_path::{
    CharTransform, arc_position, bezier_point, bezier_tangent_rotation, grid_position,
    parametric_position, spiral_position, vertical_scale, wave_offset, wave_rotation,
};
// =============================================================================
// TRAITS
// =============================================================================
pub use traits::{
    Along, ApproxEq, Axis, Double, FloatUnit, GeometryOps, Half, IsZero, NumericUnit, Sign, Unit,
};
pub use transform::Transform;
pub use transform2d::Transform2D;
// =============================================================================
// UNIT TYPES
// =============================================================================
pub use units::{
    DevicePixels, ParseLengthError, PixelDelta, Pixels, Radians, ScaleFactor, ScaledPixels,
    delta_px, device_px, px, radians, scaled_px,
};
/// Generic 2D vector (displacement) with unit-safe coordinates.
///
/// See [`vector`] module for full documentation.
pub use vector::Vec2;
pub use vector::vec2;

// =============================================================================
// TYPE ALIASES - Common instantiations for convenience
// =============================================================================

/// Point in logical pixel coordinates.
pub type PixelPoint = Point<Pixels>;

/// Point in device (physical) pixel coordinates.
pub type DevicePoint = Point<DevicePixels>;

/// Point in scaled pixel coordinates.
pub type ScaledPoint = Point<ScaledPixels>;

/// Vector in logical pixel coordinates.
pub type PixelVec2 = Vec2<Pixels>;

/// Vector in device (physical) pixel coordinates.
pub type DeviceVec2 = Vec2<DevicePixels>;

/// Vector in scaled pixel coordinates.
pub type ScaledVec2 = Vec2<ScaledPixels>;

/// Size in logical pixel coordinates.
pub type PixelSize = Size<Pixels>;

/// Size in device (physical) pixel coordinates.
pub type DeviceSize = Size<DevicePixels>;

/// Size in scaled pixel coordinates.
pub type ScaledSize = Size<ScaledPixels>;

/// Offset in logical pixel coordinates.
pub type PixelOffset = Offset<Pixels>;

/// Offset in device (physical) pixel coordinates.
pub type DeviceOffset = Offset<DevicePixels>;

/// Padding/margin insets in logical pixels.
///
/// Used by the rendering layer for padding/margin insets. Backed by
/// [`Edges<Pixels>`] so insets carry the same unit safety as every other
/// layout quantity — build with [`Edges::all`], [`Edges::symmetric`],
/// [`Edges::only_left`] (and siblings), or [`Edges::ZERO`].
pub type EdgeInsets = Edges<Pixels>;

// =============================================================================
// TESTS
// =============================================================================
