//! The window state contract: an immutable snapshot a window publishes, the
//! commands that change it and the capabilities that say which commands the
//! window honours (ADR-0237).
//!
//! # Snapshot
//!
//! [`WindowState`] describes what the operating system reports about a window,
//! never what code asked for: a size the OS clamped is the clamped size, and a
//! maximize the OS refused leaves the mode where it was. It holds every fact of
//! the window (ADR-0254 §1): geometry and mode, the drawable metrics and scale,
//! appearance, display, safe area, execution state, focus, activation, visibility, hover
//! and the [`WindowCapabilities`]. A change publishes a new snapshot with a
//! higher [`WindowRevision`] and one payload-free `on_state_changed`
//! notification; a sample equal to the published one keeps the revision
//! ([`WindowState::successor`]). The revision never wraps: the last one is
//! reached once, then the snapshot refuses further change.
//!
//! # Commands
//!
//! Every change is a [`WindowCommand`] applied through
//! [`PlatformWindow::apply`](crate::PlatformWindow::apply). A window answers
//! [`WindowCommandError::Unsupported`] exactly when the matching
//! [`WindowCapabilities`] field is [`Availability::Unavailable`], so a caller
//! can ask up front and is never told "done" for a no-op. An accepted command
//! is a request: its outcome is visible only in the next snapshot.

use flui_foundation::geometry::{EdgeInsets, Point, Size, WindowMetrics};

use crate::{
    Availability, DisplayId, Unsupported, UnsupportedReason, WindowAppearance, WindowExecutionState,
};

/// The monotone version of a [`WindowState`]. Never wraps.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WindowRevision(u64);

impl WindowRevision {
    /// The revision of a window that has published nothing yet.
    pub const ZERO: Self = Self(0);

    /// The revision number.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// The revision after this one, or `None` at the last.
    #[must_use]
    pub const fn next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(next) => Some(Self(next)),
            None => None,
        }
    }

    /// The last revision a window can publish.
    pub const LAST: Self = Self(u64::MAX);
}

/// A window state cannot advance: the last revision was reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("window revisions exhausted")]
pub struct WindowRevisionExhausted;

/// How a window occupies its display.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WindowMode {
    /// A window with its own bounds (the default).
    #[default]
    Normal,
    /// Iconified or minimized to the task bar or dock.
    Minimized,
    /// Filling the display's usable area, with its frame.
    Maximized,
    /// Borderless on the whole display. Exclusive video-mode changes are not
    /// part of this contract.
    Fullscreen,
}

/// A window rectangle in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindowGeometry {
    /// The top-left corner in global screen coordinates; `None` where the OS
    /// reports no global position (Wayland, Android, iOS).
    pub position: Option<Point<f64>>,
    /// The extent.
    pub size: Size<f64>,
}

impl WindowGeometry {
    /// A rectangle with this position and size.
    #[must_use]
    pub const fn new(position: Option<Point<f64>>, size: Size<f64>) -> Self {
        Self { position, size }
    }
}

/// The inner-size limits of a window, in logical pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SizeConstraints {
    /// The smallest inner size, if limited.
    pub min: Option<Size<f64>>,
    /// The largest inner size, if limited.
    pub max: Option<Size<f64>>,
}

impl SizeConstraints {
    /// No limits.
    pub const NONE: Self = Self {
        min: None,
        max: None,
    };

    /// Limits with these bounds.
    #[must_use]
    pub const fn new(min: Option<Size<f64>>, max: Option<Size<f64>>) -> Self {
        Self { min, max }
    }
}

