//! The owner thread: the thread that runs a host's event loop, and what
//! proves, schedules and guards work on it (ADR-0151 §10).
//!
//! - this module: the owner-thread platform capability (ADR-0039) and the
//!   cross-thread handles minted from it;
//! - [`signal`]: coalesced owner turns and quit admission;
//! - [`slot`]: owner-thread storage for a value that is not `Send`;
//! - [`panic_boundary`]: the containment a panic in user code meets before it
//!   reaches the operating system;
//! - [`call_point`]: the one point where the platform calls an application or
//!   window handler, with no lock or borrow held;
//! - `app_delivery`: what a loop delivers to the application's handler, the
//!   one passed to `run`, through one causal queue;
//! - [`handlers`]: what a loop asks of the windows it holds, and the frame
//!   requests a host stamps;
//! - `window_delivery`: what a window delivers to the handler it was opened
//!   with, through one causal queue;
//! - [`notices`]: the channel a loop pushes system notices through, which the
//!   lifecycle feature implements;
//! - `sources`: what a loop keeps of its host's preference source;
//! - `watch`: an observation a feature started for a loop, which the loop
//!   ends by dropping it;
//! - `hwnd_affinity`: the Win32 rules for which thread may reach a window
//!   and when its user data is torn down, which the owner control and the
//!   loop apply (on Windows, and in tests on every host);
//! - `windows`, `apple`, `android`, `web`, `linux` (Wayland and X11), `winit`
//!   and [`headless`]: each platform's loop, its wakes and what it owns of the
//!   thread, each on the targets its host runs on (headless everywhere);
//!   `redraw_poll`, the redraw decision around one native poll,
//!   `wake_deadline`, the rule by which a loop that waits with a timeout of its
//!   own arms the application's wake deadline, and `modal_pace`, how often a
//!   Windows modal loop asks for frames, each pure and checked on every host. A loop
//!   that serves several features through one operating-system object
//!   dispatches through hooks declared beside it, which the host registers. A
//!   feature's state or object the loop keeps (the displays, the preference
//!   source, the notice channel, a clipboard, a drag-and-drop transport) it
//!   holds in a field typed by a hook declared beside it, which the host's
//!   table fills through the loop's host trait, and it answers that position
//!   itself where it can; an observation a feature starts for the loop it keeps
//!   as a `watch::Watch`.
//!
//! [`OwnerPlatform`] carries every `Platform` operation that is owner-affine
//! on at least one real OS: window creation, display queries, activation,
//! appearance/keyboard queries, and quit. It is minted only by a backend, on
//! the thread that owns — or, before the loop starts, will own — its event
//! loop, and handed to [`AppHandler::ready`](crate::AppHandler::ready).
//! `!Send + !Sync` by phantom marker: possession proves the thread, so a
//! wrong-thread call becomes a compile error instead of a runtime assert (the
//! [`OwnerAffinity`] backstop `flui-foundation` carries).
//!
//! [`OwnerAffinity`]: flui_foundation::OwnerAffinity
//!
//! Not [`Clone`] (ADR-0039 §1): the sanctioned way to
//! hold this across owner-thread callbacks is `flui-app`'s loop-scoped
//! `OWNER_PLATFORM_HOST` TLS slot, read through its `with_owner_platform`
//! borrow-style accessor — never a durable owned copy squirreled away
//! outside that fenced accessor. [`PlatformProxy`] is how a *worker* thread
//! reaches the owner; it is `Clone` by design, since it carries no
//! thread-affine capability, only a bounded, typed, cross-thread request
//! channel.
//!
//! Only the `pub(crate)` constructor here mints an `OwnerPlatform`, so only a
//! host in this crate can hand `ready` the capability it needs; `Platform`
//! is sealed to say so in its type (ADR-0151 §5).

#[cfg(any(target_os = "ios", test))]
pub(crate) mod accessibility_slot;
#[cfg(any(target_os = "android", test))]
pub(crate) mod android;
pub(crate) mod app_delivery;
#[cfg(any(target_os = "macos", target_os = "ios", test))]
pub(crate) mod apple;
pub(crate) mod call_point;
pub mod handlers;
pub(crate) mod headless;
#[cfg(any(wayland_host, x11_host, test))]
pub(crate) mod linux;
#[cfg(any(test, windows))]
pub(crate) mod modal_pace;
pub(crate) mod notices;
// The rules by which the Win32 loop and its windows reach a window's context
// across threads: pure, so `test` compiles them on every host. Beside the
// loop's module rather than in it, which is Windows' alone.
#[cfg(any(windows, test))]
pub(crate) mod hwnd_affinity;
pub(crate) mod panic_boundary;
#[cfg(any(test, target_os = "android"))]
pub(crate) mod redraw_poll;
pub(crate) mod signal;
pub(crate) mod slot;
#[cfg(any(windows, x11_host, wayland_host, test))]
pub(crate) mod wake_deadline;
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    target_arch = "wasm32",
    linux_host
))]
pub(crate) mod watch;
pub(crate) mod window_delivery;
// Every native host keeps a source; the headless host keeps none, so a Linux
// build without a Linux host has no loop to keep one.
#[cfg(any(
    windows,
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_arch = "wasm32",
    linux_host
))]
pub(crate) mod sources;
#[cfg(target_arch = "wasm32")]
pub(crate) mod web;
#[cfg(windows)]
pub(crate) mod windows;
#[cfg(all(
    feature = "winit-backend",
    any(target_os = "windows", target_os = "macos", target_os = "linux")
))]
pub(crate) mod winit;

