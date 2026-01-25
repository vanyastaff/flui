//! Cross-platform window abstraction
//!
//! This module defines the core `Window` trait that all platform implementations must provide.
//! Platform-specific extensions are available through separate extension traits.
//!
//! # Architecture
//!
//! ```text
//! Window (trait)               - Core cross-platform API
//!   ├── MacOSWindow            - macOS implementation
//!   │   └── MacOSWindowExt     - macOS-specific extensions
//!   ├── WindowsWindow          - Windows implementation
//!   │   └── WindowsWindowExt   - Windows-specific extensions
//!   ├── LinuxWindow            - Linux implementation
//!   │   └── LinuxWindowExt     - Linux-specific extensions
//!   ├── AndroidWindow          - Android implementation
//!   └── WebWindow              - Web implementation
//! ```
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_platform::{Window, WindowState};
//!
//! // Cross-platform API works everywhere
//! window.set_title("My App");
//! window.set_state(WindowState::Maximized);
//! window.set_visible(true);
//!
//! // Platform-specific features
//! #[cfg(target_os = "macos")]
//! {
//!     use flui_platform::macos::MacOSWindowExt;
//!     window.set_liquid_glass(LiquidGlassMaterial::Standard);
//! }
//! ```

use flui_types::geometry::{Point, Rect, Size};
use flui_types::Pixels;

// ============================================================================
// Core Window Trait (Cross-Platform)
// ============================================================================

/// Cross-platform window abstraction.
///
/// This trait defines the core functionality that all platform window implementations
/// must provide. Platform-specific features are available through extension traits.
pub trait Window {
    /// Get the window's unique identifier.
    fn id(&self) -> WindowId;

    /// Get the window title.
    fn title(&self) -> String;

    /// Set the window title.
    fn set_title(&mut self, title: &str);

    /// Get the window's current position (top-left corner in screen coordinates).
    fn position(&self) -> Point<Pixels>;

    /// Set the window's position.
    fn set_position(&mut self, position: Point<Pixels>);

    /// Get the window's current size.
    fn size(&self) -> Size<Pixels>;

    /// Set the window's size.
    fn set_size(&mut self, size: Size<Pixels>);

    /// Get the window's bounds (position + size).
    fn bounds(&self) -> Rect<Pixels> {
        Rect::from_origin_size(self.position(), self.size())
    }

    /// Set the window's bounds (position + size).
    fn set_bounds(&mut self, bounds: Rect<Pixels>) {
        self.set_position(bounds.origin());
        self.set_size(bounds.size());
    }

    /// Get the window's current state (normal, minimized, maximized, fullscreen).
    fn state(&self) -> WindowState;

    /// Set the window's state.
    fn set_state(&mut self, state: WindowState);

    /// Minimize the window.
    fn minimize(&mut self) {
        self.set_state(WindowState::Minimized);
    }

    /// Maximize the window.
    fn maximize(&mut self) {
        self.set_state(WindowState::Maximized);
    }

    /// Restore the window to normal state.
    fn restore(&mut self) {
        self.set_state(WindowState::Normal);
    }

    /// Enter fullscreen mode.
    fn set_fullscreen(&mut self, fullscreen: bool) {
        if fullscreen {
            self.set_state(WindowState::Fullscreen);
        } else {
            self.set_state(WindowState::Normal);
        }
    }

    /// Check if the window is in fullscreen mode.
    fn is_fullscreen(&self) -> bool {
        matches!(self.state(), WindowState::Fullscreen)
    }

    /// Check if the window is minimized.
    fn is_minimized(&self) -> bool {
        matches!(self.state(), WindowState::Minimized)
    }

    /// Check if the window is maximized.
    fn is_maximized(&self) -> bool {
        matches!(self.state(), WindowState::Maximized)
    }

    /// Check if the window is visible.
    fn is_visible(&self) -> bool;

    /// Set the window's visibility.
    fn set_visible(&mut self, visible: bool);