/// What the operating system reports about one window, at one revision.
///
/// Built with [`new`](Self::new) and the `with_*` methods so a field can be
/// added without breaking a backend.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct WindowState {
    /// This snapshot's version.
    pub revision: WindowRevision,
    /// The window's outer rectangle (frame included).
    pub outer: WindowGeometry,
    /// The content (client) area extent.
    pub content_size: Size<f64>,
    /// How the window occupies its display.
    pub mode: WindowMode,
    /// The rectangle the window returns to from a maximized or fullscreen
    /// mode, when the OS reports it.
    pub normal_geometry: Option<WindowGeometry>,
    /// Whether the window is shown.
    pub visible: bool,
    /// Whether the window has input focus.
    pub focused: bool,
    /// The title; `None` where the OS has no window title.
    pub title: Option<String>,
    /// The inner-size limits the OS enforces.
    pub constraints: SizeConstraints,
    /// Whether the user can resize the window.
    pub resizable: bool,
    /// Whether the window has its frame and title bar.
    pub decorated: bool,
    /// The display the window is on, named by the display snapshot's identity
    /// (ADR-0236); `None` when the platform cannot tell, which a consumer reads
    /// as an unknown display, never as the primary one.
    pub display: Option<DisplayId>,
    /// The drawable area in device pixels and its ratio to logical pixels, as
    /// the window last reported a drawable one; `None` until it has. A report
    /// that is not a drawable area (a minimized window's zero size, a scale
    /// that is not finite and positive) is never published:
    /// [`successor`](Self::successor) keeps the metrics before it.
    pub metrics: Option<WindowMetrics>,
    /// The light/dark appearance the system applies to the window: the user's
    /// theme as applied to it, never one the backend chose for its own chrome
    /// (ADR-0222). A backend that cannot observe the theme reports
    /// [`WindowAppearance::Light`].
    pub appearance: WindowAppearance,
    /// Safe-area intrusions in logical pixels, relative to the content area;
    /// zero where the platform reports none. Not keyboard occlusion.
    pub safe_area: EdgeInsets,
    /// Whether the platform lets the window run its UI, independent of focus,
    /// visibility and the GPU surface (ADR-0035).
    pub execution: WindowExecutionState,
    /// Whether the pointer is over the window's content.
    pub hovered: bool,
    /// Whether the window is the active window of its desktop: the one the
    /// user works in, holding the keyboard itself or through a window it owns
    /// (a Win32 window whose owned popup has the focus is active and not
    /// focused). Where the window system has no notion apart from focus, it
    /// is the focus.
    pub active: bool,
    /// Which commands the window honours. A change in what it honours (a
    /// compositor revising what it allows) is a state change like any other.
    pub capabilities: WindowCapabilities,
}

impl WindowState {
    /// A visible, unfocused, inactive, unhovered, running, resizable, decorated window
    /// in normal mode at revision zero, with no title, limits, display,
    /// metrics or safe area, the light appearance, and capabilities that
    /// honour nothing.
    #[must_use]
    pub fn new(outer: WindowGeometry, content_size: Size<f64>) -> Self {
        Self {
            revision: WindowRevision::ZERO,
            outer,
            content_size,
            mode: WindowMode::Normal,
            normal_geometry: None,
            visible: true,
            focused: false,
            title: None,
            constraints: SizeConstraints::NONE,
            resizable: true,
            decorated: true,
            display: None,
            metrics: None,
            appearance: WindowAppearance::Light,
            safe_area: EdgeInsets::ZERO,
            execution: WindowExecutionState::Running,
            hovered: false,
            active: false,
            capabilities: WindowCapabilities::NONE,
        }
    }

