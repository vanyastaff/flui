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
//!     driver.poll();
//!     // SAFETY: the renderer keeps no clone of a scene payload past this
//!     // frame; the scene itself borrows the driver until it is dropped.
//!     match unsafe { driver.build_scene(width, height) } {
//!         Some(scene) => renderer.render_scene(&scene),
//!         None => renderer.render_scene(&fallback_scene(width, height)),
//!     }
//! }
//! ```

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use crate::{
    dynlib::LoadRefusal,
    host::{PluginScene, ScenePlugin},
    timing,
};

/// Manages the hot-reload lifecycle for a scene plugin.
///
/// Wraps [`ScenePlugin`] with automatic mtime-based change detection and
/// reload. Call [`poll()`](Self::poll) from your event loop to update the
/// loaded image, then construct a scene through [`Self::build_scene`].
///
/// When no plugin is loaded, [`build_scene()`](Self::build_scene) returns
/// `None`, allowing the caller to fall back to a built-in scene.
#[expect(
    missing_debug_implementations,
    reason = "`HotReloadDriver` owns a `ScenePlugin`, which holds raw library handles and has no `Debug`"
)]
pub struct HotReloadDriver {
    plugin: Option<ScenePlugin>,
    lib_path: PathBuf,
    poll_interval: Duration,
    last_poll: Instant,
    /// The plugin file that last failed to load, not reopened until rebuilt.
    refusal: LoadRefusal,
}

impl HotReloadDriver {
    /// Create a new driver that watches the given shared library path.
    ///
    /// Immediately attempts to load the plugin. If the file doesn't exist yet,
    /// the driver will retry on each [`poll()`](Self::poll) call.
    pub fn new(lib_path: impl AsRef<Path>) -> Self {
        let lib_path = lib_path.as_ref().to_path_buf();
        let mut refusal = LoadRefusal::default();
        let plugin = refusal.load(&lib_path, |path| ScenePlugin::load(path));

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
            refusal,
        }
    }

    /// Set the polling interval for mtime checks (default: the artifact poll
    /// interval every driver starts with).
    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// Poll for plugin updates without constructing a scene.
    ///
    /// Returns `true` when a plugin was loaded or reloaded successfully, and
    /// `false` when unchanged, throttled, or unavailable. Call
    /// [`Self::build_scene`] separately; a scene it built borrows the driver,
    /// so no poll can unload the image under it.
    pub fn poll(&mut self) -> bool {
        if self.last_poll.elapsed() < self.poll_interval {
            return false;
        }
        self.last_poll = Instant::now();

        if self.plugin.is_some() {
            let Some(old) = self.plugin.take_if(|plugin| plugin.has_update()) else {
                return false;
            };
            tracing::info!("HotReloadDriver: plugin updated — reloading");
            old.unload();
            self.plugin = self
                .refusal
                .load(&self.lib_path, |path| ScenePlugin::load(path));
            if self.plugin.is_none() {
                tracing::warn!("HotReloadDriver: reload failed — plugin not available");
                return false;
            }
            tracing::info!("HotReloadDriver: reloaded from {}", self.lib_path.display());
            return true;
        }

        // No plugin loaded: try to load a file that appeared or was rebuilt
        // since the last refusal.
        self.plugin = self
            .refusal
            .load(&self.lib_path, |path| ScenePlugin::load(path));
        if self.plugin.is_some() {
            tracing::info!(
                "HotReloadDriver: plugin now available — loaded from {}",
                self.lib_path.display()
            );
            return true;
        }
        false
    }

    /// Build a scene using the currently loaded plugin.
    ///
    /// Returns `None` if no plugin is loaded (caller should use a fallback
    /// scene) — the same outcome, for the same "skip this frame" reason, as
    /// a loaded plugin answering null because its code panicked or the call
    /// came from a thread `app_plugin!` did not pin (see
    /// [`ScenePlugin::build_scene`]'s docs). The caller cannot and need not
    /// tell them apart.
    ///
    /// The scene borrows the driver, so [`Self::poll`], which may unload the
    /// image, cannot run while it lives.
    ///
    /// # Safety
    ///
    /// Forwards [`ScenePlugin::build_scene`]'s contract unchanged: no clone of
    /// a scene payload outlives the driver's image.
    #[expect(
        unsafe_code,
        reason = "an `unsafe fn` that forwards `ScenePlugin::build_scene`'s contract, stated under `# Safety`"
    )]
    pub unsafe fn build_scene(&self, width: f64, height: f64) -> Option<PluginScene<'_>> {
        // SAFETY: the caller of this fn has assumed the same obligations.
        self.plugin
            .as_ref()
            .and_then(|p| unsafe { p.build_scene(width, height) })
    }

    /// Whether a plugin is currently loaded.
    pub fn is_loaded(&self) -> bool {
        self.plugin.is_some()
    }

    /// The library file path being watched.
    pub fn lib_path(&self) -> &Path {
        &self.lib_path
    }
}
