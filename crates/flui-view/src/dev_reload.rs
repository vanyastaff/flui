//! The development-reload seam (ADR-0094 §1).
//!
//! A reload tool — `flui-hot-reload`'s dlopen worker and scene plugin today —
//! implements [`DevReloadHook`]; the application installs the instance on its
//! configuration (`flui_app::AppConfig::dev_reload`), and the host drives
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
//! | desktop, iOS | yes | yes | yes, every frame |
//! | Android | no | no | yes, every frame |
//! | web | no | no | no |
//!
//! So a worker hook ([`DevReloadHook::poll`]-driven) does nothing on Android,
//! and a scene plugin ([`DevReloadHook::scene_frame`]) draws on every native
//! host; the Android host logs which calls it makes when a hook is installed.
//! Where a host calls them:
//!
//! - [`DevReloadHook::attach`] runs once per event loop, on the owner thread,
//!   before the first window opens; [`DevReloadHook::detach`] runs once when
//!   that loop ends. A hook is never attached twice without a detach between.
//! - [`DevReloadHook::poll`] runs on the owner thread at each UI runtime's frame
//!   boundary, before the frame's own work, and reports the newest version the
//!   tool built ([`ReloadEvent::Loaded`]). The host keeps one record per loop
//!   (ADR-0393 §3): a version of the layout the application was composed from
//!   reassembles every UI runtime once, each at its own next boundary (an idle
//!   window reassembles when it next draws, to the latest version). A version
//!   of another layout needs a restart (ADR-0393 §4), which the host reports as
//!   [`RestartFailure::NoApplication`] until it builds one. A version reported
//!   twice changes nothing. The first version after `attach` is taken as the
//!   one the application was composed from, so every UI runtime reassembles
//!   to it once. Each result goes back through
//!   [`DevReloadHook::applied`].
//! - [`DevReloadHook::scene_frame`] runs on the owner thread at the start of
//!   each window's frame, before the widget pipeline, with the window's
//!   physical size: not while the window's frames are off, nor on a host whose
//!   raster executor turns on its own thread (Wayland), which cannot draw a
//!   lent scene within the call.
//! - A hook that panics in any method is dropped and never called again; the
//!   frame that caught the panic continues without a reload.
//!
//! The per-call seam ADR-0094 §1 also describes, which routes each framework
//! call into user code through a patcher's jump table, arrives with a code
//! patcher that needs it.

use std::{fmt, num::NonZeroU64, sync::Arc};

use flui_foundation::UiRuntimeId;

use flui_layer::Scene;

/// The build a tree is made from (ADR-0393 §3). The tool that builds the application mints it,
/// and the hook reads it from the build's manifest; the host compares it for equality and never
/// orders or interprets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct CodeVersion {
    /// Equal for two builds whose kept values the later build may read.
    pub layout: LayoutEpoch,
    /// Differs for every build.
    pub revision: Revision,
}

impl CodeVersion {
    /// The version of a build laid out as `layout`, its `revision` the build's own.
    #[must_use]
    pub const fn new(layout: LayoutEpoch, revision: Revision) -> Self {
        Self { layout, revision }
    }
}

/// Which builds may read each other's kept values: two are equal or they are not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayoutEpoch(NonZeroU64);

impl LayoutEpoch {
    /// The epoch a tool wrote into a manifest; `None` for zero.
    #[must_use]
    pub const fn try_new(raw: u64) -> Option<Self> {
        match NonZeroU64::new(raw) {
            Some(raw) => Some(Self(raw)),
            None => None,
        }
    }

    /// The integer a manifest holds.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// One build: two are equal or they are not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Revision(NonZeroU64);

impl Revision {
    /// The revision a tool wrote into a manifest; `None` for zero.
    #[must_use]
    pub const fn try_new(raw: u64) -> Option<Self> {
        match NonZeroU64::new(raw) {
            Some(raw) => Some(Self(raw)),
            None => None,
        }
    }

    /// The integer a manifest holds.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0.get()
    }
}

/// What a development reload driver observed since its last poll.
///
/// Deliberately exhaustive: a host derives the reload from the version and its own record
/// (ADR-0393 §3), and a new event must not compile until each host gives it a meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[expect(
    clippy::exhaustive_enums,
    reason = "a host derives the reload from each event, so a new event must not compile until each host gives it one"
)]
pub enum ReloadEvent {
    /// No new code is loaded.
    Unchanged,
    /// New code is loaded and this is its version.
    Loaded(CodeVersion),
}

