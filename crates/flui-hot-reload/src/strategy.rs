//! Hot-reload strategy and shared constants.
//!
//! FLUI uses a **two-layer** dev model:
//!
//! 1. **Build orchestration** (dev-time) — watch sources, run `cargo build`, produce a
//!    new artifact (`.so` / `.dll` / binary).
//! 2. **Artifact reload** (runtime) — detect artifact changes (mtime poll), unload the
//!    old dynamic library, load the new one, rebuild the scene/widget tree.
//!
//! These layers are orthogonal: the CLI handles layer 1; [`crate::HotReloadDriver`] handles
//! layer 2. They compose for plugin-based workflows (`flui run --scene`, desktop
//! scene plugins) but can also be used independently.

use std::time::Duration;

/// How code changes are applied during development.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReloadStrategy {
    /// In-process dynamic library swap via [`crate::HotReloadDriver`].
    ///
    /// The host process stays alive; only the plugin `.so`/`.dll` is reloaded.
    /// Widget state is **not** preserved (`app_plugin!` performs a hot restart).
    PluginDylib,

    /// Full process kill + `cargo run` (`flui run` default desktop mode).
    ///
    /// Simplest path for apps that are not split into host + `cdylib` plugin.
    ProcessRestart,

    /// Rebuild and deploy artifact without restarting the host (`flui run --scene`).
    ///
    /// The running app picks up the new `.so` via mtime polling in
    /// [`crate::HotReloadDriver`].
    BuildAndDeploy,

    /// Host/worker split — rebuild worker `cdylib` only; host applies
    /// `HotReloadTier::HotReload` (`flui run` with `[hot_reload]` in `flui.toml`).
    WorkerHost,

    /// No runtime reload (release builds, WASM, CI).
    None,
}

impl ReloadStrategy {
    /// Whether this strategy keeps the host process alive across code changes.
    #[must_use]
    pub const fn preserves_process(self) -> bool {
        matches!(
            self,
            Self::PluginDylib | Self::BuildAndDeploy | Self::WorkerHost
        )
    }

    /// Whether this strategy can preserve in-memory widget/state across reloads.
    #[must_use]
    pub const fn preserves_state(self) -> bool {
        matches!(self, Self::WorkerHost)
    }
}

/// Environment variable names used across the hot-reload stack.
pub mod env {
    /// Path to a scene/app plugin shared library for in-process reload.
    ///
    /// Example: `FLUI_SCENE_PLUGIN=target/debug/libflui_scene.so`
    pub const SCENE_PLUGIN: &str = "FLUI_SCENE_PLUGIN";

    /// Set to `"1"` when `flui run` spawns the app with hot-reload orchestration.
    pub const HOT_RELOAD: &str = "FLUI_HOT_RELOAD";
}

/// Shared timing defaults for polling and debouncing.
pub mod timing {
    use super::Duration;

    /// How often [`crate::HotReloadDriver`] checks the plugin artifact mtime.
    pub const ARTIFACT_POLL: Duration = Duration::from_millis(500);

    /// Debounce for source-file watchers in `flui run` desktop mode.
    pub const SOURCE_DEBOUNCE: Duration = Duration::from_millis(500);

    /// Debounce for `flui run --scene` Android scene rebuild loop.
    pub const ANDROID_SCENE_DEBOUNCE: Duration = Duration::from_millis(300);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_dylib_preserves_process() {
        assert!(ReloadStrategy::PluginDylib.preserves_process());
        assert!(!ReloadStrategy::ProcessRestart.preserves_process());
    }

    #[test]
    fn no_strategy_preserves_state_yet() {
        assert!(!ReloadStrategy::PluginDylib.preserves_state());
        assert!(ReloadStrategy::WorkerHost.preserves_state());
    }
}
