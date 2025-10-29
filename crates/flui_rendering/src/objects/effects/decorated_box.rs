//! RenderDecoratedBox - paints decoration around a child

use flui_types::{Size, Rect, Point, styling::{BoxDecoration, BorderPosition}};
use flui_core::render::{RenderObject, SingleArity, LayoutCx, PaintCx, SingleChild, SingleChildPaint};
use flui_engine::{BoxedLayer, layer::pool, PictureLayer, Paint, RRect};

/// Position of the decoration relative to the child
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecorationPosition {
    /// Paint decoration behind the child
    Background,
    /// Paint decoration in front of the child
    Foreground,
}

/// Data for RenderDecoratedBox
#[derive(Debug, Clone, PartialEq)]
pub struct DecoratedBoxData {
    /// The decoration to paint
    pub decoration: BoxDecoration,
    /// Position of the decoration
    pub position: DecorationPosition,
}

impl DecoratedBoxData {
    /// Create new decorated box data
    pub fn new(decoration: BoxDecoration) -> Self {
        Self {
            decoration,
            position: DecorationPosition::Background,
        }
    }

    /// Create with specific position
    pub fn with_position(decoration: BoxDecoration, position: DecorationPosition) -> Self {
        Self {
            decoration,
            position,
        }
    }
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
/// let mut decorated = RenderDecoratedBox::new(DecoratedBoxData::new(decoration));
/// ```
#[derive(Debug)]
pub struct RenderDecoratedBox {
    /// Decoration data
    pub data: DecoratedBoxData,

    // Cache for paint
    size: Size,
}

// ===== Public API =====

impl RenderDecoratedBox {
    /// Create new RenderDecoratedBox
    pub fn new(data: DecoratedBoxData) -> Self {
        Self {
            data,
            size: Size::ZERO,
        }
    }

    /// Get the decoration
    pub fn decoration(&self) -> &BoxDecoration {
        &self.data.decoration
    }

    /// Get the decoration position
    pub fn position(&self) -> DecorationPosition {
        self.data.position
    }

    /// Set new decoration
    pub fn set_decoration(&mut self, decoration: BoxDecoration) {
        self.data.decoration = decoration;
    }

    /// Set decoration position
    pub fn set_position(&mut self, position: DecorationPosition) {
        self.data.position = position;
    }

    /// Paint decoration layers to container
    ///
    /// This creates appropriate layers for the decoration:
    /// - GradientLayer for gradients
    /// - PictureLayer for solid colors and borders
    fn paint_decoration(&self, container: &mut flui_engine::ContainerLayer, rect: Rect) {
        use flui_engine::GradientLayer;

        let decoration = &self.data.decoration;

        // TODO: Paint box shadows when shadow support is added
        // For now, we skip shadows. A full implementation would:
        // 1. Extract shadow parameters from decoration.box_shadow
        // 2. Create ShadowLayer
        // 3. Add it to the container before the background

        // Paint background (gradient or solid color)
        if let Some(ref gradient) = decoration.gradient {
            // Create GradientLayer for gradient background
            let gradient_layer = GradientLayer::new(rect, gradient.clone());
            container.add_child(Box::new(gradient_layer));
        } else if let Some(color) = decoration.color {
            // Create PictureLayer for solid color background
            let mut picture = PictureLayer::new();
            let border_radius = decoration.border_radius.map(|r| r.top_left.x);

            let paint = Paint {
                color: [
                    color.red() as f32 / 255.0,
                    color.green() as f32 / 255.0,
                    color.blue() as f32 / 255.0,
                    color.alpha() as f32 / 255.0,
                ],
                stroke_width: 0.0,
                anti_alias: true,
            };

            if let Some(radius) = border_radius {
                let rrect = RRect {
                    rect,
                    corner_radius: radius,
                };
                picture.draw_rrect(rrect, paint);
            } else {
                picture.draw_rect(rect, paint);
            }

            container.add_child(Box::new(picture));
        }

        // Paint border (if gradient or color was present, border goes on top)
        if let Some(ref border) = decoration.border {
            let mut picture = PictureLayer::new();
            let border_radius = decoration.border_radius.map(|r| r.top_left.x);
            Self::paint_border(&mut picture, rect, border, border_radius);
            container.add_child(Box::new(picture));
        }
    }

    /// Paint border on picture layer
    fn paint_border(picture: &mut PictureLayer, rect: Rect, border: &flui_types::styling::Border, border_radius: Option<f32>) {
        

        // Paint each side that exists
        if let Some(top) = border.top {
            if top.is_visible() {
                Self::paint_border_side(picture, rect, &top, BorderPosition::Top, border_radius);
            }
        }

        if let Some(right) = border.right {
            if right.is_visible() {
                Self::paint_border_side(picture, rect, &right, BorderPosition::Right, border_radius);
            }
        }

        if let Some(bottom) = border.bottom {
            if bottom.is_visible() {
                Self::paint_border_side(picture, rect, &bottom, BorderPosition::Bottom, border_radius);
            }
        }

        if let Some(left) = border.left {
            if left.is_visible() {
                Self::paint_border_side(picture, rect, &left, BorderPosition::Left, border_radius);
            }
        }
    }

