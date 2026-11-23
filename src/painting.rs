//! Recording and custom-painter contracts for application-defined drawing.
//!
//! Implement [`CustomPainter`] and pass it to [`crate::widgets::CustomPaint`].
//! Geometry lives in [`crate::geometry`]; repaint notifications use
//! [`crate::foundation::Listenable`]. A painter draws through the
//! [`CustomPaintContext`] its `paint` receives: the canvas, the size, and the UI
//! runtime's text context, lent by [`CustomPaintContext::text`] to lay out a
//! [`TextPainter`] for a label. These are the existing framework types,
//! so a painter can move between a widget and a render object without adapters.
//!
//! A render object of its own lays text out with a [`TextPainter`], measured
//! through the [`TextMeasurement`] that its layout context lends
//! ([`TextContextMut::measurement`](crate::rendering::TextContextMut::measurement)),
//! and paints it onto the [`Canvas`] or draws a [`ShapedParagraph`] with
//! [`Canvas::draw_paragraph`].

pub use flui_painting::{
    Alignment, AlignmentDirectional, AlignmentGeometry, BoxFit, BoxShape, FittedSizes, TextSizing,
};
pub use flui_painting::{
    BlurSigma, Canvas, DisplayList, DrawCommand, DrawCommandBuf, DrawCommands, DrawOp,
    ImagePlacement, ImageRegionPlacement, NestedRRects, Payload, PayloadId,
};
pub use flui_painting::{
    Invalidation, ShapedParagraph, TextLayoutError, TextMeasurement, TextMeasurementError,
    TextPainter, TextPreparationPending, TextResolvedSize,
};
/// Paint, style and text values: paths, shaders, colours, borders, decorations, text styles.
pub use flui_painting::{paint, styling, typography};
pub use flui_rendering::delegates::{CustomPaintContext, CustomPainter};
pub use flui_semantics::{
    SemanticsRegion, SemanticsRegionId, SemanticsRegionKey, SemanticsRegions,
};
