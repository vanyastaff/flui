//! Host-side worker dylib loader (state-preserving hot reload).
//!
//! Unlike [`ScenePlugin`](crate::ScenePlugin), a worker plugin does **not** own
//! the widget pipeline. It exports an `init` entry point that registers reloadable
//! `build()` implementations while the host binary retains element tree state.
//!
//! See `docs/hot-reload.md`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::mem::ManuallyDrop;
use std::path::{Path, PathBuf};

use flui_sdk::view::dev_reload::CodeVersion;

use crate::LoadError;
use crate::dynlib::{self, DynLib};
use crate::manifest::WorkerManifest;

/// Default fingerprint when the worker omits `flui_worker_fingerprint`.
pub const DEFAULT_FINGERPRINT: u64 = 0;

/// Host-side callback passed into `flui_worker_init` so worker code can register
/// reloadable build functions without touching dylib-local statics.
pub type RegisterWorkerBuildFn = extern "C" fn(fingerprint: u64, build: *const ());

/// Host-side entry passed into `flui_worker_init` so the worker's
/// `request_rebuild` reaches the hook the host registered: the worker image's
/// own copy of this crate has a rebuild slot of its own, which no host hook
/// is ever registered in. `C-unwind`, so a panicking hook unwinds into the
/// worker's caller instead of aborting.
pub type HostRequestRebuildFn = extern "C-unwind" fn();

/// The entry this host hands a worker: the rebuild hook registry with the
/// `app-plugin` feature, otherwise a warning that this host has none.
fn host_request_rebuild_fn() -> HostRequestRebuildFn {
    #[cfg(feature = "app-plugin")]
    {
        crate::dispatch::host_request_rebuild
    }
    #[cfg(not(feature = "app-plugin"))]
    {
        extern "C-unwind" fn no_rebuild_registry() {
            tracing::warn!(
                "a worker asked for a rebuild, but this host is built without \
                 flui-hot-reload's `app-plugin` feature, which holds the rebuild hook"
            );
        }
        no_rebuild_registry
    }
}

/// One build a worker's init registered: its layout fingerprint and the
/// build function's address. A [`WorkerPlugin`] keeps the address as an
/// integer, only to recognise its own entry when it prunes, so the plugin stays
/// `Send` while the registry, which holds the pointer, stays on its thread.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Registration {
    fingerprint: u64,
    address: usize,
}

/// The worker builds the host can dispatch to, and the registrations of the
/// init call in flight.
struct Registry {
    /// The newest build registered for each fingerprint.
    builds: BTreeMap<u64, *const ()>,
    /// While a [`WorkerPlugin`] init call is in flight, what it registers, so
    /// the plugin can prune exactly those entries when it is dropped. `None`
    /// outside an init call: a registration arriving then still lands in
    /// `builds` but belongs to no plugin (and is logged), so it is never
    /// pruned; workers register from `flui_worker_init` only.
    session: Option<Vec<Registration>>,
}

thread_local! {
    /// The owner thread's worker builds (ADR-0323 §1). A worker is loaded, and
    /// its builds are looked up, on the thread that drives the host's frames:
    /// another thread sees an empty registry. No borrow of it is held while
    /// code the registry does not own runs (the worker's init, a `tracing`
    /// subscriber), so no call finds it borrowed.
    static WORKER_BUILDS: RefCell<Registry> = const {
        RefCell::new(Registry {
            builds: BTreeMap::new(),
            session: None,
        })
    };
}

/// Runs a worker's `flui_worker_init` and returns what it registered, or
/// `None` when the init panicked, with those registrations pruned again.
fn run_init(init_fn: WorkerInitFn) -> Option<Vec<Registration>> {
    WORKER_BUILDS.with_borrow_mut(|registry| registry.session = Some(Vec::new()));
    let initialized = init_fn(host_register_fn(), host_request_rebuild_fn());
    let registered = WORKER_BUILDS
        .with_borrow_mut(|registry| registry.session.take())
        .unwrap_or_default();
    if initialized {
        Some(registered)
    } else {
        prune(registered);
        None
    }
}

