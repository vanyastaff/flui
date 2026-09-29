//! Host-side worker dylib loader (Flutter-parity hot reload).
//!
//! Unlike [`ScenePlugin`](crate::ScenePlugin), a worker plugin does **not** own
//! the widget pipeline. It exports an `init` entry point that registers reloadable
//! `build()` implementations while the host binary retains element tree state.
//!
//! See `docs/designs/2026-06-28-flutter-parity-hot-reload.md`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::dynlib::{self, DynLib};

/// Default fingerprint when the worker omits `flui_worker_fingerprint`.
pub const DEFAULT_FINGERPRINT: u64 = 0;

/// Host-side callback passed into `flui_worker_init` so worker code can register
/// reloadable build functions without touching dylib-local statics.
pub type RegisterWorkerBuildFn = extern "C" fn(fingerprint: u64, build: *const ());

static WORKER_BUILDS: OnceLock<Mutex<HashMap<u64, BuildPtr>>> = OnceLock::new();

fn worker_builds() -> &'static Mutex<HashMap<u64, BuildPtr>> {
    WORKER_BUILDS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// While a [`WorkerPlugin`] init call is in flight, collects the
/// `(fingerprint, ptr)` pairs it registers, so the plugin can prune exactly
/// those entries when it unloads. `None` outside an init call — a registration
/// arriving then still lands in the map but belongs to no plugin (and is
/// logged), so it can never be pruned; workers must register from
/// `flui_worker_init` only.
static REGISTRATION_SESSION: Mutex<Option<Vec<(u64, BuildPtr)>>> = Mutex::new(None);

/// Type-erased function pointer — safe to store in a `Sync` static.
#[derive(Clone, Copy, PartialEq, Eq)]
struct BuildPtr(*const ());

// SAFETY: a code address is a `Copy` integer as far as these auto traits are
// concerned — `BuildPtr` is never dereferenced as data, and neither moving nor
// sharing the value aliases anything. Both impls rest on that alone.
//
// Deliberately NOT claimed (an earlier version of this comment claimed both,
// wrongly): that the address is in the host image, and that the function is
// only invoked on the main thread. It points into the WORKER dylib, and
// nothing enforces a calling thread — `get_worker_build_ptr` is public and
// takes no thread context. This is a distinct, still-open gap from
// `app_plugin!`'s `flui_app_build` (`plugin.rs`), which pins its calling
// thread at the C ABI and refuses a foreign-thread call instead of racing
// one — the worker build-pointer registry has no equivalent guard.
//
// Lifetime: the address is only as live as the worker image it points into.
// [`WorkerPlugin`]'s `Drop` prunes this plugin's entries from `WORKER_BUILDS`
// BEFORE `DynLib::drop` unmaps the image, so a map hit implies the image is
// still mapped (single-threaded host loop; a caller racing an unload from
// another thread is outside the supported dev-loop).
#[expect(unsafe_code)]
unsafe impl Send for BuildPtr {}
#[expect(unsafe_code)]
unsafe impl Sync for BuildPtr {}

/// Host entry point wired into [`WorkerPlugin::load`].
extern "C" fn host_register_worker_build(fingerprint: u64, build: *const ()) {
    if build.is_null() {
        tracing::warn!(
            fingerprint,
            "worker attempted to register a null build pointer"
        );
        return;
    }
    let ptr = BuildPtr(build);
    worker_builds()
        .lock()
        .expect("BUG: WORKER_BUILDS mutex is never held across a panic; poisoning means a bug elsewhere already corrupted host state")
        .insert(fingerprint, ptr);
    if let Some(session) = &mut *REGISTRATION_SESSION
        .lock()
        .expect("BUG: REGISTRATION_SESSION mutex is never held across a panic; poisoning means a bug elsewhere already corrupted host state")
    {
        session.push((fingerprint, ptr));
    } else {
        tracing::warn!(
            fingerprint,
            "worker build registered outside flui_worker_init — the entry \
             cannot be pruned on unload and will dangle after the image is \
             unmapped"
        );
    }
    tracing::debug!(fingerprint, "worker build registered in host");
}

