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

/// What the platform tells and asks the application it runs, passed once to
/// [`Platform::run`] (ADR-0254 §4).
///
/// One method per obligation, none with a default body: a handler that does
/// not care about one says so in code. Every method is called on the owner
/// thread, through the one call point in `owner/`, with no lock or borrow of
/// the host held, so a method may quit, open a window or reach the platform in
/// any other way. `ready` hands over the owner capability by value; every other
/// method borrows it for the call, so a handler needs no copy of its own.
///
/// What the platform reports (a quit, a reopen, a host notice) is accepted
/// whether or not the handler can hear it yet: one that arrives before `ready`
/// has returned, while another of these methods runs, or on another thread,
/// waits in order and is delivered on the owner thread once the handler can
/// hear it. What the platform asks (a session end's answer, the exit policy,
/// the wake deadline) is asked at once, re-entrantly if need be; before `ready`
/// and after the loop ended the host answers it itself.
///
/// A panicking method is contained: what the platform had accepted behind it
/// is still delivered, each in its own containment, and the first panic is
/// raised afterwards. A second panic never replaces the first.
///
/// A trait of the host crate, not of the contract: it names
/// [`OwnerPlatform`], so what implements it is code that runs a host itself —
/// the composition root (`flui-app`), and an example or test that drives
/// `flui-native` directly (ADR-0151 §9). It is not `Send`: the handler lives
/// on the owner thread with the capability it receives.
pub trait AppHandler: 'static {
    /// The loop runs and windows can open: the application's bootstrap.
    /// Called once, before any other method, with the owner-thread capability
    /// (ADR-0039), which the application may keep for the loop's life.
    ///
    /// # Errors
    /// A bootstrap that failed (window creation, GPU initialisation, the root
    /// widget): [`Platform::run`] stops entering, or promptly exits, its loop
    /// and returns it as [`PlatformError::Bootstrap`].
    fn ready(&self, owner: OwnerPlatform) -> Result<(), BootstrapError>;

    /// The platform quits: its loop stopped, or is about to. Delivered once
    /// per run.
    fn quit_requested(&self, owner: &OwnerPlatform);

    /// The user reopened the application (a click on its Dock icon). Only
    /// AppKit reports it.
    fn reopen(&self, owner: &OwnerPlatform);

    /// A phase of the user's session ending (log off, restart, shut down),
    /// possibly inside a nested native loop. The answer to
    /// [`SessionEndPhase::Query`] decides whether the platform asks the user to
    /// wait; it is read for no other phase. A host without session-end
    /// messages never asks.
    fn session_end(&self, owner: &OwnerPlatform, phase: SessionEndPhase) -> SessionEndAnswer;

    /// A system notice other than a session end's phases: sleep, wake, the
    /// session locking or unlocking, memory pressure. Before a
    /// [`SystemNotice::Wake`] is delivered, the platform marks its display
    /// snapshot and every window's published state stale.
    fn host_notice(&self, owner: &OwnerPlatform, notice: SystemNotice);

    /// Whether the platform may end its loop now that its own window
    /// bookkeeping holds no window ([`ExitAnswer`]). Asked by the Win32,
    /// AppKit, winit and headless hosts when their last window closes, and
    /// again on [`Platform::request_exit_policy_reevaluation`]; Android, the
    /// web and iOS leave the end of the process to the Activity, the page and
    /// UIKit.
    fn exit_policy(&self, owner: &OwnerPlatform) -> ExitAnswer;

    /// The earliest wall-clock instant the loop must wake at even with
    /// nothing else to do (a gesture's hold deadline, a timer), or `None`.
    /// Asked once per idle iteration by the hosts that wait idle (Win32,
    /// AppKit, winit, Android, iOS); the headless and web hosts have no idle
    /// wait to bound.
    fn wake_deadline(&self, owner: &OwnerPlatform) -> Option<web_time::Instant>;
}

