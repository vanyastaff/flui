//! Painting utilities for custom rendering.
//!
//! This module provides utilities for painting decorations, shadows, and borders
//! using egui's painter API.

pub mod border_painter;
pub mod decoration_painter;
pub mod glow_shadow_painter;
pub mod shadow_painter;
pub mod transform_painter;



pub use decoration_painter::DecorationPainter;
pub use shadow_painter::ShadowPainter;
pub use glow_shadow_painter::GlowShadowPainter;
pub use border_painter::BorderPainter;
pub use transform_painter::TransformPainter;


