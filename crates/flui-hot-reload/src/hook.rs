//! The dlopen reload paths as development reload hooks (ADR-0094 §1).
//!
//! A host application installs one of these on its configuration, and
//! `flui-app` drives it through [`DevReloadHook`] without naming this crate:
//!
//! ```rust,ignore
//! use flui::hot_reload::WorkerReloadHook;
//!
//! let config = flui::AppConfig::new().with_dev_reload(WorkerReloadHook::new(worker_path));
//! ```
//!
//! - [`WorkerReloadHook`] — the host/worker split `flui create --hot-reload`
//!   generates: reloads the worker `cdylib` when its artifact changes, and
//!   turns a worker's [`request_rebuild`](crate::request_rebuild) into a
//!   reassemble of every UI runtime.
//! - [`ScenePluginHook`] — `flui run --scene`: a scene plugin that draws
//!   whole frames instead of the widget tree.

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::Duration,
};

use flui_sdk::view::dev_reload::{DevReloadHook, ReloadEvent, ReloadWake};

use crate::{
    HotReloadDriver, Scene, WorkerPollOutcome, WorkerReloadDriver, strategy::timing,
    worker_artifact_stamp,
};

/// Reloads a host/worker application's worker `cdylib`.
///
/// - **Attached**, it watches the worker artifact on a background thread and
///   wakes the host when it changes, so an idle window still picks up an
///   edit; with the `app-plugin` feature it also receives the worker's
///   [`request_rebuild`](crate::request_rebuild) calls.
/// - **Polled**, it reloads a changed worker on the owner thread and reports
///   [`ReloadEvent::Patched`], which reassembles every UI runtime. A reload whose
///   shared-type layout changed, or that failed, is logged loudly and keeps
///   the last good tree.
/// - **Detached** or dropped, it stops and joins the watcher and drops its
///   rebuild registration.
pub struct WorkerReloadHook {
    driver: WorkerReloadDriver,
    watch_path: PathBuf,
    watch_interval: Duration,
    /// A worker asked for a rebuild since the last poll.
    rebuild_requested: Arc<AtomicBool>,
    watcher: Option<ArtifactWatcher>,
    #[cfg(feature = "app-plugin")]
    registration: Option<crate::RebuildHookRegistration>,
}

impl std::fmt::Debug for WorkerReloadHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkerReloadHook")
            .field("watch_path", &self.watch_path)
            .field("attached", &self.watcher.is_some())
            .finish_non_exhaustive()
    }
}

impl WorkerReloadHook {
    /// A hook for the worker dylib at `path` (or the path its sidecar
    /// manifest names, as [`WorkerReloadDriver`] resolves it). The worker is
    /// loaded now if it exists, and on a later poll if it appears.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        let watch_path = path.into();
        Self {
            driver: WorkerReloadDriver::new(&watch_path),
            watch_path,
            watch_interval: timing::WATCHER_POLL,
            rebuild_requested: Arc::new(AtomicBool::new(false)),
            watcher: None,
            #[cfg(feature = "app-plugin")]
            registration: None,
        }
    }
}

impl DevReloadHook for WorkerReloadHook {
    fn attach(&mut self, wake: ReloadWake) {
        self.detach();
        #[cfg(feature = "app-plugin")]
        {
            let requested = Arc::clone(&self.rebuild_requested);
            let rebuild_wake = wake.clone();
            self.registration = Some(crate::register_request_rebuild(move || {
                requested.store(true, Ordering::Release);
                rebuild_wake.wake();
            }));
        }
        self.watcher = ArtifactWatcher::spawn(&self.watch_path, self.watch_interval, wake);
    }

    fn detach(&mut self) {
        // Joins the thread (bounded by one watch interval), so nothing
        // touches the artifact path once this returns.
        drop(self.watcher.take());
        #[cfg(feature = "app-plugin")]
        drop(self.registration.take());
    }

    fn poll(&mut self) -> ReloadEvent {
        let outcome = self.driver.poll();
        event_for(
            outcome,
            self.rebuild_requested.swap(false, Ordering::AcqRel),
        )
    }
}

impl Drop for WorkerReloadHook {
    fn drop(&mut self) {
        self.detach();
    }
}

/// The event a worker poll reports, logging the outcomes that need the
/// developer's attention. A rebuild request is honoured whatever the
/// artifact did: a degraded or failed reload keeps the last good worker (or
/// none), which a reassemble calls safely.
fn event_for(outcome: WorkerPollOutcome, rebuild_requested: bool) -> ReloadEvent {
    match outcome {
        WorkerPollOutcome::Reloaded { reload_count } => {
            tracing::info!(reload_count, "hot reload: worker reloaded; reassembling");
            return ReloadEvent::Patched;
        }
        WorkerPollOutcome::Degraded {
            old_fingerprint,
            new_fingerprint,
        } => {
            tracing::error!(
                old_fingerprint,
                new_fingerprint,
                "hot reload: the worker's shared-type layout changed, so the new build \
                 functions cannot be called against this host's state. Restart the host to \
                 adopt them (hot restart is not wired yet). The current frame keeps rendering \
                 the last good tree."
            );
        }
        WorkerPollOutcome::ReloadFailed => {
            tracing::warn!(
                "hot reload: the worker dylib changed but reload failed (locked or missing \
                 symbols); fix the build and save again"
            );
        }
        WorkerPollOutcome::NoChange => {}
    }
    if rebuild_requested {
        ReloadEvent::Patched
    } else {
        ReloadEvent::Unchanged
    }
}

