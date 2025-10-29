//! Gradient layer - renders gradients

use flui_types::{Rect, Offset, Event, HitTestResult, styling::Gradient};
use crate::layer::Layer;
use crate::painter::Painter;

/// Layer that renders a gradient
///
/// This is a leaf layer (like PictureLayer) that renders gradients
/// in a specified rectangle. The actual gradient rendering is delegated
/// to flui_painting::GradientPainter.
///
/// # Example
///
/// ```text
/// GradientLayer (rect: 100x100, linear gradient)
/// Result: Gradient fill in the rectangle
/// ```
#[derive(Debug)]
pub struct GradientLayer {
    /// Rectangle to fill with gradient
    rect: Rect,

    /// The gradient to render
    gradient: Gradient,

    /// Cached bounds
    cached_bounds: Rect,
}

impl GradientLayer {
    /// Create a new gradient layer
    ///
    /// # Arguments
    /// * `rect` - Rectangle to fill with gradient
    /// * `gradient` - The gradient to render
    pub fn new(rect: Rect, gradient: Gradient) -> Self {
        Self {
            rect,
            gradient,
            cached_bounds: rect,
        }
    }

    /// Get the rectangle
    pub fn rect(&self) -> Rect {
        self.rect
    }

    /// Get the gradient
    pub fn gradient(&self) -> &Gradient {
        &self.gradient
    }

    /// Set new gradient
    pub fn set_gradient(&mut self, gradient: Gradient) {
        self.gradient = gradient;
    }

    /// Set new rectangle
    pub fn set_rect(&mut self, rect: Rect) {
        self.rect = rect;
        self.cached_bounds = rect;
    }
}

impl GradientLayer {
    /// Helper: Convert flui Color to RGBA f32 array
    fn color_to_rgba(color: &flui_types::styling::Color) -> [f32; 4] {
        [
            color.red() as f32 / 255.0,
            color.green() as f32 / 255.0,
            color.blue() as f32 / 255.0,
            color.alpha() as f32 / 255.0,
        ]
    }

    /// Paint linear gradient
    fn paint_linear(&self, painter: &mut dyn Painter) {
        if let Gradient::Linear(ref linear) = self.gradient {
            if linear.colors.is_empty() {
                return;
            }

            if linear.colors.len() == 1 {
                // Single color - just fill
                let paint = crate::painter::Paint {
                    color: Self::color_to_rgba(&linear.colors[0]),
                    stroke_width: 0.0,
                    anti_alias: true,
                };
                painter.rect(self.rect, &paint);
                return;
            }

            // Check if it's horizontal or vertical gradient
            let is_horizontal = (linear.begin.x - 0.0).abs() < 0.01
                && (linear.end.x - 1.0).abs() < 0.01
                && (linear.begin.y - linear.end.y).abs() < 0.01;

            let is_vertical = (linear.begin.y - 0.0).abs() < 0.01
                && (linear.end.y - 1.0).abs() < 0.01
                && (linear.begin.x - linear.end.x).abs() < 0.01;

            // For now, only support 2-color gradients through Painter trait methods
            let start_color = Self::color_to_rgba(&linear.colors[0]);
            let end_color = Self::color_to_rgba(linear.colors.last().unwrap());

            if is_horizontal {
                painter.horizontal_gradient(self.rect, start_color, end_color);
            } else if is_vertical {
                painter.vertical_gradient(self.rect, start_color, end_color);
            } else {
                // Diagonal - approximate with vertical for now
                painter.vertical_gradient(self.rect, start_color, end_color);
            }
        }
    }

