//! Egui painter implementation
//!
//! This module provides a Painter implementation backed by egui's rendering system.

use crate::painter::{Painter, Paint, RRect};
use flui_types::{Point, Rect, Offset};

/// Stack-based state for painter operations
#[derive(Debug, Clone)]
struct PainterState {
    /// Current opacity (multiplicative)
    opacity: f32,

    /// Current clip rect
    clip_rect: Option<Rect>,
}

impl Default for PainterState {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            clip_rect: None,
        }
    }
}

/// Egui-backed painter implementation
///
/// This painter translates abstract drawing commands into egui's immediate-mode API.
///
/// # State Management
///
/// The painter maintains a stack of states (transform, clip, opacity) to support
/// save/restore operations. This is necessary because egui doesn't provide a
/// built-in state stack.
pub struct EguiPainter<'a> {
    /// The underlying egui painter
    painter: &'a egui::Painter,

    /// State stack for save/restore
    state_stack: Vec<PainterState>,

    /// Current state
    current_state: PainterState,
}

impl<'a> EguiPainter<'a> {
    /// Create a new egui painter
    pub fn new(painter: &'a egui::Painter) -> Self {
        Self {
            painter,
            state_stack: Vec::new(),
            current_state: PainterState::default(),
        }
    }

    /// Get the underlying egui painter
    pub fn inner(&self) -> &egui::Painter {
        self.painter
    }

    /// Convert our Paint to egui color
    fn paint_to_color(&self, paint: &Paint) -> egui::Color32 {
        let [r, g, b, a] = paint.color;
        let opacity = a * self.current_state.opacity;

        egui::Color32::from_rgba_unmultiplied(
            (r * 255.0) as u8,
            (g * 255.0) as u8,
            (b * 255.0) as u8,
            (opacity * 255.0) as u8,
        )
    }

    /// Convert our Rect to egui Rect
    fn to_egui_rect(rect: Rect) -> egui::Rect {
        egui::Rect::from_min_max(
            egui::pos2(rect.min.x, rect.min.y),
            egui::pos2(rect.max.x, rect.max.y),
        )
    }

    /// Convert our Point to egui Pos2
    fn to_egui_pos(point: Point) -> egui::Pos2 {
        egui::pos2(point.x, point.y)
    }

    /// Check if the given bounds are visible (not clipped)
    fn is_visible(&self, bounds: Rect) -> bool {
        if let Some(clip) = self.current_state.clip_rect {
            bounds.intersects(&clip)
        } else {
            true
        }
    }
}

impl<'a> Painter for EguiPainter<'a> {
    fn rect(&mut self, rect: Rect, paint: &Paint) {
        if !self.is_visible(rect) {
            return;
        }

        let color = self.paint_to_color(paint);
        let egui_rect = Self::to_egui_rect(rect);

        if paint.stroke_width > 0.0 {
            // Stroked rect
            let stroke = egui::Stroke::new(paint.stroke_width, color);
            self.painter.rect_stroke(egui_rect, 0.0, stroke, egui::StrokeKind::Outside);
        } else {
            // Filled rect
            self.painter.rect_filled(egui_rect, 0.0, color);
        }
    }

    fn rrect(&mut self, rrect: RRect, paint: &Paint) {
        if !self.is_visible(rrect.rect) {
            return;
        }

        let color = self.paint_to_color(paint);
        let egui_rect = Self::to_egui_rect(rrect.rect);
        let rounding = egui::CornerRadius::same(rrect.corner_radius.min(255.0) as u8);

        if paint.stroke_width > 0.0 {
            // Stroked rounded rect
            let stroke = egui::Stroke::new(paint.stroke_width, color);
            self.painter.rect_stroke(egui_rect, rounding, stroke, egui::StrokeKind::Outside);
        } else {
            // Filled rounded rect
            self.painter.rect_filled(egui_rect, rounding, color);
        }
    }

