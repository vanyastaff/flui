//! The owner-thread platform capability (ADR-0039).
//!
//! [`OwnerPlatform`] carries every `Platform` operation that is owner-affine
//! on at least one real OS: window creation, display queries, activation,
//! appearance/keyboard queries, and quit. It is minted only by a backend, on
//! the thread that owns — or, before the loop starts, will own — its event
//! loop, and handed to [`super::PlatformReadyCallback`]. `!Send + !Sync` by
//! phantom marker: possession proves the thread, so a wrong-thread call
//! becomes a compile error instead of a runtime assert (the [`OwnerAffinity`]
//! backstop `flui-foundation` carries).
//!
//! [`OwnerAffinity`]: flui_foundation::OwnerAffinity
//!
//! Not [`Clone`] (ADR-0039 slice-2 decision record): the sanctioned way to
//! hold this across owner-thread callbacks is `flui-app`'s loop-scoped
//! `OWNER_PLATFORM_HOST` TLS slot, read through its `with_owner_platform`
//! borrow-style accessor — never a durable owned copy squirreled away
//! outside that fenced accessor. [`PlatformProxy`] is how a *worker* thread
//! reaches the owner; it is `Clone` by design, since it carries no
//! thread-affine capability, only a bounded, typed, cross-thread request
//! channel.
//!
//! `Platform` becomes de-facto crate-sealed by this module: minting an
//! `OwnerPlatform` requires the `pub(crate)` constructor here, so an
//! out-of-crate `impl Platform` cannot hand its own `run()` the capability
//! `on_ready` needs. An external-embedder minting seam is design work
//! tracked separately (#560); until then, backend implementations of
//! `Platform` live in this crate.

use std::fmt;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::ThreadId;

use flui_foundation::{ClaimHandle, ClaimOutcome};
use static_assertions::{assert_impl_all, assert_not_impl_any};

use super::{
    Clipboard, ClipboardItem, PathPromptOptions, Platform, PlatformCapabilities, PlatformDisplay,
    PlatformExecutor, PlatformWindow, WindowEvent, WindowId, WindowOptions,
    window::WindowAppearance,
};
use crate::data_transfer::DataTransferSource;
use crate::task::Task;

// ============================================================================
// OwnerPlatform
// ============================================================================

/// Owner-thread platform capability. See the module docs for the full
/// contract.
///
/// ```compile_fail,E0277
/// // Illustration only (ALT-2): NOT cited as registry evidence — the
/// // item-position `assert_not_impl_any!` below is. This doctest is
/// // excluded from every CI gate (`justfile:177` excludes flui-platform's
/// // doc tests) and is run locally via `cargo test -p flui-platform --doc`.
/// fn assert_send<T: Send>() {}
/// assert_send::<flui_platform::OwnerPlatform>();
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

    /// Open a window. `Ready` is guaranteed inside `on_ready`; afterwards
    /// the backend may defer through its owner lane (where one exists),
    /// returning `Pending`.
    ///
    /// # Errors
    /// See [`OpenWindowError`].
    pub fn open_window(&self, options: WindowOptions) -> Result<WindowOpen, OpenWindowError> {
        self.hooks.open_owner_window(options)
    }

    /// The currently active (focused) window, if any.
    #[must_use]
    pub fn active_window(&self) -> Option<WindowId> {
        self.platform.active_window()
    }

    /// All available displays (monitors).
    #[must_use]
    pub fn displays(&self) -> Vec<Arc<dyn PlatformDisplay>> {
        self.platform.displays()
    }

    /// The primary display.
    #[must_use]
    pub fn primary_display(&self) -> Option<Arc<dyn PlatformDisplay>> {
        self.platform.primary_display()
    }

    /// Activate the application (bring to front).
    pub fn activate(&self, ignoring_other_apps: bool) {
        self.platform.activate(ignoring_other_apps);
    }

    /// The system window appearance (light/dark theme).
    #[must_use]
    pub fn window_appearance(&self) -> WindowAppearance {
        self.platform.window_appearance()
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
    /// `clipboard`, callback registration, and the rest of §2's "stays on
    /// `Platform`" list, wrapped in a `Clone + Send + Sync` handle a
    /// `std::thread::scope` worker can freely hold — see
    /// [`SharedPlatform`]'s own doc for why this can no longer be
    /// `&dyn Platform` (that type is still `Send + Sync` in full,
    /// owner-affine methods included, until slice 3's trait split).
    #[must_use]
    pub fn shared(&self) -> SharedPlatform {
        SharedPlatform::new(Arc::clone(&self.platform))
    }

    /// Mints a cross-thread capability, handed to workers, realms, or
    /// tasks that need to reach the owner from off-thread.
    #[must_use]
    pub fn proxy(&self) -> PlatformProxy {
        PlatformProxy::new(self.hooks.transport())
    }
}