use std::fmt;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::thread::ThreadId;

use flui_foundation::{ClaimHandle, ClaimOutcome};
use flui_platform::text_store::TextStoreHost;
use static_assertions::{assert_impl_all, assert_not_impl_any};

use crate::error::PlatformError;
use crate::host_window::{HostWindow, OwnerThreadToken};
use crate::platform::{PathPromptOptions, Platform, PlatformExecutor};
use crate::task::Task;
use flui_platform::data_transfer::DataTransferSource;
use flui_platform::{WindowHandler, WindowId, WindowOptions};
use signal::{OwnerSignal, SignalTransport};

// ============================================================================
// OwnerPlatform
// ============================================================================

/// Owner-thread platform capability. See the module docs for the full
/// contract.
///
/// `trybuild_ui::ui_tests` pins the `Send` refusal diagnostic and compiles
/// the valid owner-local and thread-safe residual callers.
///
/// ```compile_fail,E0277
/// // Illustration only: the item-position `assert_not_impl_any!` below is
/// // the compile-time evidence. This doctest runs with the workspace
/// // doctests (`cargo test --doc`, part of `cargo xtask ci`).
/// fn assert_send<T: Send>() {}
/// assert_send::<flui_native::OwnerPlatform>();
/// ```
pub struct OwnerPlatform {
    platform: Arc<dyn Platform>,
    hooks: Arc<dyn OwnerHooks>,
    // Slice 3 shrinks `platform` to `Arc<dyn OwnerOps>`; both fields stay
    // private regardless.
    _owner_affine: PhantomData<*const ()>,
}

impl fmt::Debug for OwnerPlatform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OwnerPlatform")
            .field("platform", &self.platform.name())
            .finish_non_exhaustive()
    }
}

impl OwnerPlatform {
    /// Minted only by a backend — at `on_ready`, or (on backends whose
    /// owner thread may create directly outside callbacks) at any later
    /// point on that same thread. `pub(crate)`: not part of the public
    /// constructor surface (see the module docs' crate-seal note).
    pub(crate) fn new(platform: Arc<dyn Platform>, hooks: Arc<dyn OwnerHooks>) -> Self {
        Self {
            platform,
            hooks,
            _owner_affine: PhantomData,
        }
    }

    /// Install the loop's owner-turn callback. Worker proxies send only wake signals.
    ///
    /// Delivery is deferred, coalesced and independent of visible windows.
    /// # Errors
    /// Returns unsupported on backends without this transport, or owner-gone after quit.
    pub fn on_wake(&self, callback: Box<dyn FnMut() + Send>) -> Result<(), WakeRegistrationError> {
        self.hooks.on_wake(callback)
    }

    /// The host's plain-text clipboard, owner-local: the application's
    /// clipboard provider builds its handles over it (ADR-0152).
    #[must_use]
    pub fn clipboard(&self) -> std::rc::Rc<dyn crate::NativeClipboard> {
        self.platform.clipboard()
    }

    /// Tell the platform the application released the session end it asked to
    /// wait for. See [`Platform::release_session_end`].
    pub fn release_session_end(&self) {
        self.platform.release_session_end();
    }

    /// What the platform does for the application when the user's session
    /// ends. See [`Platform::session_end_support`].
    #[must_use]
    pub fn session_end_support(&self) -> flui_platform::SessionEndSupport {
        self.platform.session_end_support()
    }

    /// The commands and observations every window of this platform has, known
    /// before one opens. See [`Platform::window_capabilities`].
    #[must_use]
    pub fn window_capabilities(&self) -> flui_platform::WindowCapabilities {
        self.platform.window_capabilities()
    }

    /// The title bar and border a decorated window adds to its content size on a
    /// display of scale `scale_factor`. See [`Platform::window_frame_inset`].
    #[must_use]
    pub fn window_frame_inset(&self, scale_factor: f64) -> flui_foundation::geometry::Size<f64> {
        self.platform.window_frame_inset(scale_factor)
    }

    /// Open a window whose deliveries go to `handler` (ADR-0254 §4). `Ready`
    /// is guaranteed inside `on_ready`; afterwards the backend may defer
    /// through its owner lane (where one exists), returning `Pending`.
    ///
    /// The handler exists before the window's first event: its first input
    /// is the window's capability snapshot. A refused open drops it here.
    ///
    /// # Errors
    /// See [`OpenWindowError`].
    pub fn open_window(
        &self,
        options: WindowOptions,
        handler: Rc<dyn WindowHandler>,
    ) -> Result<WindowOpen, OpenWindowError> {
        self.hooks.open_owner_window(options, handler)
    }

    /// The currently active (focused) window, if any.
    #[must_use]
    pub fn active_window(&self) -> Option<WindowId> {
        self.platform.active_window()
    }

    /// Read the host's system preferences without choosing a user window.
    ///
    /// # Errors
    /// Forwards native read and owner-lifetime failures.
    pub fn preferences(&self) -> Result<flui_platform::SystemPreferences, PlatformError> {
        self.platform.preferences()
    }

