//! CustomPaint widget for drawing custom graphics

use flui_core::render::{Arity, LayoutContext, PaintContext, Render};
use flui_core::view::{IntoElement, View};
use flui_core::BuildContext;
use flui_painting::{Canvas, Paint};
use flui_types::{BoxConstraints, Offset, Point, Size};
use std::fmt::Debug;
use std::sync::Arc;

/// Callback for custom painting
pub type PainterCallback = Arc<dyn Fn(&mut Canvas, Size, Offset) + Send + Sync>;

/// CustomPaint widget
///
/// Allows drawing custom graphics using a painter callback.
///
/// # Example
///
/// ```rust,ignore
/// use flui_widgets::prelude::*;
///
/// CustomPaint::builder()
///     .painter(|canvas, size, offset| {
///         // Draw custom graphics
///         let paint = Paint::fill(Color::RED);
///         canvas.draw_circle(Point::new(size.width / 2.0, size.height / 2.0), 50.0, &paint);
///     })
///     .size(Size::new(200.0, 200.0))
///     .build()
/// ```
#[derive(Clone)]
pub struct CustomPaint {
    /// Painter callback for custom drawing
    pub painter: Option<PainterCallback>,

    /// Optional size (defaults to parent constraints)
    pub size: Option<Size>,
}

impl CustomPaint {
    /// Create a new CustomPaint
    pub fn new() -> Self {
        Self {
            painter: None,
            size: None,
        }
    }

    /// Builder for CustomPaint
    pub fn builder() -> CustomPaintBuilder {
        CustomPaintBuilder::new()
    }
}

impl Default for CustomPaint {
    fn default() -> Self {
        Self::new()
    }
}

impl Debug for CustomPaint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CustomPaint")
            .field("has_painter", &self.painter.is_some())
            .field("size", &self.size)
            .finish()
    }
}

impl View for CustomPaint {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        (
            RenderCustomPaint {
                painter: self.painter,
                size: self.size.unwrap_or(Size::ZERO),
            },
            (),
        )
    }
}

/// RenderObject for CustomPaint
pub struct RenderCustomPaint {
    painter: Option<PainterCallback>,
    size: Size,
}

impl Debug for RenderCustomPaint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderCustomPaint")
            .field("has_painter", &self.painter.is_some())
            .field("size", &self.size)
            .finish()
    }
}

impl Render for RenderCustomPaint {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let constraints = ctx.constraints;

        // Use provided size if available, otherwise use max constraints
        if self.size != Size::ZERO {
            constraints.constrain(self.size)
        } else {
            Size::new(constraints.max_width, constraints.max_height)
        }
    }

    fn paint(&self, ctx: &PaintContext) -> Canvas {
        let mut canvas = Canvas::new();

        if let Some(ref painter) = self.painter {
            painter(&mut canvas, self.size, ctx.offset);
        }

        canvas
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Exact(0)
    }
}

/// Builder for CustomPaint widget
pub struct CustomPaintBuilder {
    painter: Option<PainterCallback>,
    size: Option<Size>,
}

impl CustomPaintBuilder {
    /// Create a new CustomPaintBuilder
    pub fn new() -> Self {
        Self {
            painter: None,
            size: None,
        }
    }

    /// Set the painter callback
    pub fn painter<F>(mut self, callback: F) -> Self
    where
        F: Fn(&mut Canvas, Size, Offset) + Send + Sync + 'static,
    {
        self.painter = Some(Arc::new(callback));
        self
    }

    /// Set the size
    pub fn size(mut self, size: Size) -> Self {
        self.size = Some(size);
        self
    }

    /// Build the CustomPaint
    pub fn build(self) -> CustomPaint {
        CustomPaint {
            painter: self.painter,
            size: self.size,
        }
    }
}

impl Default for CustomPaintBuilder {
    fn default() -> Self {
        Self::new()
    }
}
