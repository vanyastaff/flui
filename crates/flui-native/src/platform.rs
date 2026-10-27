//! [`Platform`], the contract between the framework and the core host, and
//! [`HostPlatform`], its one implementation over a host's table (ADR-0151
//! §10).

use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use flui_foundation::geometry::Size;
use flui_platform::data_transfer::DataTransferSource;
use flui_platform::{
    Clipboard, ClipboardItem, DisplaySnapshot, SessionEndAnswer, SessionEndPhase, StorageName,
    SystemNotice, SystemPreferences, WindowCapabilities, WindowHandler, WindowId, WindowOptions,
};

use crate::error::{BootstrapError, PlatformError};
use crate::host::{
    self, Clipboard as _, Displays as _, Drag as _, Host, Input as _, Lifecycle as _, Loop as _,
    Preferences as _, Services as _, Storage as _, Window as _, WindowPositions, WindowRefOf,
};
use crate::host_window::HostWindow;
use crate::owner::{OpenWindowError, OwnerPlatform};
use crate::storage::DataDirs;
use crate::task::Task;

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

mod sealed {
    /// The seal of [`Platform`](super::Platform): public in a private module,
    /// so only this crate can implement it, and implemented for
    /// [`HostPlatform`](super::HostPlatform) alone.
    #[diagnostic::on_unimplemented(
        message = "`{Self}` cannot implement `Platform`: the trait is sealed",
        label = "not the platform of a host in `flui-native`",
        note = "`HostPlatform<H>` over a host's table is the one implementation of `Platform`; a new platform is a host table in `flui-native` (ADR-0151 §5, §10)"
    )]
    pub trait Sealed {}
}