    /// Sample every display (monitor) the platform knows right now.
    ///
    /// # Errors
    /// Forwards a failed native read.
    pub fn display_snapshot(&self) -> Result<flui_platform::DisplaySnapshot, PlatformError> {
        self.platform.display_snapshot()
    }

    /// Activate the application (bring to front).
    pub fn activate(&self, ignoring_other_apps: bool) {
        self.platform.activate(ignoring_other_apps);
    }

    /// The current keyboard layout identifier.
    #[must_use]
    pub fn keyboard_layout(&self) -> String {
        self.platform.keyboard_layout()
    }

    /// Request the application to quit.
    pub fn quit(&self) {
        self.platform.quit();
    }

    /// Escapes to the residual thread-safe surface: `background_executor`,
    /// callback registration, and the rest of §2's "stays on
    /// `Platform`" list, wrapped in a `Clone + Send + Sync` handle a
    /// `std::thread::scope` worker can freely hold — see
    /// [`SharedPlatform`]'s own doc for why this can no longer be
    /// `&dyn Platform` (that type is still `Send + Sync` in full,
    /// owner-affine methods included, until the owner-thread methods leave `Platform` (ADR-0039)).
    #[must_use]
    pub fn shared(&self) -> SharedPlatform {
        SharedPlatform::new(Arc::clone(&self.platform))
    }

    /// Mints a cross-thread capability, handed to workers, UI runtimes, or
    /// tasks that need to reach the owner from off-thread.
    #[must_use]
    pub fn proxy(&self) -> PlatformProxy {
        PlatformProxy::new(self.hooks.transport())
    }

    /// `window`'s text-store host (ADR-0135): the pull-model input-method
    /// integration the presentation tells which field's store to serve.
    /// `None` when the backend offers none (a push-model backend reports
    /// its input through [`PlatformWindow::text_input`] instead).
    ///
    /// Here because the host is owner-thread state and this type proves the
    /// thread; it moves to the owner-minted window registrar of ADR-0082 §4
    /// step 2 once that exists. The runner reads it once, where it reads the
    /// accessibility bridge (`runner::presentation_window`).
    ///
    /// [`PlatformWindow::text_input`]: flui_platform::PlatformWindow::text_input
    #[must_use]
    pub fn text_store_host(&self, window: &Rc<dyn HostWindow>) -> Option<Rc<dyn TextStoreHost>> {
        window.text_store_host(OwnerThreadToken::new())
    }
}

// The evidence that wrong-thread owner ops are compile errors — expanded
// by every `cargo check`, including cross-typecheck on win/mac, because
// `static_assertions` is a real (non-dev) dependency (see Cargo.toml).
assert_not_impl_any!(OwnerPlatform: Send, Sync);

// ============================================================================
// SharedPlatform
// ============================================================================

/// The thread-safe residual of [`Platform`] — every operation NOT gated on
/// owner-thread affinity, per ADR-0039 §2's "what stays" table.
///
/// `Clone + Send + Sync`: unlike `&dyn Platform` (which is `Platform: Send +
/// Sync` in full, `open_window`/`quit`/`activate`/`display_snapshot`/
/// `keyboard_layout`/`active_window`
/// included), a `SharedPlatform` genuinely can cross a `std::thread::scope`
/// boundary into safe code without smuggling an owner-affine call along
/// with it — **the method list below IS the fence**: every method on this
/// type is safe to call from any thread, and no owner-affine method is
/// ever added to it. Minted only by [`OwnerPlatform::shared`].
///
/// This is the compile-time-checked half of the owner-platform capability
/// (`assert_impl_all!` below); `Platform` itself (the trait `dyn` object underneath) remains
/// runtime-checked (`OwnerAffinity` debug-asserts) until the owner-thread methods leave `Platform` and split its
/// owner-affine methods off entirely.
#[derive(Clone)]
pub struct SharedPlatform {
    platform: Arc<dyn Platform>,
}

impl fmt::Debug for SharedPlatform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SharedPlatform")
            .field("platform", &self.platform.name())
            .finish_non_exhaustive()
    }
}

impl SharedPlatform {
    pub(crate) fn new(platform: Arc<dyn Platform>) -> Self {
        Self { platform }
    }

