//! Core types for Flui framework
//!
//! This crate provides fundamental types used throughout Flui:
//! - **Geometry**: Point, Rect, Size, Offset, RRect, Matrix4
//! - **Layout**: Axis, EdgeInsets, Alignment, MainAxisAlignment, CrossAxisAlignment, MainAxisSize
//! - **Styling**: Color, HSLColor, HSVColor, Border, Shadow, Gradient, Decoration
//! - **Typography**: TextStyle, TextAlign, TextDecoration, TextSpan, and more
//! - **Painting**: BlendMode, BoxFit, ImageRepeat, Clip, TileMode, Shader, and more
//! - **Animation**: Curves, Tweens, AnimationStatus
//! - **Constraints**: SliverConstraints, SliverGeometry, ScrollMetrics, GrowthDirection
//! - **Gestures**: TapDetails, DragDetails, ScaleDetails, Velocity, PointerData
//! - **Physics**: SpringSimulation, FrictionSimulation, GravitySimulation, Tolerance
//! - **Semantics**: SemanticsData, SemanticsAction, SemanticsRole, SemanticsEvent
//! - **Platform**: TargetPlatform, Brightness, DeviceOrientation, Locale
//!
//! # Design Philosophy
//!
//! All types in this crate follow strict design principles to ensure:
//!
//! ## Memory Safety
//! - **Minimal unsafe code** - SIMD optimizations use unsafe in controlled manner (color, matrix)
//! - **Primarily stack-allocated** - Core geometry/layout types avoid heap allocations
//! - **Bounds checking** - Safe array access with validation
//!
//! ## Type Safety
//! - **Strong typing** - Distinct types for different concepts
//! - **`#[must_use]` annotations** - Prevent accidental value dropping
//! - **Const constructors** - Compile-time evaluation where possible
//!
//! ## Performance-Focused Allocation Strategy
//! - **Zero-allocation core** - Geometry and layout types are `Copy` (no heap usage)
//! - **Selective allocations** - Typography and caching use heap when beneficial for performance
//! - **Const methods** - Identity matrices, zero values computed at compile-time
//! - **In-place operations** - Methods like `transpose_in_place()` avoid temporaries
//! - **Zero-copy conversions** - `From`/`Into` traits without allocation where possible
//!
//! ## Idiomatic Rust APIs
//! - **Standard traits** - `Add`, `Sub`, `Mul`, `Div`, `Index`, `From`, `Into`
//! - **Ergonomic methods** - Fluent APIs, method chaining where appropriate
//! - **Consistent naming** - `try_*` for fallible operations, `is_*` for predicates
//! - **Documentation** - Comprehensive docs with examples for all public APIs
//!
//! ## Performance
//! - **`#[inline]`** - Hot-path methods are inlined
//! - **Const evaluation** - Constants computed at compile-time
//! - **SIMD-ready** - Data layouts compatible with future SIMD optimizations
//! - **No over-engineering** - Simple, direct implementations without unnecessary abstraction
//!
//! ## Mathematical Correctness
//! - **Extensive testing** - 575+ unit tests covering edge cases
//! - **Precision handling** - Proper epsilon comparisons for floating-point
//! - **Validation methods** - `is_finite()`, `is_valid()` prevent NaN/Infinity bugs
//!
//! ## Cross-Layer Compatibility
//! - **Stable ABI** - Simple `#[repr(Rust)]` structs, no complex layouts
//! - **Feature flags** - Optional serde support, no forced dependencies
//! - **No circular deps** - Base crate with zero flui dependencies
//! - **Interoperability** - Compatible with egui, glam, mint (via feature flags)
//!
//! This is the base crate with NO dependencies on other flui crates.

#![warn(missing_docs)]
pub mod animation;
pub mod constraints;
pub mod events;
pub mod geometry;
pub mod gestures;
pub mod layout;
pub mod painting;
pub mod physics;
pub mod platform;
pub mod semantics;
pub mod styling;
pub mod typography;

// Re-exports for convenience - Most commonly used types
pub use constraints::{BoxConstraints, SliverConstraints, SliverGeometry};
pub use events::{Event, KeyEvent, PointerEvent, PointerEventData, Theme, WindowEvent};
pub use geometry::{Matrix4, Offset, Point, RRect, Rect, Size};
pub use gestures::PointerDeviceKind;
pub use layout::{Alignment, Axis, EdgeInsets};
pub use styling::{Color, Color32};

/// Prelude module for convenient glob imports
///
/// Import with `use flui_types::prelude::*;` to get all commonly-used types.
pub mod prelude {
    // Geometry - Essential types
    pub use crate::geometry::{Matrix4, Offset, Point, RRect, Rect, Size};

    // Layout - Common types
    pub use crate::layout::{
        Alignment, Axis, AxisDirection, CrossAxisAlignment, EdgeInsets, MainAxisAlignment,
        MainAxisSize, Orientation, VerticalDirection,
    };

    // Styling - Essential
    pub use crate::styling::{Color, Color32, HSLColor, HSVColor};

    // Constraints - Common
    pub use crate::constraints::BoxConstraints;

    // Animation - Frequently used
    pub use crate::animation::{Curve, Curves, Linear, Tween};

    // Typography - Common
    pub use crate::typography::{
        FontStyle, FontWeight, TextAlign, TextBaseline, TextDirection, TextStyle,
    };

    // Events - Common
    pub use crate::events::{KeyEvent, PointerEvent, WindowEvent};
}
