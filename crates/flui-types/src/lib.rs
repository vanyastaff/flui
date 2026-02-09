//! Core types for Flui framework
//!
//! This crate provides fundamental types used throughout Flui:
//! - **Geometry**: Point, Rect, Size, Offset, RRect, Matrix4, Edges (with typed units)
//! - **Layout**: Axis, Alignment, MainAxisAlignment, CrossAxisAlignment, MainAxisSize
//! - **Styling**: Color, HSLColor, HSVColor, Border, Shadow, Gradient, Decoration (all generic over Unit type)
//! - **Typography**: TextStyle, TextAlign, TextDecoration, TextSpan, and more
//! - **Painting**: BlendMode, BoxFit, ImageRepeat, Clip, TileMode, Shader, and more
//! - **Gestures**: TapDetails, DragDetails, ScaleDetails, Velocity, PointerData
//! - **Physics**: SpringSimulation, FrictionSimulation, GravitySimulation, Tolerance
//! - **Semantics**: SemanticsData, SemanticsAction, SemanticsRole, SemanticsEvent
//! - **Platform**: TargetPlatform, Brightness, DeviceOrientation, Locale
//!
//! Note: Animation types (Curves, Tweens, AnimationStatus) have been moved to `flui_animation`.
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
//!
//! # Note on Constraints
//!
//! Layout constraints (`BoxConstraints`, `SliverConstraints`, `SliverGeometry`, etc.)
//! have been moved to `flui_rendering::constraints` as they are part of the rendering
//! protocol rather than basic types.

#![allow(missing_docs)] // TODO: Add comprehensive docs (486 items remaining - ongoing improvement)
pub mod geometry;
pub mod gestures;
pub mod layout;
pub mod painting;
pub mod physics;
pub mod platform;
pub mod styling;
pub mod typography;

// Note: Semantics types are in flui-semantics crate
// Note: Event types moved to flui_interaction (uses ui-events crate)

// Re-exports for convenience - Most commonly used types
pub use geometry::{Edges, Matrix4, Offset, Pixels, Point, RRect, Rect, Size};
pub use layout::{Alignment, Axis};
pub use styling::{Color, Color32};

/// Prelude module for convenient glob imports
///
/// Import with `use flui_types::prelude::*;` to get all commonly-used types.
pub mod prelude {
    // Geometry - Essential types
    pub use crate::geometry::{Matrix4, Offset, Point, RRect, Rect, Size, Vec2};

    // Layout - Common types
    pub use crate::layout::{
        Alignment, Axis, AxisDirection, CrossAxisAlignment, MainAxisAlignment, MainAxisSize,
        Orientation, VerticalDirection,
    };

    // Geometry - Edges and Pixels for layout
    pub use crate::geometry::{px, Edges, Pixels};

    // Styling - Essential
    pub use crate::styling::{Color, Color32, HSLColor, HSVColor};

    // Typography - Common
    pub use crate::typography::{
        FontStyle, FontWeight, TextAlign, TextBaseline, TextDirection, TextStyle,
    };

    // Note: Animation types (Curve, Curves, Tween) moved to flui_animation::prelude
}

// ═══════════════════════════════════════════════════════════════════════════
// Compile-Time Size Assertions (Memory Layout Contracts)
// ═══════════════════════════════════════════════════════════════════════════

/// Compile-time assertions for memory layout contracts
///
/// These assertions ensure that core types maintain expected memory sizes
/// across platforms and compiler versions. This is critical for:
/// - Performance (small types passed by value, not reference)
/// - Cache efficiency (types fit in cache lines)
/// - ABI stability (size changes break binary compatibility)
///
/// If these assertions fail, it indicates a potential performance regression
/// or breaking change that needs careful consideration.
#[doc(hidden)]
pub mod size_assertions {
    use core::mem::size_of;

    // Core geometry types - must be small enough to pass by value efficiently
    const _: () = assert!(
        size_of::<crate::Pixels>() <= 4,
        "Pixels should be 4 bytes (f32)"
    );
    const _: () = assert!(
        size_of::<crate::Point<crate::Pixels>>() <= 8,
        "Point<Pixels> should be ≤8 bytes (2×f32)"
    );
    const _: () = assert!(
        size_of::<crate::Size<crate::Pixels>>() <= 8,
        "Size<Pixels> should be ≤8 bytes (2×f32)"
    );
    const _: () = assert!(
        size_of::<crate::Rect<crate::Pixels>>() <= 16,
        "Rect<Pixels> should be ≤16 bytes (4×f32)"
    );
    const _: () = assert!(
        size_of::<crate::Offset<crate::Pixels>>() <= 8,
        "Offset<Pixels> should be ≤8 bytes (2×f32)"
    );

    // Color types - single cache line
    const _: () = assert!(
        size_of::<crate::Color32>() <= 4,
        "Color32 should be 4 bytes (RGBA8)"
    );
    const _: () = assert!(
        size_of::<crate::Color>() <= 16,
        "Color should be ≤16 bytes (4×f32 RGBA)"
    );

    // Matrix - should fit in 64 bytes (single cache line)
    const _: () = assert!(
        size_of::<crate::Matrix4>() <= 64,
        "Matrix4 should be ≤64 bytes (16×f32)"
    );

    // Edges - layout padding/margins
    const _: () = assert!(
        size_of::<crate::Edges<crate::Pixels>>() <= 16,
        "Edges<Pixels> should be ≤16 bytes (4×f32)"
    );
}
