//! The host half of the development-reload seam (ADR-0094 §1).
//!
//! `flui-app` names no reload tool. An application that wants hot reload
//! installs a [`DevReloadHook`] on its configuration
//! ([`AppConfig::dev_reload`]); this module is the only code that calls
//! it, and every runner goes through the crate-private wrappers below:
//!
//! - `WorkerReload` (desktop and iOS): attaches the hook once per loop,
//!   polls it at each UI runtime's frame boundary and keeps the loop's record of
//!   versions (`record`, ADR-0393 §3), which reassembles each UI runtime once
//!   per version;
//! - `ScenePlugin` (every native runner: desktop, iOS and Android): lets the
//!   hook's scene plugin own a presentation's frame (ADR-0391). Every native
//!   runner asks it the same way, at the start of each presentation's frame,
//!   and runs `PluginFrame::finish` in place of the widget frame when the
//!   plugin drew. The Android runner never attaches or polls.
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
//! continues, and the UI runtime is untouched.

#[cfg(not(target_arch = "wasm32"))]
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::{fmt, sync::Arc};

#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
use flui_foundation::UiRuntimeId;
use flui_view::dev_reload::DevReloadHook;
#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
use flui_view::dev_reload::ReloadWake;
#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
use flui_view::dev_reload::{ReloadOutcome, RestartFailure};
use parking_lot::Mutex;

#[cfg(not(target_arch = "wasm32"))]
use crate::app::AppConfig;
#[cfg(not(target_arch = "wasm32"))]
use crate::app::runner::raster_host::RasterSlot;
#[cfg(not(target_arch = "wasm32"))]
use crate::app::ui_runtime::UiRuntime;
#[cfg(not(target_arch = "wasm32"))]
use flui_foundation::panic::FirstPanic;

// Only the desktop and iOS runners poll the hook, so the record is read
// nowhere else.
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the restart that opens the hand-over (ADR-0393 §4 steps 1, 3 and 4) is the next change"
    )
)]
pub(crate) mod hand_over;
#[cfg_attr(
    any(target_arch = "wasm32", target_os = "android"),
    expect(
        dead_code,
        reason = "only the desktop and iOS runners poll the hook for ui_runtimes"
    )
)]
mod record;
#[cfg(all(
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
pub(crate) use hand_over::HandOver;

#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
use record::Due;
use record::Record;

/// A development reload driver installed on an [`AppConfig`].
///
/// Built by [`AppConfig::dev_reload`]. `Clone` shares the one hook and
/// its bookkeeping, so every window opened with clones of the same
/// configuration is driven by the same hook: attached once per loop, polled
/// at each UI runtime's frame boundary, and each version applied once to every
/// UI runtime through the one record of versions they share.
#[derive(Clone)]
pub struct DevReload(Arc<Mutex<Slot>>);

// The web runner drives no hook, and the Android runner only asks it for
// scene frames, so neither reads the ui_runtime bookkeeping.
#[cfg_attr(
    any(target_arch = "wasm32", target_os = "android"),
    expect(
        dead_code,
        reason = "only the desktop and iOS runners poll the hook for ui_runtimes"
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
    /// The loop's versions and what each UI runtime runs. Cleared when the
    /// loop ends.
    record: Record,
    /// Whether the log said once that a host whose executor turns on a raster
    /// thread asks the hook for no scene frame.
    unlent_noted: bool,
}

