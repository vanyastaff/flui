//! Window vocabulary: identity, creation options, lifecycle state and
//! the events a backend reports about its windows.
//!
//! These are the values the per-window contract,
//! [`PlatformWindow`](crate::PlatformWindow), and the host-facing
//! `flui_native::Platform` that opens windows (which stays in
//! `flui-native`, ADR-0082 §2) use in their signatures.

use flui_foundation::geometry::{Point, Size};

use crate::{WindowGeometry, WindowMode};

// ==================== Creation ====================

/// When a window opened [`WindowOptions::visible`]` == true` first becomes
/// visible to the viewer.
///
/// The choice is the caller's because the two callers differ in what they
/// can promise. An embedder that drives the frame loop (`flui-app`) reports
/// its first presented frame through
/// [`PlatformWindow::reveal_after_first_frame`](crate::PlatformWindow::reveal_after_first_frame),
/// so it may ask
/// for the reveal to wait for that frame and never show a bare background. A
/// direct consumer of the platform crate — an example, a probe, a test — has
/// no such report to give, and a window whose reveal waits for a call that
/// never comes is a window nobody sees; for those, the reveal happens at
/// open.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum WindowReveal {
    /// The window is on screen when `open_window` returns (the default).
    /// Every backend supports this.
    #[default]
    AtOpen,
    /// The window is ordered on screen but stays invisible to the viewer
    /// until [`PlatformWindow::reveal_after_first_frame`](crate::PlatformWindow::reveal_after_first_frame) is
    /// called; the caller commits to calling it (or to a bounded fallback
    /// that does). A backend that cannot defer treats this as
    /// [`Self::AtOpen`].
    AfterFirstFrame,
}

/// How a window trades pointer input latency against batching.
///
/// A touch or pen stroke drawn live (ink, a canvas) wants every sample the
/// hardware produced, as early as the system can deliver it; a list or a form
/// is served as well by the system's per-frame batches. A backend that cannot
/// deliver input earlier than it already does ignores [`Self::Low`], and its
/// [`WindowCapabilities`](crate::WindowCapabilities) are unchanged.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum InputLatency {
    /// The system's usual delivery: pointer samples batched once per frame.
    #[default]
    Balanced,
    /// Pointer samples as soon as the system has them: Android unbuffered
    /// dispatch, the web's `pointerrawupdate`.
    Low,
}

/// Window creation options.
///
/// Adding an option is not a breaking change: the type is `#[non_exhaustive]`,
/// so it is built from [`Default`] with the `with_*` methods.
///
/// # Geometry
///
/// [`size`](Self::size) is the **content** size: what
/// [`WindowState::content_size`](crate::WindowState::content_size) reports
/// after the window opens. The backend adds the window's frame (title bar and
/// borders) to ask the window system for the outer window, so reopening a
/// window with the size it last reported does not shrink it by its frame.
///
/// [`position`](Self::position) and
/// [`normal_geometry`](Self::normal_geometry) are measured from the origin of
/// the usable area of [`display`](Self::display) in that display's logical
/// pixels when a display is given, and in global logical pixels when it is
/// not. A backend converts through the display's own scale, so a desktop of
/// mixed scales needs no single global scale.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct WindowOptions {
    /// Window title
    pub title: String,
    /// Initial content size (logical pixels).
    pub size: Size<f64>,
    /// Whether window is resizable
    pub resizable: bool,
    /// Whether the window should be visible initially.
    ///
    /// `true` is the INTENDED state: with [`Self::reveal`] at its default
    /// the window is on screen when `open_window` returns; with
    /// [`WindowReveal::AfterFirstFrame`] a backend that can defer keeps it
    /// invisible to the viewer until the embedder reports its first
    /// presented frame, answering `is_visible() == true` meanwhile. `false`
    /// stays hidden until shown explicitly, whatever `reveal` says.
    pub visible: bool,
    /// When a `visible: true` window first becomes visible to the viewer;
    /// see [`WindowReveal`] for who should pick what. Ignored for
    /// `visible: false`.
    pub reveal: WindowReveal,
    /// Whether window is decorated (has title bar)
    pub decorated: bool,
    /// Minimum window size
    pub min_size: Option<Size<f64>>,
    /// Maximum window size
    pub max_size: Option<Size<f64>>,
    /// The display `position` and `normal_geometry` are measured on; `None`
    /// measures them in global logical pixels. A display that no longer exists
    /// is treated as `None`.
    pub display: Option<crate::DisplayId>,
    /// Initial outer position, relative to [`display`](Self::display); `None`
    /// lets the system place the window. A backend that cannot place a window
    /// at open ignores it, and says so through its
    /// [`WindowCapabilities::places_at_open`](crate::WindowCapabilities::places_at_open).
    pub position: Option<Point<f64>>,
    /// The mode the window opens in. A backend that cannot enter it opens the
    /// window in [`WindowMode::Normal`], which its first snapshot shows.
    pub mode: WindowMode,
    /// The outer rectangle the window returns to from a maximized or fullscreen
    /// opening mode, so a restored maximized window keeps its earlier bounds.
    /// Measured like [`position`](Self::position).
    pub normal_geometry: Option<WindowGeometry>,
    /// How early pointer samples reach the window; see [`InputLatency`].
    pub input_latency: InputLatency,
    /// What the window is created with on one operating system, as a value
    /// of that system's type; see [`OsWindowOptions`].
    pub os: OsWindowOptions,
}

