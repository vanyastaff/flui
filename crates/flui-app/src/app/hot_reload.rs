//! The host half of the development-reload seam (ADR-0094 §1).
//!
//! `flui-app` names no reload tool. An application that wants hot reload
//! installs a [`DevReloadHook`] on its configuration
//! ([`AppConfig::with_dev_reload`]); this module is the only code that calls
//! it, and every runner goes through the crate-private wrappers below:
//!
//! - `WorkerReload` (desktop and iOS): attaches the hook once per loop,
//!   polls it at each realm's frame boundary and applies a patch once to
//!   every realm;
//! - `ScenePlugin` (Android): lets the hook's scene plugin own a frame.
//!
//! With no hook installed every wrapper is inert: attaching starts nothing,
//! polling never reloads, and no scene plugin claims a frame. The web runner
//! drives no hook.
//!
//! # Failure containment
//!
//! The hook is user code. Each call runs with the hook lent out of its slot
//! and no lock held, so a hook that re-enters the host (a `wake` whose
//! redraw pumps a frame synchronously) finds the slot empty and the nested
//! call is a no-op instead of a deadlock. A call that panics is logged, and
//! the hook is dropped — inside its own containment, because its `Drop` may
//! panic too — and never called again; the frame that caught the panic
//! continues, and the realm is untouched.

#[cfg(not(target_arch = "wasm32"))]
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::{collections::HashMap, fmt, sync::Arc};

use flui_view::dev_reload::DevReloadHook;
#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
use flui_view::dev_reload::ReloadEvent;
#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
use flui_view::dev_reload::ReloadWake;
use parking_lot::Mutex;

#[cfg(not(target_arch = "wasm32"))]
use crate::app::AppConfig;
#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
use crate::app::ui_realm::UiRealm;
use flui_foundation::RealmId;
use flui_runtime::reload::ReloadTier;

/// A development reload driver installed on an [`AppConfig`].
///
/// Built by [`AppConfig::with_dev_reload`]. `Clone` shares the one hook and
/// its bookkeeping, so every window opened with clones of the same
/// configuration is driven by the same hook: attached once per loop, polled
/// at each realm's frame boundary, and each patch applied once to every
/// realm.
#[derive(Clone)]
pub struct DevReload(Arc<Mutex<Slot>>);

// The web runner drives no hook, and the Android runner only asks it for
// scene frames, so neither reads the realm bookkeeping.
#[cfg_attr(
    any(target_arch = "wasm32", target_os = "android"),
    expect(
        dead_code,
        reason = "only the desktop and iOS runners poll the hook for realms"
    )
)]
struct Slot {
    /// `None` while a call has the hook lent out, and for good once it
    /// panicked.
    hook: Option<Box<dyn DevReloadHook>>,
    /// Between a successful `attach` and its `detach`.
    attached: bool,
    /// A `detach` arrived while the hook was lent; the lender runs it when
    /// the call returns.
    detach_owed: bool,
    /// Advances once per `Patched` poll.
    epoch: u64,
    /// The epoch of the latest patch, and the tier it asks for.
    patched_at: u64,
    patched_tier: Option<ReloadTier>,
    /// The epoch each realm last caught up to. Cleared when the loop ends.
    seen: HashMap<RealmId, u64>,
}

impl DevReload {
    pub(crate) fn new(hook: impl DevReloadHook) -> Self {
        Self(Arc::new(Mutex::new(Slot {
            hook: Some(Box::new(hook)),
            attached: false,
            detach_owed: false,
            epoch: 0,
            patched_at: 0,
            patched_tier: None,
            seen: HashMap::new(),
        })))
    }
}

impl fmt::Debug for DevReload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let slot = self.0.lock();
        f.debug_struct("DevReload")
            .field("installed", &slot.hook.is_some())
            .field("attached", &slot.attached)
            .finish_non_exhaustive()
    }
}

/// What lending the hook for one call produced.
#[cfg(not(target_arch = "wasm32"))]
enum Lent<R> {
    /// No hook: none was installed, a call already has it, or it panicked
    /// earlier.
    Absent,
    /// The call returned.
    Returned(R),
    /// The call panicked; the hook is gone.
    Panicked,
}