/// Core platform abstraction trait
///
/// This trait provides the complete interface for platform-specific operations.
/// Every host answers it as [`HostPlatform`] over the host's table (Win32,
/// AppKit, UIKit, Android, the web, winit, headless).
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
/// use flui_native::{Platform, PlatformError, current_platform};
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
/// # Sealed (ADR-0151 §5)
///
/// A private supertrait, implemented for [`HostPlatform`] alone, keeps every
/// implementation in this crate: an outside `impl Platform` does not compile
/// (`trybuild_ui::ui_tests` pins the error). It could not have run either:
/// `run`'s `on_ready` callback receives an [`OwnerPlatform`], which only a
/// `pub(crate)` constructor here mints. A new platform is a host table in
/// `hosts/`, not another implementation of this trait.
pub trait Platform: sealed::Sealed + Send + Sync + 'static {
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
    /// every window it tracks has just closed: `true` allows the exit it was
    /// about to take anyway, `false` vetoes it.
    ///
    /// This is the embedder-facing seam issue #555's `ExitPolicy` /
    /// `AppRuntime::should_exit` wire through: an embedder that hosts more
    /// than one top-level window installs a hook here that consults its own
    /// UI runtime registry (which the platform layer cannot see — `flui-app`
    /// depends on `flui-native`, never the reverse) instead of letting
    /// this backend's native window count decide alone.
    ///
    /// The `winit`, AppKit, Win32 and headless hosts consult the hook; Android,
    /// the web and iOS leave the end of the process to the Activity lifecycle,
    /// the page and UIKit, and drop it. Win32 consults the hook on an
    /// owner-window turn after the last tracked window's `WM_DESTROY`, and with
    /// no hook installed ends its message loop there.
    fn set_exit_policy_hook(&self, hook: Box<dyn Fn() -> bool + Send>);

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
    /// The `winit`, AppKit and Win32 hosts wake their owner loop to re-check.
    /// Android, the web and iOS have no exit policy to consult again, so a
    /// keep-alive holder's release does not end the process there until an
    /// explicit [`quit`](Self::quit). The headless host parks the request; its
    /// embedder drives the actual re-check on the owner thread
    /// (`HeadlessExitReevaluation::drive`) — consulting the hook on the
    /// *calling* thread would run it against the wrong thread-local runtime
    /// state.
    fn request_exit_policy_reevaluation(&self);

    /// Install the hook this platform consults, once per idle iteration,
    /// for the earliest wall-clock instant something upstream needs the
    /// loop to wake at. The winit backend uses
    /// [`ControlFlow::WaitUntil`](https://docs.rs/winit/latest/winit/event_loop/enum.ControlFlow.html)
    /// and native Win32 bounds its message wait and, when an admitted deadline
    /// becomes due, invalidates live windows (requesting a minimized one's
    /// frame directly, since it is not painted). `None` means no upstream
    /// deadline is pending and permits the backend's ordinary idle wait.
    ///
    /// This is issue #556's wall-clock-wake seam: a gesture recognizer's
    /// armed hold/give-up deadline (long-press, double-tap) or a pending
    /// timer needs the loop to wake and re-drive a frame pump at the right
    /// instant even when nothing else is dirty and no animation is
    /// running — otherwise an idle `ControlFlow::Wait` loop never calls
    /// back in to resolve it. `flui-app` computes the earliest such instant
    /// across every UI runtime it hosts on this thread (the platform layer
    /// cannot see UI runtimes — `flui-app` depends on `flui-native`, never the
    /// reverse) and installs this hook once, the same seam
    /// [`set_exit_policy_hook`](Self::set_exit_policy_hook) uses.
    ///
    /// The AppKit and Android hosts actuate the deadline from their own loops
    /// and iOS keeps the hook for it; the headless host has no idle-blocking
    /// event loop to wake and the web host's browser loop has no idle wait to
    /// bound, so both drop it.
    fn set_wake_deadline_hook(
        &self,
        hook: Box<dyn Fn() -> Option<web_time::Instant> + Send + Sync>,
    );

    // ==================== Window Management ====================

    /// Create and open a new window, whose deliveries go to `handler`
    /// (ADR-0254 §4).
    ///
    /// The handler exists before the window's first event: its first input
    /// is the window's capability snapshot, delivered before this call
    /// returns, and it is the one place the window's input, state changes,
    /// frame and close requests, close and surface changes go. Call it on the
    /// owner thread, which the handler lives on; a refused open drops it.
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
        handler: Rc<dyn WindowHandler>,
    ) -> Result<Arc<dyn HostWindow>, OpenWindowError>;

    /// Get the currently active (focused) window ID
    fn active_window(&self) -> Option<WindowId>;

    /// Read system preferences independently of any user window.
    ///
    /// Call on the platform owner thread. Backends without an observation
    /// mechanism return unknown fields, leaving fallback policy to consumers.
    /// Windows keeps native subscriptions for this host's lifetime and schedules
    /// an owner wake when a read is needed. A wake can combine other owner work;
    /// it is not itself a preference snapshot.
    ///
    /// # Errors
    /// Reports a failed native read or an unavailable owner. An error is not an
    /// observation that a setting has been disabled.
    fn preferences(&self) -> Result<flui_platform::SystemPreferences, PlatformError>;

    // ==================== Display Management ====================

    /// Sample every display (monitor) the platform knows right now.
    ///
    /// Call on the platform owner thread. The value is immutable: identity,
    /// name, bounds, usable bounds, scale factor and the refresh period of each
    /// display's current mode (`None` when the OS does not report one — never a
    /// guessed 60 Hz). A source that learns of a change from the OS invalidates
    /// this by waking the owner; the host then samples again, so a change
    /// between registering the source and the first sample is not lost. A
    /// failed read is an error, not an empty snapshot: the host keeps the last
    /// snapshot it accepted.
    ///
    /// Display ids come from the platform's own
    /// [`DisplayRegistry`](flui_platform::DisplayRegistry) and are
    /// comparable only with ids from this platform instance (ADR-0236).
    ///
    /// # Errors
    /// Reports a failed native read or a platform with no display source
    /// compiled in.
    fn display_snapshot(&self) -> Result<DisplaySnapshot, PlatformError>;

    // ==================== Input & Clipboard ====================

    /// Get the platform's clipboard interface
    fn clipboard(&self) -> Arc<dyn Clipboard>;

    /// The data-transfer transport (ADR-0038). Contract: returns clones of
    /// ONE source instance per platform instance — the source owns
    /// connection-like state (the offer table), and per ADR-0038 §9 such
    /// state must live behind an `Arc` the platform clones out, never be
    /// reconstructed per call (two independently minted tables would split
    /// the id space and let cross-table ids pass each other's generation
    /// checks). Backends without a transport return
    /// [`flui_platform::data_transfer::NullDataTransferSource`] (inert,
    /// honest — not fake success).
    fn data_transfer(&self) -> Arc<dyn DataTransferSource>;

    // ==================== App Activation (US3) ====================

    /// Activate the application (bring to front)
    ///
    /// On Windows: brings the active window to the foreground.
    /// The other hosts do nothing: AppKit activates the application once, as
    /// its loop starts, and winit, the web, Android and iOS have no way to
    /// bring the whole application forward.
    fn activate(&self, ignoring_other_apps: bool);

    // ==================== Clipboard (US3 Enhanced) ====================

    /// Write a rich clipboard item (text + metadata)
    ///
    /// A host that keeps only text writes the item's first text entry to the
    /// [`clipboard`](Self::clipboard).
    fn write_to_clipboard(&self, item: ClipboardItem);

    /// Read a rich clipboard item
    ///
    /// A host that keeps only text answers the [`clipboard`](Self::clipboard)'s
    /// text.
    fn read_from_clipboard(&self) -> Option<ClipboardItem>;

    // ==================== File Operations (US3) ====================

    /// Open a URL with the system's default handler
    fn open_url(&self, url: &str);

    /// Reveal a path in the platform's file manager
    fn reveal_path(&self, path: &Path);

    /// Open a path with the system's default application
    fn open_path(&self, path: &Path);

    /// Show a file/directory picker dialog
    ///
    /// Returns selected paths, or `None` if the user cancelled (cancelling
    /// is not an error). The dialog runs asynchronously on a background
    /// thread; a dialog that could not run to completion resolves to
    /// [`PlatformError::Dialog`].
    fn prompt_for_paths(
        &self,
        options: PathPromptOptions,
    ) -> Task<Result<Option<Vec<PathBuf>>, PlatformError>>;

    /// Show a "Save As" dialog for selecting a new file path
    ///
    /// Returns the selected path, or `None` if the user cancelled
    /// (cancelling is not an error); a dialog that could not run to
    /// completion resolves to [`PlatformError::Dialog`].
    fn prompt_for_new_path(
        &self,
        directory: &Path,
        suggested_name: Option<&str>,
    ) -> Task<Result<Option<PathBuf>, PlatformError>>;

    // ==================== Keyboard (US3) ====================

    /// Get the current keyboard layout identifier
    ///
    /// Empty where the host does not read the layout.
    fn keyboard_layout(&self) -> String;

    /// Register a callback for keyboard layout changes
    ///
    /// A host that does not read the layout never calls it.
    fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut() + Send>);

    // ==================== Callbacks ====================

    /// Register a callback for when the application should quit
    fn on_quit(&self, callback: Box<dyn FnMut() + Send>);

    /// Register the callback the platform asks when the user's session ends
    /// (log off, restart, shut down), replacing any earlier one. It runs on
    /// the owner thread, possibly inside a nested native loop, once per
    /// phase of each session end; see [`SessionEndPhase`].
    ///
    /// A host without session-end messages never calls it and drops it
    /// unused.
    fn on_session_end(&self, callback: Box<dyn FnMut(SessionEndPhase) -> SessionEndAnswer + Send>);

    /// Register the callback the platform calls with every system notice other
    /// than a session end's phases (sleep, wake, session lock, memory
    /// pressure), replacing any earlier one. It runs on the owner thread.
    ///
    /// A host that observes none of them never calls it and drops it unused.
    ///
    /// A backend that delivers [`SystemNotice::Wake`] marks its display
    /// snapshot and every window's state stale first, in the shared dispatch
    /// (`owner::notices`), so the first read after the machine woke asks
    /// the operating system again.
    fn on_host_notice(&self, callback: Box<dyn FnMut(SystemNotice) + Send>);

    /// Tell the platform the application is done holding a session end it
    /// answered [`SessionEndAnswer::Block`] to, so the session may go on: the
    /// last [`SessionEndBlock`](flui_platform::SessionEndBlock) was
    /// released. A backend that holds the session open (Windows'
    /// `ShutdownBlockReason`) lets go here; one that answered synchronously has
    /// nothing to undo.
    fn release_session_end(&self);

    /// Which window commands and observations this platform's windows have,
    /// known before any window exists.
    ///
    /// It is the same set every window the platform opens reports through
    /// its state snapshot ([`WindowState::capabilities`](flui_platform::WindowState::capabilities)),
    /// so a caller can fit a remembered placement to what the window will be
    /// able to do (`WindowCapabilities::places_at_open`) before it opens one.
    fn window_capabilities(&self) -> flui_platform::WindowCapabilities;

    /// How much larger a decorated window's outer size is than its content
    /// size on a display of scale `scale_factor`, in logical pixels: the title
    /// bar and borders. Known before any window exists, so a remembered
    /// placement can be fitted by its outer size.
    ///
    /// Zero is exact for a platform whose windows have no non-client area
    /// (headless, mobile, web). Win32 answers zero, an under-estimate, since it
    /// does not measure the frame before a window exists; winit answers the
    /// frame it has measured so far and AppKit the one its window style gives.
    fn window_frame_inset(&self, scale_factor: f64) -> flui_foundation::geometry::Size<f64>;

    /// Register the macOS application's reopen signal callback.
    ///
    /// AppKit delivers this signal for both visible-window and no-window reopen
    /// events. FLUI suppresses AppKit's default untitled-document creation; the
    /// callback decides whether to show or create UI. Delivery is deferred to the
    /// owner lane and uses the registration current at that turn. Nested events
    /// wait until the active callback and its cleanup finish. Replacement is safe
    /// from inside a callback. Explicit quit rejects pending signals and later
    /// registrations; callbacks are released when the native run ends.
    /// Other hosts never produce the signal and drop the callback unused.
    fn on_reopen(&self, callback: Box<dyn FnMut() + Send>);

    /// Register a callback for URLs opened by the system
    ///
    /// No host produces opened URLs yet; the callback is dropped unused.
    fn on_open_urls(&self, callback: Box<dyn FnMut(Vec<String>) + Send>);

    // ==================== Platform Info ====================

    /// Get the platform's name for debugging/logging
    fn name(&self) -> &'static str;

    /// Get the compositor name (e.g., "DWM" on Windows), empty where the host
    /// names none
    fn compositor_name(&self) -> &'static str;

    /// Get the application's executable path
    ///
    /// # Errors
    /// [`PlatformError::AppPath`] when the backend's own lookup fails.
    fn app_path(&self) -> Result<PathBuf, PlatformError>;

    /// Where the application `app` keeps its data on this platform, or `None`
    /// when it has nowhere to keep it.
    ///
    /// Every host but Android answers with the current user's data directories
    /// ([`crate::storage::data_dirs`]); Android's process has no such
    /// directories, so it answers with its own.
    fn data_dirs(&self, app: &flui_platform::StorageName) -> Option<crate::storage::DataDirs>;
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

