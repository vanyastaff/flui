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
use std::sync::Arc;
use std::thread::ThreadId;

use flui_foundation::ClaimHandle;
use static_assertions::{assert_impl_all, assert_not_impl_any};

use super::{
    Platform, PlatformDisplay, PlatformWindow, WindowId, WindowOptions, window::WindowAppearance,
};

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

    /// Escapes to the residual thread-safe surface (`Send + Sync` methods
    /// that need no owner-thread proof: `background_executor`, `clipboard`,
    /// callback registration, and the rest of §2's "stays on `Platform`"
    /// list).
    #[must_use]
    pub fn shared(&self) -> &dyn Platform {
        &*self.platform
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
    // Manual impl: `transport` carries wake/send plumbing with no blanket
    // `Debug` (api-lead #9).
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
    pub fn request_quit(&self) {
        self.transport.request_quit();
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
/// ADR-0037 §3).
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
    /// The event-loop owner is gone.
    ///
    /// On backends without an owner lane (macOS/Win32 until slice 3 lane
    /// adoption; any single-threaded backend), this variant is returned on
    /// a *live* loop and is **permanent** there — not a transient condition
    /// to retry. Do not retry; this is not a transient condition. Treat it
    /// as a standing "cross-thread platform requests are unsupported on
    /// this backend today" signal (ADR-0039 slice-2 amendment b); slice 3's
    /// lane adoption is what makes it literally true everywhere.
    #[error("event-loop owner is gone")]
    OwnerGone {
        /// The value that could not be delivered.
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
    // no blanket `Debug` (api-lead #9).
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
    /// See [`WaitError`].
    pub fn wait(self) -> Result<Arc<dyn PlatformWindow>, WaitError> {
        if self.owner_thread == std::thread::current().id() {
            return Err(WaitError::WouldBlockOwner(self));
        }
        self.handle.wait().map_err(WaitError::Open)
    }

    /// Non-blocking poll; safe on any thread.
    pub fn try_take(&mut self) -> Option<Result<Arc<dyn PlatformWindow>, OpenWindowError>> {
        self.handle.try_take()
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
    fn request_quit(&self);

    /// The thread identity of the event-loop owner (diagnostic only).
    fn owner_thread(&self) -> ThreadId;
}

/// A transport with no lane behind it: every request is refused with the
/// permanent `OwnerGone` posture (ADR-0039 slice-2 amendment b) until
/// slice 3 gives the backend a real lane.
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
        tracing::warn!(
            "PlatformProxy::open_window on a lane-less backend: permanently \
             OwnerGone until slice 3 (ADR-0039 amendment b) — do not retry"
        );
        Err(ProxySendError::OwnerGone { rejected: options })
    }

    fn request_quit(&self) {
        tracing::warn!(
            "PlatformProxy::request_quit on a lane-less backend is a no-op \
             until slice 3 (ADR-0039 amendment b)"
        );
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
    fn closed_transport_open_window_is_permanently_owner_gone() {
        let transport = ClosedTransport::new(thread::current().id());
        let error = transport
            .open_window(WindowOptions::default())
            .expect_err("no lane behind a ClosedTransport");
        assert!(matches!(error, ProxySendError::OwnerGone { .. }));
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