#[cfg(not(target_arch = "wasm32"))]
impl DevReload {
    /// Run `call` on the hook with the hook out of the slot and no lock
    /// held. See the module docs for why.
    fn lend<R>(
        &self,
        what: &'static str,
        call: impl FnOnce(&mut dyn DevReloadHook) -> R,
    ) -> Lent<R> {
        let Some(mut hook) = self.0.lock().hook.take() else {
            return Lent::Absent;
        };
        match catch_unwind(AssertUnwindSafe(|| call(hook.as_mut()))) {
            Ok(value) => {
                let owed = std::mem::take(&mut self.0.lock().detach_owed);
                if owed && catch_unwind(AssertUnwindSafe(|| hook.detach())).is_err() {
                    disable("detach", hook);
                    return Lent::Returned(value);
                }
                self.0.lock().hook = Some(hook);
                Lent::Returned(value)
            }
            Err(payload) => {
                // The payload's own `Drop` may panic; never run it.
                std::mem::forget(payload);
                disable(what, hook);
                Lent::Panicked
            }
        }
    }

    /// The loop ended: detach the hook and forget every realm.
    #[cfg(not(target_os = "android"))]
    fn detach(&self) {
        {
            let mut slot = self.0.lock();
            if !slot.attached {
                return;
            }
            slot.attached = false;
            slot.seen.clear();
        }
        if let Lent::Absent = self.lend("detach", DevReloadHook::detach) {
            // A call has the hook lent (or it is gone for good, where this
            // flag is never read): that call's lender runs the detach.
            self.0.lock().detach_owed = true;
        }
    }
}

/// Log a hook's panic and drop the hook without letting its `Drop` unwind
/// into the frame.
#[cfg(not(target_arch = "wasm32"))]
fn disable(what: &'static str, hook: Box<dyn DevReloadHook>) {
    contain(|| {
        tracing::error!(
            call = what,
            "development reload hook panicked; hot reload is disabled until the app restarts"
        );
    });
    contain(|| drop(hook));
}

/// Run `body`, swallowing a panic without running its payload's `Drop`,
/// which may panic too. (`application_control::contain` does the same, but
/// is not compiled for Android.)
#[cfg(not(target_arch = "wasm32"))]
fn contain(body: impl FnOnce()) {
    if let Err(payload) = catch_unwind(AssertUnwindSafe(body)) {
        std::mem::forget(payload);
    }
}

/// The realm reload an event asks for. Exhaustive, so a new event does not
/// compile until it is given a meaning here.
#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
const fn reload_tier(event: ReloadEvent) -> Option<ReloadTier> {
    match event {
        ReloadEvent::Unchanged => None,
        ReloadEvent::Patched => Some(ReloadTier::Reassemble),
    }
}

/// The desktop and iOS driver: the configuration's hook, if any.
#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
#[derive(Clone)]
pub(crate) struct WorkerReload(Option<DevReload>);

#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
impl WorkerReload {
    pub(crate) fn from_config(config: &AppConfig) -> Self {
        if config.dev_reload.is_none() && std::env::var_os("FLUI_WORKER_PLUGIN").is_some() {
            tracing::warn!(
                "FLUI_WORKER_PLUGIN is set, but no DevReloadHook is installed on the AppConfig, so \
                 the worker will not be loaded; install one with `AppConfig::with_dev_reload`"
            );
        }
        Self(config.dev_reload.clone())
    }

