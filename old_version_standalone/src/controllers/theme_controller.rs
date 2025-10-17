//! Theme controller for managing application themes

use crate::theme::{Theme, ColorPalette, Spacing, Typography};
use crate::controllers::animation::{AnimationController, AnimationCurve};
use egui::Context;
use std::time::Duration;

/// Theme mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ThemeMode {
    /// Light theme
    Light,
    /// Dark theme
    Dark,
    /// System preference (auto)
    System,
    /// Custom theme
    Custom,
}

/// Theme transition effect
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThemeTransition {
    /// Instant switch
    None,
    /// Fade transition
    Fade(Duration),
    /// Slide transition
    Slide(Duration),
}

/// Theme controller for managing theme state and transitions
pub struct ThemeController {
    /// Current theme
    current_theme: Theme,
    /// Target theme (for transitions)
    target_theme: Option<Theme>,
    /// Current theme mode
    mode: ThemeMode,
    /// Transition animation
    transition_animation: AnimationController,
    /// Transition type
    transition: ThemeTransition,
    /// Custom themes registry
    custom_themes: Vec<(String, Theme)>,
    /// Theme change listeners
    listeners: Vec<Box<dyn Fn(&Theme)>>,
    /// Whether to persist theme preference
    persist: bool,
    /// Persisted theme key
    persist_key: String,
}

impl ThemeController {
    /// Create new theme controller
    pub fn new() -> Self {
        Self {
            current_theme: Theme::dark(),
            target_theme: None,
            mode: ThemeMode::Dark,
            transition_animation: AnimationController::new(Duration::from_millis(300))
                .with_curve(AnimationCurve::EaseInOut),
            transition: ThemeTransition::Fade(Duration::from_millis(300)),
            custom_themes: Vec::new(),
            listeners: Vec::new(),
            persist: false,
            persist_key: String::from("app_theme"),
        }
    }

    /// Enable theme persistence
    pub fn with_persistence(mut self, key: impl Into<String>) -> Self {
        self.persist = true;
        self.persist_key = key.into();
        // Try to load persisted theme
        self.load_persisted();
        self
    }

    /// Set transition effect
    pub fn with_transition(mut self, transition: ThemeTransition) -> Self {
        self.transition = transition;
        if let ThemeTransition::Fade(duration) | ThemeTransition::Slide(duration) = transition {
            self.transition_animation = AnimationController::new(duration)
                .with_curve(AnimationCurve::EaseInOut);
        }
        self
    }

    /// Register a custom theme
    pub fn register_theme(&mut self, name: impl Into<String>, theme: Theme) {
        self.custom_themes.push((name.into(), theme));
    }

    /// Get current theme mode
    pub fn mode(&self) -> ThemeMode {
        self.mode
    }

    /// Get current theme
    pub fn theme(&self) -> &Theme {
        &self.current_theme
    }

    /// Set theme mode
    pub fn set_mode(&mut self, mode: ThemeMode) {
        if self.mode == mode {
            return;
        }

        self.mode = mode;
        let new_theme = match mode {
            ThemeMode::Light => Theme::light(),
            ThemeMode::Dark => Theme::dark(),
            ThemeMode::System => self.detect_system_theme(),
            ThemeMode::Custom => {
                // Use first custom theme or fall back to dark
                self.custom_themes.first()
                    .map(|(_, theme)| theme.clone())
                    .unwrap_or_else(Theme::dark)
            }
        };

        self.switch_theme(new_theme);
    }

    /// Set custom theme by name
    pub fn set_custom_theme(&mut self, name: &str) -> bool {
        if let Some((_, theme)) = self.custom_themes.iter()
            .find(|(n, _)| n == name)
        {
            self.mode = ThemeMode::Custom;
            self.switch_theme(theme.clone());
            true
        } else {
            false
        }
    }

    /// Toggle between light and dark themes
    pub fn toggle(&mut self) {
        match self.mode {
            ThemeMode::Light => self.set_mode(ThemeMode::Dark),
            ThemeMode::Dark => self.set_mode(ThemeMode::Light),
            ThemeMode::System => {
                // When in system mode, toggle to opposite of current
                if self.is_dark() {
                    self.set_mode(ThemeMode::Light);
                } else {
                    self.set_mode(ThemeMode::Dark);
                }
            }
            ThemeMode::Custom => self.set_mode(ThemeMode::Dark),
        }
    }

    /// Check if current theme is dark
    pub fn is_dark(&self) -> bool {
        // Simple heuristic: check background luminance
        let bg = self.current_theme.colors.background;
        let luminance = 0.299 * bg.r() as f32 + 0.587 * bg.g() as f32 + 0.114 * bg.b() as f32;
        luminance < 128.0
    }

