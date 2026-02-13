//! Core platform abstraction trait
//!
//! Defines the central Platform trait that all platform implementations must provide.
//! This trait serves as the main interface between the FLUI framework and platform-specific code.

use super::window::WindowAppearance;
use super::{PlatformCapabilities, PlatformDisplay, PlatformWindow};
use crate::cursor::CursorStyle;
use anyhow::Result;
use flui_types::geometry::{Bounds, DevicePixels, Pixels, Point, Size};
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

/// Window display mode with restoration data
///
/// Combines window state (normal/minimized/maximized/fullscreen) with the data
/// needed to restore from each state. This design ensures type-safety: restoration
/// data is only available when in the corresponding state.
///
/// # Platform-specific notes
///
/// - **Windows**: `restore_style` stores WS_* window style bits
/// - **macOS**: Would store NSWindow style mask
/// - **Linux**: Would store window manager hints
///
/// # Example
///
/// ```rust,ignore
/// match window_mode {
///     WindowMode::Normal => println!("Window is in normal state"),
///     WindowMode::Fullscreen { restore_style, restore_bounds } => {
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
        /// Window style bits before fullscreen (platform-specific)
        ///
        /// - Windows: WS_OVERLAPPEDWINDOW, WS_POPUP, etc.
        /// - macOS: NSWindowStyleMask bits
        /// - Linux: X11/Wayland window type atoms
        restore_style: u32,

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
    /// All transitions are currently allowed except transitioning to the same state.
    /// This method exists as a hook for adding transition restrictions in the future.
    pub fn can_transition_to(&self, new_mode: &WindowMode) -> bool {
        // All transitions allowed except same state
        !std::mem::discriminant(self).eq(&std::mem::discriminant(new_mode))
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
    CloseRequested { window_id: WindowId },

    /// Window was closed
    Closed(WindowId),

    /// Window focus changed
    FocusChanged { window_id: WindowId, focused: bool },

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
    RedrawRequested { window_id: WindowId },

    /// Window was moved (position in logical pixels)
    Moved {
        id: WindowId,
        position: Point<Pixels>,
    },

    /// Window was minimized (iconified)
    Minimized { window_id: WindowId },

    /// Window was maximized
    Maximized {
        window_id: WindowId,
        size: Size<DevicePixels>,
    },

    /// Window was restored from minimized or maximized state
    Restored {
        window_id: WindowId,
        size: Size<DevicePixels>,
    },

    /// Window entered fullscreen mode
    Fullscreen {
        window_id: WindowId,
        /// Size of the fullscreen window (monitor size)
        size: Size<DevicePixels>,
    },

    /// Window exited fullscreen mode
    ExitFullscreen {
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

// ==================== Font Types (US5) ====================

/// Unique identifier for a loaded font face
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontId(pub usize);

/// Unique identifier for a glyph within a font
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphId(pub u32);

/// Font descriptor for resolving a specific font face
#[derive(Debug, Clone)]
pub struct Font {
    /// Font family name (e.g., "Segoe UI", "Arial")
    pub family: String,
    /// Font weight (Normal, Bold, etc.)
    pub weight: FontWeight,
    /// Font style (Normal, Italic, Oblique)
    pub style: FontStyle,
}

/// Font weight variants
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FontWeight {
    /// Thin (100)
    Thin,
    /// Light (300)
    Light,
    /// Normal/Regular (400)
    #[default]
    Normal,
    /// Medium (500)
    Medium,
    /// SemiBold (600)
    SemiBold,
    /// Bold (700)
    Bold,
    /// ExtraBold (800)
    ExtraBold,
    /// Black (900)
    Black,
}

impl FontWeight {
    /// Convert to DirectWrite/CSS numeric weight
    pub fn to_numeric(self) -> u16 {
        match self {
            FontWeight::Thin => 100,
            FontWeight::Light => 300,
            FontWeight::Normal => 400,
            FontWeight::Medium => 500,
            FontWeight::SemiBold => 600,
            FontWeight::Bold => 700,
            FontWeight::ExtraBold => 800,
            FontWeight::Black => 900,
        }
    }
}

/// Font style variants
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FontStyle {
    /// Upright (normal)
    #[default]
    Normal,
    /// Italic
    Italic,
    /// Oblique (slanted)
    Oblique,
}

/// A run of text with a specific font
#[derive(Debug, Clone, Copy)]
pub struct FontRun {
    /// Font to use for this run
    pub font_id: FontId,
    /// Number of UTF-8 bytes in this run
    pub len: usize,
}

/// Font metrics in design units
#[derive(Debug, Clone, Copy)]
pub struct FontMetrics {
    /// Design units per em square
    pub units_per_em: u16,
    /// Ascent in design units (positive, above baseline)
    pub ascent: f32,
    /// Descent in design units (positive value, below baseline)
    pub descent: f32,
    /// Line gap in design units
    pub line_gap: f32,
    /// Underline position in design units (negative = below baseline)
    pub underline_position: f32,
    /// Underline thickness in design units
    pub underline_thickness: f32,
    /// Cap height in design units
    pub cap_height: f32,
    /// x-height in design units
    pub x_height: f32,
}

/// Result of laying out a single line of text
#[derive(Debug, Clone)]
pub struct LineLayout {
    /// Font size used for layout
    pub font_size: f32,
    /// Total width of the line in logical pixels
    pub width: f32,
    /// Ascent of the line in logical pixels
    pub ascent: f32,
    /// Descent of the line in logical pixels (positive value)
    pub descent: f32,
    /// Shaped glyph runs
    pub runs: Vec<ShapedRun>,
    /// Total number of UTF-8 bytes in the laid-out text
    pub len: usize,
}

/// A run of shaped glyphs with a single font
#[derive(Debug, Clone)]
pub struct ShapedRun {
    /// Font used for this run
    pub font_id: FontId,
    /// Shaped glyphs in this run
    pub glyphs: Vec<ShapedGlyph>,
}

/// A single shaped glyph with position
#[derive(Debug, Clone, Copy)]
pub struct ShapedGlyph {
    /// Glyph identifier in the font
    pub id: GlyphId,
    /// Horizontal position from line start (logical pixels)
    pub position_x: f32,
    /// Vertical position from baseline (logical pixels)
    pub position_y: f32,
    /// Index of the source character in the original text (byte offset)
    pub index: usize,
}

// ==================== PlatformTextSystem Trait (US5) ====================

/// Platform-native text measurement and glyph shaping abstraction
///
/// Platform implementations use native APIs:
/// - Windows: DirectWrite (`IDWriteFactory5`, `IDWriteTextLayout`)
/// - macOS: Core Text (`CTFont`, `CTLine`, `CTRun`)
/// - Linux: fontconfig + freetype
///
/// # Architecture
///
/// ```text
/// flui-platform (this trait) → System font discovery + text measurement
///         ↓
/// flui-text (future) → Font registry, text layout, glyph shaping
///         ↓
/// flui_painting → GPU rendering with wgpu
/// ```
pub trait PlatformTextSystem: Send + Sync {
    /// Load font data from raw bytes (TrueType/OpenType)
    ///
    /// Registers custom font data with the platform's font system.
    /// After loading, fonts can be resolved via `font_id()`.
    fn add_fonts(&self, fonts: Vec<std::borrow::Cow<'static, [u8]>>) -> anyhow::Result<()>;

    /// List all available font family names
    ///
    /// Returns names from both system fonts and custom-loaded fonts.
    fn all_font_names(&self) -> Vec<String>;

    /// Resolve a font descriptor to a FontId
    ///
    /// Matches the requested family/weight/style to the closest available font.
    fn font_id(&self, descriptor: &Font) -> anyhow::Result<FontId>;

    /// Get metrics for a loaded font
    ///
    /// Returns design-unit metrics (ascent, descent, line gap, etc.).
    fn font_metrics(&self, font_id: FontId) -> FontMetrics;

    /// Map a character to its glyph ID in a font
    ///
    /// Returns `None` if the font does not contain a glyph for this character.
    fn glyph_for_char(&self, font_id: FontId, ch: char) -> Option<GlyphId>;

    /// Layout a single line of text with font runs
    ///
    /// Shapes text into positioned glyphs, handling kerning and ligatures.
    /// Each `FontRun` specifies a font and byte length for a segment of text.
    fn layout_line(&self, text: &str, font_size: f32, runs: &[FontRun]) -> LineLayout;
}

/// Text system errors
#[derive(Debug, Clone, PartialEq)]
pub enum TextSystemError {
    /// Feature not yet implemented (MVP stub)
    NotImplemented,

    /// Font family not found on system
    FontNotFound(String),

    /// Failed to load font data
    LoadFailed(String),

    /// Platform API error (DirectWrite, Core Text, etc.)
    PlatformError(String),
}

impl std::fmt::Display for TextSystemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TextSystemError::NotImplemented => {
                write!(f, "Text system feature not implemented (MVP stub)")
            }
            TextSystemError::FontNotFound(family) => {
                write!(f, "Font family '{}' not found on system", family)
            }
            TextSystemError::LoadFailed(msg) => {
                write!(f, "Failed to load font: {}", msg)
            }
            TextSystemError::PlatformError(msg) => {
                write!(f, "Platform text system error: {}", msg)
            }
        }
    }
}

impl std::error::Error for TextSystemError {}

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