    /// Show the window (make it visible).
    fn show(&mut self) {
        self.set_visible(true);
    }

    /// Hide the window.
    fn hide(&mut self) {
        self.set_visible(false);
    }

    /// Check if the window can be resized by the user.
    fn is_resizable(&self) -> bool;

    /// Set whether the window can be resized.
    fn set_resizable(&mut self, resizable: bool);

    /// Check if the window can be minimized.
    fn is_minimizable(&self) -> bool;

    /// Set whether the window can be minimized.
    fn set_minimizable(&mut self, minimizable: bool);

    /// Check if the window can be closed.
    fn is_closable(&self) -> bool;

    /// Set whether the window can be closed.
    fn set_closable(&mut self, closable: bool);

    /// Focus the window (bring to front and activate).
    fn focus(&mut self);

    /// Check if the window is currently focused.
    fn is_focused(&self) -> bool;

    /// Close the window.
    ///
    /// This will trigger the window close event and destroy the window
    /// if not prevented by the application.
    fn close(&mut self);

    /// Request the window to redraw.
    fn request_redraw(&mut self);

    /// Set minimum window size.
    fn set_min_size(&mut self, size: Option<Size<Pixels>>);

    /// Set maximum window size.
    fn set_max_size(&mut self, size: Option<Size<Pixels>>);

    /// Get the window's content scale factor (DPI scale).
    ///
    /// Returns 1.0 for standard DPI, 2.0 for Retina/HiDPI displays.
    fn scale_factor(&self) -> f32;

    /// Get the window's raw handle for GPU integration.
    ///
    /// Returns a raw window handle compatible with `raw-window-handle` crate.
    fn raw_window_handle(&self) -> RawWindowHandle;
}

// ============================================================================
// Window ID
// ============================================================================

/// Unique identifier for a window.
///
/// This ID is unique within the application and persists for the window's lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowId(pub u64);

impl WindowId {
    /// Create a new window ID.
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    /// Get the raw ID value.
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

// ============================================================================
// Window State
// ============================================================================

/// Window state (normal, minimized, maximized, fullscreen).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowState {
    /// Normal window state (not minimized, maximized, or fullscreen).
    Normal,

    /// Window is minimized to taskbar/dock.
    Minimized,

    /// Window is maximized to fill the screen (excluding taskbar/menu bar).
    Maximized,

    /// Window is in fullscreen mode (covers entire screen).
    Fullscreen,
}

impl WindowState {
    /// Get a human-readable description of the state.
    pub fn as_str(&self) -> &str {
        match self {
            WindowState::Normal => "Normal",
            WindowState::Minimized => "Minimized",
            WindowState::Maximized => "Maximized",
            WindowState::Fullscreen => "Fullscreen",
        }
    }
}

// ============================================================================
// Raw Window Handle
// ============================================================================

/// Raw window handle for GPU integration.
///
/// This is a simplified version. In production, use the `raw-window-handle` crate.
#[derive(Debug, Clone, Copy)]
pub enum RawWindowHandle {
    /// macOS NSView handle.
    #[cfg(target_os = "macos")]
    MacOS {
        /// Pointer to NSView.
        ns_view: *mut std::ffi::c_void,
        /// Pointer to NSWindow.
        ns_window: *mut std::ffi::c_void,
    },

    /// Windows HWND handle.
    #[cfg(target_os = "windows")]
    Windows {
        /// Window handle (HWND).
        hwnd: *mut std::ffi::c_void,
        /// Module instance (HINSTANCE).
        hinstance: *mut std::ffi::c_void,
    },

    /// X11 window handle.
    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
    X11 {
        /// X11 Window ID.
        window: u64,
        /// X11 Display pointer.
        display: *mut std::ffi::c_void,
    },

    /// Wayland window handle.
    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
    Wayland {
        /// Wayland surface pointer.
        surface: *mut std::ffi::c_void,
        /// Wayland display pointer.
        display: *mut std::ffi::c_void,
    },

