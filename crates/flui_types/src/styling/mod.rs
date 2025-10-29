//! Styling types for Flui.
//!
//! This module contains types for colors, borders, shadows, and other visual styling.

pub mod border;
pub mod border_radius;
pub mod box_border;
pub mod color;
pub mod color32;
pub mod decoration;
pub mod gradient;
pub mod hsl_hsv;
pub mod material_colors;
pub mod physical_model;
pub mod radius;
pub mod shadow;
pub mod shape_border;


// Re-exports for convenience
pub use border::{BorderPosition, BorderSide, BorderStyle};
pub use border_radius::{BorderRadius, BorderRadiusDirectional};
pub use box_border::{Border, BorderDirectional, BoxBorder};
pub use color::{Color, ParseColorError};
pub use color32::Color32;
pub use decoration::{
    BlendMode, BoxDecoration, BoxFit, ColorFilter, Decoration, DecorationImage, ImageRepeat,
};
pub use gradient::{
    Gradient, GradientRotation, GradientTransform, LinearGradient, RadialGradient, SweepGradient,
    TileMode,
};
pub use hsl_hsv::{HSLColor, HSVColor};
pub use material_colors::MaterialColors;
pub use physical_model::{Elevation, MaterialType, PhysicalShape};
pub use radius::Radius;
pub use shadow::{BoxShadow, Shadow, ShadowQuality};
pub use shape_border::{
    BeveledRectangleBorder, CircleBorder, ContinuousRectangleBorder, LinearBorder,
    LinearBorderEdges, OvalBorder, RoundedRectangleBorder, ShapeBorder, StadiumBorder, StarBorder,
};