/// The platform of host `H`: every [`Platform`] method answered by the
/// position of `H`'s table that holds it.
///
/// Built by the host's constructor in `hosts/`; the public names
/// (`WindowsPlatform`, `HeadlessPlatform`, …) are this type over each table.
pub struct HostPlatform<H: Host> {
    owner: Arc<H::Loop>,
    window_shared: Arc<WindowShared<H>>,
    host: PhantomData<fn() -> H>,
}

/// What the windows of host `H` share: its window position's
/// [`Shared`](host::Window::Shared).
pub(crate) type WindowShared<H> = <<H as Host>::Window as host::Window<<H as Host>::Loop>>::Shared;

impl<H: Host> HostPlatform<H> {
    /// The platform over what the host's windows share, which
    /// `window_shared` builds over the window-level positions of `H`'s table,
    /// and the loop `owner` builds over the same instance (the loop holds it
    /// as the hook it declares): what each host's constructor returns. The
    /// loop gets the one instance the platform keeps, so the two cannot see
    /// different windows; the positions are made here from `H`, so a window
    /// holds what its own table names.
    pub(crate) fn from_parts(
        window_shared: impl FnOnce(WindowPositions<WindowRefOf<H>>) -> WindowShared<H>,
        owner: impl FnOnce(&Arc<WindowShared<H>>) -> H::Loop,
    ) -> Self {
        let window_shared = Arc::new(window_shared(WindowPositions::of::<H>()));
        Self {
            owner: Arc::new(owner(&window_shared)),
            window_shared,
            host: PhantomData,
        }
    }

