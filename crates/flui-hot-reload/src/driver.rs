//! Hot-reload driver — manages the plugin lifecycle and mtime-based polling.
//!
//! `HotReloadDriver` is the plugin polling and reload loop a host embeds. It
//! handles:
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
//!     if driver.poll() {
//!         // SAFETY: the scene and any clones are dropped before the next poll
//!         // or before dropping the driver, keeping plugin code loaded.
//!         if let Some(scene) = unsafe { driver.build_scene(width, height) } {
//!             renderer.render_scene(&scene);
//!         }
//!     }
//! }
//! ```

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use flui_layer::Scene;

use crate::{
    host::{PluginKind, ScenePlugin},
    strategy::timing,
};

/// Manages the hot-reload lifecycle for a scene plugin.
///
/// Wraps [`ScenePlugin`] with automatic mtime-based change detection and
/// reload. Call [`poll()`](Self::poll) from your event loop to update the
/// loaded image, then construct a scene through [`Self::build_scene`].
///
/// When no plugin is loaded, [`build_scene()`](Self::build_scene) returns
/// `None`, allowing the caller to fall back to a built-in scene.
#[expect(missing_debug_implementations)]
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
            tracing::info!("HotReloadDriver: plugin loaded from {}", lib_path.display());
        } else {
            tracing::info!(
                "HotReloadDriver: no plugin at {} (will retry on poll)",
                lib_path.display()
            );
        }

        Self {
            plugin,
            lib_path,
            poll_interval: timing::ARTIFACT_POLL,
            last_poll: Instant::now(),
            reload_count: 0,
        }
    }

    /// Set the polling interval for mtime checks (default: 500ms).
    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// Poll for plugin updates without constructing a scene.
    ///
    /// Returns `true` when a plugin was loaded or reloaded successfully, and
    /// `false` when unchanged, throttled, or unavailable. Call
    /// [`Self::build_scene`] separately; its caller must ensure every scene
    /// and retained clone is dropped before a later poll can unload its image.
    pub fn poll(&mut self) -> bool {
        if self.last_poll.elapsed() < self.poll_interval {
            return false;
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
                    return true;
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
                return true;
            }
        }

        false
    }

    /// Build a scene using the currently loaded plugin.
    ///
    /// Returns `None` if no plugin is loaded (caller should use a fallback
    /// scene) — the same outcome, for the same "skip this frame" reason, as
    /// a loaded `app_plugin!` refusing a wrong-thread call (see
    /// [`ScenePlugin::build_scene`]'s docs). The caller cannot and need not
    /// tell the two apart.
    ///
    /// # Safety
    ///
    /// Forwards [`ScenePlugin::build_scene`]'s contract unchanged — see it for
    /// the obligations the caller must establish about host/plugin agreement
    /// and about dropping the `Scene` before the library is unloaded.
    #[expect(unsafe_code)]
    pub unsafe fn build_scene(&self, width: f64, height: f64) -> Option<Scene> {
        // SAFETY: the caller of this fn has assumed the same obligations.
        self.plugin
            .as_ref()
            .and_then(|p| unsafe { p.build_scene(width, height) })
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

    /// How many times the plugin has been reloaded since the driver was
    /// created.
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
    /// # Safety
    ///
    /// Forwards [`Self::build_scene`]'s contract unchanged.
    #[expect(unsafe_code)]
    pub unsafe fn build_scene_or<F>(&self, width: f64, height: f64, fallback: F) -> Scene
    where
        F: FnOnce(f64, f64) -> Scene,
    {
        // SAFETY: the caller of this fn has assumed the same obligations.
        unsafe { self.build_scene(width, height) }.unwrap_or_else(|| fallback(width, height))
    }
}