    /// Paint radial gradient
    fn paint_radial(&self, painter: &mut dyn Painter) {
        use flui_types::Point;

        if let Gradient::Radial(ref radial) = self.gradient {
            if radial.colors.is_empty() {
                return;
            }

            if radial.colors.len() == 1 {
                // Single color - just fill
                let paint = crate::painter::Paint {
                    color: Self::color_to_rgba(&radial.colors[0]),
                    stroke_width: 0.0,
                    anti_alias: true,
                };
                painter.rect(self.rect, &paint);
                return;
            }

            // Calculate center position
            let center_x = self.rect.left() + self.rect.width() * radial.center.x;
            let center_y = self.rect.top() + self.rect.height() * radial.center.y;
            let center = Point::new(center_x, center_y);

            // Calculate radius
            let max_dim = self.rect.width().max(self.rect.height());
            let inner_radius = 0.0;
            let outer_radius = radial.radius * max_dim;

            let start_color = Self::color_to_rgba(&radial.colors[0]);
            let end_color = Self::color_to_rgba(radial.colors.last().unwrap());

            painter.radial_gradient_simple(center, inner_radius, outer_radius, start_color, end_color);
        }
    }

    /// Paint sweep gradient
    fn paint_sweep(&self, painter: &mut dyn Painter) {
        if let Gradient::Sweep(ref sweep) = self.gradient {
            // TODO: Implement sweep gradients properly
            // For now, just fill with first color as a fallback
            if let Some(first_color) = sweep.colors.first() {
                let paint = crate::painter::Paint {
                    color: Self::color_to_rgba(first_color),
                    stroke_width: 0.0,
                    anti_alias: true,
                };
                painter.rect(self.rect, &paint);
            }
        }
    }
}

impl Layer for GradientLayer {
    fn paint(&self, painter: &mut dyn Painter) {
        match &self.gradient {
            Gradient::Linear(_) => self.paint_linear(painter),
            Gradient::Radial(_) => self.paint_radial(painter),
            Gradient::Sweep(_) => self.paint_sweep(painter),
        }
    }

    fn bounds(&self) -> Rect {
        self.cached_bounds
    }

    fn hit_test(&self, position: Offset, _result: &mut HitTestResult) -> bool {
        // Gradient layer itself is not interactive
        // Just check if position is within bounds
        self.rect.contains(position.to_point())
    }

    fn handle_event(&mut self, _event: &Event) -> bool {
        // Gradient layer doesn't handle events
        false
    }

    fn is_visible(&self) -> bool {
        !self.rect.is_empty()
    }

    fn is_disposed(&self) -> bool {
        false
    }

    fn dispose(&mut self) {
        // Nothing to dispose
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_types::styling::{LinearGradient, Color};

    #[test]
    fn test_gradient_layer_creation() {
        let rect = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let gradient = Gradient::Linear(LinearGradient::horizontal(vec![
            Color::RED,
            Color::BLUE,
        ]));

        let layer = GradientLayer::new(rect, gradient);

        assert_eq!(layer.rect(), rect);
        assert_eq!(layer.bounds(), rect);
        assert!(layer.is_visible());
    }

    #[test]
    fn test_gradient_layer_set_rect() {
        let rect1 = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let rect2 = Rect::from_xywh(10.0, 10.0, 50.0, 50.0);
        let gradient = Gradient::Linear(LinearGradient::horizontal(vec![
            Color::RED,
            Color::BLUE,
        ]));

        let mut layer = GradientLayer::new(rect1, gradient);
        assert_eq!(layer.rect(), rect1);

        layer.set_rect(rect2);
        assert_eq!(layer.rect(), rect2);
        assert_eq!(layer.bounds(), rect2);
    }

    #[test]
    fn test_gradient_layer_hit_test() {
        let rect = Rect::from_xywh(10.0, 10.0, 100.0, 100.0);
        let gradient = Gradient::Linear(LinearGradient::horizontal(vec![
            Color::RED,
            Color::BLUE,
        ]));

        let layer = GradientLayer::new(rect, gradient);
        let mut result = HitTestResult::new();

        // Point inside
        assert!(layer.hit_test(Offset::new(50.0, 50.0), &mut result));

        // Point outside
        assert!(!layer.hit_test(Offset::new(5.0, 5.0), &mut result));
    }

    #[test]
    fn test_gradient_layer_empty_rect() {
        let rect = Rect::ZERO;
        let gradient = Gradient::Linear(LinearGradient::horizontal(vec![
            Color::RED,
            Color::BLUE,
        ]));

        let layer = GradientLayer::new(rect, gradient);
        assert!(!layer.is_visible());
    }
}
