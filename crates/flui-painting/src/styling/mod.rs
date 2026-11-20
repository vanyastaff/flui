//! Styling types for Flui.
//!
//! This module contains types for colors, borders, shadows, and other visual
//! styling.

// Colour maths: r, g, b, a, h, s, l, v are the domain's names.
#![expect(
    clippy::many_single_char_names,
    reason = "r, g, b, a, h, s, l, v are the colour maths' domain names"
)]

mod border;
mod border_radius;
mod box_border;
mod box_decoration_paint;
mod color;
mod decoration;
mod gradient;
mod hsl_hsv;
mod lerp_retained;
mod physical_model;
mod shadow;
mod table_border;
mod table_border_paint;

pub use border::{BorderPosition, BorderSide, BorderStyle};
pub use border_radius::{BorderRadius, BorderRadiusDirectional, BorderRadiusExt};
pub use box_border::{Border, BorderDirectional, BoxBorder};
pub use box_decoration_paint::DecorationPaintOptions;
pub use color::{
    Color, Oklab, ParseColorError, PremultipliedOklab, linear_to_srgb, srgb_to_linear,
};
pub use decoration::{BoxDecoration, Decoration, DecorationImage};
pub(crate) use gradient::valid_bounds;
pub use gradient::{
    Gradient, GradientRotation, GradientTransform, LinearGradient, RadialGradient, SweepGradient,
};
pub use hsl_hsv::{HSLColor, HSVColor};
pub use physical_model::{Elevation, MaterialType, PhysicalShape};
pub use shadow::{BoxShadow, Shadow, ShadowQuality};
pub use table_border::TableBorder;
