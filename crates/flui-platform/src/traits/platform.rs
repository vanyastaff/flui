//! Core platform abstraction trait
//!
//! Defines the central Platform trait that all platform implementations must
//! provide. This trait serves as the main interface between the FLUI framework
//! and platform-specific code.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Result;
use flui_types::geometry::{Bounds, DevicePixels, Pixels, Point, Size};

use super::{
    OwnerPlatform, PlatformCapabilities, PlatformDisplay, PlatformWindow, window::WindowAppearance,
};
use crate::{data_transfer::DataTransferSource, task::Task};

/// Window creation options
#[derive(Debug, Clone)]
pub struct WindowOptions {
    // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked
    /// Window title
    pub title: String,
    /// Initial window size (logical pixels)
    pub size: Size<Pixels>,
    /// Whether window is resizable
    pub resizable: bool,
    /// Whether window should be visible initially
    pub visible: bool,
    /// Whether window is decorated (has title bar)
    pub decorated: bool,
    /// Minimum window size
    pub min_size: Option<Size<Pixels>>,
    /// Maximum window size
    pub max_size: Option<Size<Pixels>>,
}

impl Default for WindowOptions {
    fn default() -> Self {
        use flui_types::geometry::px;

        Self {
            title: "FLUI Window".to_string(),
            size: Size::new(px(800.0), px(600.0)),
            resizable: true,
            visible: true,
            decorated: true,
            min_size: None,
            max_size: None,
        }
    }
}

/// Window display mode with restoration data
///
/// Combines window state (normal/minimized/maximized/fullscreen) with the data
/// needed to restore from each state. This design ensures type-safety:
/// restoration data is only available when in the corresponding state.
///
/// Platform-specific restoration data (e.g., window style bits) should be
/// stored in the platform's own `WindowContext` or equivalent struct.
///
/// # Example
///
/// ```rust,ignore
/// match window_mode {
///     WindowMode::Normal => println!("Window is in normal state"),
///     WindowMode::Fullscreen { restore_bounds } => {
///         println!("Window is fullscreen, can restore to {:?}", restore_bounds);
///     }
///     _ => {}
/// }
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub enum WindowMode {
    /// Normal windowed state
    #[default]
    Normal,

    /// Window is minimized (iconified)
    Minimized {
        /// Bounds before minimization for restoration
        previous: Bounds<DevicePixels>,
    },

    /// Window is maximized
    Maximized {
        /// Bounds before maximization for restoration
        previous: Bounds<DevicePixels>,
    },

    /// Window is in fullscreen mode
    Fullscreen {
        /// Bounds before fullscreen for restoration
        restore_bounds: Bounds<DevicePixels>,
    },
}

impl WindowMode {
    /// Check if window is in fullscreen mode
    #[inline]
    pub fn is_fullscreen(&self) -> bool {
        matches!(self, WindowMode::Fullscreen { .. })
    }

    /// Check if window is minimized
    #[inline]
    pub fn is_minimized(&self) -> bool {
        matches!(self, WindowMode::Minimized { .. })
    }

    /// Check if window is maximized
    #[inline]
    pub fn is_maximized(&self) -> bool {
        matches!(self, WindowMode::Maximized { .. })
    }

    /// Check if window is in normal windowed mode
    #[inline]
    pub fn is_normal(&self) -> bool {
        matches!(self, WindowMode::Normal)
    }

    /// Validate if transition to new mode is allowed
    ///
    /// All transitions are currently allowed except transitioning to the same
    /// state. This method exists as a hook for adding transition
    /// restrictions in the future.
    pub fn can_transition_to(&self, new_mode: &WindowMode) -> bool {
        // All transitions allowed except same state
        !std::mem::discriminant(self).eq(&std::mem::discriminant(new_mode))
    }
}

/// Window identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowId(pub u64); // PORT-CHECK-OK-SP3: pre-existing parallel definition; consolidation tracked

