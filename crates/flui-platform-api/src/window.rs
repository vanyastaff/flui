//! Window vocabulary: identity, creation options, modes, lifecycle state and
//! the events a backend reports about its windows.
//!
//! These are the values the per-window contract,
//! [`PlatformWindow`](crate::PlatformWindow), and the host-facing
//! `flui_platform::Platform` that opens windows (which stays in
//! `flui-platform`, ADR-0082 §2) use in their signatures.

use flui_types::geometry::{Bounds, DevicePixels, Pixels, Point, Size};

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

/// Window creation options
#[derive(Debug, Clone)]
pub struct WindowOptions {
    /// Window title
    pub title: String,
    /// Initial window size (logical pixels)
    pub size: Size<Pixels>,
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
            reveal: WindowReveal::AtOpen,
            decorated: true,
            min_size: None,
            max_size: None,
        }
    }
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

/// Window bounds state (windowed, maximized, or fullscreen)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WindowBounds {
    /// Normal windowed mode with specific bounds
    Windowed(Bounds<Pixels>),
    /// Maximized with bounds
    Maximized(Bounds<Pixels>),
    /// Fullscreen with bounds
    Fullscreen(Bounds<Pixels>),
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

/// Window events that can be observed via `flui_platform::Platform::on_window_event`
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