impl WindowOptions {
    /// Set the title.
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Set the initial content size.
    #[must_use]
    pub const fn with_size(mut self, size: Size<f64>) -> Self {
        self.size = size;
        self
    }

    /// Set whether the window is resizable.
    #[must_use]
    pub const fn with_resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Set whether the window is visible initially.
    #[must_use]
    pub const fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// Set when a visible window first becomes visible to the viewer.
    #[must_use]
    pub const fn with_reveal(mut self, reveal: WindowReveal) -> Self {
        self.reveal = reveal;
        self
    }

    /// Set whether the window is decorated.
    #[must_use]
    pub const fn with_decorated(mut self, decorated: bool) -> Self {
        self.decorated = decorated;
        self
    }

    /// Set the minimum content size.
    #[must_use]
    pub const fn with_min_size(mut self, min_size: Option<Size<f64>>) -> Self {
        self.min_size = min_size;
        self
    }

    /// Set the maximum content size.
    #[must_use]
    pub const fn with_max_size(mut self, max_size: Option<Size<f64>>) -> Self {
        self.max_size = max_size;
        self
    }

    /// Set the display the position is measured on.
    #[must_use]
    pub const fn with_display(mut self, display: Option<crate::DisplayId>) -> Self {
        self.display = display;
        self
    }

    /// Set the initial outer position.
    #[must_use]
    pub const fn with_position(mut self, position: Option<Point<f64>>) -> Self {
        self.position = position;
        self
    }

    /// Set the mode the window opens in.
    #[must_use]
    pub const fn with_mode(mut self, mode: WindowMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set the rectangle a maximized or fullscreen window returns to.
    #[must_use]
    pub const fn with_normal_geometry(mut self, normal_geometry: Option<WindowGeometry>) -> Self {
        self.normal_geometry = normal_geometry;
        self
    }

    /// Set how early pointer samples reach the window.
    #[must_use]
    pub const fn with_input_latency(mut self, input_latency: InputLatency) -> Self {
        self.input_latency = input_latency;
        self
    }

    /// Set what the window is created with on one operating system.
    #[must_use]
    pub fn with_os(mut self, os: impl Into<OsWindowOptions>) -> Self {
        self.os = os.into();
        self
    }
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            title: "FLUI Window".to_string(),
            size: Size::new(800.0, 600.0),
            resizable: true,
            visible: true,
            reveal: WindowReveal::AtOpen,
            decorated: true,
            min_size: None,
            max_size: None,
            display: None,
            position: None,
            mode: WindowMode::Normal,
            normal_geometry: None,
            input_latency: InputLatency::Balanced,
            os: OsWindowOptions::None,
        }
    }
}

// ==================== One operating system's options ====================

/// What a window is created with on one operating system (ADR-0254 §8): one
/// value of that system's type, in [`WindowOptions::os`], rather than fields
/// of every system in the common options.
///
/// Only that system's host reads it. Another host (headless, winit, or the
/// host of another system) opens the window without it, and that system's
/// extension in `flui::native` has no view of the window it opened (`win32()`
/// answers `None`), so an application can tell whether its options were
/// taken.
///
/// ```
/// use flui_platform::{OsWindowOptions, Win32WindowOptions, WindowOptions, WindowsCornerPreference};
///
/// let options = WindowOptions::default().with_os(
///     Win32WindowOptions::default().with_corner_preference(WindowsCornerPreference::DoNotRound),
/// );
/// assert!(matches!(options.os, OsWindowOptions::Windows(_)));
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum OsWindowOptions {
    /// None beyond the common options (the default): every host opens the
    /// window as it does without any.
    #[default]
    None,
    /// What a Win32 window is created with.
    Windows(Win32WindowOptions),
}

impl From<Win32WindowOptions> for OsWindowOptions {
    fn from(options: Win32WindowOptions) -> Self {
        Self::Windows(options)
    }
}

/// What a Win32 window is created with beyond the common options.
///
/// The defaults are what a Win32 window gets without them: rounded corners,
/// and a title bar and borders drawn in the user's app theme.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Win32WindowOptions {
    /// How the desktop window manager rounds the window's corners (Windows 11
    /// build 22000 and later; earlier builds draw square corners).
    pub corner_preference: WindowsCornerPreference,
    /// Whether the title bar and borders are drawn light, dark, or in the
    /// user's app theme and with each later change of it.
    pub theme: WindowsTheme,
}

impl Win32WindowOptions {
    /// Set how the window's corners are rounded.
    #[must_use]
    pub const fn with_corner_preference(
        mut self,
        corner_preference: WindowsCornerPreference,
    ) -> Self {
        self.corner_preference = corner_preference;
        self
    }

    /// Set the theme the title bar and borders are drawn in.
    #[must_use]
    pub const fn with_theme(mut self, theme: WindowsTheme) -> Self {
        self.theme = theme;
        self
    }
}