// Sole registry evidence for the "wrong-thread owner ops are compile
// errors" acceptance criterion (ALT-2) — expanded by every `cargo check`,
// including cross-typecheck on win/mac, because `static_assertions` is a
// real (non-dev) dependency (see Cargo.toml).
assert_not_impl_any!(OwnerPlatform: Send, Sync);

// ============================================================================
// SharedPlatform
// ============================================================================

/// The thread-safe residual of [`Platform`] — every operation NOT gated on
/// owner-thread affinity, per ADR-0039 §2's "what stays" table.
///
/// `Clone + Send + Sync`: unlike `&dyn Platform` (which is `Platform: Send +
/// Sync` in full, `open_window`/`quit`/`activate`/`displays`/
/// `primary_display`/`window_appearance`/`keyboard_layout`/`active_window`
/// included), a `SharedPlatform` genuinely can cross a `std::thread::scope`
/// boundary into safe code without smuggling an owner-affine call along
/// with it — **the method list below IS the fence**: every method on this
/// type is safe to call from any thread, and no owner-affine method is
/// ever added to it. Minted only by [`OwnerPlatform::shared`].
///
/// The registry (`docs/runtime-contract.toml`) tracks this as the
/// compile-time-checked half of the owner-platform-capability contract;
/// `Platform` itself (the trait `dyn` object underneath) remains
/// runtime-checked (`OwnerAffinity` debug-asserts) until slice 3 splits its
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

    /// The platform's capabilities descriptor.
    #[must_use]
    pub fn capabilities(&self) -> &dyn PlatformCapabilities {
        self.platform.capabilities()
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
    /// Propagates the backend's own lookup failure.
    pub fn app_path(&self) -> anyhow::Result<PathBuf> {
        self.platform.app_path()
    }

    /// The platform's clipboard interface.
    #[must_use]
    pub fn clipboard(&self) -> Arc<dyn Clipboard> {
        self.platform.clipboard()
    }

    /// The data-transfer transport (ADR-0038).
    #[must_use]
    pub fn data_transfer(&self) -> Arc<dyn DataTransferSource> {
        self.platform.data_transfer()
    }

    /// Registers a callback for when the application should quit.
    pub fn on_quit(&self, callback: Box<dyn FnMut() + Send>) {
        self.platform.on_quit(callback);
    }

    /// Registers a callback for when the application is reopened (macOS).
    pub fn on_reopen(&self, callback: Box<dyn FnMut() + Send>) {
        self.platform.on_reopen(callback);
    }

    /// Registers a callback for window events.
    pub fn on_window_event(&self, callback: Box<dyn FnMut(WindowEvent) + Send>) {
        self.platform.on_window_event(callback);
    }

    /// Registers a callback for URLs opened by the system.
    pub fn on_open_urls(&self, callback: Box<dyn FnMut(Vec<String>) + Send>) {
        self.platform.on_open_urls(callback);
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
    /// `None` if the user cancelled. Runs asynchronously on a background
    /// thread.
    pub fn prompt_for_paths(
        &self,
        options: PathPromptOptions,
    ) -> Task<anyhow::Result<Option<Vec<PathBuf>>>> {
        self.platform.prompt_for_paths(options)
    }

    /// Shows a "Save As" dialog for selecting a new file path. Returns the
    /// selected path, or `None` if the user cancelled.
    pub fn prompt_for_new_path(
        &self,
        directory: &Path,
        suggested_name: Option<&str>,
    ) -> Task<anyhow::Result<Option<PathBuf>>> {
        self.platform.prompt_for_new_path(directory, suggested_name)
    }

    /// Writes a rich clipboard item (text + metadata).
    pub fn write_to_clipboard(&self, item: ClipboardItem) {
        self.platform.write_to_clipboard(item);
    }

    /// Reads a rich clipboard item.
    #[must_use]
    pub fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        self.platform.read_from_clipboard()
    }
}

// Sole registry evidence that `SharedPlatform` genuinely is the thread-safe
// escape hatch its doc promises — expanded by every `cargo check`, same
// discipline as `OwnerPlatform`'s `assert_not_impl_any!` above.
assert_impl_all!(SharedPlatform: Send, Sync, Clone);

/// Result of an owner-thread window-open request.
#[must_use = "a Pending window creation must be waited on or polled; \
              dropping it abandons the request (the owner unwinds or skips)"]