/// Returns the host-side registration callback for `flui_worker_init`.
#[must_use]
pub fn host_register_fn() -> RegisterWorkerBuildFn {
    host_register_worker_build
}

/// Look up a worker-registered build function by layout fingerprint.
///
/// Returns `None` when no live worker has registered this fingerprint —
/// including after an unload or a failed reload: [`WorkerPlugin`]'s `Drop`
/// prunes its own registrations from the registry BEFORE the image is
/// unmapped, so a `Some` here points into a still-mapped image. Treat `None`
/// as "worker unavailable" and fall back (or fail loudly), never cache a
/// previously returned pointer across frames.
///
/// The one unpruned case: a registration made outside `flui_worker_init`
/// (logged as a warning at registration time) belongs to no plugin and WILL
/// dangle after that image unloads — workers must register from their init
/// hook only.
#[must_use]
pub fn get_worker_build_ptr(fingerprint: u64) -> Option<*const ()> {
    worker_builds()
        .lock()
        .expect("BUG: WORKER_BUILDS mutex is never held across a panic; poisoning means a bug elsewhere already corrupted host state")
        .get(&fingerprint)
        .map(|slot| slot.0)
}

type WorkerInitFn = extern "C" fn(RegisterWorkerBuildFn);
type WorkerVersionFn = extern "C" fn() -> u32;
type WorkerFingerprintFn = extern "C" fn() -> u64;
type WorkerAbiTokenFn = extern "C" fn() -> u64;

/// A loaded hot-reload worker dylib (`my_app_logic.dll`).
#[expect(missing_debug_implementations)]
pub struct WorkerPlugin {
    lib: DynLib,
    init_fn: WorkerInitFn,
    fingerprint_fn: Option<WorkerFingerprintFn>,
    version: u32,
    mtime: u64,
    /// The `(fingerprint, ptr)` pairs this plugin's init hook registered in
    /// `WORKER_BUILDS` — pruned by `Drop` before the image is unmapped.
    registered: Mutex<Vec<(u64, BuildPtr)>>,
}