/// What the host did with a version, reported to [`DevReloadHook::applied`] after the turn
/// that applied it.
#[derive(Debug)]
#[non_exhaustive]
pub enum ReloadOutcome<'a> {
    /// One UI runtime reassembled to `version`: every element rebuilt with the code now loaded,
    /// every `State` kept.
    #[non_exhaustive]
    Reassembled {
        /// The version the UI runtime now runs.
        version: CodeVersion,
        /// The UI runtime that reassembled.
        runtime: UiRuntimeId,
    },
    /// The application was composed again from `version`.
    #[non_exhaustive]
    Restarted {
        /// The version the application was composed from.
        version: CodeVersion,
    },
    /// The restart did not compose an application; the host has none until a later version
    /// composes one.
    #[non_exhaustive]
    RestartFailed {
        /// The version the restart was for.
        version: CodeVersion,
        /// Why it composed nothing.
        reason: RestartFailure<'a>,
    },
}

impl<'a> ReloadOutcome<'a> {
    /// `runtime` reassembled to `version`.
    #[must_use]
    pub const fn reassembled(version: CodeVersion, runtime: UiRuntimeId) -> Self {
        Self::Reassembled { version, runtime }
    }

    /// The application was composed again from `version`.
    #[must_use]
    pub const fn restarted(version: CodeVersion) -> Self {
        Self::Restarted { version }
    }

    /// The restart for `version` composed no application, for `reason`.
    #[must_use]
    pub const fn restart_failed(version: CodeVersion, reason: RestartFailure<'a>) -> Self {
        Self::RestartFailed { version, reason }
    }
}

/// Why a restart composed no application.
#[derive(Debug)]
#[non_exhaustive]
pub enum RestartFailure<'a> {
    /// The application's entry panicked.
    EntryPanicked,
    /// The entry returned without handing an application over.
    NoApplication,
    /// The handed application is one the host's `run` refuses, or composing it failed; the
    /// error is the host's.
    Launch(&'a (dyn std::error::Error + 'static)),
    /// User code that composing runs (a plugin's registration, a service's start) panicked.
    ComposePanicked,
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
/// use flui_view::dev_reload::{
///     CodeVersion, DevReloadHook, LayoutEpoch, ReloadEvent, ReloadWake, Revision,
/// };
///
/// fn first_build() -> CodeVersion {
///     let layout = LayoutEpoch::try_new(1).expect("1 is not zero");
///     CodeVersion::new(layout, Revision::try_new(1).expect("1 is not zero"))
/// }
///
/// /// Reports one build after the host attaches it.
/// #[derive(Default)]
/// struct OneBuild {
///     wake: Option<ReloadWake>,
///     pending: bool,
/// }
///
/// impl DevReloadHook for OneBuild {
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
///             ReloadEvent::Loaded(first_build())
///         } else {
///             ReloadEvent::Unchanged
///         }
///     }
/// }
///
/// let mut hook = OneBuild::default();
/// hook.attach(ReloadWake::new(|| {}));
/// assert_eq!(hook.poll(), ReloadEvent::Loaded(first_build()));
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

    /// At the same frame boundary, after [`poll`](Self::poll): whether the code already loaded
    /// asked every UI runtime to rebuild since the last poll (a worker's own rebuild request).
    /// The host reassembles each UI runtime once at its next boundary, outside the version
    /// record, and reports no outcome; as Flutter's `ext.flutter.reassemble`, it delivers no
    /// code. The default asks for none.
    fn reassemble_requested(&mut self) -> bool {
        false
    }

    /// Restart: run the entry of the code now loaded, on the owner thread, while the host's
    /// hand-over is open (ADR-0393 §4). The default runs nothing, for a mechanism that cannot
    /// restart in the process.
    fn run_entry(&mut self) {}

    /// What the host did with a version, after the turn that applied it: once per UI runtime
    /// that reassembled and once per restart, succeeded or failed. The default ignores it.
    fn applied(&mut self, _outcome: &ReloadOutcome<'_>) {}

    /// Let a loaded scene plugin draw this frame instead of the widget tree
    /// (a scene plugin, on every native host). Returns `true` when `render`
    /// succeeded, in which case the host skips the widget pipeline for this
    /// frame.
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
        #[expect(unsafe_code, reason = "`scene_frame` is an `unsafe fn`")]
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
