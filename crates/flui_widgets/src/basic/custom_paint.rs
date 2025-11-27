//! CustomPaint widget for drawing custom graphics

use flui_core::render::{BoxProtocol, LayoutContext, Leaf, PaintContext, RenderBox, RenderBoxExt};
use flui_core::view::{IntoElement, StatelessView};
use flui_core::BuildContext;
use flui_painting::Canvas;
use flui_types::{Offset, Size};
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
///         canvas.circle(Point::new(size.width / 2.0, size.height / 2.0), 50.0, &paint);
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

impl StatelessView for CustomPaint {
    fn build(self, _ctx: &dyn BuildContext) -> impl IntoElement {
        RenderCustomPaintLeaf {
            painter: self.painter,
            size: self.size.unwrap_or(Size::ZERO),
        }
        .leaf()
    }
}

/// RenderObject for CustomPaint (leaf version with callback)
pub struct RenderCustomPaintLeaf {
    painter: Option<PainterCallback>,
    size: Size,
}

impl Debug for RenderCustomPaintLeaf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderCustomPaintLeaf")
            .field("has_painter", &self.painter.is_some())
            .field("size", &self.size)
            .finish()
    }
}

impl RenderBox<Leaf> for RenderCustomPaintLeaf {
    fn layout(&mut self, ctx: LayoutContext<'_, Leaf, BoxProtocol>) -> Size {
        let constraints = ctx.constraints;

        // Use provided size if available, otherwise use max constraints
        if self.size != Size::ZERO {
            constraints.constrain(self.size)
        } else {
            Size::new(constraints.max_width, constraints.max_height)
        }
    }

    fn paint(&self, ctx: &mut PaintContext<'_, Leaf>) {
        if let Some(ref painter) = self.painter {
            let offset = ctx.offset;
            painter(ctx.canvas(), self.size, offset);
        }
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