impl WorkerPlugin {
    /// Load a worker from `lib_path` and call its `flui_worker_init` hook.
    ///
    /// Returns `None` when the file is missing, unloadable, lacks worker
    /// symbols, or fails the ABI-token handshake (built by a different
    /// compiler or source revision than this host — see [`crate::abi_token`]).
    pub fn load(lib_path: impl AsRef<Path>) -> Option<Self> {
        let lib_path = lib_path.as_ref();
        let lib = DynLib::open(lib_path)?;

        // SAFETY: `lib` was just opened and outlives this block. `flui_worker_init`
        // is the worker ABI symbol this crate defines, so a library built by the
        // matching macro exports it with `WorkerInitFn`'s signature; the pointer
        // is null-checked before the transmute.
        #[expect(unsafe_code)]
        unsafe {
            let init_ptr = lib.symbol("flui_worker_init")?;
            if init_ptr.is_null() {
                return None;
            }
            let init_fn: WorkerInitFn = std::mem::transmute(init_ptr);

            // ABI handshake before calling anything else in the image: build
            // pointers registered by init are transmuted to typed fn pointers
            // whose argument types are `repr(Rust)`, so layout agreement must
            // be established first.
            let Some(token_ptr) = lib.symbol("flui_worker_abi_token") else {
                tracing::error!(
                    path = %lib_path.display(),
                    "worker refused: no ABI-token symbol — rebuild it against \
                     this tree (the ownership-safe worker ABI requires it)"
                );
                return None;
            };
            let token_fn: WorkerAbiTokenFn = std::mem::transmute(token_ptr);
            let worker_token = token_fn();
            let host_token = crate::abi_token();
            if worker_token != host_token {
                tracing::error!(
                    worker_token,
                    host_token,
                    path = %lib_path.display(),
                    "worker refused: ABI token mismatch — host and worker were \
                     built by different compilers or source revisions; rebuild \
                     both from one worktree with one toolchain"
                );
                return None;
            }

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
                registered: Mutex::new(Vec::new()),
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

    /// Re-run the worker's registration hook (`flui_worker_init`), recording
    /// what it registers so `Drop` can prune exactly those entries.
    pub fn init(&self) {
        {
            let mut session = REGISTRATION_SESSION
                .lock()
                .expect("BUG: REGISTRATION_SESSION mutex is never held across a panic; poisoning means a bug elsewhere already corrupted host state");
            *session = Some(Vec::new());
        }
        (self.init_fn)(host_register_fn());
        let registered_now = REGISTRATION_SESSION
            .lock()
            .expect("BUG: REGISTRATION_SESSION mutex is never held across a panic; poisoning means a bug elsewhere already corrupted host state")
            .take()
            .unwrap_or_default();
        self.registered
            .lock()
            .expect("BUG: WorkerPlugin::registered mutex is never held across a panic; poisoning means a bug elsewhere already corrupted host state")
            .extend(registered_now);
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
        // Drop prunes this plugin's registry entries, then DynLib unmaps.
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

impl Drop for WorkerPlugin {
    fn drop(&mut self) {
        // Prune this plugin's registrations BEFORE `self.lib` drops (fields
        // drop in declaration order; `lib` is declared first but `Drop::drop`
        // runs before ANY field drops), so `get_worker_build_ptr` can never
        // observe an address into an unmapped image. An entry is removed only
        // while it still holds the pointer this plugin registered — a newer
        // plugin that re-registered the same fingerprint keeps its (live)
        // entry.
        let registered = std::mem::take(
            &mut *self
                .registered
                .lock()
                .expect("BUG: WorkerPlugin::registered mutex is never held across a panic; poisoning means a bug elsewhere already corrupted host state"),
        );
        if registered.is_empty() {
            return;
        }
        let mut builds = worker_builds()
            .lock()
            .expect("BUG: WORKER_BUILDS mutex is never held across a panic; poisoning means a bug elsewhere already corrupted host state");
        for (fingerprint, ptr) in registered {
            if builds.get(&fingerprint) == Some(&ptr) {
                builds.remove(&fingerprint);
                tracing::debug!(fingerprint, "worker build pruned on unload");
            }
        }
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

    /// The dylib reloaded, but its layout fingerprint changed: the shared
    /// `types` crate's layout no longer matches what the host was compiled
    /// against, so the host must NOT call the newly registered build fns with
    /// its old-layout state. Lookups keyed by the host's compiled-in
    /// fingerprint miss the new registrations, degrading to "worker
    /// unavailable" (this is the producer of the degradation
    /// [`crate::HotReloadOutcome::Degraded`] names). Restart the host to pick
    /// up the new layout.
    Degraded {
        /// Fingerprint the previous worker reported.
        old_fingerprint: u64,
        /// Fingerprint the reloaded worker reports.
        new_fingerprint: u64,
    },

    /// An update was detected but reload failed (file locked / missing symbols).
    ReloadFailed,
}

/// Sidecar file written by `flui run` when staging worker builds on Windows.
fn manifest_path_for(lib_path: &Path) -> PathBuf {
    lib_path.parent().map_or_else(
        || PathBuf::from(".flui_worker_plugin"),
        |dir| dir.join(".flui_worker_plugin"),
    )
}

/// Resolve the dylib path to load — prefers the sidecar manifest when present.
fn resolve_worker_path(lib_path: &Path) -> PathBuf {
    let manifest = manifest_path_for(lib_path);
    if let Ok(text) = std::fs::read_to_string(&manifest) {
        let candidate = PathBuf::from(text.trim());
        if candidate.is_file() {
            return candidate;
        }
    }
    lib_path.to_path_buf()
}

/// Identify the artifact `lib_path` resolves to right now: the resolved path
/// plus its modification time, honouring the sidecar manifest exactly as
/// [`WorkerReloadDriver`] does.
///
/// This is the **off-thread** companion to [`WorkerReloadDriver::poll`]. A host
/// that only polls at frame boundaries cannot notice a rebuild while it is idle
/// (an unfocused or occluded window receives no frames), so a small host-side
/// watcher thread polls this instead and wakes the owner when it changes. It
/// deliberately does no loading: [`WorkerReloadDriver`] owns the
/// owner-thread-only `dlopen`/`dlclose` cycle, and this function touches only
/// the filesystem, so it is safe from any thread.
///
/// The path is part of the stamp because the CLI assigns a content-addressed
/// name (`{stem}-hot-{hash}{ext}`): two builds in the same wall-clock second can
/// share an mtime, but they never share a path. Comparing only the mtime would
/// miss that second build.
#[must_use]
pub fn worker_artifact_stamp(lib_path: &Path) -> (PathBuf, u64) {
    let resolved = resolve_worker_path(lib_path);
    let mtime = dynlib::file_mtime(&resolved);
    (resolved, mtime)
}

/// Polls a worker dylib path and reloads on mtime changes.
#[expect(missing_debug_implementations)]
pub struct WorkerReloadDriver {
    plugin: Option<WorkerPlugin>,
    /// Canonical path (used for manifest lookup).
    lib_path: PathBuf,
    /// Path the currently loaded plugin came from.
    loaded_path: PathBuf,
    poll_interval: std::time::Duration,
    last_poll: std::time::Instant,
    reload_count: u32,
    last_fingerprint: u64,
}

impl WorkerReloadDriver {
    /// Create a driver for `lib_path` and attempt an immediate load + init.
    pub fn new(lib_path: impl AsRef<Path>) -> Self {
        let lib_path = lib_path.as_ref().to_path_buf();
        let load_path = resolve_worker_path(&lib_path);
        let plugin = WorkerPlugin::load(&load_path);
        let last_fingerprint = plugin.as_ref().map_or(0, WorkerPlugin::fingerprint);
        let reload_count = u32::from(plugin.is_some());

        if plugin.is_none() {
            tracing::info!(
                path = %load_path.display(),
                "WorkerReloadDriver: worker not loaded (will retry on poll)"
            );
        }

        Self {
            loaded_path: load_path,
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

        let load_path = resolve_worker_path(&self.lib_path);

        if let Some(ref plugin) = self.plugin {
            let path_changed = load_path != self.loaded_path;
            if !path_changed && !plugin.has_update() {
                return WorkerPollOutcome::NoChange;
            }

            tracing::info!(
                path = %load_path.display(),
                "WorkerReloadDriver: worker dylib updated — reloading"
            );
            let old = self
                .plugin
                .take()
                .expect("BUG: self.plugin just matched Some above and nothing else can clear it before this take() runs");
            old.unload();

            self.plugin = WorkerPlugin::load(&load_path);
            if let Some(ref plugin) = self.plugin {
                self.loaded_path = load_path;
                self.reload_count += 1;
                let old_fingerprint = self.last_fingerprint;
                self.last_fingerprint = plugin.fingerprint();
                // A changed fingerprint means the shared `types` layout moved
                // under the host: surface the degradation instead of
                // reporting a healthy reload. (The old worker's registry
                // entries were pruned on its unload, and the host's
                // fingerprint-keyed lookups miss the new ones, so builds
                // degrade to "worker unavailable" rather than calling across
                // an incompatible layout.)
                if old_fingerprint != DEFAULT_FINGERPRINT
                    && self.last_fingerprint != old_fingerprint
                {
                    tracing::warn!(
                        old_fingerprint,
                        new_fingerprint = self.last_fingerprint,
                        "WorkerReloadDriver: worker layout fingerprint changed \
                         — degraded to worker-unavailable; restart the host to \
                         adopt the new layout"
                    );
                    return WorkerPollOutcome::Degraded {
                        old_fingerprint,
                        new_fingerprint: self.last_fingerprint,
                    };
                }
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
        self.plugin = WorkerPlugin::load(&load_path);
        if self.plugin.is_some() {
            self.loaded_path = load_path;
        }
        if self.plugin.is_some() {
            self.reload_count = 1;
            self.last_fingerprint = self.plugin.as_ref().map_or(0, WorkerPlugin::fingerprint);
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
