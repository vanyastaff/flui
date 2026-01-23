//! Core platform abstraction trait
//!
//! Defines the central Platform trait that all platform implementations must provide.
//! This trait serves as the main interface between the FLUI framework and platform-specific code.

use super::{PlatformCapabilities, PlatformDisplay, PlatformWindow};
use anyhow::Result;
use flui_types::geometry::{DevicePixels, Pixels, Point, Size};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Window creation options
#[derive(Debug, Clone)]
pub struct WindowOptions {
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

/// Window identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowId(pub u64);

/// Core platform abstraction trait
///
/// This trait provides the complete interface for platform-specific operations.
/// All platform implementations (Winit, native Windows/macOS/Linux, headless testing)
/// must implement this trait.
///
/// # Architecture
///
/// The Platform trait follows several key design principles from GPUI:
///
/// - **Unified API**: Single trait for all platform operations
/// - **Callback registry**: Framework can register handlers without tight coupling
/// - **Interior mutability**: Implementations use Mutex/RwLock for thread-safe &self methods
/// - **Type erasure**: Returns `Box<dyn Trait>` for flexibility
///
/// # Example
///
/// ```rust,ignore
/// use flui_platform::{Platform, current_platform};
///
/// let platform = current_platform();
/// platform.run(Box::new(|| {
///     println!("Platform ready!");
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

    /// Get the platform's text rendering system
    fn text_system(&self) -> Arc<dyn PlatformTextSystem>;

    // ==================== Lifecycle ====================

    /// Run the platform event loop
    ///
    /// This function takes ownership of the current thread and runs the platform's
    /// event loop. The `on_ready` callback is invoked once the platform is initialized
    /// and ready to create windows.
    ///
    /// This function only returns when the application quits.
    fn run(&self, on_ready: Box<dyn FnOnce()>);

    /// Request the application to quit
    ///
    /// This may not quit immediately - the platform will clean up and then exit.
    fn quit(&self);

    /// Request a new frame to be rendered
    ///
    /// This is used for continuous rendering modes (e.g., animations, games).
    fn request_frame(&self);

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

    // ==================== Platform Capabilities ====================

    /// Get the platform's capabilities descriptor
    fn capabilities(&self) -> &dyn PlatformCapabilities;

    /// Get the platform's name for debugging/logging
    fn name(&self) -> &'static str;

    // ==================== Callbacks ====================

    /// Register a callback for when the application should quit
    fn on_quit(&self, callback: Box<dyn FnMut() + Send>);

    /// Register a callback for when the application is reopened (macOS)
    fn on_reopen(&self, callback: Box<dyn FnMut() + Send>) {
        // Default: no-op (desktop platforms don't support this)
        let _ = callback;
    }

    /// Register a callback for window events
    fn on_window_event(&self, callback: Box<dyn FnMut(WindowEvent) + Send>);

    // ==================== File System Integration ====================

    /// Reveal a path in the platform's file manager
    fn reveal_path(&self, path: &Path) {
        let _ = path; // Default: no-op
    }

    /// Open a path with the system's default application
    fn open_path(&self, path: &Path) {
        let _ = path; // Default: no-op
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
        window_id: WindowId,
    },

    /// Window was closed
    Closed(WindowId),

    /// Window focus changed
    FocusChanged {
        window_id: WindowId,
        focused: bool,
    },

    /// Window gained focus (deprecated, use FocusChanged)
    #[deprecated(note = "Use FocusChanged instead")]
    Focused(WindowId),

    /// Window lost focus (deprecated, use FocusChanged)
    #[deprecated(note = "Use FocusChanged instead")]
    Unfocused(WindowId),

    /// Window was resized (size in device pixels)
    Resized {
        window_id: WindowId,
        size: Size<DevicePixels>,
    },

    /// Window scale factor (DPI) changed
    ScaleFactorChanged {
        window_id: WindowId,
        scale_factor: f64,
    },

    /// Window needs to be redrawn
    RedrawRequested {
        window_id: WindowId,
    },

    /// Window was moved (position in logical pixels)
    Moved {
        id: WindowId,
        position: Point<Pixels>,
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

/// Platform text rendering system
///
/// This trait will be expanded later - for now it's a placeholder.
/// Real implementation will handle font loading, text shaping, etc.
pub trait PlatformTextSystem: Send + Sync {
    /// Get the system's default font family name
    fn default_font_family(&self) -> String {
        "sans-serif".to_string()
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
