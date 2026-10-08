//! The development-reload seam (ADR-0094 §1).
//!
//! A reload tool — `flui-hot-reload`'s dlopen worker and scene plugin today —
//! implements [`DevReloadHook`]; the application installs the instance on its
//! configuration (`flui_app::AppConfig::with_dev_reload`), and the host drives
//! it. The trait lives here, below the runtime, so the host never names the
//! tool and the tool never names the host: `flui-app` has no edge to any
//! reload crate, and a reload package reaches this module through `flui-sdk`
//! (`flui_sdk::view::dev_reload`).
//!
//! # What the host promises
//!
//! Which methods a host calls depends on the platform:
//!
//! | Host | `attach` / `detach` | `poll` | `scene_frame` |
//! |------|---------------------|--------|---------------|
//! | desktop, iOS | yes | yes | no |
//! | Android | no | no | yes, every frame |
//! | web | no | no | no |
//!
//! So a worker hook ([`DevReloadHook::poll`]-driven) does nothing on Android,
//! and a scene plugin ([`DevReloadHook::scene_frame`]) draws only there; the
//! Android host logs which calls it makes when a hook is installed. Where a
//! host calls them:
//!
//! - [`DevReloadHook::attach`] runs once per event loop, on the owner thread,
//!   before the first window opens; [`DevReloadHook::detach`] runs once when
//!   that loop ends. A hook is never attached twice without a detach between.
//! - [`DevReloadHook::poll`] runs on the owner thread at each UI runtime's frame
//!   boundary, before the frame's own work. A [`ReloadEvent::Patched`] it
//!   returns is applied once to every UI runtime the host drives, each at its own
//!   next boundary (an idle window applies it when it next draws), and never
//!   to a UI runtime mounted after the poll.
//! - [`DevReloadHook::scene_frame`] runs on the owner thread at the start of
//!   each frame, before the widget pipeline.
//! - A hook that panics in any method is dropped and never called again; the
//!   frame that caught the panic continues without a reload.
//!
//! The per-call seam ADR-0094 §1 also describes, which routes each framework
//! call into user code through a patcher's jump table, and the
//! `RestartRequired` event arrive with the code patcher that needs them.

use std::{fmt, sync::Arc};

use flui_rendering::layer::Scene;

/// What a development reload driver observed since its last poll.
///
/// Deliberately exhaustive: a host maps every event to the reload it
/// performs, and a new event must not compile until each host gives it one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReloadEvent {
    /// Nothing to apply.
    Unchanged,
    /// Code changed, or reloaded code asked for a rebuild: every UI runtime
    /// reassembles — every element rebuilds, every `State` is kept.
    Patched,
}

/// Asks the host for a frame, from any thread.
///
/// A hook keeps the value [`DevReloadHook::attach`] gives it and calls
/// [`Self::wake`] when it has something for the next
/// [`DevReloadHook::poll`] to report: an idle event loop produces no frames,
/// so without a wake a change would wait for unrelated input.
#[derive(Clone)]
pub struct ReloadWake(Arc<dyn Fn() + Send + Sync>);

impl ReloadWake {
    /// Wrap the host's frame request.
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self(Arc::new(wake))
    }

    /// Ask the host for a frame. Cheap, callable from any thread, and
    /// harmless after the loop has ended.
    pub fn wake(&self) {
        (self.0)();
    }
}

impl fmt::Debug for ReloadWake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReloadWake").finish_non_exhaustive()
    }
}