    /// Set the mode.
    #[must_use]
    pub fn with_mode(mut self, mode: WindowMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set the rectangle the window returns to from maximized or fullscreen.
    #[must_use]
    pub fn with_normal_geometry(mut self, normal_geometry: Option<WindowGeometry>) -> Self {
        self.normal_geometry = normal_geometry;
        self
    }

    /// Set whether the window is shown.
    #[must_use]
    pub fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// Set whether the window has input focus.
    #[must_use]
    pub fn with_focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Set the title.
    #[must_use]
    pub fn with_title(mut self, title: Option<String>) -> Self {
        self.title = title;
        self
    }

    /// Set the size limits.
    #[must_use]
    pub fn with_constraints(mut self, constraints: SizeConstraints) -> Self {
        self.constraints = constraints;
        self
    }

    /// Set whether the user can resize the window.
    #[must_use]
    pub fn with_resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Set whether the window has its frame.
    #[must_use]
    pub fn with_decorated(mut self, decorated: bool) -> Self {
        self.decorated = decorated;
        self
    }

    /// Set the display.
    #[must_use]
    pub fn with_display(mut self, display: Option<DisplayId>) -> Self {
        self.display = display;
        self
    }

    /// Set the drawable metrics; `None` for a report that is not a drawable
    /// area, which [`successor`](Self::successor) does not publish.
    #[must_use]
    pub fn with_metrics(mut self, metrics: Option<WindowMetrics>) -> Self {
        self.metrics = metrics;
        self
    }

    /// Set the appearance the system applies to the window.
    #[must_use]
    pub fn with_appearance(mut self, appearance: WindowAppearance) -> Self {
        self.appearance = appearance;
        self
    }

    /// Set the safe-area insets.
    #[must_use]
    pub fn with_safe_area(mut self, safe_area: EdgeInsets) -> Self {
        self.safe_area = safe_area;
        self
    }

    /// Set the execution state.
    #[must_use]
    pub fn with_execution(mut self, execution: WindowExecutionState) -> Self {
        self.execution = execution;
        self
    }

    /// Set whether the pointer is over the window.
    #[must_use]
    pub fn with_hovered(mut self, hovered: bool) -> Self {
        self.hovered = hovered;
        self
    }

    /// Set whether the window is the active window of its desktop.
    #[must_use]
    pub fn with_active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Set which commands the window honours.
    #[must_use]
    pub fn with_capabilities(mut self, capabilities: WindowCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Whether the window's content can be seen: it is shown and not
    /// minimized. A window manager may keep a minimized window shown; its
    /// content is still not on screen, so its frames are not drawn.
    #[must_use]
    pub fn is_seen(&self) -> bool {
        self.visible && self.mode != WindowMode::Minimized
    }

    /// The snapshot that follows this one with `sample`: this very value when
    /// `sample` differs from it in nothing but the revision, otherwise
    /// `sample` at the next revision. The revision `sample` carries is
    /// ignored, and a sample without [`metrics`](Self::metrics) (the window
    /// reported no drawable area) keeps this snapshot's: a report that is not
    /// a drawable area is never published.
    ///
    /// # Errors
    /// [`WindowRevisionExhausted`] when `sample` differs and this snapshot is
    /// at the last revision. The revision never wraps.
    pub fn successor(&self, mut sample: Self) -> Result<Self, WindowRevisionExhausted> {
        sample.revision = self.revision;
        if sample.metrics.is_none() {
            sample.metrics = self.metrics;
        }
        if sample == *self {
            return Ok(sample);
        }
        sample.revision = self.revision.next().ok_or(WindowRevisionExhausted)?;
        Ok(sample)
    }
}

/// Which commands a window honours, and how.
///
/// A backend reports exactly what it does today. Each field is an
/// [`Availability`]: [`Native`](Availability::Native) when the window system
/// carries the command out (or makes the report) itself,
/// [`Emulated`](Availability::Emulated) when the backend builds it from what
/// the window system does provide (a fullscreen made of a borderless window
/// sized to its monitor), and [`Unavailable`](Availability::Unavailable)
/// exactly when [`apply`](crate::PlatformWindow::apply) answers
/// [`WindowCommandError::Unsupported`]; a boolean could not say "only
/// approximately". Fields are public so a backend can start from
/// [`NONE`](Self::NONE) or [`ALL`](Self::ALL) and set the difference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct WindowCapabilities {
    /// [`WindowCommand::SetTitle`].
    pub sets_title: Availability,
    /// [`WindowCommand::SetPosition`].
    pub sets_position: Availability,
    /// [`WindowCommand::SetSize`].
    pub sets_size: Availability,
    /// [`WindowCommand::SetConstraints`].
    pub sets_constraints: Availability,
    /// [`WindowCommand::SetResizable`].
    pub sets_resizable: Availability,
    /// [`WindowCommand::SetDecorated`].
    pub sets_decorated: Availability,
    /// [`WindowMode::Minimized`] through [`WindowCommand::SetMode`].
    pub minimizes: Availability,
    /// [`WindowMode::Maximized`] through [`WindowCommand::SetMode`].
    pub maximizes: Availability,
    /// [`WindowMode::Fullscreen`] through [`WindowCommand::SetMode`].
    pub enters_fullscreen: Availability,
    /// [`WindowCommand::Hide`].
    pub hides: Availability,
    /// [`WindowCommand::Show`].
    pub shows: Availability,
    /// [`WindowCommand::Activate`]: bring the window to the front and request
    /// focus.
    pub activates: Availability,
    /// [`WindowCommand::Close`]: destroy the window. A window whose platform
    /// owns its lifetime (an Android activity) cannot close itself.
    pub closes: Availability,
    /// The snapshot's [`WindowGeometry::position`] is reported.
    pub observes_position: Availability,
    /// [`WindowMode::Minimized`] is ever published.
    pub observes_minimized: Availability,
    /// A saved position can be applied again when the window opens.
    pub places_at_open: Availability,
}

impl WindowCapabilities {
    /// A window that honours no command and observes nothing.
    pub const NONE: Self = Self {
        sets_title: Availability::Unavailable,
        sets_position: Availability::Unavailable,
        sets_size: Availability::Unavailable,
        sets_constraints: Availability::Unavailable,
        sets_resizable: Availability::Unavailable,
        sets_decorated: Availability::Unavailable,
        minimizes: Availability::Unavailable,
        maximizes: Availability::Unavailable,
        enters_fullscreen: Availability::Unavailable,
        hides: Availability::Unavailable,
        shows: Availability::Unavailable,
        activates: Availability::Unavailable,
        closes: Availability::Unavailable,
        observes_position: Availability::Unavailable,
        observes_minimized: Availability::Unavailable,
        places_at_open: Availability::Unavailable,
    };

