//! Recording and custom-painter contracts for application-defined drawing.
//!
//! Implement [`CustomPainter`] and pass it to [`crate::widgets::CustomPaint`].
//! Geometry lives in [`crate::geometry`]; repaint notifications use
//! [`crate::foundation::Listenable`]. These are the existing framework types,
//! so a painter can move between a widget and a render object without adapters.

pub use flui_painting::paint::{
    BlendMode, Paint, PaintBuilder, PaintStyle, PointMode, Shader, StrokeCap, StrokeJoin,
};
pub use flui_painting::{
    Alignment, AlignmentDirectional, AlignmentGeometry, BoxFit, BoxShape, FittedSizes,
    TextBaseline, TextSizing,
};
pub use flui_painting::{Canvas, DisplayList, DrawCommand, DrawOp};
/// Paint, style and text values: paths, shaders, colours, borders, decorations, text styles.
pub use flui_painting::{paint, styling, typography};
pub use flui_rendering::delegates::{CustomPainter, SemanticsBuilder};
