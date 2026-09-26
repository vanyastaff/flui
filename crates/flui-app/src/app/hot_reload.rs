//! The development-reload seam.
//!
//! Hot reload is a development capability. `flui-hot-reload` is therefore an
//! **optional** dependency of this crate, enabled by the `hot-reload` feature,
//! and an ordinary production graph does not contain it (`cargo tree -p
//! flui-app` shows no `flui-hot-reload` unless the feature is on).
//!
//! Everything in the runner that would otherwise name a `flui_hot_reload` item
//! goes through the types here, so the platform bootstraps stay single-version
//! instead of splitting into feature-forked copies:
//!
//! - [`WorkerReload`] — the desktop worker-dylib driver, plus the
//!   [`RebuildHookGuard`] whose `Drop` detaches the rebuild hook.
//! - `ScenePlugin` (Android only) — the scene-plugin driver, which may take
//!   over a frame entirely.
//!
//! With the feature off all three are inert: constructing one is a no-op,
//! polling never reports a reload, and the scene plugin never claims a frame.
//! The inert implementations name no `flui_hot_reload` item at all, which is
//! what makes the absent dependency compile.

// Each type is scoped to the runner that consumes it — `WorkerReload` to the
// desktop bootstrap, `ScenePlugin` to the Android one — so a target that does
// not consume it does not carry it as dead code. The repeated
// `not(android)/not(ios)/not(wasm32)` triple mirrors `run_desktop`'s own cfg in
// `runner.rs`; keep them in step.
#[cfg(all(target_os = "android", feature = "hot-reload"))]
pub(crate) use enabled::ScenePlugin;
#[cfg(all(
    not(target_os = "android"),
    not(target_arch = "wasm32"),
    feature = "hot-reload"
))]
pub(crate) use enabled::{RebuildHookGuard, WorkerReload, WorkerWatcherGuard};
// Reached only by the runner's rebuild-hook lifecycle test.
#[cfg(all(
    test,
    not(target_os = "android"),
    not(target_arch = "wasm32"),
    feature = "hot-reload"
))]
pub(crate) use enabled::queued_hot_reload_hook;

#[cfg(all(target_os = "android", not(feature = "hot-reload")))]
pub(crate) use disabled::ScenePlugin;
#[cfg(all(
    not(target_os = "android"),
    not(target_arch = "wasm32"),
    not(feature = "hot-reload")
))]
pub(crate) use disabled::{RebuildHookGuard, WorkerReload, WorkerWatcherGuard};

