//! Gradient painting implementation

use flui_types::{Rect, styling::{Gradient, LinearGradient, RadialGradient, SweepGradient}};

/// Painter for gradients
pub struct GradientPainter;

impl GradientPainter {
    /// Paint a gradient
    ///
    /// # Arguments
    ///
    /// * `painter` - The egui painter to draw with
    /// * `rect` - The rectangle to paint the gradient in
    /// * `gradient` - The gradient to paint
    pub fn paint(
        painter: &egui::Painter,
        rect: Rect,
        gradient: &Gradient,
    ) {
        match gradient {
            Gradient::Linear(linear) => Self::paint_linear(painter, rect, linear),
            Gradient::Radial(radial) => Self::paint_radial(painter, rect, radial),
            Gradient::Sweep(sweep) => Self::paint_sweep(painter, rect, sweep),
        }
    }

    /// Paint a linear gradient
    fn paint_linear(
        painter: &egui::Painter,
        rect: Rect,
        gradient: &LinearGradient,
    ) {
        // Convert colors to egui
        let colors: Vec<egui::Color32> = gradient
            .colors
            .iter()
            .map(|c| egui::Color32::from_rgba_unmultiplied(
                c.red(),
                c.green(),
                c.blue(),
                c.alpha(),
            ))
            .collect();

        if colors.is_empty() {
            return;
        }

        // For now, use egui's simple gradient support
        // egui doesn't have full gradient support yet, so we'll approximate with a mesh

        // Simple case: two colors
        if colors.len() == 2 {
            let egui_rect = egui::Rect::from_min_max(
                egui::pos2(rect.left(), rect.top()),
                egui::pos2(rect.right(), rect.bottom()),
            );

            // Determine gradient direction
            let (_start, _end) = if gradient.begin.x == 0.0 && gradient.end.x == 1.0 {
                // Horizontal gradient
                (egui_rect.left_top(), egui_rect.right_top())
            } else if gradient.begin.y == 0.0 && gradient.end.y == 1.0 {
                // Vertical gradient
                (egui_rect.left_top(), egui_rect.left_bottom())
            } else {
                // Diagonal or custom - use begin/end alignment
                let start = egui::pos2(
                    rect.left() + rect.width() * gradient.begin.x,
                    rect.top() + rect.height() * gradient.begin.y,
                );
                let end = egui::pos2(
                    rect.left() + rect.width() * gradient.end.x,
                    rect.top() + rect.height() * gradient.end.y,
                );
                (start, end)
            };

            // Create a simple mesh for gradient
            // TODO: Use egui's proper gradient API when available
            // For now, fill with the first color as approximation
            painter.rect(
                egui_rect,
                egui::CornerRadius::ZERO,
                colors[0],
                egui::Stroke::NONE,
                egui::StrokeKind::Outside,
            );
        } else {
            // Multiple colors - for now, just use the first color
            // TODO: Implement proper multi-stop gradient
            let egui_rect = egui::Rect::from_min_max(
                egui::pos2(rect.left(), rect.top()),
                egui::pos2(rect.right(), rect.bottom()),
            );

            painter.rect(
                egui_rect,
                egui::CornerRadius::ZERO,
                colors[0],
                egui::Stroke::NONE,
                egui::StrokeKind::Outside,
            );
        }
    }

    /// Paint a radial gradient
    fn paint_radial(
        painter: &egui::Painter,
        rect: Rect,
        gradient: &RadialGradient,
    ) {
        // Convert colors to egui
        let colors: Vec<egui::Color32> = gradient
            .colors
            .iter()
            .map(|c| egui::Color32::from_rgba_unmultiplied(
                c.red(),
                c.green(),
                c.blue(),
                c.alpha(),
            ))
            .collect();

        if colors.is_empty() {
            return;
        }

        // egui doesn't have built-in radial gradient support
        // For now, fill with the center color
        // TODO: Implement proper radial gradient with mesh or shader
        let egui_rect = egui::Rect::from_min_max(
            egui::pos2(rect.left(), rect.top()),
            egui::pos2(rect.right(), rect.bottom()),
        );

        painter.rect(
            egui_rect,
            egui::CornerRadius::ZERO,
            colors[0],
            egui::Stroke::NONE,
            egui::StrokeKind::Outside,
        );
    }

    /// Paint a sweep gradient (conical gradient)
    fn paint_sweep(
        painter: &egui::Painter,
        rect: Rect,
        gradient: &SweepGradient,
    ) {
        // Convert colors to egui
        let colors: Vec<egui::Color32> = gradient
            .colors
            .iter()
            .map(|c| egui::Color32::from_rgba_unmultiplied(
                c.red(),
                c.green(),
                c.blue(),
                c.alpha(),
            ))
            .collect();

        if colors.is_empty() {
            return;
        }

        // egui doesn't have built-in sweep gradient support
        // For now, fill with the first color
        // TODO: Implement proper sweep gradient with mesh or shader
        let egui_rect = egui::Rect::from_min_max(
            egui::pos2(rect.left(), rect.top()),
            egui::pos2(rect.right(), rect.bottom()),
        );

        painter.rect(
            egui_rect,
            egui::CornerRadius::ZERO,
            colors[0],
            egui::Stroke::NONE,
            egui::StrokeKind::Outside,
        );
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
        let gradient = Gradient::Linear(LinearGradient::horizontal(vec![
            Color::RED,
            Color::BLUE,
        ]));

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