/// The application's answer to [`AppHandler::exit_policy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitAnswer {
    /// The loop may end.
    End,
    /// The loop keeps running: another window is about to open, or a service
    /// holds the application.
    KeepRunning,
    /// The application has no policy of its own: the host decides as it does
    /// before `ready` and after the loop ended. The Win32, AppKit and winit
    /// hosts end the loop; the headless host keeps it running.
    HostDecides,
}

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
/// - **One handler per run**: what the platform tells and asks the
///   application goes to the [`AppHandler`] passed to [`run`](Self::run), and
///   what a window delivers to the handler it was opened with
/// - **Interior mutability**: Implementations use Mutex/RwLock for thread-safe
///   &self methods
/// - **Type erasure**: Returns `Box<dyn Trait>` for flexibility
///
/// # Sealed (ADR-0151 §5)
///
/// A private supertrait, implemented for [`HostPlatform`] alone, keeps every
/// implementation in this crate: an outside `impl Platform` does not compile
/// (`trybuild_ui::ui_tests` pins the error). It could not have run either:
/// `run`'s [`AppHandler::ready`] receives an [`OwnerPlatform`], which only a
/// `pub(crate)` constructor here mints. A new platform is a host table in
/// `hosts/`, not another implementation of this trait.
pub trait Platform: sealed::Sealed + Send + Sync + 'static {
    // ==================== Core System ====================

    /// Get the platform's background executor for async tasks
    ///
    /// Background tasks run on a thread pool and can block.
    fn background_executor(&self) -> Arc<dyn PlatformExecutor>;

    // ==================== Lifecycle ====================

    /// Run the platform event loop, telling and asking `handler` what the
    /// platform has for the application (ADR-0254 §4).
    ///
    /// Takes the platform and the current thread, which becomes the owner
    /// thread. [`AppHandler::ready`] is called once, synchronously, on it,
    /// with the [`OwnerPlatform`] by value: it can open windows and reach every
    /// other owner-affine operation, and reach the residual `Send + Sync`
    /// surface through [`OwnerPlatform::shared`]. The handler is the one place
    /// a quit, a reopen, a session end, a host notice and the exit and
    /// wake-deadline questions go for the rest of the run; it is released when
    /// the loop ends.
    ///
    /// Takes `self: Box<Self>` because some backends (e.g. winit) require
    /// ownership of the event loop to run it.
    ///
    /// Desktop event loops, including standalone AppKit, return after orderly
    /// shutdown. Backends tied to a page/process lifetime may not return.
    ///
    /// # Errors
    /// Propagates `ready`'s own `Err` as [`PlatformError::Bootstrap`], with
    /// the handler's error preserved as its `source` (a bootstrap failure —
    /// window creation, GPU init, root-widget attach — has no other return
    /// path back to `run`'s caller). Every backend stops entering, or promptly
    /// exits, its loop on that `Err` rather than continuing with a broken app:
    /// a returned error means the loop never ran a single iteration with the
    /// bootstrap incomplete. A backend may also return
    /// [`PlatformError::EventLoop`] for a platform-level failure unrelated to
    /// `ready`.
    fn run(self: Box<Self>, handler: Rc<dyn AppHandler>) -> Result<(), PlatformError>;

    /// Request the application to quit
    ///
    /// This may not quit immediately - the platform will clean up and then
    /// exit, and tell the handler ([`AppHandler::quit_requested`]).
    fn quit(&self);

    /// Ask the handler's [`AppHandler::exit_policy`] again on the event-loop
    /// owner thread, even though no window is closing right now.
    ///
    /// Exists because the exit policy is otherwise only asked when a window
    /// closes: an application whose *last window is already gone* can be held
    /// open by something the platform cannot see (issue #558's keep-alive
    /// application services), and when that holder releases — on a worker
    /// thread, with no window left to produce events — nothing would ever ask
    /// again, so the veto would be permanent and the process would linger
    /// forever. Backends that support this wake the owner loop; the owner then
    /// asks exactly as the window-close path does (only when its window
    /// bookkeeping is empty) and exits if the handler now allows it.
    ///
    /// Callable from **any thread**; coalesced (a burst of requests costs
    /// one re-check); always safe to over-call — a re-check while windows
    /// are still open, or while the handler still vetoes, is a no-op.
    ///
    /// The `winit`, AppKit and Win32 hosts wake their owner loop to re-check.
    /// Android, the web and iOS have no exit policy to ask again, so a
    /// keep-alive holder's release does not end the process there until an
    /// explicit [`quit`](Self::quit). The headless host parks the request; its
    /// embedder drives the actual re-check on the owner thread
    /// (`HeadlessExitReevaluation::drive`).
    fn request_exit_policy_reevaluation(&self);

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
    ) -> Result<Rc<dyn HostWindow>, OpenWindowError>;

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

    /// Activate the application (bring to front).
    ///
    /// Every host does nothing on this call, on purpose: an application is
    /// brought forward through one of its windows (`WindowCommand::Activate`),
    /// under the OS's focus rules, which refuse an application the user is not
    /// working with and turn the request into one for the user's attention.
    /// Windows grants the foreground only to the process in front, one it
    /// started or handed the right (`AllowSetForegroundWindow`); Wayland only
    /// to a window whose `xdg-activation` token the compositor accepts. AppKit
    /// activates the application once, as its loop starts, not on this call;
    /// UIKit and Android decide which application is in front; winit raises
    /// one window at a time; the Web has no way to.
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
    /// What the platform answers the application's reads of the system with,
    /// journalled while a native capture is installed (ADR-0215 §4).
    #[cfg(all(debug_assertions, feature = "native-capture"))]
    journal: crate::journal::capture::Journal,
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
            #[cfg(all(debug_assertions, feature = "native-capture"))]
            journal: crate::journal::capture::Journal::open_stream(
                flui_protocol::native_recording::Stream::Platform,
            ),
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
        target_arch = "wasm32",
        wayland_host,
        x11_host
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
            #[cfg(all(debug_assertions, feature = "native-capture"))]
            journal: crate::journal::capture::Journal::open_stream(
                flui_protocol::native_recording::Stream::Platform,
            ),
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

    fn run(self: Box<Self>, handler: Rc<dyn AppHandler>) -> Result<(), PlatformError> {
        let owner = self.shared_owner();
        let platform: Arc<dyn Platform> = Arc::<Self>::from(self);
        owner.run(platform, handler)
    }

    fn quit(&self) {
        self.owner.quit();
    }

    fn request_exit_policy_reevaluation(&self) {
        self.owner.request_exit_policy_reevaluation();
    }

    fn open_window(
        &self,
        options: WindowOptions,
        handler: Rc<dyn WindowHandler>,
    ) -> Result<Rc<dyn HostWindow>, OpenWindowError> {
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
        let read = H::Preferences::sample(&self.owner);
        #[cfg(all(debug_assertions, feature = "native-capture"))]
        self.journal
            .record(crate::journal::capture::Captured::Preferences(
                read.as_ref(),
            ));
        read
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

    fn release_session_end(&self) {
        H::Lifecycle::release_session_end(&self.owner);
    }

    fn window_capabilities(&self) -> WindowCapabilities {
        H::Window::capabilities(&self.owner, &self.window_shared)
    }

    fn window_frame_inset(&self, scale_factor: f64) -> Size<f64> {
        H::Window::frame_inset(&self.owner, &self.window_shared, scale_factor)
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
