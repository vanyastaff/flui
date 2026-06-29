//! Host-side worker dylib loader (Flutter-parity hot reload).
//!
//! Unlike [`ScenePlugin`](crate::ScenePlugin), a worker plugin does **not** own
//! the widget pipeline. It exports an `init` entry point that registers reloadable
//! `build()` implementations while the host binary retains element tree state.
//!
//! See `docs/designs/2026-06-28-flutter-parity-hot-reload.md`.

use std::path::{Path, PathBuf};

use crate::dynlib::{self, DynLib};

/// Default fingerprint when the worker omits `flui_worker_fingerprint`.
pub const DEFAULT_FINGERPRINT: u64 = 0;

type WorkerInitFn = extern "C" fn();
type WorkerVersionFn = extern "C" fn() -> u32;
type WorkerFingerprintFn = extern "C" fn() -> u64;

/// A loaded hot-reload worker dylib (`my_app_logic.dll`).
#[allow(missing_debug_implementations)]
pub struct WorkerPlugin {
    lib: DynLib,
    init_fn: WorkerInitFn,
    fingerprint_fn: Option<WorkerFingerprintFn>,
    version: u32,
    mtime: u64,
}

impl WorkerPlugin {
    /// Load a worker from `lib_path` and call its `flui_worker_init` hook.
    ///
    /// Returns `None` when the file is missing, unloadable, or lacks worker symbols.
    pub fn load(lib_path: impl AsRef<Path>) -> Option<Self> {
        let lib_path = lib_path.as_ref();
        let lib = DynLib::open(lib_path)?;

        #[allow(unsafe_code)]
        unsafe {
            let init_ptr = lib.symbol("flui_worker_init")?;
            if init_ptr.is_null() {
                return None;
            }
            let init_fn: WorkerInitFn = std::mem::transmute(init_ptr);

            let version = lib.symbol("flui_worker_version").map_or(0, |ptr| {
                let version_fn: WorkerVersionFn = std::mem::transmute(ptr);
                version_fn()
            });

            let fingerprint_fn = lib
                .symbol("flui_worker_fingerprint")
                .map(|ptr| std::mem::transmute::<_, WorkerFingerprintFn>(ptr));

            let mtime = dynlib::file_mtime(lib_path);
            let plugin = WorkerPlugin {
                lib,
                init_fn,
                fingerprint_fn,
                version,
                mtime,
            };
            plugin.init();
            tracing::info!(
                version = plugin.version,
                path = %lib_path.display(),
                "Worker plugin loaded"
            );
            Some(plugin)
        }
    }

    /// Re-run the worker's registration hook (`flui_worker_init`).
    pub fn init(&self) {
        (self.init_fn)();
    }

    /// Type-layout fingerprint exported by the worker (0 when absent).
    pub fn fingerprint(&self) -> u64 {
        self.fingerprint_fn.map_or(DEFAULT_FINGERPRINT, |f| f())
    }

    /// Whether the on-disk library changed since load.
    pub fn has_update(&self) -> bool {
        dynlib::file_mtime(self.lib.path()) != self.mtime
    }

    /// Unload the worker library.
    pub fn unload(self) {
        tracing::info!(version = self.version, "Worker plugin unloaded");
    }

    /// Path this worker was loaded from.
    pub fn path(&self) -> &Path {
        self.lib.path()
    }

    /// Version reported by `flui_worker_version`.
    pub fn version(&self) -> u32 {
        self.version
    }
}

/// Result of polling a [`WorkerReloadDriver`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerPollOutcome {
    /// No mtime change since the last poll.
    NoChange,

    /// Worker dylib was reloaded and `flui_worker_init` ran again.
    Reloaded {
        /// How many reloads since the driver was created (includes the first load).
        reload_count: u32,
    },

    /// An update was detected but reload failed (file locked / missing symbols).
    ReloadFailed,
}

/// Polls a worker dylib path and reloads on mtime changes.
#[allow(missing_debug_implementations)]
pub struct WorkerReloadDriver {
    plugin: Option<WorkerPlugin>,
    lib_path: PathBuf,
    poll_interval: std::time::Duration,
    last_poll: std::time::Instant,
    reload_count: u32,
    last_fingerprint: u64,
}

impl WorkerReloadDriver {
    /// Create a driver for `lib_path` and attempt an immediate load + init.
    pub fn new(lib_path: impl AsRef<Path>) -> Self {
        let lib_path = lib_path.as_ref().to_path_buf();
        let plugin = WorkerPlugin::load(&lib_path);
        let last_fingerprint = plugin.as_ref().map(WorkerPlugin::fingerprint).unwrap_or(0);
        let reload_count = u32::from(plugin.is_some());

        if plugin.is_none() {
            tracing::info!(
                path = %lib_path.display(),
                "WorkerReloadDriver: worker not loaded (will retry on poll)"
            );
        }

        Self {
            plugin,
            lib_path,
            poll_interval: crate::strategy::timing::ARTIFACT_POLL,
            last_poll: std::time::Instant::now(),
            reload_count,
            last_fingerprint,
        }
    }

    /// Polling interval for mtime checks (default: 500ms).
    pub fn with_poll_interval(mut self, interval: std::time::Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// Whether a worker is currently loaded.
    pub fn is_loaded(&self) -> bool {
        self.plugin.is_some()
    }

    /// Reload count (1 after the initial successful load).
    pub fn reload_count(&self) -> u32 {
        self.reload_count
    }

    /// Last observed type fingerprint from the worker.
    pub fn fingerprint(&self) -> u64 {
        self.last_fingerprint
    }

    /// Library path being watched.
    pub fn lib_path(&self) -> &Path {
        &self.lib_path
    }

    /// Poll for worker updates. Call from the platform frame loop.
    pub fn poll(&mut self) -> WorkerPollOutcome {
        if self.last_poll.elapsed() < self.poll_interval {
            return WorkerPollOutcome::NoChange;
        }
        self.last_poll = std::time::Instant::now();

        if let Some(ref plugin) = self.plugin {
            if !plugin.has_update() {
                return WorkerPollOutcome::NoChange;
            }

            tracing::info!("WorkerReloadDriver: worker dylib updated — reloading");
            let old = self.plugin.take().expect("plugin was Some");
            old.unload();

            self.plugin = WorkerPlugin::load(&self.lib_path);
            if let Some(ref plugin) = self.plugin {
                self.reload_count += 1;
                self.last_fingerprint = plugin.fingerprint();
                tracing::info!(
                    reload = self.reload_count,
                    fingerprint = self.last_fingerprint,
                    "WorkerReloadDriver: worker reloaded"
                );
                return WorkerPollOutcome::Reloaded {
                    reload_count: self.reload_count,
                };
            }

            tracing::warn!("WorkerReloadDriver: reload failed");
            return WorkerPollOutcome::ReloadFailed;
        }

        // Lazy load — worker may appear after `cargo build -p logic`.
        self.plugin = WorkerPlugin::load(&self.lib_path);
        if self.plugin.is_some() {
            self.reload_count = 1;
            self.last_fingerprint = self.plugin.as_ref().map(WorkerPlugin::fingerprint).unwrap_or(0);
            tracing::info!(
                path = %self.lib_path.display(),
                "WorkerReloadDriver: worker now available"
            );
            return WorkerPollOutcome::Reloaded {
                reload_count: self.reload_count,
            };
        }

        WorkerPollOutcome::NoChange
    }
}
