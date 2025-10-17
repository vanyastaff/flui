//! Visual styling types.
//!
//! This module contains types for visual appearance:
//! - [`Gradient`]: Linear and radial gradients
//! - [`Border`]: Border styling with color and width
//! - [`BorderRadius`]: Corner radius for rounded borders
//! - [`Shadow`]: Drop shadows and box shadows
//! - [`BoxDecoration`]: Combined decoration (color, gradient, border, shadow)
//! - [`Clip`]: Clipping behavior
//!
//! Note: [`Color`] and [`Opacity`] are in [`crate::types::core`] as they are core primitives.

pub mod blend_mode;
pub mod border;
pub mod border_radius;
pub mod clip;
pub mod decoration;
pub mod gradient;
pub mod shadow;
pub mod stroke;



// Re-export types for convenience
pub use blend_mode::BlendMode;
pub use border::{Border, BorderSide};
pub use border_radius::{BorderRadius, Radius};
pub use clip::Clip;
pub use decoration::{BoxDecoration, ShapeDecoration};
pub use gradient::{Gradient, LinearGradient, RadialGradient};
pub use shadow::{BoxShadow, Shadow, BlurStyle};
pub use stroke::{StrokeCap, StrokeJoin, StrokeStyle};

/// Prelude module for convenient imports of commonly used styling types
pub mod prelude {
    pub use super::{
        BoxDecoration, Border, BorderRadius, BorderSide, Radius,
        BoxShadow, Shadow, BlurStyle, Clip,
        Gradient, LinearGradient, RadialGradient,
        BlendMode, StrokeCap, StrokeJoin, StrokeStyle,
    };
}


