/// A development reload driver the application installs and the host calls
/// on the owner thread. See the [module docs](self) for the call order.
///
/// `Send` because the instance travels inside the application's
/// configuration, which is `Send`; the host still calls it only on the owner
/// thread, so an implementation needs no `Sync` and no locking of its own.
///
/// # Example
///
/// ```
/// use flui_view::dev_reload::{DevReloadHook, ReloadEvent, ReloadWake};
///
/// /// Reports one patch after the host attaches it.
/// #[derive(Default)]
/// struct OnePatch {
///     wake: Option<ReloadWake>,
///     pending: bool,
/// }
///
/// impl DevReloadHook for OnePatch {
///     fn attach(&mut self, wake: ReloadWake) {
///         self.pending = true;
///         wake.wake();
///         self.wake = Some(wake);
///     }
///
///     fn detach(&mut self) {
///         self.wake = None;
///     }
///
///     fn poll(&mut self) -> ReloadEvent {
///         if std::mem::take(&mut self.pending) {
///             ReloadEvent::Patched
///         } else {
///             ReloadEvent::Unchanged
///         }
///     }
/// }
///
/// let mut hook = OnePatch::default();
/// hook.attach(ReloadWake::new(|| {}));
/// assert_eq!(hook.poll(), ReloadEvent::Patched);
/// assert_eq!(hook.poll(), ReloadEvent::Unchanged);
/// hook.detach();
/// ```
pub trait DevReloadHook: Send + 'static {
    /// Once per event loop, before the first window: keep `wake` to ask for
    /// a frame when there is something to poll, and start any background
    /// work (an artifact watcher).
    fn attach(&mut self, wake: ReloadWake);

    /// The loop is ending: stop background work and drop `wake`. The host
    /// pairs every `attach` with one `detach`.
    fn detach(&mut self) {}

    /// At each UI runtime's frame boundary: what changed since the last poll.
    fn poll(&mut self) -> ReloadEvent;

    /// Let a loaded scene plugin draw this frame instead of the widget tree
    /// (Android's `flui run --scene`). Returns `true` when `render` succeeded,
    /// in which case the host skips the widget pipeline for this frame.
    ///
    /// The callback's second argument requests a fresh font identity namespace
    /// for the plugin image. It starts true and becomes true after loading a
    /// replacement image. Return true only after successful rendering; false
    /// or unwind preserves the reset request for the next attempt.
    ///
    /// The default draws nothing.
    ///
    /// # Safety
    ///
    /// `render` must not retain any scene payload, layer clone or annotation
    /// whose storage, code or vtable depends on the plugin image beyond this call,
    /// including when it unwinds. The hook may unload that image on its next
    /// call or when dropped. Borrowing the scene alone does not prevent its
    /// shared payloads from being cloned.
    ///
    /// ```compile_fail,E0133
    /// use flui_view::dev_reload::DevReloadHook;
    /// fn render_frame(hook: &mut dyn DevReloadHook) {
    ///     // SAFETY: this callback retains no plugin-backed data.
    ///     hook.scene_frame(100.0, 100.0, &mut |_scene, _reset_fonts| true);
    /// }
    /// ```
    ///
    /// A caller meeting the payload lifetime contract acknowledges it explicitly:
    ///
    /// ```
    /// use flui_view::dev_reload::DevReloadHook;
    /// fn render_frame(hook: &mut dyn DevReloadHook) {
    ///     // SAFETY: this callback retains no plugin-backed data.
    ///     unsafe { hook.scene_frame(100.0, 100.0, &mut |_scene, _reset_fonts| true); }
    /// }
    /// ```
    #[expect(unsafe_code, reason = "plugin payload lifetime is a caller obligation")]
    unsafe fn scene_frame(
        &mut self,
        _width: f64,
        _height: f64,
        _render: &mut dyn FnMut(&Scene, bool) -> bool,
    ) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    struct PollOnly;

    impl DevReloadHook for PollOnly {
        fn attach(&mut self, _wake: ReloadWake) {}

        fn poll(&mut self) -> ReloadEvent {
            ReloadEvent::Unchanged
        }
    }

    #[test]
    fn scene_frame_default_never_calls_render() {
        let mut calls = 0;
        // SAFETY: PollOnly uses the default, which supplies no plugin payload.
        #[expect(unsafe_code)]
        let drew = unsafe {
            PollOnly.scene_frame(10.0, 10.0, &mut |_scene, _reset_fonts| {
                calls += 1;
                true
            })
        };
        assert!(!drew, "the default claims no frame");
        assert_eq!(calls, 0, "the default never renders");
    }

    #[test]
    fn a_wake_clone_calls_the_same_host_request() {
        let wakes = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&wakes);
        let wake = ReloadWake::new(move || {
            counted.fetch_add(1, Ordering::Relaxed);
        });
        let cloned = wake.clone();
        wake.wake();
        drop(wake);
        cloned.wake();
        assert_eq!(wakes.load(Ordering::Relaxed), 2);
    }
}
