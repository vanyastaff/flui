//! Application configuration.

use flui_types::geometry::px;
use flui_types::Size;

/// Application configuration.
///
/// Use this to customize the app window and behavior.
///
/// # Example
///
/// ```rust,ignore
/// let config = AppConfig::new()
///     .with_title("My App")
///     .with_size(1024, 768)
///     .with_resizable(true);
/// ```
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Window title.
    pub title: String,

    /// Initial window size.
    pub size: Size,

    /// Minimum window size.
    pub min_size: Option<Size>,

    /// Maximum window size.
    pub max_size: Option<Size>,

    /// Whether the window is resizable.
    pub resizable: bool,

    /// Whether to show the window decorations.
    pub decorations: bool,

    /// Whether to start in fullscreen mode.
    pub fullscreen: bool,

    /// Whether to enable vsync.
    pub vsync: bool,

    /// Target frame rate (FPS).
    pub target_fps: u32,

    /// Whether to show performance overlay.
    pub show_performance_overlay: bool,

    /// Whether to enable debug paint.
    pub debug_paint: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            title: "FLUI App".to_string(),
            size: Size::new(px(800.0), px(600.0)),
            min_size: None,
            max_size: None,
            resizable: true,
            decorations: true,
            fullscreen: false,
            vsync: true,
            target_fps: 60,
            show_performance_overlay: false,
            debug_paint: false,
        }
    }
}

impl AppConfig {
    /// Create a new default configuration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the window title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Set the initial window size.
    pub fn with_size(mut self, width: u32, height: u32) -> Self {
        self.size = Size::new(px(width as f32), px(height as f32));
        self
    }

    /// Set the minimum window size.
    pub fn with_min_size(mut self, width: u32, height: u32) -> Self {
        self.min_size = Some(Size::new(px(width as f32), px(height as f32)));
        self
    }

    /// Set the maximum window size.
    pub fn with_max_size(mut self, width: u32, height: u32) -> Self {
        self.max_size = Some(Size::new(px(width as f32), px(height as f32)));
        self
    }

    /// Set whether the window is resizable.
    pub fn with_resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Set whether to show window decorations.
    pub fn with_decorations(mut self, decorations: bool) -> Self {
        self.decorations = decorations;
        self
    }

    /// Set whether to start in fullscreen.
    pub fn with_fullscreen(mut self, fullscreen: bool) -> Self {
        self.fullscreen = fullscreen;
        self
    }

    /// Set whether to enable vsync.
    pub fn with_vsync(mut self, vsync: bool) -> Self {
        self.vsync = vsync;
        self
    }

    /// Set the target frame rate.
    pub fn with_target_fps(mut self, fps: u32) -> Self {
        self.target_fps = fps;
        self
    }

    /// Enable performance overlay.
    pub fn with_performance_overlay(mut self, show: bool) -> Self {
        self.show_performance_overlay = show;
        self
    }

    /// Enable debug paint.
    pub fn with_debug_paint(mut self, enabled: bool) -> Self {
        self.debug_paint = enabled;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert_eq!(config.title, "FLUI App");
        assert_eq!(config.target_fps, 60);
        assert!(config.resizable);
    }

    #[test]
    fn test_builder_pattern() {
        let config = AppConfig::new()
            .with_title("Test App")
            .with_size(1024, 768)
            .with_resizable(false);

        assert_eq!(config.title, "Test App");
        assert_eq!(config.size.width, 1024.0);
        assert_eq!(config.size.height, 768.0);
        assert!(!config.resizable);
    }
}