/// Removes the registrations that are still the ones in `registered`. A newer
/// worker registering the same fingerprint keeps its entry. On a thread whose
/// registry is already destroyed there is nothing left to prune.
fn prune(registered: Vec<Registration>) {
    if registered.is_empty() {
        return;
    }
    let pruned = WORKER_BUILDS.try_with(|registry| {
        let builds = &mut registry.borrow_mut().builds;
        registered
            .into_iter()
            .filter(|registration| {
                let current = builds
                    .get(&registration.fingerprint)
                    .is_some_and(|build| build.addr() == registration.address);
                if current {
                    builds.remove(&registration.fingerprint);
                }
                current
            })
            .count()
    });
    if let Ok(pruned) = pruned {
        tracing::debug!(pruned, "worker builds pruned");
    }
}

/// Host entry point wired into [`WorkerPlugin::load`].
extern "C" fn host_register_worker_build(fingerprint: u64, build: *const ()) {
    if build.is_null() {
        tracing::warn!(
            fingerprint,
            "worker attempted to register a null build pointer"
        );
        return;
    }
    let owned = WORKER_BUILDS.with_borrow_mut(|registry| {
        registry.builds.insert(fingerprint, build);
        registry.session.as_mut().map(|session| {
            session.push(Registration {
                fingerprint,
                address: build.addr(),
            });
        })
    });
    if owned.is_none() {
        tracing::warn!(
            fingerprint,
            "worker build registered outside flui_worker_init — the entry \
             cannot be pruned with its owner; register only from flui_worker_init"
        );
    }
    tracing::debug!(fingerprint, "worker build registered in host");
}

/// Returns the host-side registration callback for `flui_worker_init`.
#[must_use]
pub fn host_register_fn() -> RegisterWorkerBuildFn {
    host_register_worker_build
}

/// Look up a worker-registered build function by layout fingerprint, in this
/// thread's registry: the thread that loaded the worker.
///
/// Returns `None` when no current worker registered the fingerprint on this
/// thread, including after its registration owner is dropped or a reload
/// fails. Previously returned addresses remain mapped because admitted worker
/// images are retained until process exit (ADR-0111); do not cache a pointer
/// when dispatch should follow the newest registration. Calling an address
/// still requires its exact function signature and matching host/worker
/// Rust-ABI contract. Registrations must be made from `flui_worker_init` so
/// ownership and pruning remain defined.
#[must_use]
pub fn get_worker_build_ptr(fingerprint: u64) -> Option<*const ()> {
    WORKER_BUILDS.with_borrow(|registry| registry.builds.get(&fingerprint).copied())
}

type WorkerInitFn = extern "C" fn(RegisterWorkerBuildFn, HostRequestRebuildFn) -> bool;
type WorkerFingerprintFn = extern "C" fn() -> u64;
type WorkerAbiTokenFn = extern "C" fn() -> u64;

/// A loaded hot-reload worker dylib (`my_app_logic.dll`).
#[expect(
    missing_debug_implementations,
    reason = "`WorkerPlugin` holds raw library handles and has no `Debug`"
)]
pub struct WorkerPlugin {
    // Admission is permanent: arbitrary Rust-ABI descendants can escape the
    // tree and carry image-owned code, vtables or storage (ADR-0111).
    lib: ManuallyDrop<DynLib>,
    fingerprint_fn: Option<WorkerFingerprintFn>,
    mtime: Option<std::time::SystemTime>,
    /// The builds this plugin's init registered in this thread's
    /// `WORKER_BUILDS` — pruned by `Drop`; the admitted image stays mapped.
    registered: Vec<Registration>,
}

