//! Visibility controller for show/hide animations

use crate::controllers::animation::AnimationController;
use std::time::Duration;

/// Controls widget visibility with animations
#[derive(Debug, Clone)]
pub struct VisibilityController {
    /// Current visibility state
    visible: bool,
    /// Animation for fade effect
    fade_animation: AnimationController,
    /// How to hide the widget
    hide_mode: HideMode,
}

/// How to hide a widget
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HideMode {
    /// Remove from layout completely
    Remove,
    /// Make invisible but keep space
    Invisible,
    /// Fade in/out with opacity
    Fade,
    /// Collapse with height animation
    Collapse,
}

impl Default for VisibilityController {
    fn default() -> Self {
        Self::new()
    }
}

impl VisibilityController {
    /// Create new visibility controller
    pub fn new() -> Self {
        Self {
            visible: true,
            fade_animation: AnimationController::new(Duration::from_millis(200)),
            hide_mode: HideMode::Fade,
        }
    }

    /// Set hide mode
    pub fn with_hide_mode(mut self, mode: HideMode) -> Self {
        self.hide_mode = mode;
        self
    }

    /// Set visibility
    pub fn set_visible(&mut self, visible: bool) {
        if self.visible != visible {
            self.visible = visible;
            if visible {
                self.fade_animation.forward();
            } else {
                self.fade_animation.reverse();
            }
        }
    }

    /// Toggle visibility
    pub fn toggle(&mut self) {
        self.set_visible(!self.visible);
    }

    /// Check if visible
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Check if should render (considers animation)
    pub fn should_render(&mut self) -> bool {
        match self.hide_mode {
            HideMode::Remove => self.visible || self.fade_animation.tick() > 0.01,
            _ => true,
        }
    }

    /// Get opacity for fade animation
    pub fn opacity(&mut self) -> f32 {
        if self.hide_mode == HideMode::Fade {
            self.fade_animation.tick()
        } else if self.visible {
            1.0
        } else {
            0.0
        }
    }

    /// Apply visibility to UI
    pub fn apply(&mut self, ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
        // Check should_render and opacity before match to avoid borrow issues
        let should_render = self.should_render();
        let opacity = self.opacity();
        let collapse_height = if !self.visible {
            self.fade_animation.tick()
        } else {
            1.0
        };

        match self.hide_mode {
            HideMode::Remove if !should_render => {
                // Don't render anything
            }
            HideMode::Invisible => {
                if self.visible {
                    add_contents(ui);
                }
            }
            HideMode::Fade => {
                if opacity > 0.01 {
                    // In egui 0.33, we need to use multiply_opacity instead of push_opacity
                    let old_opacity = ui.opacity();
                    ui.set_opacity(old_opacity * opacity);
                    add_contents(ui);
                    ui.set_opacity(old_opacity);
                }
            }
            HideMode::Collapse => {
                if self.visible {
                    add_contents(ui);
                } else if collapse_height > 0.01 {
                    ui.allocate_space(egui::vec2(0.0, collapse_height * 100.0));
                }
            }
            _ => add_contents(ui),
        }
    }
}