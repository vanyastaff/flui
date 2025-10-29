//! Gradient painting implementation

use flui_engine::Painter;
use flui_types::{
    Point, Rect,
    styling::{Gradient, LinearGradient, RadialGradient, SweepGradient},
};

/// Painter for gradients
pub struct GradientPainter;

impl GradientPainter {
    /// Paint a gradient
    ///
    /// # Arguments
    ///
    /// * `painter` - The backend-agnostic painter to draw with
    /// * `rect` - The rectangle to paint the gradient in
    /// * `gradient` - The gradient to paint
    pub fn paint(painter: &mut dyn Painter, rect: Rect, gradient: &Gradient) {
        match gradient {
            Gradient::Linear(linear) => Self::paint_linear(painter, rect, linear),
            Gradient::Radial(radial) => Self::paint_radial(painter, rect, radial),
            Gradient::Sweep(sweep) => Self::paint_sweep(painter, rect, sweep),
        }
    }

    /// Paint a linear gradient
    fn paint_linear(painter: &mut dyn Painter, rect: Rect, gradient: &LinearGradient) {
        if gradient.colors.is_empty() {
            return;
        }

        if gradient.colors.len() == 1 {
            // Single color - just fill
            let paint = flui_engine::Paint::fill(gradient.colors[0]);
            painter.rect(rect, &paint);
            return;
        }

        // Check if it's horizontal or vertical gradient
        let is_horizontal = (gradient.begin.x - 0.0).abs() < 0.01
            && (gradient.end.x - 1.0).abs() < 0.01
            && (gradient.begin.y - gradient.end.y).abs() < 0.01;

        let is_vertical = (gradient.begin.y - 0.0).abs() < 0.01
            && (gradient.end.y - 1.0).abs() < 0.01
            && (gradient.begin.x - gradient.end.x).abs() < 0.01;

        // For now, only support 2-color gradients through Painter trait methods
        // TODO: Support multi-stop gradients with custom mesh rendering
        let start_color = gradient.colors[0];
        let end_color = *gradient.colors.last().unwrap();

        if is_horizontal {
            painter.horizontal_gradient(rect, start_color, end_color);
        } else if is_vertical {
            painter.vertical_gradient(rect, start_color, end_color);
        } else {
            // Diagonal - approximate with vertical for now
            // TODO: Implement angled gradients
            painter.vertical_gradient(rect, start_color, end_color);
        }
    }

    /// Paint a radial gradient
    fn paint_radial(painter: &mut dyn Painter, rect: Rect, gradient: &RadialGradient) {
        if gradient.colors.is_empty() {
            return;
        }

        if gradient.colors.len() == 1 {
            // Single color - just fill
            let paint = flui_engine::Paint::fill(gradient.colors[0]);
            painter.rect(rect, &paint);
            return;
        }

        // Calculate center position
        let center_x = rect.left() + rect.width() * gradient.center.x;
        let center_y = rect.top() + rect.height() * gradient.center.y;
        let center = Point::new(center_x, center_y);

        // Calculate radius
        let max_dim = rect.width().max(rect.height());
        let inner_radius = 0.0;
        let outer_radius = gradient.radius * max_dim;

        let start_color = gradient.colors[0];
        let end_color = *gradient.colors.last().unwrap();

        painter.radial_gradient_simple(center, inner_radius, outer_radius, start_color, end_color);
    }