impl WorkerPlugin {
    /// Load a worker from `lib_path` and call its `flui_worker_init` hook.
    ///
    /// # Errors
    ///
    /// The [`LoadError`] that says why: the file is missing or the platform
    /// loader refused it, it lacks a worker symbol, it fails the ABI-token
    /// handshake (built by a different compiler or source revision than this
    /// host — see [`crate::abi_token`]), or its init panicked.
    pub fn load(lib_path: impl AsRef<Path>) -> Result<Self, LoadError> {
        let lib_path = lib_path.as_ref();
        let lib = DynLib::open(lib_path)?;
        let missing = |symbol: &str| LoadError::MissingSymbol {
            path: lib_path.to_path_buf(),
            symbol: symbol.to_owned(),
        };

        // SAFETY: `lib` was just opened and outlives this block. `flui_worker_init`
        // is the worker ABI symbol this crate defines, so a library built by the
        // matching macro exports it with `WorkerInitFn`'s signature; `symbol`
        // answers `None` for a null address.
        #[expect(unsafe_code, reason = "calls the worker init symbol")]
        unsafe {
            let init_ptr = lib
                .symbol("flui_worker_init")
                .ok_or_else(|| missing("flui_worker_init"))?;
            let init_fn: WorkerInitFn = std::mem::transmute(init_ptr);

            // ABI handshake before calling anything else in the image: build
            // pointers registered by init are transmuted to typed fn pointers
            // whose argument types are `repr(Rust)`, so layout agreement must
            // be established first.
            let token_ptr = lib
                .symbol("flui_worker_abi_token")
                .ok_or_else(|| missing("flui_worker_abi_token"))?;
            let token_fn: WorkerAbiTokenFn = std::mem::transmute(token_ptr);
            let worker_token = token_fn();
            let host_token = crate::abi_token();
            if worker_token != host_token {
                return Err(LoadError::AbiMismatch {
                    path: lib_path.to_path_buf(),
                    host: host_token,
                    plugin: worker_token,
                });
            }

            let fingerprint_fn = lib
                .symbol("flui_worker_fingerprint")
                .map(|ptr| std::mem::transmute::<_, WorkerFingerprintFn>(ptr));

            // Before the image is admitted: an image whose init panicked is
            // refused and closes when `lib` drops here.
            let registered = run_init(init_fn).ok_or_else(|| LoadError::InitFailed {
                path: lib_path.to_path_buf(),
            })?;
            let mtime = dynlib::file_revision(lib_path);
            let plugin = WorkerPlugin {
                lib: ManuallyDrop::new(lib),
                fingerprint_fn,
                mtime,
                registered,
            };
            tracing::info!(
                path = %lib_path.display(),
                "Worker plugin loaded"
            );
            Ok(plugin)
        }
    }

    /// Type-layout fingerprint exported by the worker (0 when absent).
    pub fn fingerprint(&self) -> u64 {
        self.fingerprint_fn.map_or(DEFAULT_FINGERPRINT, |f| f())
    }

    /// Whether the on-disk library changed since load.
    pub fn has_update(&self) -> bool {
        dynlib::file_revision(self.lib.path()) != self.mtime
    }

    /// Retire this worker's registrations.
    ///
    /// Its admitted image remains mapped until process exit so host-retained
    /// Rust values and callbacks can still be used and destroyed safely.
    /// Restart the development host to reclaim older images and staged files.
    pub fn unload(self) {
        tracing::info!(path = %self.lib.path().display(), "Worker registrations retired; image retained");
    }

    /// Path this worker was loaded from.
    pub fn path(&self) -> &Path {
        self.lib.path()
    }
}

impl Drop for WorkerPlugin {
    fn drop(&mut self) {
        // The loader reference is permanently retained independently of this
        // registry. A plugin dropped on another thread than the one that
        // loaded it finds nothing of its own to prune there.
        prune(std::mem::take(&mut self.registered));
    }
}

/// Result of polling a [`WorkerReloadDriver`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WorkerPollOutcome {
    /// No mtime change since the last poll.
    NoChange,

    /// Worker dylib was reloaded and `flui_worker_init` ran again.
    Reloaded {
        /// How many worker images this driver has loaded, the first included.
        reload_count: u32,
    },

    /// The dylib reloaded, but its layout fingerprint changed: the shared
    /// `types` crate's layout no longer matches what the host was compiled
    /// against, so the host must NOT call the newly registered build fns with
    /// its old-layout state. Lookups keyed by the host's compiled-in
    /// fingerprint miss the new registrations, degrading to "worker
    /// unavailable". Restart the host to pick
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

