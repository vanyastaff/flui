//! The context a [`CustomPainter`](super::CustomPainter) paints through.
//!
//! A painter draws on the node's canvas at the node's laid-out size and, when
//! it draws text, shapes it through the UI runtime's text context, lent for
//! the call as layout lends it. FLUI has no process-wide font system, so a
//! painter has no other place to shape text from.

use flui_foundation::Arity;
use flui_foundation::geometry::Size;
use flui_painting::Canvas;

use crate::context::PaintContext;
use crate::error::{RenderError, RenderResult};
use crate::pipeline::{TextContextMut, TextSource, lend_text};

/// What a [`CustomPainter::paint`](super::CustomPainter::paint) call draws
/// with: the canvas, the size to fill and, on demand, the text context.
///
/// `RenderCustomPaint` builds one from its own [`PaintContext`] for each
/// painter, so the painter's text loan is the node's: a painter that calls
/// [`Self::text`] is recorded as a text measurer and repaints when a font is
/// registered, and one that never calls it costs nothing.
pub struct CustomPaintContext<'a> {
    canvas: &'a mut Canvas,
    /// `None` for a context built apart from a paint pass
    /// ([`Self::without_text`]).
    text: Option<TextSource<'a>>,
    size: Size,
}

impl std::fmt::Debug for CustomPaintContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The canvas is the live recording; report what the painter is given.
        f.debug_struct("CustomPaintContext")
            .field("size", &self.size)
            .field("has_text", &self.text.is_some())
            .finish_non_exhaustive()
    }
}

impl<'a> CustomPaintContext<'a> {
    /// A context over `paint`'s canvas, its node's text source and its
    /// laid-out size: what a render object hands the painters it delegates
    /// to.
    pub fn new<A: Arity>(paint: &'a mut PaintContext<'_, A>) -> Self {
        let size = paint.size();
        let (canvas, text) = paint.canvas_and_text();
        Self {
            canvas,
            text: Some(text),
            size,
        }
    }

    /// A context over `canvas` alone, with no UI runtime behind it: a test or
    /// tool that records a painter into a canvas of its own. [`Self::text`]
    /// refuses with [`RenderError::TextContextUnavailable`].
    pub fn without_text(canvas: &'a mut Canvas, size: Size) -> Self {
        Self {
            canvas,
            text: None,
            size,
        }
    }

    /// The canvas, with its origin at the top left of the box.
    pub fn canvas(&mut self) -> &mut Canvas {
        self.canvas
    }

    /// The size of the box to paint; paint stays inside it.
    pub fn size(&self) -> Size {
        self.size
    }

    /// The UI runtime's text context, lent for as long as the returned
    /// [`TextContextMut`] lives, to lay out a
    /// [`TextPainter`](flui_painting::TextPainter) through its
    /// [`measurement`](TextContextMut::measurement).
    ///
    /// Asking records the painting node as one that measured text, so
    /// registering a font paints it again. The loan borrows this context:
    /// shape first, then draw on [`Self::canvas`].
    ///
    /// # Errors
    ///
    /// [`RenderError::TextContextBusy`] when an independently held loan
    /// already uses the runtime's text context, and
    /// [`RenderError::TextContextUnavailable`] for a context built
    /// [`without_text`](Self::without_text).
    pub fn text(&mut self) -> RenderResult<TextContextMut<'_>> {
        lend_text(self.text.ok_or(RenderError::TextContextUnavailable)?)
    }
}