    /// [`Self::from_parts`] for a loop whose construction can fail.
    ///
    /// # Errors
    /// What `owner` reports.
    #[cfg(any(
        windows,
        target_os = "macos",
        target_os = "ios",
        target_arch = "wasm32"
    ))]
    pub(crate) fn try_from_parts<E>(
        window_shared: impl FnOnce(WindowPositions<WindowRefOf<H>>) -> WindowShared<H>,
        owner: impl FnOnce(&Arc<WindowShared<H>>) -> Result<H::Loop, E>,
    ) -> Result<Self, E> {
        let window_shared = Arc::new(window_shared(WindowPositions::of::<H>()));
        Ok(Self {
            owner: Arc::new(owner(&window_shared)?),
            window_shared,
            host: PhantomData,
        })
    }

    /// The host's loop, for what a host offers beyond [`Platform`] (test
    /// controls, a scene registration).
    pub(crate) fn owner(&self) -> &H::Loop {
        &self.owner
    }

    /// What the host's windows share, for what its window position offers
    /// beyond [`Platform`] (headless test controls).
    pub(crate) fn window_shared(&self) -> &Arc<WindowShared<H>> {
        &self.window_shared
    }

    /// A handle on the host's loop, for a host whose loop runs as an `Arc`.
    pub(crate) fn shared_owner(&self) -> Arc<H::Loop> {
        Arc::clone(&self.owner)
    }
}