    /// The platform's background executor for async tasks.
    #[must_use]
    pub fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        self.platform.background_executor()
    }

    /// The platform's name for debugging/logging.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.platform.name()
    }

    /// The compositor name (e.g., "DWM" on Windows).
    #[must_use]
    pub fn compositor_name(&self) -> &'static str {
        self.platform.compositor_name()
    }

    /// The application's executable path.
    ///
    /// # Errors
    /// Propagates the backend's own lookup failure as
    /// [`PlatformError::AppPath`].
    pub fn app_path(&self) -> Result<PathBuf, PlatformError> {
        self.platform.app_path()
    }

    /// Where the application `app` keeps its data, or `None` when this
    /// platform has nowhere to keep it.
    #[must_use]
    pub fn data_dirs(&self, app: &flui_platform::StorageName) -> Option<crate::storage::DataDirs> {
        self.platform.data_dirs(app)
    }

    /// The data-transfer transport (ADR-0038).
    #[must_use]
    pub fn data_transfer(&self) -> Arc<dyn DataTransferSource> {
        self.platform.data_transfer()
    }

    /// Tells the platform the application released the session end it asked to
    /// wait for; see [`Platform::release_session_end`].
    pub fn release_session_end(&self) {
        self.platform.release_session_end();
    }

    /// What the platform does for the application when the user's session
    /// ends. See [`Platform::session_end_support`].
    #[must_use]
    pub fn session_end_support(&self) -> flui_platform::SessionEndSupport {
        self.platform.session_end_support()
    }

    /// The commands and observations every window of this platform has, known
    /// before one opens; see [`Platform::window_capabilities`].
    #[must_use]
    pub fn window_capabilities(&self) -> flui_platform::WindowCapabilities {
        self.platform.window_capabilities()
    }

    /// The frame a decorated window adds to its content size on a display of
    /// scale `scale_factor`; see [`Platform::window_frame_inset`].
    #[must_use]
    pub fn window_frame_inset(&self, scale_factor: f64) -> flui_foundation::geometry::Size<f64> {
        self.platform.window_frame_inset(scale_factor)
    }

    /// Requests that the owner thread ask the application handler's exit
    /// policy again — see [`Platform::request_exit_policy_reevaluation`]'s
    /// doc for the full contract. On this fence deliberately: it is the one
    /// exit-path operation a *worker* thread legitimately needs (a keep-alive
    /// application service completing is what makes a prior last-window veto
    /// stale), it is coalesced and enqueue-and-wake only — the handler itself
    /// is always asked on the owner thread, never here.
    pub fn request_exit_policy_reevaluation(&self) {
        self.platform.request_exit_policy_reevaluation();
    }

    /// Registers a callback for keyboard layout changes.
    pub fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut() + Send>) {
        self.platform.on_keyboard_layout_change(callback);
    }

    /// Opens a URL with the system's default handler.
    pub fn open_url(&self, url: &str) {
        self.platform.open_url(url);
    }

    /// Reveals a path in the platform's file manager.
    pub fn reveal_path(&self, path: &Path) {
        self.platform.reveal_path(path);
    }

    /// Opens a path with the system's default application.
    pub fn open_path(&self, path: &Path) {
        self.platform.open_path(path);
    }

    /// Shows a file/directory picker dialog. Returns selected paths, or
    /// `None` if the user cancelled (cancelling is not an error); a dialog
    /// that could not run resolves to [`PlatformError::Dialog`]. Runs
    /// asynchronously on a background thread.
    pub fn prompt_for_paths(
        &self,
        options: PathPromptOptions,
    ) -> Task<Result<Option<Vec<PathBuf>>, PlatformError>> {
        self.platform.prompt_for_paths(options)
    }

    /// Shows a "Save As" dialog for selecting a new file path. Returns the
    /// selected path, or `None` if the user cancelled (cancelling is not an
    /// error); a dialog that could not run resolves to
    /// [`PlatformError::Dialog`].
    pub fn prompt_for_new_path(
        &self,
        directory: &Path,
        suggested_name: Option<&str>,
    ) -> Task<Result<Option<PathBuf>, PlatformError>> {
        self.platform.prompt_for_new_path(directory, suggested_name)
    }
}

// The evidence that `SharedPlatform` genuinely is the thread-safe
// escape hatch its doc promises — expanded by every `cargo check`, same
// discipline as `OwnerPlatform`'s `assert_not_impl_any!` above.
assert_impl_all!(SharedPlatform: Send, Sync, Clone);

/// Result of an owner-thread window-open request.
#[must_use = "a Pending window creation must be waited on or polled; \
              dropping it abandons the request (the owner unwinds or skips)"]
pub enum WindowOpen {
    /// Created synchronously. Always the case inside `on_ready`, and on
    /// backends whose owner thread may create directly outside callbacks.
    Ready(Rc<dyn HostWindow>),
    /// Enqueued on the owner lane; resolves at the loop's next drain
    /// anchor.
    Pending(PendingWindow),
}

impl fmt::Debug for WindowOpen {
    // Manual impl: `HostWindow` carries no blanket `Debug`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ready(_) => f.debug_tuple("Ready").field(&"<dyn HostWindow>").finish(),
            Self::Pending(pending) => f.debug_tuple("Pending").field(pending).finish(),
        }
    }
}

impl WindowOpen {
    /// Bootstrap convenience: unwrap `Ready`; typed error otherwise.
    /// Guaranteed to succeed inside `on_ready`.
    ///
    /// # Errors
    /// [`OpenWindowError::NotReady`] if this call is deferred (never the
    /// case inside `on_ready`).
    pub fn try_ready(self) -> Result<Rc<dyn HostWindow>, OpenWindowError> {
        match self {
            Self::Ready(window) => Ok(window),
            Self::Pending(pending) => Err(OpenWindowError::NotReady(pending)),
        }
    }
}

