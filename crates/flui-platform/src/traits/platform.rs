//! Core platform abstraction trait
//!
//! Defines the central Platform trait that all platform implementations must
//! provide. This trait serves as the main interface between the FLUI framework
//! and platform-specific code.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use flui_platform_api::{
    Clipboard, ClipboardItem, PlatformDisplay, WindowAppearance, WindowEvent, WindowId,
    WindowOptions,
};

use super::{OpenWindowError, OwnerPlatform, PlatformCapabilities, PlatformWindow};
use crate::{
    data_transfer::DataTransferSource,
    error::{BootstrapError, PlatformError},
    task::Task,
};

/// [`Platform::run`]'s ready callback: invoked once, synchronously, with the
/// owner-thread capability (ADR-0039). Named to keep
/// `Box<dyn FnOnce(OwnerPlatform) -> Result<(), BootstrapError>>` out of
/// every call site's signature.
///
/// Replaces the pre-ADR-0039 `Box<dyn FnOnce(&dyn Platform)>` shape: the
/// callback now receives [`OwnerPlatform`] by value instead of a borrowed
/// `&dyn Platform`, so it may stash the capability in owner-thread state for
/// the rest of the loop's life (e.g. `flui-app`'s `OWNER_PLATFORM_HOST` TLS
/// slot) rather than being limited to the callback's own stack frame.
///
/// Fallible (slice-2 maintainer fix): bootstrap run from inside `on_ready`
/// (window creation, GPU init, root-widget attach) can fail, and a callback
/// that swallowed that failure would leave the loop running with a broken
/// app — Android would keep pumping with no UI, web would install its RAF
/// loop over a half-built page. Returning `Err` here propagates out of
/// [`Platform::run`] itself instead, as
/// [`PlatformError::Bootstrap`] with the callback's own error as its
/// `source`: every backend stops entering (or promptly exits) its loop on
/// `Err` and hands the error back to `run`'s own caller. The error type is
/// the opaque [`BootstrapError`] — embedder bootstrap code is
/// application-land and returns arbitrary errors, so the boundary carries a
/// boxed `std::error::Error` rather than committing the signature to any
/// one error library.
pub type PlatformReadyCallback = Box<dyn FnOnce(OwnerPlatform) -> Result<(), BootstrapError>>;