    /// Android window handle.
    #[cfg(target_os = "android")]
    Android {
        /// Pointer to ANativeWindow.
        a_native_window: *mut std::ffi::c_void,
    },

    /// Web canvas handle.
    #[cfg(target_arch = "wasm32")]
    Web {
        /// Canvas element ID.
        id: u32,
    },
}

// SAFETY: Raw window handles are just pointers and can be sent between threads
unsafe impl Send for RawWindowHandle {}
unsafe impl Sync for RawWindowHandle {}

// ============================================================================
// Window Builder
// ============================================================================

/// Builder for creating windows with specific options.
#[derive(Debug, Clone)]
pub struct WindowBuilder {
    /// Window title.
    pub title: String,

    /// Initial position (None = auto-position).
    pub position: Option<Point<Pixels>>,

    /// Initial size.
    pub size: Size<Pixels>,

    /// Initial state.
    pub state: WindowState,

    /// Start visible.
    pub visible: bool,

    /// Allow resizing.
    pub resizable: bool,

    /// Allow minimizing.
    pub minimizable: bool,

    /// Allow closing.
    pub closable: bool,

    /// Minimum size.
    pub min_size: Option<Size<Pixels>>,

    /// Maximum size.
    pub max_size: Option<Size<Pixels>>,
}

impl WindowBuilder {
    /// Create a new window builder with default settings.
    pub fn new() -> Self {
        Self {
            title: "FLUI Window".to_string(),
            position: None,
            size: Size::new(Pixels(800.0), Pixels(600.0)),
            state: WindowState::Normal,
            visible: true,
            resizable: true,
            minimizable: true,
            closable: true,
            min_size: None,
            max_size: None,
        }
    }

    /// Set the window title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Set the window position.
    pub fn with_position(mut self, position: Point<Pixels>) -> Self {
        self.position = Some(position);
        self
    }

    /// Set the window size.
    pub fn with_size(mut self, size: Size<Pixels>) -> Self {
        self.size = size;
        self
    }

    /// Set the initial window state.
    pub fn with_state(mut self, state: WindowState) -> Self {
        self.state = state;
        self
    }

    /// Set whether the window starts visible.
    pub fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// Set whether the window can be resized.
    pub fn with_resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Set minimum size.
    pub fn with_min_size(mut self, size: Size<Pixels>) -> Self {
        self.min_size = Some(size);
        self
    }

    /// Set maximum size.
    pub fn with_max_size(mut self, size: Size<Pixels>) -> Self {
        self.max_size = Some(size);
        self
    }
}

impl Default for WindowBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Window Manager
// ============================================================================

/// Cross-platform window manager trait.
///
/// Manages multiple windows, focus, and window coordination.
/// Platform implementations provide the actual window creation and management.
pub trait WindowManager {
    /// The window type this manager handles.
    type Window: Window;

    /// Create a new window with the given builder options.
    ///
    /// Returns the window ID on success.
    fn create_window(&mut self, builder: WindowBuilder) -> Result<WindowId, WindowError>;

    /// Get a window by ID.
    fn get_window(&self, id: WindowId) -> Option<&Self::Window>;

    /// Get a mutable reference to a window by ID.
    fn get_window_mut(&mut self, id: WindowId) -> Option<&mut Self::Window>;

    /// Close and destroy a window.
    ///
    /// Returns true if the window was found and closed.
    fn close_window(&mut self, id: WindowId) -> bool;

    /// Get all window IDs managed by this manager.
    fn all_windows(&self) -> Vec<WindowId>;

    /// Get the number of windows.
    fn window_count(&self) -> usize {
        self.all_windows().len()
    }

    /// Get the currently focused window ID.
    fn focused_window(&self) -> Option<WindowId>;

    /// Focus a specific window.
    ///
    /// Returns true if the window was found and focused.
    fn focus_window(&mut self, id: WindowId) -> bool;

