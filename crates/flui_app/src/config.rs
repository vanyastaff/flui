//! Application Configuration
//!
//! This module provides configuration types for FLUI applications,
//! designed to work seamlessly with the existing FLUI ecosystem.

use flui_types::{Offset, Size};
use std::time::Duration;

/// Application configuration
///
/// Provides sensible defaults for all platforms while allowing
/// customization where needed.
#[derive(Debug, Clone)]
pub struct AppConfig {
    /// Window configuration
    pub window: WindowConfig,

    /// Rendering configuration
    pub rendering: RenderConfig,

    /// Performance configuration
    pub performance: PerformanceConfig,

    /// Debug configuration
    pub debug: DebugConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            window: WindowConfig::default(),
            rendering: RenderConfig::default(),
            performance: PerformanceConfig::default(),
            debug: DebugConfig::default(),
        }
    }
}

impl AppConfig {
    /// Create a new configuration builder
    pub fn builder() -> AppConfigBuilder {
        AppConfigBuilder::new()
    }
}

/// Window configuration
#[derive(Debug, Clone)]
pub struct WindowConfig {
    /// Window title
    pub title: String,

    /// Initial window size (logical pixels)
    pub size: Size,

    /// Minimum window size
    pub min_size: Option<Size>,

    /// Maximum window size
    pub max_size: Option<Size>,

    /// Whether window is resizable
    pub resizable: bool,

    /// Whether window has decorations
    pub decorations: bool,

    /// Whether window starts maximized
    pub maximized: bool,

    /// Whether window starts in fullscreen
    pub fullscreen: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: "FLUI Application".to_string(),
            size: Size::new(800.0, 600.0),
            min_size: Some(Size::new(300.0, 200.0)),
            max_size: None,
            resizable: true,
            decorations: true,
            maximized: false,
            fullscreen: false,
        }
    }
}

/// Rendering configuration
#[derive(Debug, Clone)]
pub struct RenderConfig {
    /// Enable vertical sync
    pub vsync: bool,

    /// Power preference
    pub power_preference: PowerPreference,

    /// MSAA sample count (1 = disabled)
    pub msaa_samples: u32,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            vsync: true,
            power_preference: PowerPreference::HighPerformance,
            msaa_samples: 1,
        }
    }
}

/// Power preference for graphics adapter
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerPreference {
    /// Prefer integrated GPU for battery life
    LowPower,
    /// Prefer discrete GPU for performance
    HighPerformance,
}

/// Performance configuration
#[derive(Debug, Clone)]
pub struct PerformanceConfig {
    /// Target frames per second
    pub target_fps: u32,

    /// Enable performance monitoring
    pub monitoring: bool,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            target_fps: 60,
            monitoring: cfg!(debug_assertions),
        }
    }
}

/// Debug configuration
#[derive(Debug, Clone)]
pub struct DebugConfig {
    /// Enable debug overlays
    pub debug_overlays: bool,

    /// Log filter string
    pub log_filter: Option<String>,

    /// Enable hot reload
    pub hot_reload: bool,
}

impl Default for DebugConfig {
    fn default() -> Self {
        Self {
            debug_overlays: cfg!(debug_assertions),
            log_filter: None,
            hot_reload: cfg!(debug_assertions),
        }
    }
}

/// Application configuration builder
pub struct AppConfigBuilder {
    config: AppConfig,
}

impl AppConfigBuilder {
    /// Create a new configuration builder
    pub fn new() -> Self {
        Self {
            config: AppConfig::default(),
        }
    }

    /// Set window title
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.config.window.title = title.into();
        self
    }

    /// Set window size
    pub fn window_size(mut self, width: f64, height: f64) -> Self {
        self.config.window.size = Size::new(width, height);
        self
    }

    /// Set whether window is resizable
    pub fn resizable(mut self, resizable: bool) -> Self {
        self.config.window.resizable = resizable;
        self
    }

    /// Enable/disable VSync
    pub fn vsync(mut self, vsync: bool) -> Self {
        self.config.rendering.vsync = vsync;
        self
    }

    /// Set target FPS
    pub fn target_fps(mut self, fps: u32) -> Self {
        self.config.performance.target_fps = fps;
        self
    }

    /// Enable debug overlays
    pub fn debug_overlays(mut self, enabled: bool) -> Self {
        self.config.debug.debug_overlays = enabled;
        self
    }

    /// Set log filter
    pub fn log_filter(mut self, filter: impl Into<String>) -> Self {
        self.config.debug.log_filter = Some(filter.into());
        self
    }

    /// Build the final configuration
    pub fn build(self) -> AppConfig {
        self.config
    }
}

impl Default for AppConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert_eq!(config.window.title, "FLUI Application");
        assert_eq!(config.window.size, Size::new(800.0, 600.0));
        assert!(config.rendering.vsync);
        assert_eq!(config.performance.target_fps, 60);
    }

    #[test]
    fn test_config_builder() {
        let config = AppConfig::builder()
            .title("Test App")
            .window_size(1024.0, 768.0)
            .vsync(false)
            .target_fps(120)
            .build();

        assert_eq!(config.window.title, "Test App");
        assert_eq!(config.window.size, Size::new(1024.0, 768.0));
        assert!(!config.rendering.vsync);
        assert_eq!(config.performance.target_fps, 120);
    }
}
