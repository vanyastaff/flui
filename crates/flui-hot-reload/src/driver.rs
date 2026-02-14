//! Hot-reload driver — manages the plugin lifecycle and mtime-based polling.
//!
//! `HotReloadDriver` replaces the ad-hoc polling/reload loops (like the one in
//! `android_demo`) with a reusable abstraction. It handles:
//!
//! - Initial plugin load attempt
//! - Periodic mtime polling to detect changes
//! - Automatic unload → reload when the library file changes
//! - Lazy loading (plugin can appear on disk after the driver starts)
//! - Fallback scene support when no plugin is loaded
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_hot_reload::HotReloadDriver;
//! use std::path::Path;
//! use std::time::Duration;
//!
//! let mut driver = HotReloadDriver::new(Path::new("/path/to/libflui_scene.so"))
//!     .with_poll_interval(Duration::from_millis(500));
//!
//! // In your event loop:
//! loop {
//!     if let Some(scene) = driver.poll(width, height) {
//!         renderer.render_scene(&scene);
//!     }
//! }
//! ```

use crate::host::{PluginKind, ScenePlugin};
use flui_layer::Scene;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Default polling interval for mtime checks.
const DEFAULT_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Manages the hot-reload lifecycle for a scene plugin.
///
/// Wraps [`ScenePlugin`] with automatic mtime-based change detection and
/// reload. Call [`poll()`](Self::poll) from your event loop — it returns
/// `Some(Scene)` when a plugin was (re)loaded and a new scene is available.
///
/// When no plugin is loaded, [`build_scene()`](Self::build_scene) returns `None`,
/// allowing the caller to fall back to a built-in scene.
#[allow(missing_debug_implementations)]
pub struct HotReloadDriver {
    plugin: Option<ScenePlugin>,
    lib_path: PathBuf,
    poll_interval: Duration,
    last_poll: Instant,
    reload_count: u32,
}

impl HotReloadDriver {
    /// Create a new driver that watches the given shared library path.
    ///
    /// Immediately attempts to load the plugin. If the file doesn't exist yet,
    /// the driver will retry on each [`poll()`](Self::poll) call.
    pub fn new(lib_path: impl AsRef<Path>) -> Self {
        let lib_path = lib_path.as_ref().to_path_buf();
        let plugin = ScenePlugin::load(&lib_path);

        if plugin.is_some() {
            tracing::info!(
                "HotReloadDriver: plugin loaded from {}",
                lib_path.display()
            );
        } else {
            tracing::info!(
                "HotReloadDriver: no plugin at {} (will retry on poll)",
                lib_path.display()
            );
        }

        Self {
            plugin,
            lib_path,
            poll_interval: DEFAULT_POLL_INTERVAL,
            last_poll: Instant::now(),
            reload_count: 0,
        }
    }

    /// Set the polling interval for mtime checks (default: 500ms).
    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// Poll for plugin updates and return a new scene if the plugin was (re)loaded.
    ///
    /// This method should be called from your event loop. It:
    /// 1. Checks if enough time has elapsed since the last poll
    /// 2. If so, checks the library file's mtime for changes
    /// 3. If the file changed, unloads the old plugin and loads the new one
    /// 4. If a reload happened, builds and returns a new scene
    ///
    /// Returns `Some(Scene)` when a reload happened (caller should re-render).
    /// Returns `None` when no update was detected or the poll interval hasn't elapsed.
    pub fn poll(&mut self, width: f32, height: f32) -> Option<Scene> {
        if self.last_poll.elapsed() < self.poll_interval {
            return None;
        }
        self.last_poll = Instant::now();

        if let Some(ref plugin) = self.plugin {
            // Plugin loaded — check for updates
            if plugin.has_update() {
                tracing::info!("HotReloadDriver: plugin updated — reloading");
                let old = self.plugin.take().expect("plugin was Some");
                let kind = old.kind();
                old.unload();

                self.plugin = ScenePlugin::load(&self.lib_path);
                if self.plugin.is_some() {
                    self.reload_count += 1;
                    tracing::info!(
                        "HotReloadDriver: reloaded ({:?}, reload #{})",
                        kind,
                        self.reload_count
                    );
                    return self.build_scene(width, height);
                }
                tracing::warn!("HotReloadDriver: reload failed — plugin not available");
            }
        } else {
            // No plugin loaded — try to load (file may have appeared on disk)
            self.plugin = ScenePlugin::load(&self.lib_path);
            if self.plugin.is_some() {
                tracing::info!(
                    "HotReloadDriver: plugin now available — loaded from {}",
                    self.lib_path.display()
                );
                return self.build_scene(width, height);
            }
        }

        None
    }

    /// Build a scene using the currently loaded plugin.
    ///
    /// Returns `None` if no plugin is loaded (caller should use a fallback scene).
    pub fn build_scene(&self, width: f32, height: f32) -> Option<Scene> {
        self.plugin
            .as_ref()
            .map(|p| p.build_scene(width, height))
    }

    /// Whether a plugin is currently loaded.
    pub fn is_loaded(&self) -> bool {
        self.plugin.is_some()
    }

    /// The kind of plugin loaded, if any.
    pub fn plugin_kind(&self) -> Option<PluginKind> {
        self.plugin.as_ref().map(ScenePlugin::kind)
    }

    /// The plugin version, if loaded.
    pub fn plugin_version(&self) -> Option<u32> {
        self.plugin.as_ref().map(ScenePlugin::version)
    }

    /// How many times the plugin has been reloaded since the driver was created.
    pub fn reload_count(&self) -> u32 {
        self.reload_count
    }

    /// The library file path being watched.
    pub fn lib_path(&self) -> &Path {
        &self.lib_path
    }

    /// Build a scene or fall back to a default scene builder.
    ///
    /// Convenience method that calls the plugin's `build_scene` if loaded,
    /// otherwise calls the provided fallback function.
    pub fn build_scene_or<F>(&self, width: f32, height: f32, fallback: F) -> Scene
    where
        F: FnOnce(f32, f32) -> Scene,
    {
        self.build_scene(width, height)
            .unwrap_or_else(|| fallback(width, height))
    }
}