/// The image a worker loads, and the version `flui run` minted for it.
struct Resolved {
    path: PathBuf,
    version: Option<CodeVersion>,
}

/// Why [`resolve_worker`] names no image: the caller keeps the one it has.
enum Unresolved {
    /// The manifest is not one this release reads (an older plain path, a
    /// newer field): a `flui run` and a `flui-hot-reload` of different
    /// releases.
    Unreadable(std::io::Error),
    /// The manifest names an image that is gone: `flui run` pruned it, and
    /// its next manifest names the build that replaced it.
    Missing(PathBuf),
}

impl Unresolved {
    fn warn(&self) {
        match self {
            Self::Unreadable(error) => tracing::warn!(
                %error,
                "hot reload: the worker's `.flui_worker_plugin` manifest is not one this \
                 flui-hot-reload reads; use a `flui run` of the same release. The loaded \
                 worker stays."
            ),
            Self::Missing(path) => tracing::debug!(
                path = %path.display(),
                "hot reload: the worker manifest names an image that is gone; keeping the \
                 loaded worker until the next manifest"
            ),
        }
    }
}

/// The image to load for the canonical library `lib_path`: the one its
/// sidecar [`WorkerManifest`] names, else, with no manifest, `lib_path`
/// itself with no version.
fn resolve_worker(lib_path: &Path) -> Result<Resolved, Unresolved> {
    match WorkerManifest::read(lib_path) {
        Ok(Some(manifest)) if manifest.path().is_file() => Ok(Resolved {
            version: manifest.version(),
            path: manifest.path().to_path_buf(),
        }),
        Ok(Some(manifest)) => Err(Unresolved::Missing(manifest.path().to_path_buf())),
        Ok(None) => Ok(Resolved {
            path: lib_path.to_path_buf(),
            version: None,
        }),
        Err(error) => Err(Unresolved::Unreadable(error)),
    }
}

/// The path [`resolve_worker`] names, or `lib_path` when it names none.
fn resolve_worker_path(lib_path: &Path) -> PathBuf {
    resolve_worker(lib_path).map_or_else(|_| lib_path.to_path_buf(), |resolved| resolved.path)
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
pub fn worker_artifact_stamp(lib_path: &Path) -> (PathBuf, Option<std::time::SystemTime>) {
    let resolved = resolve_worker_path(lib_path);
    let revision = dynlib::file_revision(&resolved);
    (resolved, revision)
}

/// Polls a worker dylib path and reloads on mtime changes.
#[expect(
    missing_debug_implementations,
    reason = "`WorkerReloadDriver` owns a `WorkerPlugin`, which holds raw library handles and has no `Debug`"
)]
pub struct WorkerReloadDriver {
    plugin: Option<WorkerPlugin>,
    /// Canonical path (used for manifest lookup).
    lib_path: PathBuf,
    /// The last manifest could not be resolved, and a warning said so.
    unresolved_noted: bool,
    /// The version the manifest named for the loaded image, if any.
    loaded_version: Option<CodeVersion>,
    /// Path the currently loaded plugin came from.
    loaded_path: PathBuf,
    poll_interval: std::time::Duration,
    /// When the artifact was last read.
    pub(crate) last_poll: std::time::Instant,
    /// How many worker images this driver has loaded, the first included.
    reload_count: u32,
    last_fingerprint: u64,
    /// The artifact that last failed to load, not reopened until rebuilt.
    refusal: crate::dynlib::LoadRefusal,
}