/// Failure to open a window through [`OwnerPlatform`], [`PlatformProxy`],
/// or [`Platform::open_window`] itself — the trait method adopted this
/// typed taxonomy when `anyhow` was retired from the crate's public API
/// (the growth ADR-0039 forecast for the methods leaving `Platform`).
///
/// `#[non_exhaustive]`: the taxonomy still grows as backends gain
/// capabilities; additions are not breaks.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum OpenWindowError {
    /// The owner lane is at capacity. `rejected` is returned so the
    /// producer can retry without rebuilding the options.
    #[error("owner lane is full (capacity {capacity})")]
    LaneFull {
        /// The lane's fixed capacity.
        capacity: usize,
        /// The options that could not be enqueued.
        rejected: Box<WindowOptions>,
    },
    /// The event-loop owner is gone. `rejected` is `Some` when refusal
    /// happens at enqueue (the producer can retry without rebuilding
    /// options) and `None` when the loop died after the request was
    /// already accepted.
    #[error("event-loop owner is gone")]
    OwnerGone {
        /// The options that could not be delivered, if known at refusal.
        rejected: Option<Box<WindowOptions>>,
    },
    /// The backend could not create the window.
    #[error("the backend could not create the window: {message}")]
    Backend {
        /// The backend's own error message.
        message: String,
    },
    /// This host opens no windows: its table's window position is absent.
    #[error("this host opens no windows: {0}")]
    Unsupported(#[from] flui_platform::Unsupported),
    /// The event loop cannot service window creation from this call site in
    /// its current lifecycle state: called before `Platform::run` started
    /// the loop, from another thread while bootstrap (`on_ready`) is still
    /// running, or on the owner thread where blocking on the very lane the
    /// owner drains would deadlock. Distinct from
    /// [`OwnerGone`](Self::OwnerGone) — the loop has not died; this call
    /// site simply cannot be serviced right now. The message names the
    /// exact condition and the sanctioned alternative.
    #[error("window creation is unavailable from this call site: {message}")]
    Unavailable {
        /// The exact lifecycle condition, in the backend's own words.
        message: String,
    },
    /// Window creation was deferred; this call site requires `Ready`.
    #[error("window creation was deferred; this call site requires Ready")]
    NotReady(PendingWindow),
    /// An earlier [`try_take`](PendingWindow::try_take) on this same
    /// `PendingWindow` already claimed the result — there is nothing left
    /// to hand back. Not a slot-invariant violation, just a
    /// caller-sequencing fact (mirrors [`WaitError::AlreadyClaimed`]):
    /// `try_take` takes `&mut self`, so nothing stops a caller from
    /// following a successful `try_take` with a poll of the `Future` impl
    /// on the same handle.
    #[error("the pending window was already claimed by an earlier try_take call")]
    AlreadyClaimed,
}

// ============================================================================
// PlatformProxy
// ============================================================================

/// Builds a window's handler on the owner thread, for a window opened from
/// another thread ([`PlatformProxy::open_window`]): the handler is an `Rc`,
/// which cannot cross threads, so the request carries what builds it.
pub type WindowHandlerFactory = Box<dyn FnOnce() -> Rc<dyn WindowHandler> + Send>;

/// A window opened from another thread: its options, and what builds its
/// handler on the owner thread. A refused request hands both back.
pub struct WindowRequest {
    /// The window's options.
    pub options: WindowOptions,
    /// Builds the window's handler, once, on the owner thread.
    pub handler: WindowHandlerFactory,
}

impl WindowRequest {
    /// A request for a window with `options`, whose handler `handler` builds
    /// on the owner thread.
    #[must_use]
    pub fn new(
        options: WindowOptions,
        handler: impl FnOnce() -> Rc<dyn WindowHandler> + Send + 'static,
    ) -> Self {
        Self {
            options,
            handler: Box::new(handler),
        }
    }
}

impl WindowRequest {
    /// Run the factory, once, on the owner thread: the window's options and
    /// its handler. The factory is application code: its panic is contained
    /// here (logged, its payload retained) and the open fails with what it
    /// said, so a loop draining requests carries on.
    #[cfg_attr(
        not(all(
            feature = "winit-backend",
            any(target_os = "windows", target_os = "macos", target_os = "linux")
        )),
        expect(dead_code, reason = "only the winit lane carries deferred opens")
    )]
    pub(crate) fn build_on_owner(
        self,
    ) -> Result<(WindowOptions, Rc<dyn WindowHandler>), OpenWindowError> {
        let Self { options, handler } = self;
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(handler)) {
            Ok(handler) => Ok((options, handler)),
            Err(payload) => {
                let message = format!(
                    "the window's handler factory panicked: {}",
                    flui_foundation::panic::payload_text(&*payload)
                        .unwrap_or("non-string panic payload")
                );
                panic_boundary::contain_owner_callback(|| tracing::error!("{message}"));
                flui_foundation::panic::retain_opaque_payload(payload);
                Err(OpenWindowError::Backend { message })
            }
        }
    }
}

impl fmt::Debug for WindowRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WindowRequest")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

/// Cross-thread request capability. `Clone + Send + Sync`. Enqueue-and-wake
/// with bounded backpressure; never blocks the sender. The one closure it
/// carries is a window's handler factory (ADR-0254 §4), run once on the
/// owner; every other request is a closed vocabulary (ADR-0037 §3).
#[derive(Clone)]
pub struct PlatformProxy {
    transport: Arc<dyn ProxyTransport>,
}

impl fmt::Debug for PlatformProxy {
    // Manual impl: `transport` is `Arc<dyn ProxyTransport>`, and trait
    // objects carry no blanket `Debug` impl.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PlatformProxy")
            .field("is_owner_thread", &self.is_owner_thread())
            .finish_non_exhaustive()
    }
}

impl PlatformProxy {
    pub(crate) fn new(transport: Arc<dyn ProxyTransport>) -> Self {
        Self { transport }
    }