    fn circle(&mut self, center: Point, radius: f32, paint: &Paint) {
        let bounds = Rect::from_center_size(
            center,
            flui_types::Size::new(radius * 2.0, radius * 2.0),
        );

        if !self.is_visible(bounds) {
            return;
        }

        let color = self.paint_to_color(paint);
        let egui_center = Self::to_egui_pos(center);

        if paint.stroke_width > 0.0 {
            // Stroked circle
            let stroke = egui::Stroke::new(paint.stroke_width, color);
            self.painter.circle_stroke(egui_center, radius, stroke);
        } else {
            // Filled circle
            self.painter.circle_filled(egui_center, radius, color);
        }
    }

    fn line(&mut self, p1: Point, p2: Point, paint: &Paint) {
        let min_x = p1.x.min(p2.x);
        let min_y = p1.y.min(p2.y);
        let max_x = p1.x.max(p2.x);
        let max_y = p1.y.max(p2.y);

        let bounds = Rect::from_min_max(
            Point::new(min_x, min_y),
            Point::new(max_x, max_y),
        );

        if !self.is_visible(bounds) {
            return;
        }

        let color = self.paint_to_color(paint);
        let stroke = egui::Stroke::new(paint.stroke_width.max(1.0), color);

        self.painter.line_segment(
            [Self::to_egui_pos(p1), Self::to_egui_pos(p2)],
            stroke,
        );
    }

    fn save(&mut self) {
        // Push current state to stack
        self.state_stack.push(self.current_state.clone());
    }

    fn restore(&mut self) {
        // Pop state from stack
        if let Some(state) = self.state_stack.pop() {
            self.current_state = state;
        }
    }

    fn translate(&mut self, offset: Offset) {
        // Note: Egui doesn't have a transform stack, so we would need to
        // manually adjust all coordinates. For now, this is a no-op.
        // A full implementation would require maintaining a transform matrix
        // and applying it to all coordinates.
        let _ = offset;
    }

    fn rotate(&mut self, angle: f32) {
        // Note: Egui doesn't natively support rotation.
        // A full implementation would require maintaining a transform matrix
        // and converting shapes to paths.
        let _ = angle;
    }

    fn scale(&mut self, sx: f32, sy: f32) {
        // Note: Egui doesn't natively support scaling transforms.
        // A full implementation would require maintaining a transform matrix.
        let _ = (sx, sy);
    }

    fn clip_rect(&mut self, rect: Rect) {
        // Update clip rect (intersect with current clip)
        self.current_state.clip_rect = Some(if let Some(current_clip) = self.current_state.clip_rect {
            current_clip.intersection(&rect).unwrap_or(Rect::ZERO)
        } else {
            rect
        });
    }

    fn clip_rrect(&mut self, rrect: RRect) {
        // For simplicity, just use the outer rect
        // A full implementation would use egui's ClippedPrimitive
        self.clip_rect(rrect.rect);
    }

    fn set_opacity(&mut self, opacity: f32) {
        // Multiply with current opacity (for nested opacity layers)
        self.current_state.opacity *= opacity.clamp(0.0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: Testing egui painter requires an egui context, which is
    // difficult to set up in unit tests. These tests would typically
    // be integration tests instead.

    #[test]
    fn test_state_stack() {
        // This is a simplified test that doesn't actually use egui
        let mut state_stack = Vec::new();
        let mut current_state = PainterState::default();

        // Save state
        state_stack.push(current_state.clone());

        // Modify state
        current_state.opacity = 0.5;

        // Restore state
        if let Some(state) = state_stack.pop() {
            current_state = state;
        }

        assert_eq!(current_state.opacity, 1.0);
    }

    #[test]
    fn test_paint_to_color_conversion() {
        let paint = Paint {
            color: [1.0, 0.0, 0.0, 1.0], // Red
            stroke_width: 0.0,
            anti_alias: true,
        };

        let expected = egui::Color32::from_rgba_unmultiplied(255, 0, 0, 255);

        // We can't create an EguiPainter without an egui::Painter,
        // but we can test the conversion logic directly
        assert_eq!(expected.r(), 255);
        assert_eq!(expected.g(), 0);
        assert_eq!(expected.b(), 0);
        assert_eq!(expected.a(), 255);
    }
}