/// [`Platform::run`]'s ready callback: invoked once, synchronously, with the
/// owner-thread capability (ADR-0039). Named to keep
/// `Box<dyn FnOnce(OwnerPlatform) -> anyhow::Result<()>>` out of every call
/// site's signature.
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
/// [`Platform::run`] itself instead: every backend stops entering (or
/// promptly exits) its loop on `Err` and hands the error back to `run`'s own
/// caller.
pub type PlatformReadyCallback = Box<dyn FnOnce(OwnerPlatform) -> anyhow::Result<()>>;

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
/// use flui_platform::{Platform, current_platform};
///
/// fn main() -> anyhow::Result<()> {
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
    /// This function only returns when the application quits (never, on
    /// backends whose native run loop does not return control — e.g. macOS,
    /// where `terminate:` exits the process).
    ///
    /// # Errors
    /// Propagates `on_ready`'s own `Err` (a bootstrap failure — window
    /// creation, GPU init, root-widget attach — has no other return path
    /// back to `run`'s caller). Every backend stops entering, or promptly
    /// exits, its loop on that `Err` rather than continuing with a broken
    /// app: a returned error means the loop never ran a single iteration
    /// with `on_ready`'s bootstrap incomplete. A backend may also return its
    /// own `Err` for a platform-level failure unrelated to `on_ready`.
    fn run(self: Box<Self>, on_ready: PlatformReadyCallback) -> anyhow::Result<()>;

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
    /// except `winit` and `headless` today — Win32/AppKit/Android/Web/iOS
    /// remain cross-typecheck-only for this specific mechanism, stated
    /// honestly rather than silently assumed) keeps its pre-existing
    /// unconditional "last window closed -> exit" behavior exactly as
    /// before; installing a hook there is inert.
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
    /// except `winit` and `headless` today — Win32/AppKit/Android/Web/iOS
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
    fn open_window(&self, options: WindowOptions) -> Result<Arc<dyn PlatformWindow>>;

    /// Get the currently active (focused) window ID
    fn active_window(&self) -> Option<WindowId>;

    /// Get all window IDs in z-order (front to back)
    ///
    /// Not all platforms support this (returns None).
    fn window_stack(&self) -> Option<Vec<WindowId>> {
        None
    }

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
    /// connection-like state (the offer table), and per ADR-0034 §3 such
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

    /// Hide the application
    fn hide(&self) {}

    /// Hide all other applications
    fn hide_other_apps(&self) {}

    /// Unhide all other applications
    fn unhide_other_apps(&self) {}

    // ==================== Appearance (US3) ====================

    /// Get the system window appearance (light/dark theme)
    fn window_appearance(&self) -> WindowAppearance {
        WindowAppearance::default()
    }

    /// Whether scrollbars should auto-hide
    fn should_auto_hide_scrollbars(&self) -> bool {
        false
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
    /// Returns selected paths, or `None` if the user cancelled.
    /// The dialog runs asynchronously on a background thread.
    fn prompt_for_paths(&self, options: PathPromptOptions) -> Task<Result<Option<Vec<PathBuf>>>> {
        let _ = options;
        Task::ready(Ok(None))
    }

    /// Show a "Save As" dialog for selecting a new file path
    ///
    /// Returns the selected path, or `None` if the user cancelled.
    fn prompt_for_new_path(
        &self,
        directory: &Path,
        suggested_name: Option<&str>,
    ) -> Task<Result<Option<PathBuf>>> {
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

    /// Register a callback for when the application is reopened (macOS)
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
    fn app_path(&self) -> Result<PathBuf>;
}

/// Window events that can be observed via Platform::on_window_event
#[derive(Debug, Clone)]
pub enum WindowEvent {
    /// Window was created
    Created(WindowId),

    /// Window close was requested (user clicked X button)
    CloseRequested {
        /// The window whose close button was activated
        window_id: WindowId,
    },

    /// Window was closed
    Closed(WindowId),

    /// Window focus changed
    FocusChanged {
        /// The window whose focus state changed
        window_id: WindowId,
        /// `true` if the window gained focus, `false` if it lost focus
        focused: bool,
    },

    /// Window was resized (size in device pixels)
    Resized {
        /// The window that was resized
        window_id: WindowId,
        /// New client-area size in device pixels
        size: Size<DevicePixels>,
    },

    /// Window scale factor (DPI) changed
    ScaleFactorChanged {
        /// The window whose scale factor changed
        window_id: WindowId,
        /// New device-pixel-per-logical-pixel ratio
        scale_factor: f64,
    },

    /// Window needs to be redrawn
    RedrawRequested {
        /// The window that must be repainted
        window_id: WindowId,
    },

    /// Window was moved (position in logical pixels)
    Moved {
        /// The window that was moved
        window_id: WindowId,
        /// New top-left position in logical pixels
        position: Point<Pixels>,
    },

    /// Window was minimized (iconified)
    Minimized {
        /// The window that was minimized
        window_id: WindowId,
    },

    /// Window was maximized
    Maximized {
        /// The window that was maximized
        window_id: WindowId,
        /// Maximized client-area size in device pixels
        size: Size<DevicePixels>,
    },

    /// Window was restored from minimized or maximized state
    Restored {
        /// The window that was restored
        window_id: WindowId,
        /// Restored client-area size in device pixels
        size: Size<DevicePixels>,
    },

    /// Window entered fullscreen mode
    Fullscreen {
        /// The window that entered fullscreen
        window_id: WindowId,
        /// Size of the fullscreen window (monitor size)
        size: Size<DevicePixels>,
    },

    /// Window exited fullscreen mode
    ExitFullscreen {
        /// The window that left fullscreen
        window_id: WindowId,
        /// Restored window size
        size: Size<DevicePixels>,
    },
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

/// Clipboard operations
pub trait Clipboard: Send + Sync {
    /// Read text from clipboard
    fn read_text(&self) -> Option<String>;

    /// Write text to clipboard
    fn write_text(&self, text: String);

    /// Check if clipboard has text
    fn has_text(&self) -> bool {
        self.read_text().is_some()
    }
}

/// Rich clipboard item with text content and optional metadata
///
/// Wraps clipboard content for cross-platform exchange. Currently supports
/// plain text; future versions will add images and custom MIME types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardItem {
    /// Plain text content
    text: Option<String>,
    /// Optional metadata (e.g., source application, MIME type hints)
    metadata: Option<String>,
}

impl ClipboardItem {
    /// Create a clipboard item from plain text
    pub fn text(content: String) -> Self {
        Self {
            text: Some(content),
            metadata: None,
        }
    }

    /// Create a clipboard item with text and metadata
    pub fn with_metadata(content: String, metadata: String) -> Self {
        Self {
            text: Some(content),
            metadata: Some(metadata),
        }
    }

    /// Get the text content, if any
    pub fn text_content(&self) -> Option<&str> {
        self.text.as_deref()
    }

    /// Get the metadata, if any
    pub fn metadata(&self) -> Option<&str> {
        self.metadata.as_deref()
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
