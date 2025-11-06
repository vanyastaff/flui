//! BoxDecoration painting implementation

use crate::{BorderPainter, GradientPainter, ShadowPainter};
use flui_engine::{Paint, Painter, RRect};
use flui_types::{styling::BoxDecoration, Rect};

/// Painter for BoxDecoration
///
/// Handles painting of:
/// - Background color
/// - Gradient backgrounds
/// - Box shadows
/// - Borders with rounded corners
pub struct BoxDecorationPainter;

impl BoxDecorationPainter {
    /// Paint a box decoration
    ///
    /// # Arguments
    ///
    /// * `painter` - The backend-agnostic painter to draw with
    /// * `rect` - The rectangle to paint the decoration in
    /// * `decoration` - The box decoration to paint
    ///
    /// # Painting Order
    ///
    /// 1. Box shadows (behind everything)
    /// 2. Background color or gradient
    /// 3. Border (on top)
    pub fn paint(painter: &mut dyn Painter, rect: Rect, decoration: &BoxDecoration) {
        let border_radius = decoration.border_radius.map(|r| r.top_left.x);

        // 1. Paint box shadows (if any)
        if let Some(ref shadows) = decoration.box_shadow {
            ShadowPainter::paint(painter, rect, shadows, border_radius);
        }

        // 2. Paint background (color or gradient)
        if let Some(ref gradient) = decoration.gradient {
            // Gradient takes precedence over color
            GradientPainter::paint(painter, rect, gradient);
        } else if let Some(color) = decoration.color {
            // Solid color background
            let paint = Paint::fill(color);

            if let Some(_radius) = border_radius {
                // Rounded rectangle
                let rrect = RRect {
                    rect,
                    top_left: decoration.border_radius.unwrap().top_left,
                    top_right: decoration.border_radius.unwrap().top_right,
                    bottom_right: decoration.border_radius.unwrap().bottom_right,
                    bottom_left: decoration.border_radius.unwrap().bottom_left,
                };
                painter.rrect(rrect, &paint);
            } else {
                // Sharp rectangle
                painter.rect(rect, &paint);
            }
        }

        // 3. Paint border (if any)
        if let Some(ref border) = decoration.border {
            BorderPainter::paint(painter, rect, border, decoration.border_radius);
        }
    }

    /// Paint just the background (color or gradient) without shadows or border
    ///
    /// Useful for optimizations when shadows/border are not needed
    pub fn paint_background(painter: &mut dyn Painter, rect: Rect, decoration: &BoxDecoration) {
        let border_radius = decoration.border_radius.map(|r| r.top_left.x);

        if let Some(ref gradient) = decoration.gradient {
            GradientPainter::paint(painter, rect, gradient);
        } else if let Some(color) = decoration.color {
            let paint = Paint::fill(color);

            if let Some(_radius) = border_radius {
                let rrect = RRect {
                    rect,
                    top_left: decoration.border_radius.unwrap().top_left,
                    top_right: decoration.border_radius.unwrap().top_right,
                    bottom_right: decoration.border_radius.unwrap().bottom_right,
                    bottom_left: decoration.border_radius.unwrap().bottom_left,
                };
                painter.rrect(rrect, &paint);
            } else {
                painter.rect(rect, &paint);
            }
        }
    }

    /// Paint just the border without background or shadows
    ///
    /// Useful for optimizations when only border is needed
    pub fn paint_border(painter: &mut dyn Painter, rect: Rect, decoration: &BoxDecoration) {
        if let Some(ref border) = decoration.border {
            BorderPainter::paint(painter, rect, border, decoration.border_radius);
        }
    }