/// Polls the worker artifact's stamp off the owner thread and wakes the host
/// when it changes. It does no loading: the `dlopen`/`dlclose` cycle stays
/// on the owner thread, inside [`WorkerReloadDriver::poll`]. It watches the
/// **artifact**, not `src/`: the CLI watches sources and owns the rebuild
/// (the two-layer rule in `docs/hot-reload.md`).
struct ArtifactWatcher {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl ArtifactWatcher {
    fn spawn(path: &Path, interval: Duration, wake: ReloadWake) -> Option<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let thread_path = path.to_path_buf();
        let spawned = std::thread::Builder::new()
            .name("flui-worker-watch".to_string())
            .spawn(move || {
                let mut last = worker_artifact_stamp(&thread_path);
                while !stop_thread.load(Ordering::Acquire) {
                    std::thread::sleep(interval);
                    if stop_thread.load(Ordering::Acquire) {
                        break;
                    }
                    let now = worker_artifact_stamp(&thread_path);
                    if now != last {
                        tracing::debug!(
                            path = %now.0.display(),
                            "hot reload: worker artifact changed; waking owner"
                        );
                        last = now;
                        wake.wake();
                    }
                }
            });
        match spawned {
            Ok(handle) => {
                tracing::info!(path = %path.display(), "hot reload: worker watcher started");
                Some(Self {
                    stop,
                    handle: Some(handle),
                })
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    "hot reload: could not start the worker watcher; an idle window picks up \
                     an edit at its next frame"
                );
                None
            }
        }
    }
}

impl Drop for ArtifactWatcher {
    fn drop(&mut self) {
        // Signal first, then join: the thread sleeps one interval at a time,
        // so the join is bounded by it.
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Lets an Android scene plugin (`flui run --scene`) draw whole frames.
///
/// Each frame it checks the plugin library for an update, reloading it on
/// the owner thread, and, while a plugin is loaded, has it build the frame's
/// scene, which the host renders in place of the widget tree. It never
/// reports a patch: a scene plugin replaces frames, not code the widget tree
/// runs.
pub struct ScenePluginHook {
    driver: HotReloadDriver,
    pending_font_reset: bool,
}

impl std::fmt::Debug for ScenePluginHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScenePluginHook")
            .field("lib_path", &self.driver.lib_path())
            .field("loaded", &self.driver.is_loaded())
            .finish_non_exhaustive()
    }
}

impl ScenePluginHook {
    /// The plugin's file name, which `flui run --scene` pushes.
    pub const LIBRARY_NAME: &'static str = "libflui_scene.so";

    /// A hook for the scene plugin library at `path`; loaded now if it
    /// exists, and when it appears otherwise.
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            driver: HotReloadDriver::new(path),
            pending_font_reset: true,
        }
    }

    fn observe_image_update(&mut self, updated: bool) {
        self.pending_font_reset |= updated;
    }

    fn render_loaded_scene(
        &mut self,
        scene: &Scene,
        render: &mut dyn FnMut(&Scene, bool) -> bool,
    ) -> bool {
        let rendered = render(scene, self.pending_font_reset);
        if rendered {
            self.pending_font_reset = false;
        }
        rendered
    }

    /// Where `flui run --scene` puts the plugin on a device: the app's
    /// internal files directory (`AndroidApp::internal_data_path`), or
    /// `/data/local/tmp` when the app has none.
    pub fn device_library_path(internal_data_path: Option<&Path>) -> PathBuf {
        internal_data_path
            .unwrap_or_else(|| Path::new("/data/local/tmp"))
            .join(Self::LIBRARY_NAME)
    }
}

impl DevReloadHook for ScenePluginHook {
    fn attach(&mut self, _wake: ReloadWake) {}

    fn poll(&mut self) -> ReloadEvent {
        ReloadEvent::Unchanged
    }

    #[expect(
        unsafe_code,
        reason = "implements the plugin payload lifetime contract"
    )]
    unsafe fn scene_frame(
        &mut self,
        width: f64,
        height: f64,
        render: &mut dyn FnMut(&Scene, bool) -> bool,
    ) -> bool {
        let updated = self.driver.poll();
        self.observe_image_update(updated);

        // SAFETY: `HotReloadDriver::build_scene` reclaims a scene the plugin
        // allocated, so host and plugin must agree on `Scene`'s layout and
        // the scene must be dropped before the library is unloaded. The
        // layout agreement is the ABI-token handshake the loader checked; the
        // original scene is dropped before this method returns, while the
        // driver holds the image. The unsafe scene_frame caller additionally
        // guarantees render retains no cloned image-dependent payloads.
        #[expect(unsafe_code)]
        let built = unsafe { self.driver.build_scene(width, height) };
        let Some(scene) = built else {
            return false;
        };
        let rendered = self.render_loaded_scene(&scene, render);
        drop(scene);
        rendered
    }
}

#[cfg(test)]
mod tests;