    /// Paint a sweep gradient (conical gradient)
    fn paint_sweep(painter: &mut dyn Painter, rect: Rect, _gradient: &SweepGradient) {
        // TODO: Implement sweep gradients
        // For now, just fill with first color as a fallback
        if let Some(first_color) = _gradient.colors.first() {
            let paint = flui_engine::Paint::fill(*first_color);
            painter.rect(rect, &paint);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::styling::Color;

    #[test]
    fn test_linear_gradient_horizontal() {
        // Test horizontal gradient
        let gradient = LinearGradient::horizontal(vec![Color::RED, Color::BLUE]);
        assert_eq!(gradient.colors.len(), 2);
        assert_eq!(gradient.colors[0], Color::RED);
        assert_eq!(gradient.colors[1], Color::BLUE);
    }

    #[test]
    fn test_linear_gradient_vertical() {
        // Test vertical gradient
        let gradient = LinearGradient::vertical(vec![Color::GREEN, Color::YELLOW]);
        assert_eq!(gradient.colors.len(), 2);
        assert_eq!(gradient.colors[0], Color::GREEN);
        assert_eq!(gradient.colors[1], Color::YELLOW);
    }

    #[test]
    fn test_linear_gradient_multi_stop() {
        // Test gradient with multiple colors
        let colors = vec![Color::RED, Color::GREEN, Color::BLUE, Color::YELLOW];
        let gradient = LinearGradient::horizontal(colors.clone());
        assert_eq!(gradient.colors.len(), 4);
        assert_eq!(gradient.colors, colors);
    }

    #[test]
    fn test_radial_gradient() {
        // Test radial gradient
        let gradient = RadialGradient::centered(1.0, vec![Color::WHITE, Color::BLACK]);
        assert_eq!(gradient.colors.len(), 2);
        assert_eq!(gradient.colors[0], Color::WHITE);
        assert_eq!(gradient.colors[1], Color::BLACK);
    }

    #[test]
    fn test_sweep_gradient() {
        // Test sweep gradient
        let gradient = SweepGradient::centered(vec![Color::RED, Color::GREEN, Color::BLUE]);
        assert_eq!(gradient.colors.len(), 3);
        assert_eq!(gradient.colors[0], Color::RED);
        assert_eq!(gradient.colors[1], Color::GREEN);
        assert_eq!(gradient.colors[2], Color::BLUE);
    }

    #[test]
    fn test_gradient_enum_linear() {
        // Test Gradient enum with linear
        let gradient = Gradient::Linear(LinearGradient::horizontal(vec![Color::RED, Color::BLUE]));

        match gradient {
            Gradient::Linear(linear) => {
                assert_eq!(linear.colors.len(), 2);
            }
            _ => panic!("Expected linear gradient"),
        }
    }

    #[test]
    fn test_gradient_enum_radial() {
        // Test Gradient enum with radial
        let gradient = Gradient::Radial(RadialGradient::centered(
            1.0,
            vec![Color::WHITE, Color::BLACK],
        ));

        match gradient {
            Gradient::Radial(radial) => {
                assert_eq!(radial.colors.len(), 2);
            }
            _ => panic!("Expected radial gradient"),
        }
    }

    #[test]
    fn test_gradient_enum_sweep() {
        // Test Gradient enum with sweep
        let gradient = Gradient::Sweep(SweepGradient::centered(vec![
            Color::RED,
            Color::GREEN,
            Color::BLUE,
        ]));

        match gradient {
            Gradient::Sweep(sweep) => {
                assert_eq!(sweep.colors.len(), 3);
            }
            _ => panic!("Expected sweep gradient"),
        }
    }

    #[test]
    fn test_gradient_single_color() {
        // Test gradient with single color (should still work)
        let gradient = LinearGradient::horizontal(vec![Color::RED]);
        assert_eq!(gradient.colors.len(), 1);
        assert_eq!(gradient.colors[0], Color::RED);
    }

    #[test]
    fn test_gradient_empty_colors() {
        // Test gradient with no colors (edge case)
        let gradient = LinearGradient::horizontal(vec![]);
        assert_eq!(gradient.colors.len(), 0);
    }

    #[test]
    fn test_color_interpolation() {
        // Test that we can create gradients with various colors
        let colors = vec![
            Color::BLACK,
            Color::WHITE,
            Color::RED,
            Color::GREEN,
            Color::BLUE,
            Color::YELLOW,
            Color::TRANSPARENT,
        ];

        for color in colors {
            let gradient = LinearGradient::horizontal(vec![color, Color::WHITE]);
            assert_eq!(gradient.colors[0], color);
        }
    }

    #[test]
    fn test_rect_for_gradient() {
        // Test that rects work properly with gradients
        let rect = Rect::from_xywh(10.0, 20.0, 100.0, 150.0);

        assert_eq!(rect.left(), 10.0);
        assert_eq!(rect.top(), 20.0);
        assert_eq!(rect.width(), 100.0);
        assert_eq!(rect.height(), 150.0);
        assert_eq!(rect.right(), 110.0);
        assert_eq!(rect.bottom(), 170.0);
    }
}