/// Core platform abstraction trait
///
/// This trait provides the complete interface for platform-specific operations.
/// All platform implementations (Winit, native Windows/macOS/Linux, headless
/// testing) must implement this trait.
///
/// # Architecture
///
/// The Platform trait follows several key design principles from GPUI:
///
/// - **Unified API**: Single trait for all platform operations
/// - **Callback registry**: Framework can register handlers without tight
///   coupling
/// - **Interior mutability**: Implementations use Mutex/RwLock for thread-safe
///   &self methods
/// - **Type erasure**: Returns `Box<dyn Trait>` for flexibility
///
/// # Example
///
/// ```rust,ignore
/// use flui_platform::{Platform, PlatformError, current_platform};
///
/// fn main() -> Result<(), PlatformError> {
///     let platform = current_platform()?;
///     platform.run(Box::new(|owner| {
///         println!("Platform ready: {}", owner.shared().name());
///         Ok(())
///     }))?;
///     Ok(())
/// }
/// ```
///
/// # De-facto crate seal (ADR-0039)
///
/// `run`'s `on_ready` callback receives an [`OwnerPlatform`] — a capability
/// minted only through a `pub(crate)` constructor in this crate. An
/// out-of-crate `impl Platform` can implement `run` but cannot construct the
/// `OwnerPlatform` it must hand to `on_ready`, so this trait is de-facto
/// sealed to backends living inside `flui-platform` even though nothing
/// marks it `sealed` in the type system. An external-embedder minting seam
/// is design work tracked separately (#560, `flui-platform` issue tracker);
/// until then, new backends land in this crate.
pub trait Platform: Send + Sync + 'static {
    // ==================== Core System ====================

    /// Get the platform's background executor for async tasks
    ///
    /// Background tasks run on a thread pool and can block.
    fn background_executor(&self) -> Arc<dyn PlatformExecutor>;

    // ==================== Lifecycle ====================

    /// Run the platform event loop
    ///
    /// This function takes ownership of the platform and the current thread,
    /// running the platform's event loop. The `on_ready` callback is invoked
    /// once, synchronously, on the thread that owns (or will own) the event
    /// loop, and is passed an [`OwnerPlatform`] — the owner-thread capability
    /// (ADR-0039) — by value: it can call `open_window` and every other
    /// owner-affine operation, and reach the residual `Send + Sync` surface
    /// via [`OwnerPlatform::shared`]. `on_ready` may stash the capability in
    /// owner-thread state for the rest of the loop's life; the outer
    /// `Box<dyn Platform>` binding is no longer reachable once `run` has
    /// taken ownership of it.
    ///
    /// Takes `self: Box<Self>` because some backends (e.g. winit) require
    /// ownership of the event loop to run it.
    ///
    /// Desktop event loops, including standalone AppKit, return after orderly
    /// shutdown. Backends tied to a page/process lifetime may not return.
    ///
    /// # Errors
    /// Propagates `on_ready`'s own `Err` as [`PlatformError::Bootstrap`],
    /// with the callback's error preserved as its `source` (a bootstrap
    /// failure — window creation, GPU init, root-widget attach — has no
    /// other return path back to `run`'s caller). Every backend stops
    /// entering, or promptly exits, its loop on that `Err` rather than
    /// continuing with a broken app: a returned error means the loop never
    /// ran a single iteration with `on_ready`'s bootstrap incomplete. A
    /// backend may also return [`PlatformError::EventLoop`] for a
    /// platform-level failure unrelated to `on_ready`.
    fn run(self: Box<Self>, on_ready: PlatformReadyCallback) -> Result<(), PlatformError>;

    /// Request the application to quit
    ///
    /// This may not quit immediately - the platform will clean up and then
    /// exit.
    fn quit(&self);

    /// Install the hook this platform consults before it would otherwise
    /// exit unconditionally because its own window bookkeeping believes
    /// every window it tracks has just closed (see
    /// [`crate::shared::PlatformHandlers::exit_policy`]'s doc for the exact
    /// contract: `true` allows the exit it was about to take anyway, `false`
    /// vetoes it).
    ///
    /// This is the embedder-facing seam issue #555's `ExitPolicy` /
    /// `AppRuntime::should_exit` wire through: an embedder that hosts more
    /// than one top-level window installs a hook here that consults its own
    /// realm registry (which the platform layer cannot see — `flui-app`
    /// depends on `flui-platform`, never the reverse) instead of letting
    /// this backend's native window count decide alone.
    ///
    /// Default no-op: a backend that never overrides this (every backend
    /// except `winit`, AppKit, and `headless` today — Win32/Android/Web/iOS
    /// remain cross-typecheck-only for this specific mechanism, stated
    /// honestly rather than silently assumed) keeps its pre-existing
    /// native lifecycle behavior; installing a hook there is inert.
    fn set_exit_policy_hook(&self, hook: Box<dyn Fn() -> bool + Send>) {
        let _ = hook;
    }

    /// Request that the exit-policy hook
    /// ([`set_exit_policy_hook`](Self::set_exit_policy_hook)) be
    /// re-consulted on the event-loop owner thread, even though no window
    /// is closing right now.
    ///
    /// Exists because that hook is otherwise only consulted when a window
    /// closes: an application whose *last window is already gone* can be
    /// held open by something the platform cannot see (issue #558's
    /// keep-alive application services), and when that holder releases —
    /// on a worker thread, with no window left to produce events — nothing
    /// would ever re-ask the hook, so the veto would be permanent and the
    /// process would linger forever. Backends that support this wake the
    /// owner loop; the owner then re-consults the hook exactly as the
    /// window-close path does (only when its window bookkeeping is empty)
    /// and exits if the hook now allows it.
    ///
    /// Callable from **any thread**; coalesced (a burst of requests costs
    /// one re-check); always safe to over-call — a re-check while windows
    /// are still open, or while the hook still vetoes, is a no-op.
    ///
    /// Default no-op: a backend that never overrides this (every backend
    /// except `winit`, AppKit, and `headless` today — Win32/Android/Web/iOS
    /// remain cross-typecheck-only for this mechanism, stated honestly
    /// rather than silently assumed) simply never re-evaluates: on those
    /// backends a keep-alive holder's release does not end the process
    /// until an explicit [`quit`](Self::quit). The headless backend parks
    /// the request; its embedder drives the actual re-check on the owner
    /// thread (`HeadlessExitReevaluation::drive`) — consulting the hook on
    /// the *calling* thread would run it against the wrong thread-local
    /// runtime state.
    fn request_exit_policy_reevaluation(&self) {}

    /// Install the hook this platform consults, once per idle iteration,
    /// for the earliest wall-clock instant something upstream needs the
    /// loop to wake at — a wired-through
    /// [`ControlFlow::WaitUntil`](https://docs.rs/winit/latest/winit/event_loop/enum.ControlFlow.html)
    /// deadline instead of blocking forever on
    /// `ControlFlow::Wait`. `None` (no upstream deadline pending) keeps the
    /// unconditional-`Wait` behavior exactly as before this hook existed.
    ///
    /// This is issue #556's wall-clock-wake seam: a gesture recognizer's
    /// armed hold/give-up deadline (long-press, double-tap) or a pending
    /// timer needs the loop to wake and re-drive a frame pump at the right
    /// instant even when nothing else is dirty and no animation is
    /// running — otherwise an idle `ControlFlow::Wait` loop never calls
    /// back in to resolve it. `flui-app` computes the earliest such instant
    /// across every realm it hosts on this thread (the platform layer
    /// cannot see realms — `flui-app` depends on `flui-platform`, never the
    /// reverse) and installs this hook once, the same seam
    /// [`set_exit_policy_hook`](Self::set_exit_policy_hook) uses.
    ///
    /// Default no-op: a backend that never overrides this (every backend
    /// except `winit` today — headless has no idle-blocking event loop to
    /// wake, and Win32/AppKit/Android/Web/iOS remain cross-typecheck-only
    /// for this mechanism, stated honestly rather than silently assumed)
    /// keeps its pre-existing behavior exactly as before; installing a hook
    /// there is inert.
    fn set_wake_deadline_hook(
        &self,
        hook: Box<dyn Fn() -> Option<web_time::Instant> + Send + Sync>,
    ) {
        let _ = hook;
    }

    // ==================== Window Management ====================

    /// Create and open a new window
    ///
    /// Returns the canonical shared window identity. The platform event loop,
    /// presentation owner, and raster surface clone this same allocation; no
    /// boxed forwarding handle or duplicate window wrapper is created.
    ///
    /// # Errors
    /// [`OpenWindowError`] — the same typed taxonomy the
    /// [`OwnerPlatform`]/`PlatformProxy` capability surfaces use (ADR-0039):
    /// [`Backend`](OpenWindowError::Backend) when the OS-level creation
    /// itself fails; backends with an owner lane (winit) additionally
    /// surface [`LaneFull`](OpenWindowError::LaneFull) /
    /// [`OwnerGone`](OpenWindowError::OwnerGone) /
    /// [`Unavailable`](OpenWindowError::Unavailable) for cross-thread
    /// lifecycle refusals.
    fn open_window(
        &self,
        options: WindowOptions,
    ) -> Result<Arc<dyn PlatformWindow>, OpenWindowError>;

    /// Get the currently active (focused) window ID
    fn active_window(&self) -> Option<WindowId>;

    // ==================== Display Management ====================

    /// Get all available displays (monitors)
    fn displays(&self) -> Vec<Arc<dyn PlatformDisplay>>;

    /// Get the primary display
    fn primary_display(&self) -> Option<Arc<dyn PlatformDisplay>>;

    // ==================== Input & Clipboard ====================

    /// Get the platform's clipboard interface
    fn clipboard(&self) -> Arc<dyn Clipboard>;

    /// The data-transfer transport (ADR-0038). Contract: returns clones of
    /// ONE source instance per platform instance — the source owns
    /// connection-like state (the offer table), and per ADR-0038 §9 such
    /// state must live behind an `Arc` the platform clones out, never be
    /// reconstructed per call (two independently minted tables would split
    /// the id space and let cross-table ids pass each other's generation
    /// checks). Deliberately a required method with no default body so a
    /// new backend must make that choice explicitly. Backends without a
    /// transport return [`crate::data_transfer::NullDataTransferSource`]
    /// (inert, honest — not fake success).
    fn data_transfer(&self) -> Arc<dyn DataTransferSource>;

    // ==================== App Activation (US3) ====================

    /// Activate the application (bring to front)
    ///
    /// On Windows: brings the active window to the foreground.
    /// On macOS: activates the app via NSApp.
    fn activate(&self, ignoring_other_apps: bool) {
        let _ = ignoring_other_apps;
    }

    // ==================== Appearance (US3) ====================

    /// Get the system window appearance (light/dark theme)
    fn window_appearance(&self) -> WindowAppearance {
        WindowAppearance::default()
    }

    // ==================== Clipboard (US3 Enhanced) ====================

    /// Write a rich clipboard item (text + metadata)
    fn write_to_clipboard(&self, item: ClipboardItem) {
        // Default: write first text entry via existing Clipboard trait
        if let Some(text) = item.text_content() {
            self.clipboard().write_text(text.to_string());
        }
    }

    /// Read a rich clipboard item
    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        // Default: read via existing Clipboard trait
        self.clipboard().read_text().map(ClipboardItem::text)
    }

    // ==================== File Operations (US3) ====================

    /// Open a URL with the system's default handler
    fn open_url(&self, url: &str) {
        let _ = url;
    }

    /// Reveal a path in the platform's file manager
    fn reveal_path(&self, path: &Path) {
        let _ = path;
    }

    /// Open a path with the system's default application
    fn open_path(&self, path: &Path) {
        let _ = path;
    }

    /// Show a file/directory picker dialog
    ///
    /// Returns selected paths, or `None` if the user cancelled (cancelling
    /// is not an error). The dialog runs asynchronously on a background
    /// thread; a dialog that could not run to completion resolves to
    /// [`PlatformError::Dialog`].
    fn prompt_for_paths(
        &self,
        options: PathPromptOptions,
    ) -> Task<Result<Option<Vec<PathBuf>>, PlatformError>> {
        let _ = options;
        Task::ready(Ok(None))
    }

    /// Show a "Save As" dialog for selecting a new file path
    ///
    /// Returns the selected path, or `None` if the user cancelled
    /// (cancelling is not an error); a dialog that could not run to
    /// completion resolves to [`PlatformError::Dialog`].
    fn prompt_for_new_path(
        &self,
        directory: &Path,
        suggested_name: Option<&str>,
    ) -> Task<Result<Option<PathBuf>, PlatformError>> {
        let _ = (directory, suggested_name);
        Task::ready(Ok(None))
    }

    // ==================== Keyboard (US3) ====================

    /// Get the current keyboard layout identifier
    fn keyboard_layout(&self) -> String {
        String::new()
    }

    /// Register a callback for keyboard layout changes
    fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut() + Send>) {
        let _ = callback;
    }

    // ==================== Callbacks ====================

    /// Register a callback for when the application should quit
    fn on_quit(&self, callback: Box<dyn FnMut() + Send>);

    /// Register the macOS application's reopen signal callback.
    ///
    /// AppKit delivers this signal for both visible-window and no-window reopen
    /// events. FLUI suppresses AppKit's default untitled-document creation; the
    /// callback decides whether to show or create UI. Delivery is deferred to the
    /// owner lane and uses the registration current at that turn. Nested events
    /// wait until the active callback and its cleanup finish. Replacement is safe
    /// from inside a callback. Explicit quit rejects pending signals and later
    /// registrations; callbacks are released when the native run ends.
    /// Other backends may leave this optional native transport unsupported.
    fn on_reopen(&self, callback: Box<dyn FnMut() + Send>) {
        let _ = callback;
    }

    /// Register a callback for window events
    fn on_window_event(&self, callback: Box<dyn FnMut(WindowEvent) + Send>);

    /// Register a callback for URLs opened by the system
    fn on_open_urls(&self, callback: Box<dyn FnMut(Vec<String>) + Send>) {
        let _ = callback;
    }

    // ==================== Platform Info ====================

    /// Get the platform's capabilities descriptor
    fn capabilities(&self) -> &dyn PlatformCapabilities;

    /// Get the platform's name for debugging/logging
    fn name(&self) -> &'static str;

    /// Get the compositor name (e.g., "DWM" on Windows)
    fn compositor_name(&self) -> &'static str {
        ""
    }

    /// Get the application's executable path
    ///
    /// # Errors
    /// [`PlatformError::AppPath`] when the backend's own lookup fails.
    fn app_path(&self) -> Result<PathBuf, PlatformError>;
}

/// Platform executor trait for async task execution
///
/// This is a minimal interface - platforms can return their own executor types
/// that implement this trait.
pub trait PlatformExecutor: Send + Sync {
    /// Spawn a task on this executor
    fn spawn(&self, task: Box<dyn FnOnce() + Send>);

    /// Check if we're currently on this executor's thread(s)
    fn is_on_executor(&self) -> bool {
        false // Default implementation
    }
}

/// Options for the file/directory picker dialog
#[derive(Debug, Clone)]
pub struct PathPromptOptions {
    /// Allow selecting files
    pub files: bool,
    /// Allow selecting directories
    pub directories: bool,
    /// Allow selecting multiple items
    pub multiple: bool,
}

impl Default for PathPromptOptions {
    fn default() -> Self {
        Self {
            files: true,
            directories: false,
            multiple: false,
        }
    }
}