#[cfg(feature = "hot-reload")]
mod enabled {
    #[cfg(target_os = "android")]
    use std::path::Path;
    #[cfg(any(
        target_os = "android",
        all(not(target_os = "android"), not(target_arch = "wasm32"))
    ))]
    use std::sync::Arc;

    #[cfg(target_os = "android")]
    use flui_hot_reload::HotReloadDriver;
    #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
    use flui_hot_reload::{
        HotReloadTier, RebuildHookRegistration, WorkerPollOutcome, WorkerReloadDriver, engine::env,
        register_request_rebuild, strategy::timing, worker_artifact_stamp,
    };
    #[cfg(any(
        target_os = "android",
        all(not(target_os = "android"), not(target_arch = "wasm32"))
    ))]
    use parking_lot::Mutex;

    #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
    use crate::app::{
        AppConfig,
        ui_realm::{UiCommandSender, UiRealm},
    };
    #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
    use flui_runtime::reload::ReloadTier;

    /// The realm's reload tier for a driver's: the realm lives in
    /// `flui-runtime`, which may not name `flui-hot-reload`, so the tier is
    /// translated here, at the host. Exhaustive, so a new driver tier does not
    /// compile until it is given a meaning.
    #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
    const fn reload_tier(tier: HotReloadTier) -> ReloadTier {
        match tier {
            HotReloadTier::HotReload => ReloadTier::Reassemble,
            HotReloadTier::HotRestart => ReloadTier::Restart,
            HotReloadTier::FullRestart => ReloadTier::ProcessRestart,
        }
    }

    /// Turn a worker-side rebuild request into a queued hot-reload command on
    /// the realm's inbox.
    ///
    /// The hook fires on whatever thread the file watcher runs on, so it may
    /// only *enqueue*: the reassemble itself commits on the owner thread, at
    /// the next Idle drain.
    #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
    pub(crate) fn queued_hot_reload_hook(
        sender: UiCommandSender,
    ) -> impl Fn() + Send + Sync + 'static {
        move || {
            if let Err(error) = sender.request_hot_reload(reload_tier(HotReloadTier::HotReload)) {
                tracing::warn!(
                    ?error,
                    "ignoring hot-reload request for a dead or busy realm"
                );
            }
        }
    }

    /// Keeps a registered rebuild hook attached; dropping it detaches.
    #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
    pub(crate) struct RebuildHookGuard(#[expect(dead_code)] Option<RebuildHookRegistration>);

    /// The desktop worker-plugin driver, shared between the bootstrap and the
    /// per-frame callback. `Clone` shares the underlying driver.
    #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
    #[derive(Clone)]
    pub(crate) struct WorkerReload {
        driver: Arc<Mutex<Option<WorkerReloadDriver>>>,
        active: bool,
        /// Canonical worker path the watcher thread watches. `None` when no
        /// worker is configured (the whole capability is inert).
        watch_path: Option<std::path::PathBuf>,
    }

    #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
    impl WorkerReload {
        /// Resolve the worker dylib from the application config, falling back
        /// to `FLUI_WORKER_PLUGIN` for CLI compatibility.
        pub(crate) fn from_config(config: &AppConfig) -> Self {
            let path = config
                .worker_plugin_path
                .clone()
                .or_else(|| std::env::var(env::WORKER_PLUGIN).ok().map(Into::into));
            let driver = path.clone().map(WorkerReloadDriver::new);
            Self {
                active: driver.is_some(),
                driver: Arc::new(Mutex::new(driver)),
                watch_path: path,
            }
        }

        /// Start a background thread that watches the worker artifact and
        /// wakes the owner when it is rebuilt.
        ///
        /// [`Self::poll_and_apply`] runs at a frame boundary, which is enough
        /// while the app is animating but wrong when it is idle: an idle event
        /// loop produces no frames at all, so an edit would not be noticed
        /// until something unrelated produced one — the developer clicking the
        /// window, in the observed failure. This watcher closes that gap from
        /// the other side: it polls the artifact's stamp off-thread and, on a
        /// change, fires `wake`, which requests a frame; the next frame's
        /// `poll_and_apply` then performs the actual `dlopen` on the owner
        /// thread, exactly as before.
        ///
        /// Deliberately does no loading: the `dlopen`/`dlclose` cycle stays on
        /// the owner thread inside [`WorkerReloadDriver::poll`]. This watches
        /// the **artifact**, not `src/` — layer 1 (the CLI) already watches
        /// sources and owns the rebuild; layer 2 (this) only notices the new
        /// artifact, per the two-layer rule in `docs/hot-reload.md`. The
        /// mechanism is the same mtime/identity check the driver already uses,
        /// merely run where it can wake a sleeping loop rather than only at a
        /// frame nothing is producing.
        ///
        /// Returns `None` when no worker is configured. The returned guard
        /// stops the thread when dropped; it must outlive the event loop.
        pub(crate) fn spawn_watcher(
            &self,
            wake: Arc<dyn Fn() + Send + Sync>,
        ) -> Option<WorkerWatcherGuard> {
            let path = self.watch_path.clone()?;
            let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let stop_thread = Arc::clone(&stop);
            let thread_path = path.clone();

            let handle = std::thread::Builder::new()
                .name("flui-worker-watch".to_string())
                .spawn(move || {
                    let mut last = worker_artifact_stamp(&thread_path);
                    while !stop_thread.load(std::sync::atomic::Ordering::Acquire) {
                        std::thread::sleep(timing::WATCHER_POLL);
                        if stop_thread.load(std::sync::atomic::Ordering::Acquire) {
                            break;
                        }
                        let now = worker_artifact_stamp(&thread_path);
                        if now != last {
                            let changed_path = now.0.clone();
                            last = now;
                            tracing::debug!(
                                path = %changed_path.display(),
                                "hot reload: worker artifact changed; waking owner"
                            );
                            wake();
                        }
                    }
                })
                .ok()?;

            tracing::info!(
                path = %path.display(),
                "hot reload: background worker watcher started"
            );
            Some(WorkerWatcherGuard {
                stop,
                handle: Some(handle),
            })
        }

        /// Attach the rebuild hook for this realm, if a worker dylib is
        /// actually configured. The returned guard must outlive the event
        /// loop; dropping it detaches the hook.
        pub(crate) fn register_rebuild_hook(&self, sender: UiCommandSender) -> RebuildHookGuard {
            RebuildHookGuard(
                self.active
                    .then(|| register_request_rebuild(queued_hot_reload_hook(sender))),
            )
        }

        /// Poll the worker driver at a frame boundary and reassemble the realm
        /// when the dylib has been rebuilt.
        ///
        /// Every outcome is surfaced. `Reloaded` reassembles. `Degraded` (the
        /// worker's shared-type layout changed) and `ReloadFailed` are *not*
        /// silently dropped: the host keeps rendering the last good tree, and a
        /// loud warning names the required action — restart the host — so a
        /// developer is never left wondering why their edit had no effect. A
        /// full `HotRestart` remount that would adopt the new layout is not
        /// implemented; this is the honest interim behaviour, not a claim it is.
        pub(crate) fn poll_and_apply(&self, realm: &UiRealm) {
            let Some(ref mut driver) = *self.driver.lock() else {
                return;
            };
            match driver.poll() {
                WorkerPollOutcome::Reloaded { reload_count } => {
                    tracing::info!(reload_count, "hot reload: worker reloaded; reassembling");
                    realm.perform_hot_reload_entered(reload_tier(HotReloadTier::HotReload));
                }
                WorkerPollOutcome::Degraded {
                    old_fingerprint,
                    new_fingerprint,
                } => {
                    tracing::error!(
                        old_fingerprint,
                        new_fingerprint,
                        "hot reload: the worker's shared-type layout changed, so the new \
                         build functions cannot be called against this host's state. \
                         Restart the host to adopt them (hot restart is not wired yet). \
                         The current frame keeps rendering the last good tree."
                    );
                }
                WorkerPollOutcome::ReloadFailed => {
                    tracing::warn!(
                        "hot reload: the worker dylib changed but reload failed (locked or \
                         missing symbols); fix the build and save again"
                    );
                }
                WorkerPollOutcome::NoChange => {}
            }
        }
    }

    /// Stops the background worker-artifact watcher on drop. Must outlive the
    /// event loop, alongside [`RebuildHookGuard`].
    #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
    pub(crate) struct WorkerWatcherGuard {
        stop: Arc<std::sync::atomic::AtomicBool>,
        handle: Option<std::thread::JoinHandle<()>>,
    }

    #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
    #[cfg(test)]
    impl WorkerWatcherGuard {
        pub(crate) fn test_lifetime(
            &self,
        ) -> (std::thread::ThreadId, Arc<std::sync::atomic::AtomicBool>) {
            (
                self.handle
                    .as_ref()
                    .expect("watcher is live until Drop")
                    .thread()
                    .id(),
                Arc::clone(&self.stop),
            )
        }
    }

    #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
    impl Drop for WorkerWatcherGuard {
        fn drop(&mut self) {
            // Signal first, then join: the thread sleeps in short intervals, so
            // the join is bounded by one poll period, and joining keeps a
            // detached thread from touching the artifact path during teardown.
            self.stop.store(true, std::sync::atomic::Ordering::Release);
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    /// The Android scene-plugin driver: a rebuilt dylib may render the frame
    /// itself, bypassing the widget tree entirely.
    #[cfg(target_os = "android")]
    #[derive(Clone)]
    pub(crate) struct ScenePlugin {
        driver: Arc<Mutex<HotReloadDriver>>,
    }

    #[cfg(target_os = "android")]
    impl ScenePlugin {
        pub(crate) fn new(plugin_path: &Path) -> Self {
            Self {
                driver: Arc::new(Mutex::new(HotReloadDriver::new(plugin_path))),
            }
        }

        /// Poll for a plugin update and, if a plugin is live, let it own this
        /// frame. Returns `true` when the plugin rendered, in which case the
        /// caller must not run the widget pipeline for this frame.
        pub(crate) fn try_render_frame(
            &self,
            renderer: &mut flui_engine::Renderer,
            width: f32,
            height: f32,
        ) -> bool {
            let mut driver = self.driver.lock();

            // Poll for plugin updates (mtime check, auto-reload).
            driver.poll(width, height);

            // SAFETY: `HotReloadDriver::build_scene` reclaims a Box the plugin
            // allocated, so host and plugin must agree on `Scene`'s layout and
            // allocator, and the scene must be dropped before the library is
            // unloaded. Both hold here: the plugin is built from this workspace
            // by the same toolchain, and `scene` is consumed by `render_scene`
            // and dropped inside this block, while `driver` (owning the
            // library) outlives it. See that method's `# Safety` for why this
            // cannot be a safe call.
            #[expect(unsafe_code)]
            let built = unsafe { driver.build_scene(width, height) };

            let Some(scene) = built else {
                return false;
            };
            if let Err(error) = renderer.render_scene(&scene) {
                tracing::error!(?error, "Plugin render failed");
            }
            true
        }
    }

    #[cfg(all(test, not(target_os = "android"), not(target_arch = "wasm32")))]
    mod tests {
        use super::{HotReloadTier, ReloadTier, reload_tier};

        #[test]
        fn each_driver_tier_maps_to_the_realm_tier_that_applies_it() {
            assert_eq!(
                reload_tier(HotReloadTier::HotReload),
                ReloadTier::Reassemble
            );
            assert_eq!(reload_tier(HotReloadTier::HotRestart), ReloadTier::Restart);
            assert_eq!(
                reload_tier(HotReloadTier::FullRestart),
                ReloadTier::ProcessRestart
            );
        }
    }
}

#[cfg(not(feature = "hot-reload"))]
mod disabled {
    #[cfg(target_os = "android")]
    use std::path::Path;
    #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
    use std::sync::Arc;

    #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
    use crate::app::{
        AppConfig,
        ui_realm::{UiCommandSender, UiRealm},
    };

    /// Inert stand-in for a rebuild-hook registration.
    #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
    pub(crate) struct RebuildHookGuard;

    // The real guard detaches the hook on drop, and the runner drops this value
    // deliberately when the event loop exits. Keeping the `Drop` impl on both
    // sides means that call site reads identically in either build.
    #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
    impl Drop for RebuildHookGuard {
        fn drop(&mut self) {}
    }

    /// Inert stand-in for the desktop worker-plugin driver.
    #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
    #[derive(Clone)]
    pub(crate) struct WorkerReload;

    #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
    impl WorkerReload {
        pub(crate) fn from_config(_config: &AppConfig) -> Self {
            // The typed configuration API does not exist without the feature,
            // but the CLI may still leave its environment variable behind.
            // Surface that mismatch instead of silently ignoring it.
            if std::env::var_os("FLUI_WORKER_PLUGIN").is_some() {
                tracing::warn!(
                    "FLUI_WORKER_PLUGIN is set, but flui-app was built without the `hot-reload` \
                     feature — the plugin will not be loaded"
                );
            }
            Self
        }

        #[expect(
            clippy::unused_self,
            reason = "signature must mirror the enabled implementation"
        )]
        pub(crate) fn register_rebuild_hook(&self, _sender: UiCommandSender) -> RebuildHookGuard {
            RebuildHookGuard
        }

        #[expect(
            clippy::unused_self,
            reason = "signature must mirror the enabled implementation"
        )]
        pub(crate) fn poll_and_apply(&self, _realm: &UiRealm) {}

        /// Inert: with no worker machinery linked there is nothing to watch,
        /// so no thread is spawned and the guard is never needed.
        #[expect(
            clippy::unused_self,
            reason = "signature must mirror the enabled implementation"
        )]
        pub(crate) fn spawn_watcher(
            &self,
            _wake: Arc<dyn Fn() + Send + Sync>,
        ) -> Option<WorkerWatcherGuard> {
            None
        }
    }

    /// Inert stand-in for the background worker-artifact watcher guard.
    #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
    pub(crate) struct WorkerWatcherGuard;

    // Keep the `Drop` impl on both sides so the runner's teardown reads
    // identically in either build (mirrors `RebuildHookGuard`).
    #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
    impl Drop for WorkerWatcherGuard {
        fn drop(&mut self) {}
    }

    /// Inert stand-in for the Android scene-plugin driver.
    #[cfg(target_os = "android")]
    #[derive(Clone)]
    pub(crate) struct ScenePlugin;

    #[cfg(target_os = "android")]
    impl ScenePlugin {
        pub(crate) fn new(_plugin_path: &Path) -> Self {
            Self
        }

        /// Always `false`: with no plugin machinery linked, the widget
        /// pipeline owns every frame.
        #[expect(
            clippy::unused_self,
            reason = "signature must mirror the enabled implementation"
        )]
        pub(crate) fn try_render_frame(
            &self,
            _renderer: &mut flui_engine::Renderer,
            _width: f32,
            _height: f32,
        ) -> bool {
            false
        }
    }
}