    /// Attach the hook for this loop, handing it `wake` to request a frame
    /// with. The returned guard detaches it when dropped, so it must outlive
    /// the event loop.
    ///
    /// `None` when no hook is installed, when it is already attached to a
    /// live loop (refused with a warning: the hook's contract is one attach
    /// per detach), or when `attach` panicked.
    pub(crate) fn spawn_watcher(
        &self,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Option<WorkerWatcherGuard> {
        let reload = self.0.as_ref()?;
        {
            let mut slot = reload.0.lock();
            slot.hook.as_ref()?;
            if slot.attached {
                drop(slot);
                tracing::warn!(
                    "development reload hook is already attached to a running loop; this loop \
                     runs without hot reload"
                );
                return None;
            }
            slot.attached = true;
        }
        let wake = ReloadWake::new(move || wake());
        match reload.lend("attach", |hook| hook.attach(wake)) {
            Lent::Returned(()) => Some(WorkerWatcherGuard {
                reload: reload.clone(),
            }),
            Lent::Absent | Lent::Panicked => {
                reload.0.lock().attached = false;
                None
            }
        }
    }

    /// Record a realm the runner has just mounted as current with the latest
    /// patch: its tree was built from the code loaded now, so it must apply
    /// every later patch, including one another realm's boundary polls
    /// before this realm's first frame. The runner calls this right after
    /// the root attaches.
    pub(crate) fn register_realm(&self, realm: &UiRealm) {
        let Some(reload) = &self.0 else {
            return;
        };
        let mut slot = reload.0.lock();
        let epoch = slot.epoch;
        slot.seen.insert(realm.realm_id(), epoch);
    }

    /// Poll the hook at `realm`'s frame boundary and reassemble the realm
    /// if a patch has arrived that it has not applied yet.
    ///
    /// A patch reaches every realm exactly once: the poll that sees it
    /// advances an epoch, and each realm applies the latest patch when its
    /// own boundary finds it behind. A realm the runner did not
    /// [register](Self::register_realm) starts at the epoch of its first
    /// poll, so it never replays a patch older than itself.
    pub(crate) fn poll_and_apply(&self, realm: &UiRealm) {
        let Some(reload) = &self.0 else {
            return;
        };
        let id = realm.realm_id();
        {
            let mut slot = reload.0.lock();
            let epoch = slot.epoch;
            slot.seen.entry(id).or_insert(epoch);
        }
        let event = reload.lend("poll", DevReloadHook::poll);
        let apply = {
            let mut slot = reload.0.lock();
            if let Lent::Returned(event) = event
                && let Some(tier) = reload_tier(event)
            {
                slot.epoch += 1;
                slot.patched_at = slot.epoch;
                slot.patched_tier = Some(tier);
            }
            let epoch = slot.epoch;
            let patched_at = slot.patched_at;
            let behind = slot
                .seen
                .insert(id, epoch)
                .is_some_and(|seen| seen < patched_at);
            behind.then_some(slot.patched_tier).flatten()
        };
        if let Some(tier) = apply {
            tracing::info!(?tier, "hot reload: reassembling");
            realm.perform_hot_reload_entered(tier);
        }
    }
}

/// Detaches the loop's hook when dropped. Must outlive the event loop.
#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
pub(crate) struct WorkerWatcherGuard {
    reload: DevReload,
}

#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
impl Drop for WorkerWatcherGuard {
    fn drop(&mut self) {
        self.reload.detach();
    }
}

/// The Android scene-plugin driver: the configuration's hook, if any, may
/// render a frame itself, bypassing the widget tree.
#[cfg(target_os = "android")]
#[derive(Clone)]
pub(crate) struct ScenePlugin(Option<DevReload>);

#[cfg(target_os = "android")]
impl ScenePlugin {
    pub(crate) fn from_config(config: &AppConfig) -> Self {
        Self(config.dev_reload.clone())
    }

    /// Let the hook's scene plugin own this frame. Returns `true` when it
    /// rendered, in which case the caller must not run the widget pipeline
    /// for this frame.
    pub(crate) fn try_render_frame(
        &self,
        renderer: &mut flui_engine::Renderer,
        width: f64,
        height: f64,
    ) -> bool {
        let Some(reload) = &self.0 else {
            return false;
        };
        let rendered = reload.lend("scene_frame", |hook| {
            hook.scene_frame(width, height, &mut |scene| {
                if let Err(error) = renderer.render_scene(scene) {
                    tracing::error!(?error, "Plugin render failed");
                }
            })
        });
        matches!(rendered, Lent::Returned(true))
    }
}

#[cfg(all(test, not(target_os = "android"), not(target_arch = "wasm32")))]
mod tests;
