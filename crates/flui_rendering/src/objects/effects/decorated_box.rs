//! RenderDecoratedBox - paints decoration around a child

use flui_types::{Size, Rect, styling::BoxDecoration};
use flui_core::render::{RenderObject, SingleArity, LayoutCx, PaintCx, SingleChild, SingleChildPaint};
use flui_engine::{BoxedLayer, ContainerLayer, PictureLayer, Paint, RRect};

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

    /// Helper function to paint decoration into a PictureLayer
    ///
    /// This is a simplified version of BoxDecorationPainter that works with PictureLayer
    /// instead of the Painter trait. It handles:
    /// - Background color or gradient
    /// - Borders with rounded corners
    /// (Note: Box shadows are not yet implemented in this version)
    fn paint_decoration_to_picture(&self, picture: &mut PictureLayer, rect: Rect) {
        let decoration = &self.data.decoration;
        let border_radius = decoration.border_radius.map(|r| r.top_left.x);

        // TODO: Paint box shadows when shadow support is added to PictureLayer
        // For now, we skip shadows. A full implementation would:
        // 1. Extract shadow parameters from decoration.box_shadow
        // 2. Create shadow draw commands
        // 3. Add them to the picture before the background

        // Paint background (color or gradient)
        if let Some(ref _gradient) = decoration.gradient {
            // TODO: Implement gradient support
            // For now, we skip gradients and just paint solid color if available
            if let Some(color) = decoration.color {
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
            }
        } else if let Some(color) = decoration.color {
            // Solid color background
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
        }

        // TODO: Paint border
        // For now, we skip borders. A full implementation would:
        // 1. Extract border parameters from decoration.border
        // 2. Use BorderPainter or create border draw commands
        // 3. Add them to the picture after the background
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
        let mut container = ContainerLayer::new();
        let rect = Rect::from_xywh(0.0, 0.0, self.size.width, self.size.height);

        // Paint decoration in background position
        if self.data.position == DecorationPosition::Background {
            let mut picture = PictureLayer::new();
            self.paint_decoration_to_picture(&mut picture, rect);
            container.add_child(Box::new(picture));
        }

        // Paint child on top
        let child = cx.child();
        let child_layer = cx.capture_child_layer(child);
        container.add_child(child_layer);

        // Paint decoration in foreground position
        if self.data.position == DecorationPosition::Foreground {
            let mut picture = PictureLayer::new();
            self.paint_decoration_to_picture(&mut picture, rect);
            container.add_child(Box::new(picture));
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
        };
        let mut decorated = RenderDecoratedBox::new(DecoratedBoxData::new(decoration1));

        // Set decoration
        let decoration2 = BoxDecoration {
            color: Some(Color::BLACK),
            border: None,
            border_radius: None,
            box_shadow: None,
            gradient: None,
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
        };
        let data = DecoratedBoxData::with_position(decoration.clone(), DecorationPosition::Foreground);
        assert_eq!(data.decoration, decoration);
        assert_eq!(data.position, DecorationPosition::Foreground);
    }
}
