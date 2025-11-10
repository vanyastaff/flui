//! RenderDecoratedBox - paints decoration around a child

use flui_core::render::{Arity, LayoutContext, PaintContext, Render};
use flui_painting::{Canvas, Paint};
use flui_types::{
    styling::{BorderPosition, BoxDecoration, Radius},
    Offset, Point, RRect, Rect, Size,
};

/// Position of decoration relative to child
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecorationPosition {
    /// Paint decoration behind the child
    Background,
    /// Paint decoration in front of the child
    Foreground,
}

/// RenderObject that paints a decoration around its child
///
/// This renders backgrounds, borders, shadows, and gradients.
///
/// # Example
///
/// ```rust,ignore
/// use flui_rendering::RenderDecoratedBox;
/// use flui_types::styling::{BoxDecoration, Color};
///
/// let decoration = BoxDecoration {
///     color: Some(Color::WHITE),
///     border: None,
///     border_radius: None,
///     box_shadow: None,
///     gradient: None,
/// };
/// let mut decorated = RenderDecoratedBox::new(decoration, DecorationPosition::Background);
/// ```
#[derive(Debug)]
pub struct RenderDecoratedBox {
    /// The decoration to paint
    pub decoration: BoxDecoration,

    /// Position of decoration (background or foreground)
    pub position: DecorationPosition,

    // Cache for paint
    size: Size,
}

// ===== Public API =====

impl RenderDecoratedBox {
    /// Create new RenderDecoratedBox with background position
    pub fn new(decoration: BoxDecoration) -> Self {
        Self {
            decoration,
            position: DecorationPosition::Background,
            size: Size::ZERO,
        }
    }

    /// Create new RenderDecoratedBox with specified position
    pub fn with_position(decoration: BoxDecoration, position: DecorationPosition) -> Self {
        Self {
            decoration,
            position,
            size: Size::ZERO,
        }
    }

    /// Get the decoration
    pub fn decoration(&self) -> &BoxDecoration {
        &self.decoration
    }

    /// Get the decoration position
    pub fn position(&self) -> DecorationPosition {
        self.position
    }

    /// Set new decoration
    pub fn set_decoration(&mut self, decoration: BoxDecoration) {
        self.decoration = decoration;
    }

    /// Set decoration position
    pub fn set_position(&mut self, position: DecorationPosition) {
        self.position = position;
    }

    /// Paint decoration to canvas
    ///
    /// This draws the decoration (background, borders) directly to the canvas
    fn paint_decoration(&self, canvas: &mut Canvas, rect: Rect) {
        let decoration = &self.decoration;

        // TODO: Paint box shadows when shadow support is added to Canvas
        // For now, we skip shadows. A full implementation would add:
        // canvas.draw_shadow() method

        // Paint background (gradient or solid color)
        if let Some(ref _gradient) = decoration.gradient {
            // TODO: Gradient support not implemented yet in Canvas
            // When implemented, use canvas.draw_gradient()
        } else if let Some(color) = decoration.color {
            let border_radius = decoration.border_radius.map(|r| r.top_left.x);
            let paint = Paint::fill(color);

            if let Some(radius) = border_radius {
                let circular_radius = Radius::circular(radius);
                let rrect = RRect {
                    rect,
                    top_left: circular_radius,
                    top_right: circular_radius,
                    bottom_right: circular_radius,
                    bottom_left: circular_radius,
                };
                canvas.draw_rrect(rrect, &paint);
            } else {
                canvas.draw_rect(rect, &paint);
            }
        }

        // Paint border (if gradient or color was present, border goes on top)
        if let Some(ref border) = decoration.border {
            let border_radius = decoration.border_radius.map(|r| r.top_left.x);
            Self::paint_border(canvas, rect, border, border_radius);
        }
    }

    /// Paint border on canvas
    fn paint_border(
        canvas: &mut Canvas,
        rect: Rect,
        border: &flui_types::styling::Border,
        border_radius: Option<f32>,
    ) {
        // Paint each side that exists
        if let Some(top) = border.top {
            if top.is_visible() {
                Self::paint_border_side(canvas, rect, &top, BorderPosition::Top, border_radius);
            }
        }

        if let Some(right) = border.right {
            if right.is_visible() {
                Self::paint_border_side(
                    canvas,
                    rect,
                    &right,
                    BorderPosition::Right,
                    border_radius,
                );
            }
        }

        if let Some(bottom) = border.bottom {
            if bottom.is_visible() {
                Self::paint_border_side(
                    canvas,
                    rect,
                    &bottom,
                    BorderPosition::Bottom,
                    border_radius,
                );
            }
        }

        if let Some(left) = border.left {
            if left.is_visible() {
                Self::paint_border_side(canvas, rect, &left, BorderPosition::Left, border_radius);
            }
        }
    }