impl Default for Win32WindowOptions {
    fn default() -> Self {
        Self {
            corner_preference: WindowsCornerPreference::Round,
            theme: WindowsTheme::System,
        }
    }
}

/// How the Windows 11 desktop window manager rounds a window's corners
/// (`DWM_WINDOW_CORNER_PREFERENCE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowsCornerPreference {
    /// As the system decides: rounded for a window with a title bar, square
    /// for one without.
    Default,
    /// Square corners.
    DoNotRound,
    /// Rounded corners.
    Round,
    /// Corners rounded with a smaller radius.
    RoundSmall,
}

/// The theme a Win32 window's title bar and borders are drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowsTheme {
    /// Light.
    Light,
    /// Dark.
    Dark,
    /// The user's app theme (Settings › Personalization › Colors), now and
    /// with each later change of it.
    System,
}

// ==================== Identity and mode ====================

/// Window identifier
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

// ==================== Lifecycle state ====================

/// Native execution eligibility, independent of focus, visibility and GPU surface readiness.
///
/// `Detached` is a reversible native attachment observation. Terminal window
/// closure is a separate lifetime event and cannot be reversed with this state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum WindowExecutionState {
    /// The platform permits UI execution; this does not guarantee a usable GPU surface.
    #[default]
    Running,
    /// UI execution is suspended, even if stale native focus/visibility remain true.
    Suspended,
    /// The native presentation is detached, but may subsequently attach again.
    Detached,
}

/// Failure to show an existing window without changing its display mode.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum WindowShowError {
    /// This backend cannot show and request focus for an existing window.
    #[error("showing an existing window is unsupported")]
    Unsupported,
    /// The native window has closed.
    #[error("the window is closed")]
    Closed,
    /// A native operation failed.
    #[error("could not show the window: {message}")]
    Native {
        /// Native failure description.
        message: String,
    },
}

// ==================== Value Types ====================

/// Window appearance (light/dark theme)
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum WindowAppearance {
    /// Light appearance (default)
    #[default]
    Light,
    /// Dark appearance
    Dark,
    /// Vibrant light (macOS-style translucent light)
    VibrantLight,
    /// Vibrant dark (macOS-style translucent dark)
    VibrantDark,
}

impl From<WindowAppearance> for crate::Brightness {
    /// The light/dark half of an appearance; vibrancy is a material, not a theme.
    fn from(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::Light,
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::Dark,
        }
    }
}

/// Window background appearance (backdrop material)
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum WindowBackgroundAppearance {
    /// Opaque background (default)
    #[default]
    Opaque,
    /// Transparent background
    Transparent,
    /// Blurred background
    Blurred,
    /// Windows 11 Mica backdrop
    MicaBackdrop,
    /// Windows 11 Mica Alt backdrop
    MicaAltBackdrop,
}

/// Failure to apply a cursor to one exact platform window.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CursorError {
    /// The backend has no pointer-cursor facility for this window.
    #[error("this platform window does not support pointer cursors")]
    Unsupported,
    /// The backend rejected a concrete cursor update.
    #[error("platform cursor update failed: {0}")]
    Backend(String),
}

// ==================== Events ====================

/// Window events that can be observed via `flui_native::Platform::on_window_event`
#[derive(Debug, Clone)]
pub enum WindowEvent {
    /// Window was created
    Created(WindowId),

    /// A close was *asked for* and the window's should-close veto passed.
    ///
    /// The rule is the veto, not the caller: this event accompanies every
    /// close that consults `on_should_close` and survives it. A refused close
    /// emits nothing at all.
    ///
    /// In practice that means the user route — a close button or compositor
    /// close on winit, `WM_CLOSE` on Win32, `simulate_close` on the headless
    /// double. A programmatic [`PlatformWindow::close`](crate::PlatformWindow::close) on the
    /// owning thread is a decision rather than a request, asks no veto, and
    /// so reports only [`Closed`](Self::Closed). The one case where a
    /// programmatic close does emit this is Win32's *cross-thread* route,
    /// which cannot call `DestroyWindow` and posts `WM_CLOSE` instead — so
    /// the veto is re-asked and the close becomes a request, exactly as that
    /// impl documents.
    ///
    /// Ordering: emitted BEFORE the window's own `on_close` callback runs,
    /// so a global handler observes the window still intact when told the
    /// close is going ahead; [`Closed`](Self::Closed) follows once the
    /// window has left the backend's tracking. (Earlier winit versions ran
    /// `on_close` first — a deliberate change, made when both close routes
    /// were unified into one teardown.)
    CloseRequested {
        /// The window whose close button was activated
        window_id: WindowId,
    },

    /// Window was closed — left the backend's tracking, whichever route
    /// (user-initiated or programmatic) took it there. Emitted after the
    /// window's own `on_close` callback and before the exit-policy consult.
    ///
    /// Emitted for *every* window that closes, not only the last one: a
    /// consumer tracking open windows by these events would otherwise believe
    /// a closed window is still open whenever another remains.
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
        size: Size<i32>,
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
}