pub enum WindowOpen {
    /// Created synchronously. Always the case inside `on_ready`, and on
    /// backends whose owner thread may create directly outside callbacks.
    Ready(Arc<dyn PlatformWindow>),
    /// Enqueued on the owner lane; resolves at the loop's next drain
    /// anchor.
    Pending(PendingWindow),
}

impl fmt::Debug for WindowOpen {
    // Manual impl: `PlatformWindow` carries no blanket `Debug`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ready(_) => f
                .debug_tuple("Ready")
                .field(&"<dyn PlatformWindow>")
                .finish(),
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
    pub fn try_ready(self) -> Result<Arc<dyn PlatformWindow>, OpenWindowError> {
        match self {
            Self::Ready(window) => Ok(window),
            Self::Pending(pending) => Err(OpenWindowError::NotReady(pending)),
        }
    }
}

/// Failure to open a window through [`OwnerPlatform`] or [`PlatformProxy`].
///
/// `#[non_exhaustive]`: the ADR forecasts variant growth when slice 3's
/// moved methods adopt this typed taxonomy in place of today's `anyhow`
/// surface.
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
        rejected: WindowOptions,
    },
    /// The event-loop owner is gone. `rejected` is `Some` when refusal
    /// happens at enqueue (the producer can retry without rebuilding
    /// options) and `None` when the loop died after the request was
    /// already accepted.
    #[error("event-loop owner is gone")]
    OwnerGone {
        /// The options that could not be delivered, if known at refusal.
        rejected: Option<WindowOptions>,
    },
    /// The backend could not create the window.
    #[error("the backend could not create the window: {message}")]
    Backend {
        /// The backend's own error message.
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

/// Cross-thread request capability. `Clone + Send + Sync`. Enqueue-and-wake
/// with bounded backpressure; never blocks the sender; never carries
/// closures (ADR-0037 §3 closed vocabulary).
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

    /// Enqueues a window-open request. Never blocks — on any thread,
    /// including the owner (deferral replaces the old owner-side refusal;
    /// the blocking hazard lives in [`PendingWindow::wait`], which is where
    /// it is refused). Fails fast with the rejected options returned so the
    /// producer can retry without rebuilding them.
    ///
    /// # Errors
    /// See [`ProxySendError`].
    pub fn open_window(
        &self,
        options: WindowOptions,
    ) -> Result<PendingWindow, ProxySendError<WindowOptions>> {
        self.transport.open_window(options)
    }

    /// Coalesced, non-starvable quit flag — bypasses queue capacity.
    ///
    /// The `Result` return is already `#[must_use]` (clippy's
    /// `double_must_use` rejects a redundant fn-level attribute on top of
    /// it) — discarding it silently swallows exactly the
    /// permanent-`Unsupported` signal below.
    ///
    /// # Errors
    /// See [`ProxySendError`]. [`ProxySendError::Unsupported`] is
    /// **permanent** on lane-less backends (windows/macos/web/android/
    /// headless) until slice 3 lane adoption — not a transient condition;
    /// do not retry.
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

/// Failure to enqueue a cross-thread request through [`PlatformProxy`].
///
/// Exhaustive: a deliberately closed cross-thread vocabulary (ADR-0027 §4,
/// ADR-0037 §3) — `Unsupported` completes it (ADR-0039 slice-2 amendment b
/// revision) rather than reopening it; every lane-less backend today maps
/// onto this one variant instead of overloading `OwnerGone`.
#[derive(Debug, thiserror::Error)]
pub enum ProxySendError<T: fmt::Debug> {
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
    /// windows/macos/web/android/headless until slice 3 lane adoption (ADR-
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

/// A deferred window-open request in flight on the owner lane.
///
/// Dropping a `PendingWindow` disclaims the request: the owner skips
/// creation (if it has not started) or unwinds the created window (if
/// delivery already landed) — see `flui-foundation`'s `ClaimSlot` module
/// docs for the full state machine this wraps.
#[must_use = "dropping a PendingWindow disclaims the request; the owner \
              skips or unwinds the window"]
pub struct PendingWindow {
    handle: ClaimHandle<Result<Arc<dyn PlatformWindow>, OpenWindowError>>,
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
    // `WinitProxyTransport::open_window`, `platforms/winit/platform.rs`) —
    // every lane-less backend (`DirectOwnerHooks`) always resolves `Ready`.
    // Building this crate without the `winit-backend` feature (e.g. the
    // headless-only default) makes this constructor genuinely unused.
    #[cfg_attr(not(feature = "winit-backend"), allow(dead_code))]
    pub(crate) fn new(
        handle: ClaimHandle<Result<Arc<dyn PlatformWindow>, OpenWindowError>>,
        owner_thread: ThreadId,
    ) -> Self {
        Self {
            handle,
            owner_thread,
        }
    }

    /// Blocking wait — worker threads only. On the owner thread this
    /// returns [`WaitError::WouldBlockOwner`] instead of deadlocking on the
    /// lane the caller itself drains.
    ///
    /// # Errors
    /// See [`WaitError`]. Notably [`WaitError::AlreadyClaimed`] if an
    /// earlier [`try_take`](Self::try_take) on this same `PendingWindow`
    /// already claimed the result — `try_take` takes `&mut self`, so
    /// nothing stops a caller from following a successful `try_take` with a
    /// `wait` on the same handle; there is nothing left to wait for.
    pub fn wait(self) -> Result<Arc<dyn PlatformWindow>, WaitError> {
        if self.owner_thread == std::thread::current().id() {
            return Err(WaitError::WouldBlockOwner(self));
        }
        match self.handle.wait() {
            ClaimOutcome::Delivered(result) => result.map_err(WaitError::Open),
            ClaimOutcome::AlreadyClaimed => Err(WaitError::AlreadyClaimed),
            ClaimOutcome::OwnerGone => Err(WaitError::Open(OpenWindowError::OwnerGone {
                rejected: None,
            })),
        }
    }

    /// Non-blocking poll; safe on any thread.
    #[must_use = "discarding Some(_) strands whatever the owner delivered \
                  (a live window, or the typed error explaining why not)"]
    pub fn try_take(&mut self) -> Option<Result<Arc<dyn PlatformWindow>, OpenWindowError>> {
        self.handle.try_take()
    }
}

impl std::future::Future for PendingWindow {
    type Output = Result<Arc<dyn PlatformWindow>, OpenWindowError>;

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
    /// Creates or enqueues a window from the owner thread. Backends with an
    /// owner lane (winit) enqueue when called outside `on_ready`; every
    /// other backend creates directly and always returns `Ready`.
    fn open_owner_window(&self, options: WindowOptions) -> Result<WindowOpen, OpenWindowError>;

    /// The cross-thread transport backing [`PlatformProxy`]. Backends
    /// without an owner lane return [`ClosedTransport`] — permanently,
    /// until slice 3 gives them one (ADR amendment b).
    fn transport(&self) -> Arc<dyn ProxyTransport>;
}

/// Direct-creation [`OwnerHooks`] for backends without an owner lane
/// (windows/macos/headless/web/android): every `open_owner_window` call
/// creates synchronously via the wrapped [`Platform`] and is always
/// `Ready` — there is no deferral without a lane to defer onto. The
/// backend's own `anyhow` error maps onto the typed
/// [`OpenWindowError::Backend`] arm; the trait method itself keeps its
/// untyped `anyhow` signature until slice 3.
pub(crate) struct DirectOwnerHooks {
    platform: Arc<dyn Platform>,
    owner_thread: ThreadId,
}

impl DirectOwnerHooks {
    /// Captures the calling thread as the permanent owner — call this from
    /// the backend's `on_ready` (or wherever it mints its `OwnerPlatform`).
    pub(crate) fn new(platform: Arc<dyn Platform>) -> Self {
        Self {
            platform,
            owner_thread: std::thread::current().id(),
        }
    }
}

impl OwnerHooks for DirectOwnerHooks {
    fn open_owner_window(&self, options: WindowOptions) -> Result<WindowOpen, OpenWindowError> {
        self.platform
            .open_window(options)
            .map(WindowOpen::Ready)
            .map_err(|error| OpenWindowError::Backend {
                message: error.to_string(),
            })
    }

    fn transport(&self) -> Arc<dyn ProxyTransport> {
        Arc::new(ClosedTransport::new(self.owner_thread))
    }
}

/// Cross-thread transport behind [`PlatformProxy`]. Backends with an owner
/// lane (winit) implement this over their lane; lane-less backends use
/// [`ClosedTransport`].
pub(crate) trait ProxyTransport: Send + Sync {
    /// Enqueues a window-open request from a worker thread.
    fn open_window(
        &self,
        options: WindowOptions,
    ) -> Result<PendingWindow, ProxySendError<WindowOptions>>;

    /// Requests application quit — coalesced, non-starvable.
    ///
    /// # Errors
    /// See [`ProxySendError`]. [`ProxySendError::Unsupported`] on a
    /// lane-less backend is **permanent** — not a transient condition.
    fn request_quit(&self) -> Result<(), ProxySendError<()>>;

    /// The thread identity of the event-loop owner (diagnostic only).
    fn owner_thread(&self) -> ThreadId;
}

/// A transport with no lane behind it: every request is refused with
/// [`ProxySendError::Unsupported`] (ADR-0039 slice-2 amendment b) until
/// slice 3 gives the backend a real lane — permanently, not
/// `OwnerGone`: no lane ever existed here to die.
pub(crate) struct ClosedTransport {
    owner_thread: ThreadId,
}

impl ClosedTransport {
    pub(crate) fn new(owner_thread: ThreadId) -> Self {
        Self { owner_thread }
    }
}

impl ProxyTransport for ClosedTransport {
    fn open_window(
        &self,
        options: WindowOptions,
    ) -> Result<PendingWindow, ProxySendError<WindowOptions>> {
        // `debug!`, not `warn!` (this backend's posture is permanent and
        // known at compile time, not an anomaly worth surfacing by
        // default) — a caller probing/retrying `PlatformProxy::open_window`
        // on a lane-less backend would otherwise flood a `warn!` per
        // attempt.
        tracing::debug!(
            "PlatformProxy::open_window on a lane-less backend: permanently \
             unsupported until slice 3 (ADR-0039 amendment b) — do not retry"
        );
        Err(ProxySendError::Unsupported { rejected: options })
    }

    fn request_quit(&self) -> Result<(), ProxySendError<()>> {
        // See `open_window`'s identical `debug!`-not-`warn!` rationale.
        tracing::debug!(
            "PlatformProxy::request_quit on a lane-less backend: permanently \
             unsupported until slice 3 (ADR-0039 amendment b)"
        );
        Err(ProxySendError::Unsupported { rejected: () })
    }

    fn owner_thread(&self) -> ThreadId {
        self.owner_thread
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;

    use flui_foundation::claim_slot;

    use super::*;

    #[test]
    fn try_ready_on_pending_yields_not_ready() {
        let (_slot, handle) =
            claim_slot::<Result<Arc<dyn PlatformWindow>, OpenWindowError>>(Arc::new(|| {}));
        let pending = PendingWindow::new(handle, thread::current().id());
        let open = WindowOpen::Pending(pending);

        match open.try_ready() {
            Err(OpenWindowError::NotReady(_)) => {}
            Err(other) => panic!("wrong error variant: {other:?}"),
            Ok(_) => panic!("Pending must not resolve to Ready"),
        }
    }

    #[test]
    fn wait_on_owner_thread_refuses_with_the_handle_back() {
        let (_slot, handle) =
            claim_slot::<Result<Arc<dyn PlatformWindow>, OpenWindowError>>(Arc::new(|| {}));
        let pending = PendingWindow::new(handle, thread::current().id());

        match pending.wait() {
            Err(WaitError::WouldBlockOwner(_returned)) => {}
            Err(other) => panic!("wrong error variant: {other:?}"),
            Ok(_) => panic!("owner-thread wait must not succeed"),
        }
    }

    #[test]
    fn closed_transport_open_window_is_permanently_unsupported() {
        let transport = ClosedTransport::new(thread::current().id());
        let error = transport
            .open_window(WindowOptions::default())
            .expect_err("no lane behind a ClosedTransport");
        assert!(matches!(error, ProxySendError::Unsupported { .. }));
    }

    #[test]
    fn closed_transport_request_quit_is_permanently_unsupported() {
        let transport = ClosedTransport::new(thread::current().id());
        let error = transport
            .request_quit()
            .expect_err("no lane behind a ClosedTransport");
        assert!(matches!(
            error,
            ProxySendError::Unsupported { rejected: () }
        ));
    }

    #[test]
    fn proxy_request_quit_surfaces_unsupported_on_a_lane_less_backend() {
        let transport: Arc<dyn ProxyTransport> =
            Arc::new(ClosedTransport::new(thread::current().id()));
        let proxy = PlatformProxy::new(transport);
        assert!(matches!(
            proxy.request_quit(),
            Err(ProxySendError::Unsupported { rejected: () })
        ));
    }

    #[test]
    fn proxy_is_owner_thread_reflects_the_calling_thread() {
        let transport: Arc<dyn ProxyTransport> =
            Arc::new(ClosedTransport::new(thread::current().id()));
        let proxy = PlatformProxy::new(transport);
        assert!(proxy.is_owner_thread());

        let moved = proxy.clone();
        let on_worker = thread::spawn(move || moved.is_owner_thread())
            .join()
            .expect("worker does not panic");
        assert!(!on_worker, "a worker thread is never the owner");
    }
}
