//! Styling types for Flui.
//!
//! This module contains types for colors, borders, shadows, and other visual
//! styling.

// Colour maths: r, g, b, a, h, s, l, v are the domain's names.
#![expect(clippy::many_single_char_names)]

pub mod border;
pub mod border_radius;
pub mod box_border;
pub mod color;
pub mod decoration;
pub mod gradient;
pub mod hsl_hsv;
pub mod physical_model;
pub mod shadow;
mod srgb_tables;
pub mod table_border;

// Re-exports for convenience
pub use border::{BorderPosition, BorderSide, BorderStyle};
pub use border_radius::{BorderRadius, BorderRadiusDirectional, BorderRadiusExt};
pub use box_border::{Border, BorderDirectional, BoxBorder};
pub use color::{
    Color, Oklab, ParseColorError, PremultipliedOklab, linear_to_srgb, srgb_to_linear,
};
pub use decoration::{
    BlendMode, BoxDecoration, BoxFit, ColorFilter, Decoration, DecorationImage, ImageRepeat,
};
pub use gradient::{
    Gradient, GradientRotation, GradientTransform, LinearGradient, RadialGradient, SweepGradient,
    TileMode,
};
pub use hsl_hsv::{HSLColor, HSVColor};
pub use physical_model::{Elevation, MaterialType, PhysicalShape};
pub use shadow::{BoxShadow, Shadow, ShadowQuality};
pub use table_border::TableBorder;

// Re-export Radius and Corners from geometry module for styling convenience
pub use flui_foundation::geometry::{Corners, Radius};