    /// Paint just the shadows without background or border
    ///
    /// Useful for optimizations when only shadows are needed
    pub fn paint_shadows(painter: &mut dyn Painter, rect: Rect, decoration: &BoxDecoration) {
        if let Some(ref shadows) = decoration.box_shadow {
            let border_radius = decoration.border_radius.map(|r| r.top_left.x);
            ShadowPainter::paint(painter, rect, shadows, border_radius);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::{
        styling::{
            Border, BorderRadius, BorderSide, BorderStyle, BoxShadow, Color, Gradient,
            LinearGradient,
        },
        Offset,
    };

    #[test]
    fn test_box_decoration_with_color() {
        // Test decoration with solid color
        let decoration = BoxDecoration {
            color: Some(Color::WHITE),
            border: None,
            border_radius: None,
            box_shadow: None,
            gradient: None,
        };

        assert_eq!(decoration.color, Some(Color::WHITE));
        assert!(decoration.border.is_none());
        assert!(decoration.gradient.is_none());
    }

    #[test]
    fn test_box_decoration_with_border() {
        // Test decoration with border
        let border = Border::all(BorderSide::new(Color::BLACK, 1.0, BorderStyle::Solid));

        let decoration = BoxDecoration {
            color: Some(Color::WHITE),
            border: Some(border),
            border_radius: None,
            box_shadow: None,
            gradient: None,
        };

        assert!(decoration.border.is_some());
        assert_eq!(decoration.border.unwrap().top.unwrap().width, 1.0);
    }

    #[test]
    fn test_box_decoration_with_border_radius() {
        // Test decoration with rounded corners
        let decoration = BoxDecoration {
            color: Some(Color::WHITE),
            border: None,
            border_radius: Some(BorderRadius::circular(10.0)),
            box_shadow: None,
            gradient: None,
        };

        assert!(decoration.border_radius.is_some());
        let radius = decoration.border_radius.unwrap();
        assert_eq!(radius.top_left.x, 10.0);
    }

    #[test]
    fn test_box_decoration_with_shadow() {
        // Test decoration with box shadow
        let shadow = BoxShadow {
            color: Color::rgba(0, 0, 0, 64),
            offset: Offset::new(2.0, 2.0),
            blur_radius: 4.0,
            spread_radius: 0.0,
        };

        let decoration = BoxDecoration {
            color: Some(Color::WHITE),
            border: None,
            border_radius: None,
            box_shadow: Some(vec![shadow]),
            gradient: None,
        };

        assert!(decoration.box_shadow.is_some());
        assert_eq!(decoration.box_shadow.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_box_decoration_with_gradient() {
        // Test decoration with gradient
        let gradient = Gradient::Linear(LinearGradient::horizontal(vec![Color::RED, Color::BLUE]));

        let decoration = BoxDecoration {
            color: Some(Color::WHITE), // Color should be ignored when gradient is present
            border: None,
            border_radius: None,
            box_shadow: None,
            gradient: Some(gradient),
        };

        assert!(decoration.gradient.is_some());
        assert!(decoration.color.is_some()); // Still set, but gradient takes precedence
    }

    #[test]
    fn test_box_decoration_all_features() {
        // Test decoration with all features combined
        let border = Border::all(BorderSide::new(Color::BLACK, 2.0, BorderStyle::Solid));
        let shadow = BoxShadow {
            color: Color::rgba(0, 0, 0, 64),
            offset: Offset::new(0.0, 2.0),
            blur_radius: 4.0,
            spread_radius: 0.0,
        };
        let gradient = Gradient::Linear(LinearGradient::vertical(vec![
            Color::rgb(255, 100, 100),
            Color::rgb(100, 100, 255),
        ]));

        let decoration = BoxDecoration {
            color: Some(Color::WHITE),
            border: Some(border),
            border_radius: Some(BorderRadius::circular(8.0)),
            box_shadow: Some(vec![shadow]),
            gradient: Some(gradient),
        };

        assert!(decoration.color.is_some());
        assert!(decoration.border.is_some());
        assert!(decoration.border_radius.is_some());
        assert!(decoration.box_shadow.is_some());
        assert!(decoration.gradient.is_some());
    }

    #[test]
    fn test_box_decoration_multiple_shadows() {
        // Test decoration with multiple shadows
        let shadows = vec![
            BoxShadow {
                color: Color::rgba(0, 0, 0, 64),
                offset: Offset::new(0.0, 1.0),
                blur_radius: 2.0,
                spread_radius: 0.0,
            },
            BoxShadow {
                color: Color::rgba(0, 0, 0, 32),
                offset: Offset::new(0.0, 4.0),
                blur_radius: 8.0,
                spread_radius: 0.0,
            },
        ];

        let decoration = BoxDecoration {
            color: Some(Color::WHITE),
            border: None,
            border_radius: None,
            box_shadow: Some(shadows.clone()),
            gradient: None,
        };

        assert_eq!(decoration.box_shadow.unwrap().len(), 2);
    }

    #[test]
    fn test_box_decoration_empty() {
        // Test decoration with no features (fully transparent)
        let decoration = BoxDecoration {
            color: None,
            border: None,
            border_radius: None,
            box_shadow: None,
            gradient: None,
        };

        assert!(decoration.color.is_none());
        assert!(decoration.border.is_none());
        assert!(decoration.border_radius.is_none());
        assert!(decoration.box_shadow.is_none());
        assert!(decoration.gradient.is_none());
    }

    #[test]
    fn test_box_decoration_with_transparent_color() {
        // Test decoration with transparent color
        let decoration = BoxDecoration {
            color: Some(Color::TRANSPARENT),
            border: None,
            border_radius: None,
            box_shadow: None,
            gradient: None,
        };

        assert_eq!(decoration.color, Some(Color::TRANSPARENT));
    }

    #[test]
    fn test_rect_for_decoration() {
        // Test rect setup for decoration painting
        let rect = Rect::from_xywh(10.0, 20.0, 100.0, 150.0);

        assert_eq!(rect.left(), 10.0);
        assert_eq!(rect.top(), 20.0);
        assert_eq!(rect.width(), 100.0);
        assert_eq!(rect.height(), 150.0);
        assert_eq!(rect.right(), 110.0);
        assert_eq!(rect.bottom(), 170.0);
    }
}