    /// Request a coalesced owner turn without requiring a window.
    /// # Errors
    /// Returns a closed/unsupported transport or native wake-posting failure.
    pub fn wake(&self) -> Result<(), ProxySendError<()>> {
        self.transport.wake()
    }

    /// Enqueues a window-open request. Never blocks — on any thread,
    /// including the owner (deferral replaces the old owner-side refusal;
    /// the blocking hazard lives in [`PendingWindow::wait`], which is where
    /// it is refused). The request's handler factory runs once, on the owner,
    /// right before the window is built. Fails fast with the rejected request
    /// returned so the producer can retry without rebuilding it.
    ///
    /// # Errors
    /// See [`ProxySendError`].
    #[expect(
        clippy::result_large_err,
        reason = "the rejection hands the caller's request back for a retry; a window open is not a hot path"
    )]
    pub fn open_window(
        &self,
        request: WindowRequest,
    ) -> Result<PendingWindow, ProxySendError<WindowRequest>> {
        self.transport.open_window(request)
    }

    /// Coalesced quit signal, independent of window-command queue capacity.
    /// Immediately fences new wake/window admission. A failed native post may
    /// be retried; admission remains fenced until the owner stops.
    ///
    /// # Errors
    /// `Unsupported` on mobile/web transports, `OwnerGone` after shutdown,
    /// or `WakeFailed` when physical notification fails. Native desktop and
    /// headless implement this independently of proxy window creation support.
    pub fn request_quit(&self) -> Result<(), ProxySendError<()>> {
        self.transport.request_quit()
    }

    /// True iff the calling thread is the event-loop owner. Diagnostic
    /// only — correctness never depends on it (the types carry the
    /// guarantee).
    #[must_use]
    pub fn is_owner_thread(&self) -> bool {
        self.transport.owner_thread() == std::thread::current().id()
    }
}

assert_impl_all!(PlatformProxy: Clone, Send, Sync);

/// Failure to install a loop-owned wake callback.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum WakeRegistrationError {
    /// This backend does not implement owner turns.
    #[error("owner wake callbacks are unsupported on this backend")]
    Unsupported,
    /// The loop has stopped accepting owner work.
    #[error("event-loop owner is gone")]
    OwnerGone,
}

/// Failure to enqueue a cross-thread request through [`PlatformProxy`].
///
/// Exhaustive: a deliberately closed cross-thread vocabulary (ADR-0027 §4,
/// ADR-0037 §3) — `Unsupported` completes it (ADR-0039 §3
/// revision). Support is operation-specific; absent window creation does not
/// imply absent wake or quit transport.
#[derive(Debug, thiserror::Error)]
pub enum ProxySendError<T: fmt::Debug> {
    /// Native wake submission failed while admitting this request.
    #[error("native owner wake failed: {source}")]
    WakeFailed {
        /// The request whose wake could not be submitted.
        rejected: T,
        /// The backend's posting failure.
        #[source]
        source: PlatformError,
    },
    /// The owner lane is at capacity.
    #[error("platform owner lane is full (capacity {capacity})")]
    Full {
        /// The lane's fixed capacity.
        capacity: usize,
        /// The value that could not be enqueued.
        rejected: T,
    },
    /// The event-loop owner is gone: a lane existed and its loop has since
    /// died. Strictly transient-loop-death, never "no lane at all" — see
    /// [`Unsupported`](Self::Unsupported) for that case.
    #[error("event-loop owner is gone")]
    OwnerGone {
        /// The value that could not be delivered.
        rejected: T,
    },
    /// This backend has no owner lane behind [`PlatformProxy`] at all —
    /// not "the queue is full", not "the loop died", but "cross-thread
    /// platform requests are not implemented here". **Permanent** on
    /// windows/macos/web/android/headless until those backends adopt the owner lane (ADR-
    /// 0039 slice-2 amendment b) — do not retry; this is not a transient
    /// condition. Treat it as a standing capability signal, the same way a
    /// missing OS feature would be reported, not a request to back off and
    /// try again.
    #[error("cross-thread platform requests are unsupported on this backend")]
    Unsupported {
        /// The value that could not be enqueued or delivered.
        rejected: T,
    },
}

// ============================================================================
// PendingWindow
// ============================================================================

/// A window a deferred open built (ADR-0254 §4): its identity, which any
/// thread may hold, and the window itself, which only the owner thread
/// reaches.
///
/// The window is not `Send`, so a thread that is not the owner never holds
/// it: what it gets is the [`WindowId`]. The owner's host keeps the window
/// tracked until it closes; this holds it weakly, so a value dropped on
/// another thread keeps no window alive.
pub struct OpenedWindow {
    id: WindowId,
    window: slot::OwnerWeak<dyn HostWindow>,
}

impl OpenedWindow {
    /// The opened window, which `window` names on the owner thread. Built
    /// there.
    pub(crate) fn new(window: &Rc<dyn HostWindow>) -> Self {
        Self {
            id: window.id(),
            window: slot::OwnerWeak::new(window),
        }
    }

    /// The window's identity.
    #[must_use]
    pub fn id(&self) -> WindowId {
        self.id
    }

    /// The window, on the owner thread while it is still open.
    #[must_use]
    pub fn window(&self) -> OpenedWindowReach {
        if !self.window.is_owner_thread() {
            return OpenedWindowReach::OtherThread;
        }
        self.window
            .upgrade()
            .map_or(OpenedWindowReach::Closed, OpenedWindowReach::Open)
    }
}

