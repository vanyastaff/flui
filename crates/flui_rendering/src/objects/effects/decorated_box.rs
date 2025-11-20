//! RenderDecoratedBox - paints decoration around a child

use flui_core::render::{BoxProtocol, LayoutContext, PaintContext};
use flui_core::render::{Optional, RenderBox};
use flui_painting::{Canvas, Paint};
use flui_types::{
    styling::{BorderPosition, BoxDecoration, Radius},
    Color, Point, RRect, Rect, Size,
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
/// # Without Child
///
/// When no child is present, still paints the decoration (useful for decorative boxes).
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

        // Paint box shadows first (they go behind the box)
        if let Some(ref shadows) = decoration.box_shadow {
            for shadow in shadows {
                if !shadow.inset {
                    Self::paint_box_shadow(canvas, rect, shadow, decoration.border_radius);
                }
            }
        }

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
                Self::paint_border_side(canvas, rect, &right, BorderPosition::Right, border_radius);
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

    /// Paint a box shadow
    fn paint_box_shadow(
        canvas: &mut Canvas,
        rect: Rect,
        shadow: &flui_types::styling::BoxShadow,
        border_radius: Option<flui_types::styling::BorderRadius>,
    ) {
        // Calculate shadow rect with offset and spread
        let shadow_rect = Rect::from_xywh(
            rect.left() + shadow.offset.dx - shadow.spread_radius,
            rect.top() + shadow.offset.dy - shadow.spread_radius,
            rect.width() + shadow.spread_radius * 2.0,
            rect.height() + shadow.spread_radius * 2.0,
        );

        // For simplicity, we'll render the shadow as a semi-transparent rectangle with blur effect
        // This is a simplified implementation - a full implementation would use proper blur
        let blur_steps = (shadow.blur_radius / 2.0).max(1.0) as usize;
        let alpha_step = shadow.color.a as f32 / (blur_steps as f32);

        for i in 0..blur_steps {
            let step_radius = (i as f32) * 2.0;
            let step_rect = Rect::from_xywh(
                shadow_rect.left() - step_radius,
                shadow_rect.top() - step_radius,
                shadow_rect.width() + step_radius * 2.0,
                shadow_rect.height() + step_radius * 2.0,
            );

            let step_alpha = ((blur_steps - i) as f32 * alpha_step) as u8;
            let step_color =
                Color::rgba(shadow.color.r, shadow.color.g, shadow.color.b, step_alpha);
            let paint = Paint::fill(step_color);

            if let Some(radius) = border_radius.map(|r| r.top_left.x + step_radius) {
                let circular_radius = Radius::circular(radius);
                let rrect = RRect {
                    rect: step_rect,
                    top_left: circular_radius,
                    top_right: circular_radius,
                    bottom_right: circular_radius,
                    bottom_left: circular_radius,
                };
                canvas.draw_rrect(rrect, &paint);
            } else {
                canvas.draw_rect(step_rect, &paint);
            }
        }
    }
}

// ===== RenderObject Implementation =====

impl RenderBox<Optional> for RenderDecoratedBox {
    fn layout(&mut self, ctx: LayoutContext<'_, Optional, BoxProtocol>) -> Size {
        let constraints = ctx.constraints;

        let size = if let Some(child_id) = ctx.children.get() {
            // Layout child and use its size
            ctx.layout_child(child_id, constraints)
        } else {
            // No child - use max constraints for decoration size
            Size::new(constraints.max_width, constraints.max_height)
        };

        // Store size for paint
        self.size = size;

        size
    }

    fn paint(&self, ctx: &mut PaintContext<'_, Optional>) {
        let offset = ctx.offset;

        // Paint decoration in LOCAL coordinates at offset
        let rect = Rect::from_xywh(offset.dx, offset.dy, self.size.width, self.size.height);

        // Paint decoration in background position
        if self.position == DecorationPosition::Background {
            self.paint_decoration(ctx.canvas(), rect);
        }

        // Paint child if present
        if let Some(child_id) = ctx.children.get() {
            ctx.paint_child(child_id, offset);
        }

        // Paint decoration in foreground position
        if self.position == DecorationPosition::Foreground {
            self.paint_decoration(ctx.canvas(), rect);
        }
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
        assert_eq!(decorated.position, DecorationPosition::Foreground);
    }
}