    /// A window whose window system honours every command and reports
    /// everything itself.
    pub const ALL: Self = Self {
        sets_title: Availability::Native,
        sets_position: Availability::Native,
        sets_size: Availability::Native,
        sets_constraints: Availability::Native,
        sets_resizable: Availability::Native,
        sets_decorated: Availability::Native,
        minimizes: Availability::Native,
        maximizes: Availability::Native,
        enters_fullscreen: Availability::Native,
        hides: Availability::Native,
        shows: Availability::Native,
        activates: Availability::Native,
        closes: Availability::Native,
        observes_position: Availability::Native,
        observes_minimized: Availability::Native,
        places_at_open: Availability::Native,
    };

    /// How this window honours `command`.
    ///
    /// [`WindowMode::Normal`] is always [`Native`](Availability::Native):
    /// asking a window for the mode every window has is never a no-op that
    /// pretends.
    #[must_use]
    pub const fn availability(&self, command: &WindowCommand) -> Availability {
        match command {
            WindowCommand::SetTitle(_) => self.sets_title,
            WindowCommand::SetPosition(_) => self.sets_position,
            WindowCommand::SetSize(_) => self.sets_size,
            WindowCommand::SetConstraints(_) => self.sets_constraints,
            WindowCommand::SetResizable(_) => self.sets_resizable,
            WindowCommand::SetDecorated(_) => self.sets_decorated,
            WindowCommand::SetMode(WindowMode::Minimized) => self.minimizes,
            WindowCommand::SetMode(WindowMode::Maximized) => self.maximizes,
            WindowCommand::SetMode(WindowMode::Fullscreen) => self.enters_fullscreen,
            WindowCommand::SetMode(WindowMode::Normal) => Availability::Native,
            WindowCommand::Hide => self.hides,
            WindowCommand::Show => self.shows,
            WindowCommand::Activate => self.activates,
            WindowCommand::Close => self.closes,
        }
    }

    /// Whether this window honours `command` at all, natively or emulated.
    #[must_use]
    pub const fn permits(&self, command: &WindowCommand) -> bool {
        self.availability(command).is_available()
    }
}

/// A change requested of a window.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum WindowCommand {
    /// Set the title.
    SetTitle(String),
    /// Move the window's outer top-left corner to a global position in
    /// logical pixels.
    SetPosition(Point<f64>),
    /// Resize the window's inner (content) area, in logical pixels.
    SetSize(Size<f64>),
    /// Set the inner-size limits.
    SetConstraints(SizeConstraints),
    /// Allow or forbid user resizing.
    SetResizable(bool),
    /// Show or remove the frame and title bar.
    SetDecorated(bool),
    /// Enter a mode. [`WindowMode::Normal`] restores from any other mode.
    SetMode(WindowMode),
    /// Show the window.
    Show,
    /// Hide the window.
    Hide,
    /// Bring the window to the front and request focus.
    Activate,
    /// Close and destroy the window: a decision already made, which asks no
    /// should-close veto (ADR-0123). Asking whether the window may close is
    /// [`WindowControl::request_close`].
    Close,
}