impl WorkerReloadDriver {
    /// Create a driver for `lib_path` and attempt an immediate load + init.
    pub fn new(lib_path: impl AsRef<Path>) -> Self {
        let lib_path = lib_path.as_ref().to_path_buf();
        let Resolved {
            path: load_path,
            version,
        } = resolve_worker(&lib_path).unwrap_or_else(|unresolved| {
            unresolved.warn();
            Resolved {
                path: lib_path.clone(),
                version: None,
            }
        });
        let mut refusal = crate::dynlib::LoadRefusal::default();
        let plugin = refusal.load(&load_path, |path| WorkerPlugin::load(path));
        let last_fingerprint = plugin.as_ref().map_or(0, WorkerPlugin::fingerprint);
        let reload_count = u32::from(plugin.is_some());

        if plugin.is_none() {
            tracing::info!(
                path = %load_path.display(),
                "WorkerReloadDriver: worker not loaded (will retry on poll)"
            );
        }

        Self {
            unresolved_noted: false,
            loaded_version: version.filter(|_| plugin.is_some()),
            loaded_path: load_path,
            plugin,
            lib_path,
            poll_interval: crate::timing::ARTIFACT_POLL,
            last_poll: std::time::Instant::now(),
            reload_count,
            last_fingerprint,
            refusal,
        }
    }

    /// Polling interval for mtime checks (default: the artifact poll interval
    /// every driver starts with).
    pub fn with_poll_interval(mut self, interval: std::time::Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// The version `flui run` minted for the image loaded now (ADR-0393 §2),
    /// read from its sidecar manifest; `None` for an image no manifest named.
    #[must_use]
    pub fn loaded_version(&self) -> Option<CodeVersion> {
        self.loaded_version
    }

    /// Poll for worker updates. Call from the platform frame loop: the
    /// artifact is read at most once per poll interval.
    pub fn poll(&mut self) -> WorkerPollOutcome {
        if self.last_poll.elapsed() < self.poll_interval {
            return WorkerPollOutcome::NoChange;
        }
        self.poll_now()
    }

    /// Read the artifact now, whenever it was last read: the poll a watcher
    /// that saw the artifact change asks for. Held to the interval, the poll
    /// its wake produced could fall inside it, and an idle host would draw no
    /// later frame to pick the change up.
    pub(crate) fn poll_now(&mut self) -> WorkerPollOutcome {
        self.last_poll = std::time::Instant::now();

        let Ok(Resolved {
            path: load_path,
            version,
        }) = resolve_worker(&self.lib_path).map_err(|unresolved| {
            // Once until the manifest reads again: the host polls every frame.
            if !std::mem::replace(&mut self.unresolved_noted, true) {
                unresolved.warn();
            }
        })
        else {
            return WorkerPollOutcome::NoChange;
        };
        self.unresolved_noted = false;

        if self.plugin.is_some() {
            let path_changed = load_path != self.loaded_path;
            let Some(old) = self
                .plugin
                .take_if(|plugin| path_changed || plugin.has_update())
            else {
                return WorkerPollOutcome::NoChange;
            };

            tracing::info!(
                path = %load_path.display(),
                "WorkerReloadDriver: worker dylib updated — reloading"
            );
            old.unload();

            self.plugin = self
                .refusal
                .load(&load_path, |path| WorkerPlugin::load(path));
            if let Some(ref plugin) = self.plugin {
                self.loaded_path = load_path;
                self.loaded_version = version;
                self.reload_count = self.reload_count.saturating_add(1);
                let old_fingerprint = self.last_fingerprint;
                self.last_fingerprint = plugin.fingerprint();
                // A changed fingerprint means the shared `types` layout moved
                // under the host: surface the degradation instead of
                // reporting a healthy reload. (The old worker's registry
                // entries were pruned on retirement, and the host's
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

            // The old image is unloaded and nothing replaced it.
            self.loaded_version = None;
            tracing::warn!("WorkerReloadDriver: reload failed");
            return WorkerPollOutcome::ReloadFailed;
        }

        // Lazy load: the worker may appear after `cargo build -p logic`, or a
        // refused one be rebuilt.
        self.plugin = self
            .refusal
            .load(&load_path, |path| WorkerPlugin::load(path));
        let Some(plugin) = &self.plugin else {
            return WorkerPollOutcome::NoChange;
        };
        self.loaded_path = load_path;
        self.loaded_version = version;
        self.reload_count = self.reload_count.saturating_add(1);
        self.last_fingerprint = plugin.fingerprint();
        tracing::info!(
            path = %self.lib_path.display(),
            "WorkerReloadDriver: worker now available"
        );
        WorkerPollOutcome::Reloaded {
            reload_count: self.reload_count,
        }
    }
}