/// What [`OpenedWindow::window`] reaches.
pub enum OpenedWindowReach {
    /// The window, which is open: the caller is on its owner thread.
    Open(Rc<dyn HostWindow>),
    /// The window closed after it was opened.
    Closed,
    /// The caller is not the window's owner thread, which alone reaches it:
    /// what another thread has of the window is its identity.
    OtherThread,
}

impl fmt::Debug for OpenedWindowReach {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(window) => f.debug_tuple("Open").field(&window.id()).finish(),
            Self::Closed => f.write_str("Closed"),
            Self::OtherThread => f.write_str("OtherThread"),
        }
    }
}

impl fmt::Debug for OpenedWindow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenedWindow")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

assert_impl_all!(OpenedWindow: Send, Sync);

/// A deferred window-open request in flight on the owner lane.
///
/// Dropping a `PendingWindow` disclaims the request: the owner skips
/// creation (if it has not started) or unwinds the created window (if
/// delivery already landed) — see `flui-foundation`'s `ClaimSlot` module
/// docs for the full state machine this wraps.
#[must_use = "dropping a PendingWindow disclaims the request; the owner \
              skips or unwinds the window"]
pub struct PendingWindow {
    handle: ClaimHandle<Result<OpenedWindow, OpenWindowError>>,
    owner_thread: ThreadId,
}

impl fmt::Debug for PendingWindow {
    // Manual impl: the wrapped `ClaimHandle` carries a wake callback with
    // no blanket `Debug` impl.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PendingWindow").finish_non_exhaustive()
    }
}

impl PendingWindow {
    // Only the winit backend's deferred owner-lane path ever constructs a
    // `PendingWindow` (`WinitOwnerHooks::open_owner_window` and
    // `WinitProxyTransport::open_window`, `owner/winit.rs`) —
    // every lane-less backend (`DirectOwnerHooks`) always resolves `Ready`.
    // Building this crate without the `winit-backend` feature (e.g. the
    // headless-only default) makes this constructor genuinely unused.
    #[cfg_attr(not(feature = "winit-backend"), allow(dead_code))]
    pub(crate) fn new(
        handle: ClaimHandle<Result<OpenedWindow, OpenWindowError>>,
        owner_thread: ThreadId,
    ) -> Self {
        Self {
            handle,
            owner_thread,
        }
    }

    /// Blocking wait — worker threads only — for the identity of the opened
    /// window; the window itself stays on the owner (ADR-0254 §4). On the
    /// owner thread this
    /// returns [`WaitError::WouldBlockOwner`] instead of deadlocking on the
    /// lane the caller itself drains.
    ///
    /// # Errors
    /// See [`WaitError`]. Notably [`WaitError::AlreadyClaimed`] if an
    /// earlier [`try_take`](Self::try_take) on this same `PendingWindow`
    /// already claimed the result — `try_take` takes `&mut self`, so
    /// nothing stops a caller from following a successful `try_take` with a
    /// `wait` on the same handle; there is nothing left to wait for.
    pub fn wait(self) -> Result<WindowId, WaitError> {
        if self.owner_thread == std::thread::current().id() {
            return Err(WaitError::WouldBlockOwner(self));
        }
        match self.handle.wait() {
            ClaimOutcome::Delivered(result) => {
                result.map(|opened| opened.id()).map_err(WaitError::Open)
            }
            ClaimOutcome::AlreadyClaimed => Err(WaitError::AlreadyClaimed),
            ClaimOutcome::OwnerGone => Err(WaitError::Open(OpenWindowError::OwnerGone {
                rejected: None,
            })),
        }
    }

    /// Non-blocking poll; safe on any thread.
    #[must_use = "discarding Some(_) strands whatever the owner delivered \
                  (a live window, or the typed error explaining why not)"]
    pub fn try_take(&mut self) -> Option<Result<OpenedWindow, OpenWindowError>> {
        self.handle.try_take()
    }
}

impl std::future::Future for PendingWindow {
    type Output = Result<OpenedWindow, OpenWindowError>;

