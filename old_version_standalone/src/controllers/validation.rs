//! Validation controller for form fields

use crate::controllers::animation::AnimationController;
use egui::Color32;
use instant::Instant;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Controls validation state and display
#[derive(Debug, Clone)]
pub struct ValidationController {
    /// Current validation state
    state: ValidationState,
    /// Validation errors
    errors: Vec<ValidationError>,
    /// Validation warnings
    warnings: Vec<ValidationWarning>,
    /// Display mode for validation feedback
    display_mode: ValidationDisplayMode,
    /// Animation for error appearance
    error_animation: AnimationController,
    /// Debounce timer for validation
    debounce_timer: Option<Instant>,
    /// Debounce duration
    debounce_duration: Duration,
}

/// Validation state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValidationState {
    /// Not yet validated
    NotValidated,
    /// Currently validating
    Validating,
    /// Validation passed
    Valid,
    /// Validation failed
    Invalid,
    /// Has warnings but valid
    Warning,
}

/// Validation error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    /// Error code
    pub code: String,
    /// Human-readable message
    pub message: String,
    /// Optional field name
    pub field: Option<String>,
}

/// Validation warning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationWarning {
    /// Warning code
    pub code: String,
    /// Human-readable message
    pub message: String,
}

/// How to display validation feedback
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValidationDisplayMode {
    /// Show message inline below field
    Inline,
    /// Show as tooltip on hover
    Tooltip,
    /// Show only an icon with tooltip
    Icon,
    /// Shake animation on error
    Shake,
    /// Change border color only
    BorderColor,
}

impl Default for ValidationController {
    fn default() -> Self {
        Self::new()
    }
}

impl ValidationController {
    /// Create a new validation controller
    pub fn new() -> Self {
        Self {
            state: ValidationState::NotValidated,
            errors: Vec::new(),
            warnings: Vec::new(),
            display_mode: ValidationDisplayMode::Inline,
            error_animation: AnimationController::new(Duration::from_millis(300)),
            debounce_timer: None,
            debounce_duration: Duration::from_millis(500),
        }
    }

    /// Set display mode
    pub fn with_display_mode(mut self, mode: ValidationDisplayMode) -> Self {
        self.display_mode = mode;
        self
    }

    /// Set debounce duration
    pub fn with_debounce(mut self, duration: Duration) -> Self {
        self.debounce_duration = duration;
        self
    }

    /// Set validation error
    pub fn set_error(&mut self, error: ValidationError) {
        self.state = ValidationState::Invalid;
        self.errors = vec![error];
        self.warnings.clear();
        self.error_animation.forward();
    }

    /// Set multiple validation errors
    pub fn set_errors(&mut self, errors: Vec<ValidationError>) {
        if !errors.is_empty() {
            self.state = ValidationState::Invalid;
            self.errors = errors;
            self.warnings.clear();
            self.error_animation.forward();
        }
    }

    /// Set validation warning
    pub fn set_warning(&mut self, warning: ValidationWarning) {
        self.state = ValidationState::Warning;
        self.warnings = vec![warning];
        self.errors.clear();
        self.error_animation.forward();
    }

    /// Mark as valid
    pub fn set_valid(&mut self) {
        self.state = ValidationState::Valid;
        self.errors.clear();
        self.warnings.clear();
        self.error_animation.reverse();
    }

    /// Mark as validating
    pub fn set_validating(&mut self) {
        self.state = ValidationState::Validating;
    }

    /// Reset to not validated
    pub fn reset(&mut self) {
        self.state = ValidationState::NotValidated;
        self.errors.clear();
        self.warnings.clear();
        self.error_animation.reset();
        self.debounce_timer = None;
    }

    /// Check if should trigger validation based on debounce
    pub fn should_validate(&mut self) -> bool {
        if let Some(timer) = self.debounce_timer {
            if timer.elapsed() >= self.debounce_duration {
                self.debounce_timer = None;
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    /// Trigger validation (starts debounce timer)
    pub fn trigger_validation(&mut self) {
        self.debounce_timer = Some(Instant::now());
    }

    /// Get current validation state
    pub fn state(&self) -> ValidationState {
        self.state
    }

    /// Check if valid
    pub fn is_valid(&self) -> bool {
        matches!(self.state, ValidationState::Valid | ValidationState::NotValidated)
    }

    /// Check if has errors
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Get first error message if any
    pub fn error_message(&self) -> Option<&str> {
        self.errors.first().map(|e| e.message.as_str())
    }

    /// Get first warning message if any
    pub fn warning_message(&self) -> Option<&str> {
        self.warnings.first().map(|w| w.message.as_str())
    }

    /// Get display mode
    pub fn display_mode(&self) -> ValidationDisplayMode {
        self.display_mode
    }

    /// Render validation feedback
    pub fn render(&mut self, ui: &mut egui::Ui) {
        match self.display_mode {
            ValidationDisplayMode::Inline => {
                self.render_inline(ui);
            }
            ValidationDisplayMode::Shake => {
                self.render_shake(ui);
            }
            _ => {
                // Other modes handled by widget itself
            }
        }
    }

    /// Render inline validation message
    fn render_inline(&mut self, ui: &mut egui::Ui) {
        let alpha = self.error_animation.tick();
        if alpha <= 0.01 {
            return;
        }

        if let Some(error) = self.errors.first() {
            let color = Color32::from_rgba_unmultiplied(255, 100, 100, (alpha * 255.0) as u8);
            ui.colored_label(color, &error.message);
        } else if let Some(warning) = self.warnings.first() {
            let color = Color32::from_rgba_unmultiplied(255, 200, 100, (alpha * 255.0) as u8);
            ui.colored_label(color, &warning.message);
        }
    }

    /// Render shake animation
    fn render_shake(&mut self, ui: &mut egui::Ui) {
        let shake = self.error_animation.tick();
        if shake > 0.0 {
            let offset = (shake * 10.0 * std::f32::consts::PI).sin() * 5.0 * shake;
            ui.add_space(offset);
        }
    }

    /// Get color for border based on validation state
    pub fn get_border_color(&self, default_color: Color32) -> Color32 {
        match self.state {
            ValidationState::Invalid => Color32::from_rgb(255, 100, 100),
            ValidationState::Warning => Color32::from_rgb(255, 200, 100),
            ValidationState::Valid => Color32::from_rgb(100, 255, 100),
            _ => default_color,
        }
    }

    /// Apply validation style to response
    pub fn apply_style(&self, response: &egui::Response) -> egui::Response {
        if self.display_mode == ValidationDisplayMode::BorderColor {
            if let ValidationState::Invalid = self.state {
                response.clone().on_hover_cursor(egui::CursorIcon::NotAllowed)
            } else {
                response.clone()
            }
        } else {
            response.clone()
        }
    }
}

impl ValidationError {
    /// Create a new validation error
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            field: None,
        }
    }

    /// Set the field name
    pub fn with_field(mut self, field: impl Into<String>) -> Self {
        self.field = Some(field.into());
        self
    }
}

impl ValidationWarning {
    /// Create a new validation warning
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}