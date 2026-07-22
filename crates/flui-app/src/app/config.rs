//! Application configuration.

use std::path::PathBuf;

use flui_types::{Size, geometry::px};

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
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
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
    ///
    /// Not currently wired: `From<&AppConfig> for flui_platform::WindowOptions`
    /// has no fullscreen field to carry this into, and nothing calls
    /// `PlatformWindow::toggle_fullscreen` at startup to honor it. The intended
    /// consumer is the desktop bootstrap in `runner.rs`, once `WindowOptions`
    /// (or a post-creation `toggle_fullscreen` call keyed off this field)
    /// grows the plumbing end to end.
    pub fullscreen: bool,

    /// Whether to enable vsync.
    ///
    /// Not currently wired: `From<&AppConfig> for flui_platform::WindowOptions`
    /// drops this field. `AppBinding::vsync()`/`VsyncScope` is an unrelated
    /// animation-ticker registry, not the GPU present mode — do not confuse
    /// the two. The intended consumer is `flui-engine`'s
    /// `select_present_mode`, which today always chooses `Fifo` regardless of
    /// this value.
    pub vsync: bool,

    /// Advisory target frame rate (FPS) — **not enforced pacing**.
    ///
    /// The desktop runner's steady-state pacing comes from the GPU-side
    /// blocking Fifo present (`flui-engine::wgpu::Renderer::render_scene`
    /// blocks in `present()` until the next vsync for every frame that
    /// actually presents), not from this value. Consumer audit (App.1
    /// vsync pacing):
    /// - `run_app_with_config_impl` logs it (`target_fps_advisory`) at
    ///   startup; informational only.
    /// - `flui-platform`'s `PlatformCapabilities::default_target_fps` is a
    ///   platform-reported hint (e.g. `120` for a ProMotion display) that
    ///   nothing currently reads into this field — `AppConfig::default`
    ///   hardcodes `60` regardless of platform.
    ///
    /// The one place a target-fps-shaped value governs anything real is
    /// the no-present fallback throttle in `runner.rs`'s `run_desktop`
    /// (a fixed ~1/60s constant, not derived from this field) — see the
    /// frame-pacing ADR.
    pub target_fps: u32,

    /// Whether to show performance overlay.
    ///
    /// Not currently wired: `From<&AppConfig> for flui_platform::WindowOptions`
    /// drops this field and no overlay widget reads it yet. Intended
    /// consumer: a future debug overlay (`flui-devtools`'s frame profiler, or
    /// an equivalent in-tree overlay widget), analogous to Flutter's
    /// `showPerformanceOverlay`.
    pub show_performance_overlay: bool,

    /// Whether to enable debug paint.
    ///
    /// Not currently wired: `From<&AppConfig> for flui_platform::WindowOptions`
    /// drops this field and no paint-phase debug visualization reads it yet.
    /// Intended consumer: a future paint-phase hook analogous to Flutter's
    /// `debugPaintSizeEnabled`.
    pub debug_paint: bool,

    /// Optional hot-reload worker dylib path for host/worker apps.
    ///
    /// When unset, the desktop runner falls back to `FLUI_WORKER_PLUGIN` for
    /// CLI compatibility.
    pub worker_plugin_path: Option<PathBuf>,
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
            worker_plugin_path: None,
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

    /// Set the advisory target frame rate. See [`AppConfig::target_fps`] —
    /// this does not change how the frame loop is paced.
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

    /// Set the hot-reload worker dylib path for host/worker apps.
    pub fn with_worker_plugin_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.worker_plugin_path = Some(path.into());
        self
    }
}

impl From<&AppConfig> for flui_platform::WindowOptions {
    fn from(config: &AppConfig) -> Self {
        flui_platform::WindowOptions {
            title: config.title.clone(),
            size: config.size,
            resizable: config.resizable,
            visible: true,
            decorated: config.decorations,
            min_size: config.min_size,
            max_size: config.max_size,
        }
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
        assert!(config.worker_plugin_path.is_none());
    }

    #[test]
    fn test_builder_pattern() {
        let config = AppConfig::new()
            .with_title("Test App")
            .with_size(1024, 768)
            .with_resizable(false);

        assert_eq!(config.title, "Test App");
        assert_eq!(config.size.width, px(1024.0));
        assert_eq!(config.size.height, px(768.0));
        assert!(!config.resizable);
    }

    #[test]
    fn test_worker_plugin_path() {
        let config = AppConfig::new().with_worker_plugin_path("target/debug/libworker.so");

        assert_eq!(
            config.worker_plugin_path,
            Some(PathBuf::from("target/debug/libworker.so"))
        );
    }
}
