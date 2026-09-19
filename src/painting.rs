//! Recording and custom-painter contracts for application-defined drawing.
//!
//! Implement [`CustomPainter`] and pass it to [`crate::widgets::CustomPaint`].
//! Geometry lives in [`crate::geometry`]; repaint notifications use
//! [`crate::foundation::Listenable`]. These are the existing framework types,
//! so a painter can move between a widget and a render object without adapters.

pub use flui_painting::{Canvas, DisplayList, DrawCommand, DrawOp};
pub use flui_rendering::delegates::{CustomPainter, SemanticsBuilder};
pub use flui_types::painting::{
    BlendMode, Paint, PaintBuilder, PaintStyle, PointMode, Shader, StrokeCap, StrokeJoin,
};