    /// Paint a single border side
    fn paint_border_side(
        picture: &mut PictureLayer,
        rect: Rect,
        side: &flui_types::styling::BorderSide,
        position: BorderPosition,
        border_radius: Option<f32>,
    ) {
        let paint = Paint {
            color: [
                side.color.red() as f32 / 255.0,
                side.color.green() as f32 / 255.0,
                side.color.blue() as f32 / 255.0,
                side.color.alpha() as f32 / 255.0,
            ],
            stroke_width: side.width,
            anti_alias: true,
        };

        // If we have rounded corners, draw using rounded rect
        if let Some(radius) = border_radius {
            // For rounded borders, we need to draw the full rounded rect outline
            // and then optionally mask individual sides (future enhancement)
            let rrect = RRect {
                rect,
                corner_radius: radius,
            };
            picture.draw_rrect(rrect, paint);
        } else {
            // For straight borders, draw individual lines for each side
            match position {
                BorderPosition::Top => {
                    let p1 = Point::new(rect.left(), rect.top() + side.width / 2.0);
                    let p2 = Point::new(rect.right(), rect.top() + side.width / 2.0);
                    picture.draw_line(p1, p2, paint);
                }
                BorderPosition::Right => {
                    let p1 = Point::new(rect.right() - side.width / 2.0, rect.top());
                    let p2 = Point::new(rect.right() - side.width / 2.0, rect.bottom());
                    picture.draw_line(p1, p2, paint);
                }
                BorderPosition::Bottom => {
                    let p1 = Point::new(rect.left(), rect.bottom() - side.width / 2.0);
                    let p2 = Point::new(rect.right(), rect.bottom() - side.width / 2.0);
                    picture.draw_line(p1, p2, paint);
                }
                BorderPosition::Left => {
                    let p1 = Point::new(rect.left() + side.width / 2.0, rect.top());
                    let p2 = Point::new(rect.left() + side.width / 2.0, rect.bottom());
                    picture.draw_line(p1, p2, paint);
                }
            }
        }
    }
}

// ===== RenderObject Implementation =====

impl RenderObject for RenderDecoratedBox {
    type Arity = SingleArity;

    fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
        let constraints = cx.constraints();

        // SingleArity always has exactly one child
        let child = cx.child();
        let size = cx.layout_child(child, constraints);

        // Store size for paint
        self.size = size;

        size
    }

    fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
        let mut container = pool::acquire_container();
        let rect = Rect::from_xywh(0.0, 0.0, self.size.width, self.size.height);

        // Paint decoration in background position
        if self.data.position == DecorationPosition::Background {
            self.paint_decoration(&mut container, rect);
        }

        // Paint child on top
        let child = cx.child();
        let child_layer = cx.capture_child_layer(child);
        container.add_child(child_layer);

        // Paint decoration in foreground position
        if self.data.position == DecorationPosition::Foreground {
            self.paint_decoration(&mut container, rect);
        }

        Box::new(container)
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
        let decorated = RenderDecoratedBox::new(DecoratedBoxData::new(decoration.clone()));
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
        let mut decorated = RenderDecoratedBox::new(DecoratedBoxData::new(decoration1));

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
        let mut decorated = RenderDecoratedBox::new(DecoratedBoxData::new(decoration));

        // Set position
        decorated.set_position(DecorationPosition::Foreground);
        assert_eq!(decorated.position(), DecorationPosition::Foreground);
    }

    #[test]
    fn test_decoration_position_variants() {
        // Test enum variants
        assert_eq!(DecorationPosition::Background, DecorationPosition::Background);
        assert_eq!(DecorationPosition::Foreground, DecorationPosition::Foreground);
        assert_ne!(DecorationPosition::Background, DecorationPosition::Foreground);
    }

    #[test]
    fn test_decorated_box_data_new() {
        let decoration = BoxDecoration {
            color: Some(Color::WHITE),
            border: None,
            border_radius: None,
            box_shadow: None,
            gradient: None,
            image: None,
        };
        let data = DecoratedBoxData::new(decoration.clone());
        assert_eq!(data.decoration, decoration);
        assert_eq!(data.position, DecorationPosition::Background);
    }

    #[test]
    fn test_decorated_box_data_with_position() {
        let decoration = BoxDecoration {
            color: Some(Color::WHITE),
            border: None,
            border_radius: None,
            box_shadow: None,
            gradient: None,
            image: None,
        };
        let data = DecoratedBoxData::with_position(decoration.clone(), DecorationPosition::Foreground);
        assert_eq!(data.decoration, decoration);
        assert_eq!(data.position, DecorationPosition::Foreground);
    }
}