impl DevReload {
    pub(crate) fn new(hook: impl DevReloadHook) -> Self {
        Self(Arc::new(Mutex::new(Slot {
            hook: Some(Box::new(hook)),
            attached: false,
            detach_owed: false,
            record: Record::default(),
            unlent_noted: false,
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
        #[expect(
            clippy::disallowed_methods,
            reason = "ADR-0287: a dev hook's panic disables it; the lent slot is never lost"
        )]
        match catch_unwind(AssertUnwindSafe(|| call(hook.as_mut()))) {
            Ok(value) => {
                let owed = std::mem::take(&mut self.0.lock().detach_owed);
                if owed && let Err(payload) = catch_unwind(AssertUnwindSafe(|| hook.detach())) {
                    flui_foundation::panic::discard_payload(payload);
                    disable("detach", hook);
                    return Lent::Returned(value);
                }
                self.0.lock().hook = Some(hook);
                Lent::Returned(value)
            }
            Err(payload) => {
                flui_foundation::panic::discard_payload(payload);
                disable(what, hook);
                Lent::Panicked
            }
        }
    }

    /// A presentation's executor turns on a raster thread, which cannot draw
    /// a lent scene within the call, so the hook is asked for no scene frame
    /// there. Logged once per hook, with no lock held: a subscriber is not
    /// ours.
    fn note_unlent_host(&self) {
        let first = !std::mem::replace(&mut self.0.lock().unlent_noted, true);
        if first {
            tracing::info!(
                "development reload hook installed on a host whose raster executor turns on its \
                 own thread; a scene plugin cannot draw there, so its windows draw their widget \
                 trees"
            );
        }
    }

    /// Tell the hook what the host did with a version. A hook lent to another
    /// call (it re-entered the host) misses the report; one that panics in
    /// `applied` is disabled like in any call.
    #[cfg(not(target_os = "android"))]
    fn report(&self, outcome: &ReloadOutcome<'_>) {
        let _reported = self.lend("applied", |hook| hook.applied(outcome));
    }

    /// The loop ended: detach the hook and forget every UI runtime.
    #[cfg(not(target_os = "android"))]
    fn detach(&self) {
        {
            let mut slot = self.0.lock();
            if !slot.attached {
                return;
            }
            slot.attached = false;
            slot.record.clear();
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

/// Run `body`, swallowing a panic and discarding its payload under
/// containment (ADR-0281 §2). (`application_control::contain` does the same,
/// but is not compiled for Android.)
#[cfg(not(target_arch = "wasm32"))]
fn contain(body: impl FnOnce()) {
    #[expect(
        clippy::disallowed_methods,
        reason = "ADR-0287: a hook-reload body's panic is discarded so the caller proceeds"
    )]
    if let Err(payload) = catch_unwind(AssertUnwindSafe(body)) {
        flui_foundation::panic::discard_payload(payload);
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
                 the worker will not be loaded; install one with `AppConfig::dev_reload`"
            );
        }
        Self(config.dev_reload.clone())
    }

    /// The scene-plugin driver of the same hook, which the runner's frame
    /// driver asks at the start of each presentation's frame.
    pub(crate) fn scene_plugin(&self) -> ScenePlugin {
        ScenePlugin(self.0.clone())
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

    /// Record a UI runtime the runner has just mounted as running the code
    /// loaded now, so it reassembles to every later version, including one
    /// another UI runtime's boundary polls before this UI runtime's first
    /// frame. The runner calls this right after the root attaches and keeps
    /// the handle as long as the UI runtime: dropping it removes the UI
    /// runtime from the record.
    pub(crate) fn register_ui_runtime(&self, ui_runtime: &UiRuntime) -> RuntimeReload {
        let runtime = ui_runtime.id();
        if let Some(reload) = &self.0 {
            reload.0.lock().record.mounted(runtime);
        }
        RuntimeReload {
            reload: self.clone(),
            runtime,
        }
    }
}

/// One UI runtime's place in the loop's record (ADR-0393 §3), which its frame
/// driver holds as long as the UI runtime. Dropping it removes the UI runtime
/// from the record.
#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
#[must_use = "dropping the handle removes the UI runtime from the record"]
pub(crate) struct RuntimeReload {
    reload: WorkerReload,
    runtime: UiRuntimeId,
}

#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
impl RuntimeReload {
    /// The scene-plugin driver of the same hook.
    pub(crate) fn scene_plugin(&self) -> ScenePlugin {
        self.reload.scene_plugin()
    }

    /// Poll the hook at the UI runtime's frame boundary, then do what the
    /// record says the UI runtime owes and report it to the hook.
    ///
    /// A version of the layout the application was composed from reaches
    /// every UI runtime once: each reassembles to the latest such version when
    /// its own boundary finds it on an older one. A version of another layout
    /// needs a restart (ADR-0393 §4), which this host cannot do yet: it
    /// reports [`RestartFailure::NoApplication`], and no later version
    /// reassembles.
    pub(crate) fn poll_and_apply(&self, ui_runtime: &UiRuntime) {
        let Some(reload) = &self.reload.0 else {
            return;
        };
        let id = self.runtime;
        debug_assert_eq!(
            ui_runtime.id(),
            id,
            "BUG: a handle polled for another UI runtime"
        );
        // After a detach cleared the record, a later loop's poll records the
        // UI runtime again, at the code loaded then.
        reload.0.lock().record.mounted(id);
        let polled = reload.lend("poll", |hook| (hook.poll(), hook.reassemble_requested()));
        let (restart, due) = {
            let mut slot = reload.0.lock();
            let restart = match polled {
                Lent::Returned((event, rebuild)) => slot.record.report(event, rebuild),
                Lent::Absent | Lent::Panicked => None,
            };
            if restart.is_some() {
                slot.record.restart_failed();
            }
            (restart, slot.record.due(id))
        };
        if let Some(version) = restart {
            tracing::warn!(
                layout = version.layout.get(),
                "hot reload: the new build changed the layout of kept values, and this host \
                 cannot restart the application; restart it to run the new code"
            );
            reload.report(&ReloadOutcome::restart_failed(
                version,
                RestartFailure::NoApplication,
            ));
        }
        if let Due::Reassemble(version) = due {
            tracing::info!(
                revision = version.map(|version| version.revision.get()),
                "hot reload: reassembling"
            );
            ui_runtime.perform_reassemble_entered();
            if let Some(version) = version {
                reload.report(&ReloadOutcome::reassembled(version, id));
            }
        }
    }
}

#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
impl Drop for RuntimeReload {
    fn drop(&mut self) {
        if let Some(reload) = &self.reload.0 {
            reload.0.lock().record.retired(self.runtime);
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

/// The scene-plugin driver every native runner holds: the configuration's
/// hook, if any, may draw a presentation's frame itself, in place of the
/// widget tree (ADR-0391).
///
/// Each runner asks [`own_frame`](Self::own_frame) at the start of every
/// presentation's frame on the owner thread, after the owner-inbox drain and
/// before its dirty gate, and on a [`PluginFrame`] runs
/// [`PluginFrame::finish`] instead of the widget pipeline.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
pub(crate) struct ScenePlugin(Option<DevReload>);

#[cfg(target_os = "android")]
impl ScenePlugin {
    /// `internal_data_path` is the app's internal files directory, where
    /// `flui run --scene` pushes the plugin (`/data/local/tmp` without one);
    /// it is read only to warn about a pushed plugin nothing will load.
    pub(crate) fn from_config(
        config: &AppConfig,
        internal_data_path: Option<&std::path::Path>,
    ) -> Self {
        // `flui_hot_reload::ScenePluginHook::LIBRARY_NAME`; this crate does not
        // name the reload tool.
        const SCENE_LIBRARY: &str = "libflui_scene.so";
        if config.dev_reload.is_some() {
            tracing::info!(
                "development reload hook installed; the Android runner drives only its \
                 `scene_frame` (no attach, poll or detach)"
            );
        } else {
            let pushed = internal_data_path
                .unwrap_or_else(|| std::path::Path::new("/data/local/tmp"))
                .join(SCENE_LIBRARY);
            if pushed.exists() {
                tracing::warn!(
                    path = %pushed.display(),
                    "a scene plugin is on the device, but no DevReloadHook is installed on the \
                     AppConfig, so it will not be loaded; install \
                     `flui_hot_reload::ScenePluginHook` with `AppConfig::dev_reload`"
                );
            }
        }
        Self(config.dev_reload.clone())
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl ScenePlugin {
    /// Let the hook's scene plugin own `ui_runtime`'s frame in `raster`'s
    /// window, drawn at the window's physical size. Run inside the runtime's
    /// entry, after its owner-inbox drain and before the frame's gate.
    ///
    /// `Some` when the plugin's scene was shown: the caller runs no widget
    /// frame this wake and hands the answer to [`PluginFrame::finish`]. `None`
    /// with no hook installed (one `Option` check), while the presentation's
    /// frames are off (its window cannot be seen, so nothing is drawn), on a
    /// host whose executor turns on a raster thread (which cannot draw a lent
    /// scene within the call), and when the hook drew nothing, refused the
    /// scene or panicked: the widget frame runs as without a hook.
    pub(crate) fn own_frame(
        &self,
        ui_runtime: &UiRuntime,
        raster: &RasterSlot,
    ) -> Option<PluginFrame> {
        let reload = self.0.as_ref()?;
        if !ui_runtime.scheduler().frames_enabled() {
            return None;
        }
        if !raster.lends_scenes() {
            reload.note_unlent_host();
            return None;
        }
        let (width, height) = raster.size;
        let rendered = reload.lend("scene_frame", |hook| {
            let mut render = |scene: &flui_layer::Scene, reset_fonts: bool| {
                raster.render_lent(scene, reset_fonts)
            };
            // SAFETY: `render` lends the scene to the raster executor's
            // `render_lent`, which traverses it synchronously and keeps no
            // reference past the call. It ignores AnnotatedRegion payloads;
            // cached images are concrete Arc<Vec<u8>> data, and first font
            // admission copies source bytes into the host registry's
            // Arc<[u8]>. Recorded GPU commands retain no plugin trait objects.
            // This callback retains no image-dependent payload, including
            // when rendering unwinds.
            #[expect(unsafe_code, reason = "`scene_frame` is an `unsafe fn`")]
            unsafe {
                hook.scene_frame(f64::from(width), f64::from(height), &mut render)
            }
        });
        matches!(rendered, Lent::Returned(true)).then_some(PluginFrame(()))
    }
}

/// A presentation's frame the scene plugin drew: what the frame accepted
/// beside its visual pass is still owed.
#[cfg(not(target_arch = "wasm32"))]
#[must_use = "a frame the scene plugin drew still owes the runtime its background work"]
pub(crate) struct PluginFrame(());

#[cfg(not(target_arch = "wasm32"))]
impl PluginFrame {
    /// After the runtime's entry, in place of the widget frame: the runtime's
    /// background pump consumes the frame request and serves the work no
    /// visual pass is needed for (owner commands, async tasks, due input
    /// deadlines), so a request made while the plugin draws asks for the next
    /// frame instead of finding the latch still set, and a due deadline does
    /// not wake the loop again for a frame that never serves it. Then the
    /// window reports what the lent render left of a panic. A panic of either
    /// leaves the other to run and resumes once both have.
    pub(crate) fn finish(self, ui_runtime: &mut UiRuntime, raster: &RasterSlot) {
        // The frame is spent here, once.
        let Self(()) = self;
        let mut caught = FirstPanic::new();
        let _served = caught.contain(|| ui_runtime.pump_background());
        let _reported = caught.contain(|| raster.report_frame_panic(ui_runtime));
        caught.resume();
    }
}

#[cfg(all(test, not(target_os = "android"), not(target_arch = "wasm32")))]
mod tests;
