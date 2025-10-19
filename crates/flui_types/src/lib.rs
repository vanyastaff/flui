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
//! - **No `unsafe` code** - All operations are memory-safe
//! - **Stack-allocated only** - Zero heap allocations for all core types
//! - **Bounds checking** - Safe array access with validation
//!
//! ## Type Safety
//! - **Strong typing** - Distinct types for different concepts
//! - **`#[must_use]` annotations** - Prevent accidental value dropping
//! - **Const constructors** - Compile-time evaluation where possible
//!
//! ## Zero Allocations
//! - **Copy types** - All geometry/layout types are `Copy`
//! - **Const methods** - Identity matrices, zero values computed at compile-time
//! - **In-place operations** - Methods like `transpose_in_place()` avoid temporaries
//! - **Zero-copy conversions** - `From`/`Into` traits without allocation
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


// Re-exports for convenience
pub use events::{HitTestEntry, HitTestResult, PointerButton, PointerDeviceKind, PointerEvent, PointerEventData};
pub use geometry::{Matrix4, Offset, Point, Rect, RRect, Size};
pub use layout::{
    Alignment, Axis, AxisDirection, CrossAxisAlignment, EdgeInsets, MainAxisAlignment,
    MainAxisSize, Orientation, VerticalDirection,
};
pub use styling::{Color, HSLColor, HSVColor, MaterialColors, ParseColorError};
pub use typography::{
    FontStyle, FontWeight, TextAlign, TextAlignVertical, TextAffinity, TextBaseline,
    TextDecoration, TextDecorationStyle, TextDirection, TextOverflow, TextPosition, TextRange,
    TextSelection, TextSpan, TextStyle,
};

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::geometry::{Matrix4, Offset, Point, Rect, RRect, Size};
    pub use crate::layout::{
        Alignment, Axis, AxisDirection, CrossAxisAlignment, EdgeInsets, MainAxisAlignment,
        MainAxisSize, Orientation, VerticalDirection,
    };
    pub use crate::styling::{Color, HSLColor, HSVColor, MaterialColors};
    pub use crate::typography::{
        FontStyle, FontWeight, TextAlign, TextAlignVertical, TextAffinity, TextBaseline,
        TextDecoration, TextDecorationStyle, TextDirection, TextOverflow, TextPosition, TextRange,
        TextSelection, TextSpan, TextStyle,
    };
}