impl<H: Host> std::fmt::Debug for HostPlatform<H>
where
    H::Loop: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // What the windows share is the loop's to show: it holds it too.
        f.debug_struct("HostPlatform")
            .field("host", &H::NAME)
            .field("loop", &self.owner)
            .finish_non_exhaustive()
    }
}

impl<H: Host> sealed::Sealed for HostPlatform<H> {}

impl<H: Host> Platform for HostPlatform<H> {
    fn background_executor(&self) -> Arc<dyn PlatformExecutor> {
        self.owner.background_executor()
    }

    fn run(self: Box<Self>, on_ready: PlatformReadyCallback) -> Result<(), PlatformError> {
        let owner = self.shared_owner();
        let platform: Arc<dyn Platform> = Arc::<Self>::from(self);
        owner.run(platform, on_ready)
    }

    fn quit(&self) {
        self.owner.quit();
    }

    fn set_exit_policy_hook(&self, hook: Box<dyn Fn() -> bool + Send>) {
        self.owner.set_exit_policy_hook(hook);
    }

    fn request_exit_policy_reevaluation(&self) {
        self.owner.request_exit_policy_reevaluation();
    }

    fn set_wake_deadline_hook(
        &self,
        hook: Box<dyn Fn() -> Option<web_time::Instant> + Send + Sync>,
    ) {
        self.owner.set_wake_deadline_hook(hook);
    }

