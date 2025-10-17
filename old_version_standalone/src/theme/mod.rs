//! Theme system for consistent styling

use egui::{Color32, Context, Stroke, Style, Visuals};

/// Application theme
#[derive(Debug, Clone)]
pub struct Theme {
    /// Color palette
    pub colors: ColorPalette,
    /// Spacing values
    pub spacing: Spacing,
    /// Typography settings
    pub typography: Typography,
    /// Animation settings
    pub animations: AnimationConfig,
}

/// Color palette
#[derive(Debug, Clone)]
pub struct ColorPalette {
    /// Primary brand color
    pub primary: Color32,
    /// Secondary brand color
    pub secondary: Color32,
    /// Success color (green)
    pub success: Color32,
    /// Warning color (yellow/orange)
    pub warning: Color32,
    /// Error color (red)
    pub error: Color32,
    /// Info color (blue)
    pub info: Color32,
    /// Background color
    pub background: Color32,
    /// Surface color (cards, panels)
    pub surface: Color32,
    /// Primary text color
    pub text: Color32,
    /// Secondary text color (dimmed)
    pub text_secondary: Color32,
    /// Border color
    pub border: Color32,
}

/// Spacing configuration
#[derive(Debug, Clone)]
pub struct Spacing {
    /// Extra small spacing (2px)
    pub xs: f32,
    /// Small spacing (4px)
    pub sm: f32,
    /// Medium spacing (8px)
    pub md: f32,
    /// Large spacing (16px)
    pub lg: f32,
    /// Extra large spacing (24px)
    pub xl: f32,
    /// Double extra large spacing (32px)
    pub xxl: f32,
}

/// Typography configuration
#[derive(Debug, Clone)]
pub struct Typography {
    /// Font family
    pub font_family: String,
    /// Body text size
    pub body_size: f32,
    /// Small text size
    pub small_size: f32,
    /// Heading sizes for different levels
    pub h1_size: f32,
    /// Heading 2 size
    pub h2_size: f32,
    /// Heading 3 size
    pub h3_size: f32,
    /// Line height multiplier
    pub line_height: f32,
}

/// Animation configuration
#[derive(Debug, Clone)]
pub struct AnimationConfig {
    /// Fast animations (100ms)
    pub fast_ms: u64,
    /// Normal animations (200ms)
    pub normal_ms: u64,
    /// Slow animations (300ms)
    pub slow_ms: u64,
    /// Enable animations
    pub enabled: bool,
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

impl Theme {
    /// Dark theme
    pub fn dark() -> Self {
        Self {
            colors: ColorPalette {
                primary: Color32::from_rgb(100, 150, 255),
                secondary: Color32::from_rgb(150, 100, 255),
                success: Color32::from_rgb(100, 255, 100),
                warning: Color32::from_rgb(255, 200, 100),
                error: Color32::from_rgb(255, 100, 100),
                info: Color32::from_rgb(100, 200, 255),
                background: Color32::from_rgb(24, 24, 28),
                surface: Color32::from_rgb(32, 32, 38),
                text: Color32::from_rgb(240, 240, 240),
                text_secondary: Color32::from_rgb(160, 160, 160),
                border: Color32::from_rgb(60, 60, 70),
            },
            spacing: Spacing::default(),
            typography: Typography::default(),
            animations: AnimationConfig::default(),
        }
    }

    /// Light theme
    pub fn light() -> Self {
        Self {
            colors: ColorPalette {
                primary: Color32::from_rgb(50, 100, 200),
                secondary: Color32::from_rgb(100, 50, 200),
                success: Color32::from_rgb(50, 200, 50),
                warning: Color32::from_rgb(200, 150, 50),
                error: Color32::from_rgb(200, 50, 50),
                info: Color32::from_rgb(50, 150, 200),
                background: Color32::from_rgb(250, 250, 250),
                surface: Color32::from_rgb(255, 255, 255),
                text: Color32::from_rgb(32, 32, 32),
                text_secondary: Color32::from_rgb(100, 100, 100),
                border: Color32::from_rgb(220, 220, 220),
            },
            spacing: Spacing::default(),
            typography: Typography::default(),
            animations: AnimationConfig::default(),
        }
    }

    /// Apply theme to egui context
    pub fn apply(&self, ctx: &Context) {
        let mut style = Style::default();
        let mut visuals = if self.is_dark() {
            Visuals::dark()
        } else {
            Visuals::light()
        };

        // Apply colors
        visuals.override_text_color = Some(self.colors.text);
        visuals.hyperlink_color = self.colors.primary;
        visuals.selection.bg_fill = self.colors.primary.gamma_multiply(0.3);
        visuals.widgets.noninteractive.bg_fill = self.colors.surface;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.colors.border);
        visuals.widgets.inactive.bg_fill = self.colors.surface;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, self.colors.border);
        visuals.widgets.hovered.bg_fill = self.colors.surface.gamma_multiply(1.1);
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, self.colors.primary);
        visuals.widgets.active.bg_fill = self.colors.primary.gamma_multiply(0.2);
        visuals.widgets.active.bg_stroke = Stroke::new(2.0, self.colors.primary);
        visuals.extreme_bg_color = self.colors.background;
        visuals.error_fg_color = self.colors.error;
        visuals.warn_fg_color = self.colors.warning;

        // Apply spacing
        style.spacing.item_spacing = egui::vec2(self.spacing.md, self.spacing.md);
        style.spacing.button_padding = egui::vec2(self.spacing.md, self.spacing.sm);
        style.spacing.indent = self.spacing.lg;

        // Rounding is already part of widget visuals in egui 0.33

        style.visuals = visuals;
        ctx.set_style(style);
    }

    /// Check if theme is dark
    pub fn is_dark(&self) -> bool {
        let brightness = (self.colors.background.r() as f32 +
            self.colors.background.g() as f32 +
            self.colors.background.b() as f32) / 3.0;
        brightness < 128.0
    }
}

impl Default for Spacing {
    fn default() -> Self {
        Self {
            xs: 2.0,
            sm: 4.0,
            md: 8.0,
            lg: 16.0,
            xl: 24.0,
            xxl: 32.0,
        }
    }
}

impl Default for Typography {
    fn default() -> Self {
        Self {
            font_family: "Inter".to_string(),
            body_size: 14.0,
            small_size: 12.0,
            h1_size: 24.0,
            h2_size: 20.0,
            h3_size: 16.0,
            line_height: 1.5,
        }
    }
}

impl Default for AnimationConfig {
    fn default() -> Self {
        Self {
            fast_ms: 100,
            normal_ms: 200,
            slow_ms: 300,
            enabled: true,
        }
    }
}