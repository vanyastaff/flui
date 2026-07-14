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

use super::{PlatformCapabilities, PlatformDisplay, PlatformWindow, window::WindowAppearance};
use crate::{cursor::CursorStyle, task::Task};

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

/// [`Platform::run`]'s ready callback: invoked once, synchronously, with a
/// platform handle. Named to keep `Box<dyn FnOnce(&dyn Platform)>` out of
/// every call site's signature.
pub type PlatformReadyCallback = Box<dyn FnOnce(&dyn Platform)>;

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
/// let platform = current_platform();
/// platform.run(Box::new(|platform| {
///     println!("Platform ready: {}", platform.name());
/// }));
/// ```
pub trait Platform: Send + Sync + 'static {
    // ==================== Core System ====================

    /// Get the platform's background executor for async tasks
    ///
    /// Background tasks run on a thread pool and can block.
    fn background_executor(&self) -> Arc<dyn PlatformExecutor>;

    /// Get the platform's foreground executor for UI tasks
    ///
    /// Foreground tasks run on the main thread and must not block.
    fn foreground_executor(&self) -> Arc<dyn PlatformExecutor>;

    // ==================== Lifecycle ====================

    /// Run the platform event loop
    ///
    /// This function takes ownership of the platform and the current thread,
    /// running the platform's event loop. The `on_ready` callback is invoked
    /// once the platform is initialized and ready to create windows, and is
    /// passed a platform handle so it can call `open_window`, `on_quit`, and
    /// other `&self` methods — the outer `Box<dyn Platform>` binding is no
    /// longer reachable once `run` has taken ownership of it.
    ///
    /// Takes `self: Box<Self>` because some backends (e.g. winit) require
    /// ownership of the event loop to run it.
    ///
    /// This function only returns when the application quits.
    fn run(self: Box<Self>, on_ready: PlatformReadyCallback);

    /// Request the application to quit
    ///
    /// This may not quit immediately - the platform will clean up and then
    /// exit.
    fn quit(&self);

    // ==================== Window Management ====================

    /// Create and open a new window
    ///
    /// Returns a boxed PlatformWindow implementation. The window is owned by
    /// the platform and will be destroyed when dropped.
    fn open_window(&self, options: WindowOptions) -> Result<Box<dyn PlatformWindow>>;

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

    // ==================== Cursor (US3) ====================

    /// Set the platform cursor style
    fn set_cursor_style(&self, style: CursorStyle) {
        let _ = style;
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