    fn open_window(
        &self,
        options: WindowOptions,
        handler: Rc<dyn WindowHandler>,
    ) -> Result<Arc<dyn HostWindow>, OpenWindowError> {
        let window = H::Window::open(&self.owner, &self.window_shared, options, handler)?;
        // Every lock the open took is released: the handler hears its
        // window's capabilities now, before anything later.
        window.delivery().deliver_capability_snapshot();
        Ok(window)
    }

    fn active_window(&self) -> Option<WindowId> {
        H::Window::active(&self.owner, &self.window_shared)
    }

    fn preferences(&self) -> Result<SystemPreferences, PlatformError> {
        H::Preferences::sample(&self.owner)
    }

    fn display_snapshot(&self) -> Result<DisplaySnapshot, PlatformError> {
        H::Displays::snapshot(&self.owner)
    }

    fn clipboard(&self) -> Arc<dyn flui_platform::Clipboard> {
        H::Clipboard::clipboard(&self.owner)
    }

    fn data_transfer(&self) -> Arc<dyn DataTransferSource> {
        H::Drag::data_transfer(&self.owner)
    }

    fn activate(&self, ignoring_other_apps: bool) {
        H::Lifecycle::activate(&self.owner, ignoring_other_apps);
    }

    fn write_to_clipboard(&self, item: ClipboardItem) {
        H::Clipboard::write(&self.owner, item);
    }

    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        H::Clipboard::read(&self.owner)
    }

    fn open_url(&self, url: &str) {
        self.owner.open_url(url);
    }

    fn reveal_path(&self, path: &Path) {
        self.owner.reveal_path(path);
    }

    fn open_path(&self, path: &Path) {
        self.owner.open_path(path);
    }

    fn prompt_for_paths(
        &self,
        options: PathPromptOptions,
    ) -> Task<Result<Option<Vec<PathBuf>>, PlatformError>> {
        self.owner.prompt_for_paths(options)
    }

    fn prompt_for_new_path(
        &self,
        directory: &Path,
        suggested_name: Option<&str>,
    ) -> Task<Result<Option<PathBuf>, PlatformError>> {
        self.owner.prompt_for_new_path(directory, suggested_name)
    }

    fn keyboard_layout(&self) -> String {
        H::Input::keyboard_layout(&self.owner)
    }

    fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut() + Send>) {
        H::Input::on_keyboard_layout_change(&self.owner, callback);
    }

    fn on_quit(&self, callback: Box<dyn FnMut() + Send>) {
        self.owner.on_quit(callback);
    }

    fn on_session_end(&self, callback: Box<dyn FnMut(SessionEndPhase) -> SessionEndAnswer + Send>) {
        H::Lifecycle::on_session_end(&self.owner, callback);
    }

    fn on_host_notice(&self, callback: Box<dyn FnMut(SystemNotice) + Send>) {
        H::Lifecycle::on_host_notice(&self.owner, callback);
    }

    fn release_session_end(&self) {
        H::Lifecycle::release_session_end(&self.owner);
    }

    fn window_capabilities(&self) -> WindowCapabilities {
        H::Window::capabilities(&self.owner, &self.window_shared)
    }

    fn window_frame_inset(&self, scale_factor: f64) -> Size<f64> {
        H::Window::frame_inset(&self.owner, &self.window_shared, scale_factor)
    }

    fn on_reopen(&self, callback: Box<dyn FnMut() + Send>) {
        H::Lifecycle::on_reopen(&self.owner, callback);
    }

    fn on_open_urls(&self, callback: Box<dyn FnMut(Vec<String>) + Send>) {
        // No host produces opened URLs yet: the activation feature brings the
        // producer and replaces this registration with its handler.
        drop(callback);
    }

    fn name(&self) -> &'static str {
        H::NAME
    }

    fn compositor_name(&self) -> &'static str {
        self.owner.compositor_name()
    }

    fn app_path(&self) -> Result<PathBuf, PlatformError> {
        self.owner.app_path()
    }

    fn data_dirs(&self, app: &StorageName) -> Option<DataDirs> {
        H::Storage::data_dirs(&self.owner, app)
    }
}
