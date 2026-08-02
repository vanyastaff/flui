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
    not(target_os = "ios"),
    not(target_arch = "wasm32"),
    feature = "hot-reload"
))]
pub(crate) use enabled::{RebuildHookGuard, WorkerReload};
// Reached only by the runner's rebuild-hook lifecycle test.
#[cfg(all(
    test,
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32"),
    feature = "hot-reload"
))]
pub(crate) use enabled::queued_hot_reload_hook;

#[cfg(all(target_os = "android", not(feature = "hot-reload")))]
pub(crate) use disabled::ScenePlugin;
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32"),
    not(feature = "hot-reload")
))]
pub(crate) use disabled::{RebuildHookGuard, WorkerReload};

#[cfg(feature = "hot-reload")]
mod enabled {
    #[cfg(target_os = "android")]
    use std::path::Path;
    use std::sync::Arc;

    #[cfg(target_os = "android")]
    use flui_hot_reload::HotReloadDriver;
    use flui_hot_reload::{
        HotReloadTier, RebuildHookRegistration, WorkerPollOutcome, WorkerReloadDriver, engine::env,
        register_request_rebuild,
    };
    use parking_lot::Mutex;

    use crate::app::{
        AppConfig,
        binding::AppBinding,
        ui_realm::{UiCommandSender, UiRealm},
    };

    /// Turn a worker-side rebuild request into a queued hot-reload command on
    /// the realm's inbox.
    ///
    /// The hook fires on whatever thread the file watcher runs on, so it may
    /// only *enqueue*: the reassemble itself commits on the owner thread, at
    /// the next Idle drain.
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    pub(crate) fn queued_hot_reload_hook(
        sender: UiCommandSender,
    ) -> impl Fn() + Send + Sync + 'static {
        move || {
            if let Err(error) = sender.request_hot_reload(HotReloadTier::HotReload) {
                tracing::warn!(
                    ?error,
                    "ignoring hot-reload request for a dead or busy realm"
                );
            }
        }
    }

    /// Keeps a registered rebuild hook attached; dropping it detaches.
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    pub(crate) struct RebuildHookGuard(#[allow(dead_code)] Option<RebuildHookRegistration>);

    /// The desktop worker-plugin driver, shared between the bootstrap and the
    /// per-frame callback. `Clone` shares the underlying driver.
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    #[derive(Clone)]
    pub(crate) struct WorkerReload {
        driver: Arc<Mutex<Option<WorkerReloadDriver>>>,
        active: bool,
    }

    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    impl WorkerReload {
        /// Resolve the worker dylib from the application config, falling back
        /// to `FLUI_WORKER_PLUGIN` for CLI compatibility.
        pub(crate) fn from_config(config: &AppConfig) -> Self {
            let driver = config
                .worker_plugin_path
                .clone()
                .or_else(|| std::env::var(env::WORKER_PLUGIN).ok().map(Into::into))
                .map(WorkerReloadDriver::new);
            Self {
                active: driver.is_some(),
                driver: Arc::new(Mutex::new(driver)),
            }
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
        pub(crate) fn poll_and_apply(&self, realm: &UiRealm) {
            if let Some(ref mut driver) = *self.driver.lock()
                && matches!(driver.poll(), WorkerPollOutcome::Reloaded { .. })
            {
                AppBinding::instance().perform_hot_reload_entered(realm, HotReloadTier::HotReload);
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
            renderer: &mut flui_engine::wgpu::Renderer,
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
            #[allow(unsafe_code)]
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
}

#[cfg(not(feature = "hot-reload"))]
mod disabled {
    #[cfg(target_os = "android")]
    use std::path::Path;

    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    use crate::app::{
        AppConfig,
        ui_realm::{UiCommandSender, UiRealm},
    };

    /// Inert stand-in for a rebuild-hook registration.
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    pub(crate) struct RebuildHookGuard;

    // The real guard detaches the hook on drop, and the runner drops this value
    // deliberately when the event loop exits. Keeping the `Drop` impl on both
    // sides means that call site reads identically in either build.
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    impl Drop for RebuildHookGuard {
        fn drop(&mut self) {}
    }

    /// Inert stand-in for the desktop worker-plugin driver.
    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    #[derive(Clone)]
    pub(crate) struct WorkerReload;

    #[cfg(all(
        not(target_os = "android"),
        not(target_os = "ios"),
        not(target_arch = "wasm32")
    ))]
    impl WorkerReload {
        pub(crate) fn from_config(config: &AppConfig) -> Self {
            // A configured worker plugin that silently does nothing is worse
            // than a missing feature, so say so once at bootstrap.
            if config.worker_plugin_path.is_some() {
                tracing::warn!(
                    "a worker plugin path is configured, but flui-app was built without the \
                     `hot-reload` feature — the plugin will not be loaded"
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
            _renderer: &mut flui_engine::wgpu::Renderer,
            _width: f32,
            _height: f32,
        ) -> bool {
            false
        }
    }
}