    /// Paint a single border side
    fn paint_border_side(
        canvas: &mut Canvas,
        rect: Rect,
        side: &flui_types::styling::BorderSide,
        position: BorderPosition,
        border_radius: Option<f32>,
    ) {
        let paint = Paint::stroke(side.color, side.width);

        // If we have rounded corners, draw using rounded rect
        if let Some(radius) = border_radius {
            // For rounded borders, we need to draw the full rounded rect outline
            // and then optionally mask individual sides (future enhancement)
            let circular_radius = Radius::circular(radius);
            let rrect = RRect {
                rect,
                top_left: circular_radius,
                top_right: circular_radius,
                bottom_right: circular_radius,
                bottom_left: circular_radius,
            };
            canvas.draw_rrect(rrect, &paint);
        } else {
            // For straight borders, draw individual lines for each side
            match position {
                BorderPosition::Top => {
                    let p1 = Point::new(rect.left(), rect.top() + side.width / 2.0);
                    let p2 = Point::new(rect.right(), rect.top() + side.width / 2.0);
                    canvas.draw_line(p1, p2, &paint);
                }
                BorderPosition::Right => {
                    let p1 = Point::new(rect.right() - side.width / 2.0, rect.top());
                    let p2 = Point::new(rect.right() - side.width / 2.0, rect.bottom());
                    canvas.draw_line(p1, p2, &paint);
                }
                BorderPosition::Bottom => {
                    let p1 = Point::new(rect.left(), rect.bottom() - side.width / 2.0);
                    let p2 = Point::new(rect.right(), rect.bottom() - side.width / 2.0);
                    canvas.draw_line(p1, p2, &paint);
                }
                BorderPosition::Left => {
                    let p1 = Point::new(rect.left() + side.width / 2.0, rect.top());
                    let p2 = Point::new(rect.left() + side.width / 2.0, rect.bottom());
                    canvas.draw_line(p1, p2, &paint);
                }
            }
        }
    }
}

// ===== RenderObject Implementation =====

impl Render for RenderDecoratedBox {
    fn layout(&mut self, ctx: &LayoutContext) -> Size {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let constraints = ctx.constraints;
        // SingleArity always has exactly one child
        let size = tree.layout_child(child_id, constraints);

        // Store size for paint
        self.size = size;

        size
    }

    fn paint(&self, ctx: &PaintContext) -> Canvas {
        let tree = ctx.tree;
        let child_id = ctx.children.single();
        let offset = ctx.offset;

        let mut canvas = Canvas::new();

        // Paint decoration in LOCAL coordinates at offset
        let rect = Rect::from_xywh(offset.dx, offset.dy, self.size.width, self.size.height);

        // Paint decoration in background position
        if self.position == DecorationPosition::Background {
            self.paint_decoration(&mut canvas, rect);
        }

        // Paint child
        let child_canvas = tree.paint_child(child_id, offset);
        canvas.append_canvas(child_canvas);

        // Paint decoration in foreground position
        if self.position == DecorationPosition::Foreground {
            self.paint_decoration(&mut canvas, rect);
        }

        canvas
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn arity(&self) -> Arity {
        Arity::Exact(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::styling::Color;

    #[test]
    fn test_render_decorated_box_new() {
        let decoration = BoxDecoration {
            color: Some(Color::WHITE),
            border: None,
            border_radius: None,
            box_shadow: None,
            gradient: None,
            image: None,
        };
        let decorated = RenderDecoratedBox::new(decoration.clone());
        assert_eq!(decorated.decoration(), &decoration);
        assert_eq!(decorated.position(), DecorationPosition::Background);
    }

    #[test]
    fn test_render_decorated_box_set_decoration() {
        let decoration1 = BoxDecoration {
            color: Some(Color::WHITE),
            border: None,
            border_radius: None,
            box_shadow: None,
            gradient: None,
            image: None,
        };
        let mut decorated = RenderDecoratedBox::new(decoration1);

        // Set decoration
        let decoration2 = BoxDecoration {
            color: Some(Color::BLACK),
            border: None,
            border_radius: None,
            box_shadow: None,
            gradient: None,
            image: None,
        };
        decorated.set_decoration(decoration2.clone());
        assert_eq!(decorated.decoration(), &decoration2);
    }

    #[test]
    fn test_render_decorated_box_set_position() {
        let decoration = BoxDecoration {
            color: Some(Color::WHITE),
            border: None,
            border_radius: None,
            box_shadow: None,
            gradient: None,
            image: None,
        };
        let mut decorated = RenderDecoratedBox::new(decoration);

        // Set position
        decorated.set_position(DecorationPosition::Foreground);
        assert_eq!(decorated.position(), DecorationPosition::Foreground);
    }

    #[test]
    fn test_decoration_position_variants() {
        // Test enum variants
        assert_eq!(
            DecorationPosition::Background,
            DecorationPosition::Background
        );
        assert_eq!(
            DecorationPosition::Foreground,
            DecorationPosition::Foreground
        );
        assert_ne!(
            DecorationPosition::Background,
            DecorationPosition::Foreground
        );
    }

    #[test]
    fn test_decorated_box_with_default_position() {
        let decoration = BoxDecoration {
            color: Some(Color::WHITE),
            border: None,
            border_radius: None,
            box_shadow: None,
            gradient: None,
            image: None,
        };
        let decorated = RenderDecoratedBox::new(decoration.clone());
        assert_eq!(decorated.decoration, decoration);
        assert_eq!(decorated.position, DecorationPosition::Background);
    }

    #[test]
    fn test_decorated_box_with_foreground_position() {
        let decoration = BoxDecoration {
            color: Some(Color::WHITE),
            border: None,
            border_radius: None,
            box_shadow: None,
            gradient: None,
            image: None,
        };
        let decorated =
            RenderDecoratedBox::with_position(decoration.clone(), DecorationPosition::Foreground);
        assert_eq!(decorated.decoration, decoration);
        assert_eq!(decorated.position, DecorationPosition::Foreground);}
}