    /// Calculate a cascade position for a new window.
    ///
    /// This provides the standard "staircase" positioning for new windows.
    fn calculate_cascade_position(&self, _window_size: Size<Pixels>) -> Point<Pixels> {
        let count = self.window_count();
        let cascade_offset = 28.0; // Standard cascade offset

        let x = 100.0 + (count as f32 * cascade_offset);
        let y = 100.0 + (count as f32 * cascade_offset);

        Point::new(Pixels(x), Pixels(y))
    }

    /// Find windows by title (partial match).
    fn find_by_title(&self, title: &str) -> Vec<WindowId> {
        self.all_windows()
            .into_iter()
            .filter(|&id| {
                if let Some(window) = self.get_window(id) {
                    window.title().contains(title)
                } else {
                    false
                }
            })
            .collect()
    }

    /// Get all visible windows.
    fn visible_windows(&self) -> Vec<WindowId> {
        self.all_windows()
            .into_iter()
            .filter(|&id| {
                if let Some(window) = self.get_window(id) {
                    window.is_visible()
                } else {
                    false
                }
            })
            .collect()
    }

    /// Get all focused windows (windows that can receive input).
    fn focused_windows(&self) -> Vec<WindowId> {
        self.all_windows()
            .into_iter()
            .filter(|&id| {
                if let Some(window) = self.get_window(id) {
                    window.is_focused()
                } else {
                    false
                }
            })
            .collect()
    }
}

// ============================================================================
// Window Error
// ============================================================================

/// Errors that can occur during window operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowError {
    /// Window creation failed.
    CreationFailed(String),

    /// Window not found.
    NotFound(WindowId),

    /// Invalid window state.
    InvalidState(String),

    /// Platform-specific error.
    PlatformError(String),
}

impl std::fmt::Display for WindowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WindowError::CreationFailed(msg) => write!(f, "Window creation failed: {}", msg),
            WindowError::NotFound(id) => write!(f, "Window not found: {:?}", id),
            WindowError::InvalidState(msg) => write!(f, "Invalid window state: {}", msg),
            WindowError::PlatformError(msg) => write!(f, "Platform error: {}", msg),
        }
    }
}

impl std::error::Error for WindowError {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_state_display() {
        assert_eq!(WindowState::Normal.as_str(), "Normal");
        assert_eq!(WindowState::Minimized.as_str(), "Minimized");
        assert_eq!(WindowState::Maximized.as_str(), "Maximized");
        assert_eq!(WindowState::Fullscreen.as_str(), "Fullscreen");
    }

    #[test]
    fn test_window_id() {
        let id1 = WindowId::new(123);
        let id2 = WindowId::new(123);
        let id3 = WindowId::new(456);

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);
        assert_eq!(id1.as_u64(), 123);
    }

    #[test]
    fn test_window_builder_defaults() {
        let builder = WindowBuilder::new();
        assert_eq!(builder.title, "FLUI Window");
        assert_eq!(builder.size.width, Pixels(800.0));
        assert_eq!(builder.size.height, Pixels(600.0));
        assert_eq!(builder.state, WindowState::Normal);
        assert!(builder.visible);
        assert!(builder.resizable);
    }

    #[test]
    fn test_window_builder_customization() {
        let builder = WindowBuilder::new()
            .with_title("Custom Window")
            .with_size(Size::new(Pixels(1024.0), Pixels(768.0)))
            .with_position(Point::new(Pixels(100.0), Pixels(100.0)))
            .with_resizable(false)
            .with_state(WindowState::Maximized);

        assert_eq!(builder.title, "Custom Window");
        assert_eq!(builder.size.width, Pixels(1024.0));
        assert_eq!(builder.position, Some(Point::new(Pixels(100.0), Pixels(100.0))));
        assert!(!builder.resizable);
        assert_eq!(builder.state, WindowState::Maximized);
    }
}