impl WindowCommand {
    /// Refuse a command whose numbers cannot describe a window, before any
    /// operating system call.
    ///
    /// # Errors
    /// [`WindowCommandError::InvalidGeometry`] for a position or size that is
    /// not finite, a size that is not positive, or constraints whose minimum
    /// exceeds their maximum in either dimension.
    pub fn validate(&self) -> Result<(), WindowCommandError> {
        let valid = match self {
            Self::SetPosition(at) => at.x.is_finite() && at.y.is_finite(),
            Self::SetSize(size) => positive_size(*size),
            Self::SetConstraints(constraints) => {
                constraints.min.is_none_or(positive_size)
                    && constraints.max.is_none_or(positive_size)
                    && match (constraints.min, constraints.max) {
                        (Some(min), Some(max)) => {
                            min.width <= max.width && min.height <= max.height
                        }
                        _ => true,
                    }
            }
            Self::SetTitle(_)
            | Self::SetResizable(_)
            | Self::SetDecorated(_)
            | Self::SetMode(_)
            | Self::Show
            | Self::Hide
            | Self::Activate
            | Self::Close => true,
        };
        if valid {
            Ok(())
        } else {
            Err(WindowCommandError::InvalidGeometry)
        }
    }

    /// Admit a command to a window with these capabilities: its numbers must
    /// be valid, then the window must honour it. The check every backend's
    /// `apply` and every queue in front of one runs first.
    ///
    /// # Errors
    /// What [`validate`](Self::validate) refuses, or
    /// [`WindowCommandError::Unsupported`] when the command's capability is
    /// [`Availability::Unavailable`]
    /// in `capabilities`: the window's platform does not take it
    /// ([`UnsupportedReason::NotOnThisPlatform`]).
    pub fn admit(&self, capabilities: &WindowCapabilities) -> Result<(), WindowCommandError> {
        self.validate()?;
        if capabilities.permits(self) {
            Ok(())
        } else {
            Err(Unsupported::new(UnsupportedReason::NotOnThisPlatform).into())
        }
    }
}

fn positive_size(size: Size<f64>) -> bool {
    size.width.is_finite() && size.height.is_finite() && size.width > 0.0 && size.height > 0.0
}

/// Why a window did not take a command.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum WindowCommandError {
    /// This window does not honour the command; its capability is
    /// [`Availability::Unavailable`].
    #[error("the window does not take this command: {0}")]
    Unsupported(#[from] Unsupported),
    /// The command's numbers cannot describe a window.
    #[error(
        "the command's geometry is not finite and positive, or its minimum exceeds its maximum"
    )]
    InvalidGeometry,
    /// The window has closed.
    #[error("the window is closed")]
    Closed,
    /// The operating system refused or failed the request.
    #[error("the window system failed the command: {message}")]
    Backend {
        /// What the window system reported.
        message: String,
    },
}

impl WindowCommandError {
    /// What is not here, when that is why the command was refused.
    #[must_use]
    pub const fn unsupported(&self) -> Option<Unsupported> {
        match self {
            Self::Unsupported(unsupported) => Some(*unsupported),
            Self::InvalidGeometry | Self::Closed | Self::Backend { .. } => None,
        }
    }
}

/// The control surface the framework gives app code over one window
/// (ADR-0237 decision 4). The host implements it over its window; a handle in
/// the view layer calls it.
pub trait WindowControl {
    /// The window's last published snapshot, with the capabilities that say
    /// which commands it honours.
    fn state(&self) -> WindowState;

    /// Validate `command`, check it against the snapshot's capabilities and
    /// queue it for the owner's next turn.
    ///
    /// # Errors
    /// What [`WindowCommand::admit`] refuses, or
    /// [`WindowCommandError::Closed`] after the window closed.
    fn submit(&self, command: WindowCommand) -> Result<(), WindowCommandError>;

    /// Ask the window to close through the vetoed close path.
    fn request_close(&self);
}
