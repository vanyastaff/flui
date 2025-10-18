//! Core types for Flui framework
//!
//! This crate provides fundamental types used throughout Flui:
//! - **Geometry**: Point, Rect, Size, Offset, RRect
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
//! This is the base crate with NO dependencies on other flui crates.

#![warn(missing_docs)]

pub mod animation;
pub mod constraints;
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
pub use geometry::{Offset, Point, Rect, Size};
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
    pub use crate::geometry::{Offset, Point, Rect, Size};
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