    /// Non-blocking poll — safe on any thread, including the owner (unlike
    /// [`wait`](Self::wait), which refuses there because it would block on
    /// the very lane the owner itself drains). Intended usage: poll from
    /// `Idle` (or drive this future via the framework's async driver);
    /// never from inside a pipeline hot path (build/layout/paint/
    /// composite) — the same fence every other owner-thread capability in
    /// this crate observes.
    ///
    /// Resolves on delivery, on owner disconnection
    /// ([`OpenWindowError::OwnerGone`], woken by the underlying
    /// `flui-foundation` `ClaimSlot`'s `Drop`), or immediately if this
    /// `PendingWindow` was already resolved by an earlier
    /// [`try_take`](Self::try_take) call
    /// ([`OpenWindowError::AlreadyClaimed`] — a typed caller-sequencing
    /// fact, not a fabricated backend failure: there is nothing left to
    /// hand back, and this is not the backend's fault).
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        match std::future::Future::poll(std::pin::Pin::new(&mut self.handle), cx) {
            std::task::Poll::Ready(ClaimOutcome::Delivered(result)) => {
                std::task::Poll::Ready(result)
            }
            std::task::Poll::Ready(ClaimOutcome::OwnerGone) => {
                std::task::Poll::Ready(Err(OpenWindowError::OwnerGone { rejected: None }))
            }
            std::task::Poll::Ready(ClaimOutcome::AlreadyClaimed) => {
                std::task::Poll::Ready(Err(OpenWindowError::AlreadyClaimed))
            }
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

assert_impl_all!(PendingWindow: Send);

/// Failure to wait on a [`PendingWindow`].
///
/// `#[non_exhaustive]`: growth pressure via the `#[from] OpenWindowError`
/// arm and possible future wait modes.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum WaitError {
    /// Waiting on the owner thread would deadlock the lane it drains. The
    /// handle is returned so the caller can still poll with `try_take`.
    #[error(
        "waiting on the owner thread would deadlock the lane it drains; poll with try_take instead"
    )]
    WouldBlockOwner(PendingWindow),
    /// The request itself failed.
    #[error(transparent)]
    Open(#[from] OpenWindowError),
    /// An earlier `try_take` on this same `PendingWindow` already claimed
    /// the result — there is nothing left for `wait` to return. Not a
    /// slot-invariant violation, just a caller-sequencing fact: `try_take`
    /// takes `&mut self`, so nothing stops a caller from following a
    /// successful poll with a `wait` call on the same handle.
    #[error("the pending window was already claimed by an earlier try_take call")]
    AlreadyClaimed,
}

// ============================================================================
// pub(crate) seams: the slice-3 `OwnerOps` seed
// ============================================================================

/// Backend-specific window-open dispatch behind [`OwnerPlatform`]. The
/// slice-3 `OwnerOps` seed (ADR-0039 §2/§8) — stays `pub(crate)` until then,
/// alongside [`OwnerPlatform`]'s own third private field.
pub(crate) trait OwnerHooks: Send + Sync {
    fn on_wake(&self, callback: Box<dyn FnMut() + Send>) -> Result<(), WakeRegistrationError> {
        drop(callback);
        Err(WakeRegistrationError::Unsupported)
    }

    /// Creates or enqueues a window from the owner thread. Backends with an
    /// owner lane (winit) enqueue when called outside `on_ready`; every
    /// other backend creates directly and always returns `Ready`.
    fn open_owner_window(
        &self,
        options: WindowOptions,
        handler: Rc<dyn WindowHandler>,
    ) -> Result<WindowOpen, OpenWindowError>;

    /// The cross-thread transport backing [`PlatformProxy`]. Operation support
    /// is independent: a signal-only transport need not support window creation.
    fn transport(&self) -> Arc<dyn ProxyTransport>;
}

/// Direct-creation [`OwnerHooks`] for backends without an owner lane
/// (windows/macos/headless/web/android): every `open_owner_window` call
/// creates synchronously via the wrapped [`Platform`] and is always
/// `Ready` — there is no deferral without a lane to defer onto.
/// [`Platform::open_window`] itself returns the typed [`OpenWindowError`]
/// taxonomy, so its failure passes through unmapped.
pub(crate) struct DirectOwnerHooks {
    signal: Arc<OwnerSignal>,
    platform: Arc<dyn Platform>,
}

impl DirectOwnerHooks {
    /// Captures the calling thread as the permanent owner — call this from
    /// the backend's `on_ready` (or wherever it mints its `OwnerPlatform`).
    pub(crate) fn with_signal(platform: Arc<dyn Platform>, signal: Arc<OwnerSignal>) -> Self {
        Self { signal, platform }
    }
}

impl OwnerHooks for DirectOwnerHooks {
    fn on_wake(&self, callback: Box<dyn FnMut() + Send>) -> Result<(), WakeRegistrationError> {
        self.signal.register(callback)
    }

    fn open_owner_window(
        &self,
        options: WindowOptions,
        handler: Rc<dyn WindowHandler>,
    ) -> Result<WindowOpen, OpenWindowError> {
        if !self.signal.accepting() {
            return Err(OpenWindowError::OwnerGone {
                rejected: Some(Box::new(options)),
            });
        }
        self.platform
            .open_window(options, handler)
            .map(WindowOpen::Ready)
    }

    fn transport(&self) -> Arc<dyn ProxyTransport> {
        Arc::new(SignalTransport::new(&self.signal))
    }
}

/// Cross-thread transport behind [`PlatformProxy`]. Winit supports window
/// requests and signals; other desktop/headless backends support signals only.
/// Direct backends support signals while refusing deferred window creation.
pub(crate) trait ProxyTransport: Send + Sync {
    fn wake(&self) -> Result<(), ProxySendError<()>> {
        Err(ProxySendError::Unsupported { rejected: () })
    }

    /// Enqueues a window-open request from a worker thread.
    #[expect(
        clippy::result_large_err,
        reason = "the rejection hands the caller's request back for a retry; a window open is not a hot path"
    )]
    fn open_window(
        &self,
        request: WindowRequest,
    ) -> Result<PendingWindow, ProxySendError<WindowRequest>>;

    /// Requests application quit — coalesced, non-starvable.
    ///
    /// # Errors
    /// See [`ProxySendError`]. [`ProxySendError::Unsupported`] on a
    /// lane-less backend is **permanent** — not a transient condition.
    fn request_quit(&self) -> Result<(), ProxySendError<()>>;

    /// The thread identity of the event-loop owner (diagnostic only).
    fn owner_thread(&self) -> ThreadId;
}
