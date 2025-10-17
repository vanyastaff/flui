//! Focus controller for managing widget focus and interaction states

use crate::controllers::animation::AnimationController;
use egui::{Color32, Response};
use std::time::Duration;

/// Controls focus and interaction states
#[derive(Debug, Clone)]
pub struct FocusController {
    /// Widget has keyboard focus
    has_focus: bool,
    /// Mouse is hovering over widget
    is_hovered: bool,
    /// Widget is being pressed
    is_pressed: bool,
    /// Widget was touched/interacted with
    was_touched: bool,
    /// Animation for focus highlight
    focus_animation: AnimationController,
    /// Animation for hover effect
    hover_animation: AnimationController,
    /// Animation for press effect
    press_animation: AnimationController,
}

impl Default for FocusController {
    fn default() -> Self {
        Self::new()
    }
}

impl FocusController {
    /// Create a new focus controller
    pub fn new() -> Self {
        Self {
            has_focus: false,
            is_hovered: false,
            is_pressed: false,
            was_touched: false,
            focus_animation: AnimationController::new(Duration::from_millis(150)),
            hover_animation: AnimationController::new(Duration::from_millis(100)),
            press_animation: AnimationController::new(Duration::from_millis(50)),
        }
    }

    /// Update state from egui Response
    pub fn update(&mut self, response: &Response) {
        let prev_focus = self.has_focus;
        let prev_hover = self.is_hovered;
        let prev_pressed = self.is_pressed;

        // Update states
        self.has_focus = response.has_focus();
        self.is_hovered = response.hovered();
        self.is_pressed = response.is_pointer_button_down_on();

        // Mark as touched if clicked
        if response.clicked() {
            self.was_touched = true;
        }

        // Handle focus animation
        if self.has_focus != prev_focus {
            if self.has_focus {
                self.focus_animation.forward();
            } else {
                self.focus_animation.reverse();
            }
        }

        // Handle hover animation
        if self.is_hovered != prev_hover {
            if self.is_hovered {
                self.hover_animation.forward();
            } else {
                self.hover_animation.reverse();
            }
        }

        // Handle press animation
        if self.is_pressed != prev_pressed {
            if self.is_pressed {
                self.press_animation.forward();
            } else {
                self.press_animation.reverse();
            }
        }
    }

    /// Check if has focus
    pub fn has_focus(&self) -> bool {
        self.has_focus
    }

    /// Check if hovered
    pub fn is_hovered(&self) -> bool {
        self.is_hovered
    }

    /// Check if pressed
    pub fn is_pressed(&self) -> bool {
        self.is_pressed
    }

    /// Check if was touched
    pub fn was_touched(&self) -> bool {
        self.was_touched
    }

    /// Reset touched state
    pub fn reset_touched(&mut self) {
        self.was_touched = false;
    }

    /// Get animated focus value (0.0 to 1.0)
    pub fn focus_value(&mut self) -> f32 {
        self.focus_animation.tick()
    }

    /// Get animated hover value (0.0 to 1.0)
    pub fn hover_value(&mut self) -> f32 {
        self.hover_animation.tick()
    }

    /// Get animated press value (0.0 to 1.0)
    pub fn press_value(&mut self) -> f32 {
        self.press_animation.tick()
    }

    /// Get focus color with animation
    pub fn get_focus_color(&mut self, base_color: Color32) -> Color32 {
        let focus = self.focus_animation.tick();
        let hover = self.hover_animation.tick();
        let press = self.press_animation.tick();

        let mut color = base_color;

        // Apply focus highlight
        if focus > 0.0 {
            let focus_color = Color32::from_rgba_unmultiplied(100, 150, 255, (focus * 60.0) as u8);
            color = Self::blend_colors(color, focus_color);
        }

        // Apply hover effect
        if hover > 0.0 {
            color = Self::lighten_color(color, hover * 0.1);
        }

        // Apply press effect
        if press > 0.0 {
            color = Self::darken_color(color, press * 0.1);
        }

        color
    }

    /// Get outline/border width based on focus
    pub fn get_outline_width(&mut self, base_width: f32) -> f32 {
        let focus = self.focus_value();
        base_width + focus * 2.0
    }

    /// Get scale factor for press animation
    pub fn get_scale(&mut self) -> f32 {
        let press = self.press_value();
        1.0 - press * 0.05 // Scale down slightly when pressed
    }

    /// Blend two colors
    fn blend_colors(base: Color32, overlay: Color32) -> Color32 {
        let alpha = overlay.a() as f32 / 255.0;
        let inv_alpha = 1.0 - alpha;

        Color32::from_rgba_unmultiplied(
            ((base.r() as f32 * inv_alpha) + (overlay.r() as f32 * alpha)) as u8,
            ((base.g() as f32 * inv_alpha) + (overlay.g() as f32 * alpha)) as u8,
            ((base.b() as f32 * inv_alpha) + (overlay.b() as f32 * alpha)) as u8,
            base.a(),
        )
    }

    /// Lighten a color
    fn lighten_color(color: Color32, amount: f32) -> Color32 {
        let amount = amount.clamp(0.0, 1.0);
        Color32::from_rgba_unmultiplied(
            ((color.r() as f32 * (1.0 - amount)) + (255.0 * amount)) as u8,
            ((color.g() as f32 * (1.0 - amount)) + (255.0 * amount)) as u8,
            ((color.b() as f32 * (1.0 - amount)) + (255.0 * amount)) as u8,
            color.a(),
        )
    }

    /// Darken a color
    fn darken_color(color: Color32, amount: f32) -> Color32 {
        let amount = amount.clamp(0.0, 1.0);
        Color32::from_rgba_unmultiplied(
            (color.r() as f32 * (1.0 - amount)) as u8,
            (color.g() as f32 * (1.0 - amount)) as u8,
            (color.b() as f32 * (1.0 - amount)) as u8,
            color.a(),
        )
    }
}