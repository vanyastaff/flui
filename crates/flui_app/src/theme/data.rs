//! Theme data and builder.

use super::colors::ColorScheme;

/// Theme mode - light, dark, or follow system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    /// Light theme.
    #[default]
    Light,
    /// Dark theme.
    Dark,
    /// Follow system preference.
    System,
}

/// Complete theme configuration.
///
/// # Example
///
/// ```rust,ignore
/// // Use defaults
/// let theme = Theme::light();
///
/// // Or build custom
/// let theme = Theme::builder()
///     .mode(ThemeMode::Dark)
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct Theme {
    /// Theme mode.
    pub mode: ThemeMode,

    /// Color scheme.
    pub colors: ColorScheme,

    /// Default font family.
    pub font_family: String,

    /// Base font size.
    pub font_size: f32,

    /// Default border radius.
    pub border_radius: f32,

    /// Default spacing unit.
    pub spacing: f32,

    /// Animation duration in milliseconds.
    pub animation_duration_ms: u32,
}

impl Default for Theme {
    fn default() -> Self {
        Self::light()
    }
}

impl Theme {
    /// Create a light theme with defaults.
    pub fn light() -> Self {
        Self {
            mode: ThemeMode::Light,
            colors: ColorScheme::light(),
            font_family: "system-ui".to_string(),
            font_size: 14.0,
            border_radius: 4.0,
            spacing: 8.0,
            animation_duration_ms: 200,
        }
    }

    /// Create a dark theme with defaults.
    pub fn dark() -> Self {
        Self {
            mode: ThemeMode::Dark,
            colors: ColorScheme::dark(),
            font_family: "system-ui".to_string(),
            font_size: 14.0,
            border_radius: 4.0,
            spacing: 8.0,
            animation_duration_ms: 200,
        }
    }

    /// Create a theme builder.
    pub fn builder() -> ThemeBuilder {
        ThemeBuilder::default()
    }
}

/// Builder for creating custom themes.
#[derive(Debug, Clone, Default)]
pub struct ThemeBuilder {
    mode: Option<ThemeMode>,
    colors: Option<ColorScheme>,
    font_family: Option<String>,
    font_size: Option<f32>,
    border_radius: Option<f32>,
    spacing: Option<f32>,
    animation_duration_ms: Option<u32>,
}

impl ThemeBuilder {
    /// Set theme mode.
    pub fn mode(mut self, mode: ThemeMode) -> Self {
        self.mode = Some(mode);
        self
    }

    /// Set color scheme.
    pub fn colors(mut self, colors: ColorScheme) -> Self {
        self.colors = Some(colors);
        self
    }

    /// Set font family.
    pub fn font_family(mut self, family: impl Into<String>) -> Self {
        self.font_family = Some(family.into());
        self
    }

    /// Set base font size.
    pub fn font_size(mut self, size: f32) -> Self {
        self.font_size = Some(size);
        self
    }

    /// Set default border radius.
    pub fn border_radius(mut self, radius: f32) -> Self {
        self.border_radius = Some(radius);
        self
    }

    /// Set spacing unit.
    pub fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = Some(spacing);
        self
    }

    /// Set animation duration.
    pub fn animation_duration_ms(mut self, ms: u32) -> Self {
        self.animation_duration_ms = Some(ms);
        self
    }

    /// Build the theme.
    pub fn build(self) -> Theme {
        let mode = self.mode.unwrap_or_default();
        let base = match mode {
            ThemeMode::Light | ThemeMode::System => Theme::light(),
            ThemeMode::Dark => Theme::dark(),
        };

        Theme {
            mode,
            colors: self.colors.unwrap_or(base.colors),
            font_family: self.font_family.unwrap_or(base.font_family),
            font_size: self.font_size.unwrap_or(base.font_size),
            border_radius: self.border_radius.unwrap_or(base.border_radius),
            spacing: self.spacing.unwrap_or(base.spacing),
            animation_duration_ms: self
                .animation_duration_ms
                .unwrap_or(base.animation_duration_ms),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_defaults() {
        let light = Theme::light();
        assert_eq!(light.mode, ThemeMode::Light);
        assert_eq!(light.font_size, 14.0);

        let dark = Theme::dark();
        assert_eq!(dark.mode, ThemeMode::Dark);
    }

    #[test]
    fn test_theme_builder() {
        let theme = Theme::builder()
            .mode(ThemeMode::Dark)
            .font_size(16.0)
            .spacing(12.0)
            .build();

        assert_eq!(theme.mode, ThemeMode::Dark);
        assert_eq!(theme.font_size, 16.0);
        assert_eq!(theme.spacing, 12.0);
    }
}