    /// Apply theme to context
    pub fn apply(&mut self, ctx: &Context) {
        // Handle transitions
        if let Some(ref target) = self.target_theme {
            match self.transition {
                ThemeTransition::None => {
                    self.current_theme = target.clone();
                    self.target_theme = None;
                }
                ThemeTransition::Fade(_) => {
                    let progress = self.transition_animation.tick();
                    if progress >= 0.99 {
                        self.current_theme = target.clone();
                        self.target_theme = None;
                    } else {
                        // Interpolate between themes
                        self.current_theme = self.interpolate_theme(&self.current_theme, target, progress);
                    }
                }
                ThemeTransition::Slide(_) => {
                    // For slide, we'd need more complex transition
                    // For now, treat as fade
                    let progress = self.transition_animation.tick();
                    if progress >= 0.99 {
                        self.current_theme = target.clone();
                        self.target_theme = None;
                    }
                }
            }
        }

        // Apply current theme
        self.current_theme.apply(ctx);

        // Save if persistence is enabled
        if self.persist {
            self.save_persisted();
        }
    }

    /// Add theme change listener
    pub fn on_change(&mut self, listener: impl Fn(&Theme) + 'static) {
        self.listeners.push(Box::new(listener));
    }

    /// Get list of available theme names
    pub fn available_themes(&self) -> Vec<(&str, ThemeMode)> {
        let mut themes = vec![
            ("Light", ThemeMode::Light),
            ("Dark", ThemeMode::Dark),
            ("System", ThemeMode::System),
        ];

        for (name, _) in &self.custom_themes {
            themes.push((name.as_str(), ThemeMode::Custom));
        }

        themes
    }

    /// Create a theme from current settings
    pub fn create_custom_theme(&self, name: impl Into<String>) -> (String, Theme) {
        (name.into(), self.current_theme.clone())
    }

    /// Update current theme colors
    pub fn update_colors(&mut self, updater: impl FnOnce(&mut ColorPalette)) {
        updater(&mut self.current_theme.colors);
        self.notify_listeners();
    }

    /// Update current theme spacing
    pub fn update_spacing(&mut self, updater: impl FnOnce(&mut Spacing)) {
        updater(&mut self.current_theme.spacing);
        self.notify_listeners();
    }

    /// Update current theme typography
    pub fn update_typography(&mut self, updater: impl FnOnce(&mut Typography)) {
        updater(&mut self.current_theme.typography);
        self.notify_listeners();
    }

    // Private methods

    fn switch_theme(&mut self, new_theme: Theme) {
        match self.transition {
            ThemeTransition::None => {
                self.current_theme = new_theme;
                self.notify_listeners();
            }
            _ => {
                self.target_theme = Some(new_theme);
                self.transition_animation.forward();
            }
        }
    }

    fn interpolate_theme(&self, from: &Theme, to: &Theme, t: f32) -> Theme {
        // For now, just return the target theme when transition is > 50%
        // Full interpolation would require interpolating all color values
        if t > 0.5 {
            to.clone()
        } else {
            from.clone()
        }
    }

    fn detect_system_theme(&self) -> Theme {
        // This would normally check system preferences
        // For now, default to dark theme
        Theme::dark()
    }

    fn notify_listeners(&self) {
        for listener in &self.listeners {
            listener(&self.current_theme);
        }
    }

    fn load_persisted(&mut self) {
        // In a real implementation, this would load from localStorage or preferences
        // For now, just use defaults
    }

    fn save_persisted(&self) {
        // In a real implementation, this would save to localStorage or preferences
        // For now, no-op
    }
}

impl Default for ThemeController {
    fn default() -> Self {
        Self::new()
    }
}

/// Theme builder for creating custom themes
pub struct ThemeBuilder {
    theme: Theme,
}

impl ThemeBuilder {
    /// Start building from a base theme
    pub fn from_base(base: Theme) -> Self {
        Self { theme: base }
    }

    /// Start building from dark theme
    pub fn dark() -> Self {
        Self { theme: Theme::dark() }
    }

    /// Start building from light theme
    pub fn light() -> Self {
        Self { theme: Theme::light() }
    }

    /// Set primary color
    pub fn primary(mut self, color: egui::Color32) -> Self {
        self.theme.colors.primary = color;
        self
    }

    /// Set secondary color
    pub fn secondary(mut self, color: egui::Color32) -> Self {
        self.theme.colors.secondary = color;
        self
    }

    /// Set background color
    pub fn background(mut self, color: egui::Color32) -> Self {
        self.theme.colors.background = color;
        self
    }

    /// Set text color
    pub fn text(mut self, color: egui::Color32) -> Self {
        self.theme.colors.text = color;
        self
    }

    /// Set all spacing values
    pub fn spacing(mut self, xs: f32, sm: f32, md: f32, lg: f32, xl: f32, xxl: f32) -> Self {
        self.theme.spacing.xs = xs;
        self.theme.spacing.sm = sm;
        self.theme.spacing.md = md;
        self.theme.spacing.lg = lg;
        self.theme.spacing.xl = xl;
        self.theme.spacing.xxl = xxl;
        self
    }

    /// Set font sizes
    pub fn font_sizes(mut self, body: f32, h1: f32, h2: f32, h3: f32) -> Self {
        self.theme.typography.body_size = body;
        self.theme.typography.h1_size = h1;
        self.theme.typography.h2_size = h2;
        self.theme.typography.h3_size = h3;
        self
    }

    /// Build the theme
    pub fn build(self) -> Theme {
        self.theme
    }
}